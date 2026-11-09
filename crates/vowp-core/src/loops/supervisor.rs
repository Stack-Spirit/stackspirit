//! L1 supervisor: the ordered tick, request application, dispatch, gates, verify and merge.
//!
//! Spec: `docs/design/loops.md` §1.1 (steps 0–13), `docs/design/crates/vowp-core.md` §7.
//! Every step is wrapped: one failing step is logged, bumps the L1 fail streak and the tick
//! continues with the later steps.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use vowp_graph::{
    DispatchCaps, DispatchChoice, Effect, Graph, JobEvent, StallInput, StallJudge, StallThresholds, StallVerdict,
    TransitionBounds, plan_dispatch,
};
use vowp_protocol::{
    ActiveState, CancelToken, GateResult, InboxKind, Job, JobId, JobKind, JobState, Level, LoopBounds, NewSession,
    ParkedReason, Profile, Run, RunId, RunPurpose, SessionRole, Timestamp, Worker, WorkerKind, WorkerState,
};
use vowp_store::{JobFilter, Store};

use crate::CoreError;
use crate::deps::Deps;
use crate::gates::{GateCtx, GateRunner};
use crate::ledger::{BoardSnapshot, Ledger};
use crate::schedules::ScheduleEngine;
use crate::session::service::SessionService;

use super::l2_worker::ReportOutcome;
use super::merge_queue::{MergeQueue, MergeStep};
use super::runner::{RunnerPool, WakeHandle};
use super::{Applied, NewInbox, apply_event, dispatch, l3_verify, reconcile};

/// Counters reported by [`Supervisor::startup`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconcileReport {
    /// Open runs finalised as crash.
    pub orphaned_runs: u32,
    /// Switch rows recovered by the session service.
    pub switches_recovered: u32,
    /// Missed schedules fired once.
    pub schedules_caught_up: u32,
    /// tmux windows recreated.
    pub windows_recreated: u32,
}

/// One tick step's result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepReport {
    /// Step name (`requests`, `harvest`, …).
    pub name: &'static str,
    /// False when the step returned an error or panicked.
    pub ok: bool,
    /// Short human-readable detail.
    pub detail: String,
}

/// Everything one L1 tick did.
#[derive(Debug, Clone, PartialEq)]
pub struct TickReport {
    /// Instant the tick started.
    pub at: Timestamp,
    /// One entry per step, in order.
    pub steps: Vec<StepReport>,
    /// Jobs dispatched this tick.
    pub dispatched: Vec<JobId>,
    /// State transitions applied this tick.
    pub transitions: Vec<(JobId, JobState, JobState)>,
    /// Jobs merged this tick.
    pub merged: Vec<JobId>,
}

/// Result of one background gate run.
struct GateTaskResult {
    job_id: JobId,
    results: Vec<GateResult>,
    all_passed: bool,
}

/// Runs one job's gates on a runner thread with its own store connection.
fn spawn_gate_task(
    deps: Deps,
    project: vowp_protocol::ProjectName,
    job: Job,
) -> Result<JoinHandle<Result<GateTaskResult, CoreError>>, CoreError> {
    Ok(thread::spawn(move || {
        let store = deps.open_store()?;
        let repo = super::main_repo(&deps, &store, &project)?;
        let worktree = job.worktree.as_ref().map(PathBuf::from).unwrap_or_else(|| repo.clone());
        let ctx = GateCtx {
            deps: &deps,
            job: &job,
            run_id: store.last_run_of_job(&job.id)?.map(|run| run.id),
            worktree: &worktree,
            repo: &repo,
            store: &store,
            attempt: job.attempts,
        };
        let outcome = GateRunner::new(deps.clone()).evaluate_all(&ctx)?;
        let ledger = Ledger::new(deps.clone());
        for result in &outcome.results {
            ledger.record_gate(&store, result)?;
        }
        Ok(GateTaskResult {
            job_id: job.id.clone(),
            results: outcome.results,
            all_passed: outcome.all_passed_before_review,
        })
    }))
}

/// The L1 supervisor.
pub struct Supervisor {
    pub(crate) deps: Deps,
    pub(crate) project: vowp_protocol::ProjectName,
    pub(crate) store: Store,
    pub(crate) runners: RunnerPool,
    pub(crate) ledger: Ledger,
    pub(crate) schedules: ScheduleEngine,
    pub(crate) sessions: SessionService,
    pub(crate) wake: WakeHandle,
    wake_rx: Option<Receiver<()>>,
    handled_runs: HashSet<RunId>,
    gate_tasks: HashMap<JobId, JoinHandle<Result<GateTaskResult, CoreError>>>,
    merge_task: Option<JoinHandle<Result<MergeStep, CoreError>>>,
    verify_retries: HashSet<(JobId, u32)>,
    start_purpose_override: Option<RunPurpose>,
    transitions: Vec<(JobId, JobState, JobState)>,
    dispatched: Vec<JobId>,
    merged: Vec<JobId>,
    last_checkpoint: Option<Timestamp>,
}

impl Supervisor {
    /// Creates a supervisor for one project.
    pub fn new(deps: Deps, project: vowp_protocol::ProjectName) -> Result<Self, CoreError> {
        let store = deps.open_store()?;
        let (wake, wake_rx) = WakeHandle::channel();
        reconcile::ensure_loops(&store, &deps)?;
        store.ensure_worker_slots(&deps.node, WorkerKind::Worker, deps.config.node.max_workers)?;
        store.ensure_worker_slots(&deps.node, WorkerKind::Verifier, deps.config.node.max_verifiers)?;
        Ok(Self {
            sessions: SessionService::new(deps.clone()),
            ledger: Ledger::new(deps.clone()),
            schedules: ScheduleEngine::new(deps.clone()),
            runners: RunnerPool::new(),
            deps,
            project,
            store,
            wake,
            wake_rx: Some(wake_rx),
            handled_runs: HashSet::new(),
            gate_tasks: HashMap::new(),
            merge_task: None,
            verify_retries: HashSet::new(),
            start_purpose_override: None,
            transitions: Vec::new(),
            dispatched: Vec::new(),
            merged: Vec::new(),
            last_checkpoint: None,
        })
    }

    /// Startup reconciliation (loops.md §1 step 1 and §11 catch-up).
    pub fn startup(&mut self) -> Result<ReconcileReport, CoreError> {
        self.handled_runs.clear();
        let active = self.active_run_ids();
        let report = reconcile::startup(&self.deps, &self.store, &active)?;
        self.store
            .ensure_worker_slots(&self.deps.node, WorkerKind::Worker, self.deps.config.node.max_workers)?;
        self.store.ensure_worker_slots(
            &self.deps.node,
            WorkerKind::Verifier,
            self.deps.config.node.max_verifiers,
        )?;
        Ok(report)
    }

    /// One full L1 tick, steps 0–13 of loops.md §1.1.
    pub fn tick(&mut self) -> Result<TickReport, CoreError> {
        let now = self.deps.clock.now();
        self.transitions.clear();
        self.dispatched.clear();
        self.merged.clear();
        let paused = self.store.kv_get("paused")?.as_deref() == Some("1");
        let mut steps = Vec::new();

        steps.push(self.step("requests", Self::step_requests));
        steps.push(self.step("reconcile", |this| this.step_reconcile(now)));
        if !paused {
            steps.push(self.step("harvest", |this| this.step_harvest(now)));
            steps.push(self.step("stalls", |this| this.step_stalls(now)));
            steps.push(self.step("states", |this| this.step_states(now)));
            steps.push(self.step("gates", |this| this.step_gates(now)));
            steps.push(self.step("verify", |this| this.step_verify(now)));
            steps.push(self.step("merge", |this| this.step_merge(now)));
            steps.push(self.step("dispatch", |this| this.step_dispatch(now)));
            steps.push(self.step("triggers", |_this| {
                Ok("planner/keeper triggers owned by n08d".to_owned())
            }));
            steps.push(self.step("probes", |_this| Ok("L6 probes owned by n08d".to_owned())));
            steps.push(self.step("schedules", |this| this.step_schedules(now)));
        }
        steps.push(self.step("board", |this| this.step_board(now)));
        steps.push(self.step("heartbeat", |this| this.step_heartbeat(now)));

        let ok = steps.iter().all(|step| step.ok);
        let failed: Vec<&str> = steps.iter().filter(|step| !step.ok).map(|step| step.name).collect();
        let status = if failed.is_empty() {
            "ok".to_owned()
        } else {
            format!("degraded: {}", failed.join(", "))
        };
        let next = now.plus(Duration::from_secs(self.deps.config.node.tick_secs.max(1)));
        if let Err(error) = self
            .store
            .record_loop_run("l1-supervisor", now, Some(next), &status, ok)
        {
            self.deps
                .log
                .log(Level::Error, "l1", &format!("cannot record loop row: {error}"));
        }

        Ok(TickReport {
            at: now,
            steps,
            dispatched: std::mem::take(&mut self.dispatched),
            transitions: std::mem::take(&mut self.transitions),
            merged: std::mem::take(&mut self.merged),
        })
    }

    /// Startup then tick every `[node].tick_secs` or on wake, until `cancel`.
    pub fn run(&mut self, cancel: &CancelToken) -> Result<(), CoreError> {
        self.startup()?;
        let tick = Duration::from_secs(self.deps.config.node.tick_secs.max(1));
        while !cancel.is_cancelled() {
            self.tick()?;
            match self.wake_rx.as_ref() {
                Some(receiver) => match receiver.recv_timeout(tick) {
                    Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                },
                None => std::thread::sleep(tick),
            }
        }
        self.runners.cancel_all();
        // Mark run rows whose threads were interrupted as stalled, as the crate doc requires.
        let store = self.deps.open_store()?;
        for run in store.open_runs()? {
            if run.node != self.deps.node {
                continue;
            }
            let _ = store.finish_run(
                &run.id,
                &vowp_store::RunFinish {
                    outcome: vowp_protocol::RunOutcome::Stalled,
                    exit_code: run.exit_code,
                    limit_kind: run.limit_kind,
                    cost: run.cost.clone(),
                    native_session_id: run.native_session_id.clone(),
                    detail: "supervisor stopping".to_owned(),
                },
            );
        }
        Ok(())
    }

    /// Returns a cloneable handle that ends the idle wait early.
    pub fn wake_handle(&self) -> WakeHandle {
        self.wake.clone()
    }

    /// Runs one tick step under `catch_unwind` and turns failures into a report entry.
    fn step<F>(&mut self, name: &'static str, f: F) -> StepReport
    where
        F: FnOnce(&mut Self) -> Result<String, CoreError>,
    {
        let result = catch_unwind(AssertUnwindSafe(|| f(self)));
        match result {
            Ok(Ok(detail)) => StepReport { name, ok: true, detail },
            Ok(Err(error)) => {
                self.deps
                    .log
                    .log(Level::Error, "l1", &format!("step {name} failed: {error}"));
                StepReport {
                    name,
                    ok: false,
                    detail: error.to_string(),
                }
            }
            Err(payload) => {
                let detail = panic_text(payload);
                self.deps
                    .log
                    .log(Level::Error, "l1", &format!("step {name} panicked: {detail}"));
                StepReport {
                    name,
                    ok: false,
                    detail: format!("panic: {detail}"),
                }
            }
        }
    }

    /// Step 0: pause flag and every queued CLI request.
    fn step_requests(&mut self) -> Result<String, CoreError> {
        let requests = crate::requests::drain(&self.store)?;
        let mut applied = 0;
        for (id, request) in requests {
            match self.apply_request(&request) {
                Ok(()) => applied += 1,
                Err(error) => self.deps.log.log(
                    Level::Warn,
                    "requests",
                    &format!("dropped request {} {}: {error}", id.0, request_name(&request)),
                ),
            }
        }
        Ok(format!("{applied} request(s) applied"))
    }

    /// Applies one drained request and logs the loop-request ledger rows.
    fn apply_request(&mut self, request: &crate::requests::Request) -> Result<(), CoreError> {
        use crate::requests::Request;
        match request {
            Request::ResolveJob { job } => self.apply_simple(job, JobEvent::Resolved, None),
            Request::RetryJob { job } => {
                let current = self.store.require_job(job)?;
                let event = match current.state {
                    JobState::Failed => JobEvent::Retry,
                    JobState::Stalled => JobEvent::KeeperRestart,
                    other => {
                        return Err(CoreError::Invalid(format!(
                            "retry is not allowed in state {}",
                            other.as_str()
                        )));
                    }
                };
                self.apply_simple(job, event, None)
            }
            Request::ParkJob { job, reason } => self.apply_simple(job, JobEvent::HumanPark, Some(reason.clone())),
            Request::CancelJob { job } => self.apply_simple(job, JobEvent::Cancel, None),
            Request::NudgeJob { job } => self.apply_simple(job, JobEvent::KeeperNudge { text: None }, None),
            Request::RestartJob { job } => self.apply_simple(job, JobEvent::KeeperRestart, None),
            Request::ReplanJob { job } => self.apply_simple(job, JobEvent::KeeperReplan, None),
            Request::FailJob { job, reason } => self.apply_simple(job, JobEvent::KeeperFail, Some(reason.clone())),
            Request::AnswerInbox { id } => self.apply_inbox_answer(id),
            Request::PauseLoop { name } | Request::ResumeLoop { name } => {
                let pause = matches!(request, Request::PauseLoop { .. });
                if name == "l1-supervisor" {
                    return Err(CoreError::Invalid(
                        "the l1-supervisor loop cannot be paused or resumed".to_owned(),
                    ));
                }
                let state = if pause {
                    ActiveState::Paused
                } else {
                    ActiveState::Active
                };
                self.store.set_loop_state(name, state)?;
                let (from, to) = if pause {
                    ("active", "paused")
                } else {
                    ("paused", "active")
                };
                super::record_loop_request(&self.store, &self.project, name, from, to, request_name(request))?;
                Ok(())
            }
            Request::RunLoop { name } => {
                let mut loop_ = self
                    .store
                    .loop_(name)?
                    .ok_or_else(|| CoreError::NotFound(format!("loop {name}")))?;
                let now = self.deps.clock.now();
                loop_.next_run_at = Some(now);
                loop_.updated_at = now;
                self.store.upsert_loop(&loop_)?;
                super::record_loop_request(
                    &self.store,
                    &self.project,
                    name,
                    "scheduled",
                    "due",
                    request_name(request),
                )
            }
            Request::Wake => super::record_loop_request(
                &self.store,
                &self.project,
                "l1-supervisor",
                "idle",
                "wake",
                request_name(request),
            ),
        }
    }

    /// Applies one job event with no findings and optional note.
    pub(crate) fn apply_simple(&mut self, job: &JobId, event: JobEvent, note: Option<String>) -> Result<(), CoreError> {
        self.apply_with(job, event, None, Vec::new(), note).map(|_| ())
    }

    /// Applies an inbox answer by resolving the parked job it belongs to.
    fn apply_inbox_answer(&mut self, id: &vowp_protocol::InboxId) -> Result<(), CoreError> {
        let item = self
            .store
            .inbox_item(id)?
            .ok_or_else(|| CoreError::NotFound(format!("inbox item {id}")))?;
        if item.state != vowp_protocol::InboxState::Answered {
            return Err(CoreError::Invalid(format!("inbox item {id} has not been answered")));
        }
        if let Some(job_id) = item.job_id {
            let job = self.store.require_job(&job_id)?;
            if job.state == JobState::Parked {
                self.apply_simple(&job_id, JobEvent::Resolved, None)?;
            }
        }
        Ok(())
    }

    /// Step 1: finalise open runs with no live runner thread.
    fn step_reconcile(&mut self, _now: Timestamp) -> Result<String, CoreError> {
        let active = self.active_run_ids();
        let count = reconcile::orphaned_runs(&self.deps, &self.store, &active)?;
        Ok(format!("{count} orphaned run(s)"))
    }

    /// Step 2: harvest finished runs and feed their outcome to the job state machine.
    fn step_harvest(&mut self, now: Timestamp) -> Result<String, CoreError> {
        let mut harvested = 0;
        self.runners.poll();
        let jobs = self.store.list_jobs(&JobFilter {
            project: Some(self.project.clone()),
            ..JobFilter::default()
        })?;
        for job in jobs {
            let Some(run) = self.store.last_run_of_job(&job.id)? else {
                continue;
            };
            if run.ended_at.is_none() || self.handled_runs.contains(&run.id) {
                continue;
            }
            self.handled_runs.insert(run.id.clone());
            self.release_worker(&run.id)?;
            if job.state != JobState::Running {
                continue;
            }
            let (event, note) = self.event_for_run(&job, &run, now)?;
            let applied = self.apply_with(&job.id, event.clone(), None, Vec::new(), note)?;
            self.deps.log.log(
                Level::Info,
                "l1",
                &format!(
                    "harvest {} run {} -> {} ({})",
                    job.id,
                    run.id,
                    applied.to.as_str(),
                    super::event_name(&event)
                ),
            );
            harvested += 1;
        }
        Ok(format!("{harvested} run(s) harvested"))
    }

    /// Chooses the job event for one finished run, plus the note its inbox item carries.
    fn event_for_run(&self, job: &Job, run: &Run, now: Timestamp) -> Result<(JobEvent, Option<String>), CoreError> {
        Ok(match run.outcome {
            Some(vowp_protocol::RunOutcome::Ok) => {
                let worktree = match job.worktree.as_ref() {
                    Some(path) => PathBuf::from(path),
                    None => return Ok((JobEvent::RunEndedNoReport, None)),
                };
                match super::l2_worker::classify_report(&worktree, job, run.started_at) {
                    ReportOutcome::Ready => (JobEvent::ReportReady, None),
                    ReportOutcome::Blocked(questions) => (
                        JobEvent::Blocked {
                            reason: ParkedReason::Question,
                        },
                        Some(questions.join("\n")),
                    ),
                    ReportOutcome::NoReport => (JobEvent::RunEndedNoReport, None),
                }
            }
            Some(vowp_protocol::RunOutcome::Rate) => {
                let until = run
                    .provider
                    .as_deref()
                    .and_then(|provider| self.store.limit_for(provider).ok().flatten())
                    .and_then(|limit| limit.resets_at)
                    .filter(|until| *until > now)
                    .unwrap_or_else(|| now.plus(Duration::from_secs(600)));
                (JobEvent::RunRateLimited { until }, None)
            }
            Some(vowp_protocol::RunOutcome::Limit)
            | Some(vowp_protocol::RunOutcome::Credits)
            | Some(vowp_protocol::RunOutcome::Auth)
            | Some(vowp_protocol::RunOutcome::Context) => (JobEvent::RunEndedNoReport, None),
            Some(vowp_protocol::RunOutcome::Crash)
            | Some(vowp_protocol::RunOutcome::Error)
            | Some(vowp_protocol::RunOutcome::Killed)
            | Some(vowp_protocol::RunOutcome::Stalled)
            | None => (JobEvent::RunEndedNoReport, None),
        })
    }

    /// Step 3: stall detection for live worker/verifier runs.
    fn step_stalls(&mut self, now: Timestamp) -> Result<String, CoreError> {
        let thresholds = StallThresholds {
            stall_minutes: self.bounds().stall_minutes.unwrap_or(20),
            silent_minutes: self.bounds().silent_minutes.unwrap_or(8),
        };
        let open = self.store.open_runs()?;
        let mut stalled = 0;
        for run in open {
            let session_backed = matches!(
                run.role,
                vowp_protocol::RunRole::Planner | vowp_protocol::RunRole::Keeper
            );
            if run.role != vowp_protocol::RunRole::Worker
                && run.role != vowp_protocol::RunRole::Verifier
                && !session_backed
            {
                continue;
            }
            if self.handled_runs.contains(&run.id) {
                continue;
            }
            if session_backed {
                let last_event_at = match &run.session_id {
                    Some(session) => self.store.session(session)?.map(|session| session.updated_at),
                    None => None,
                };
                let verdict = StallJudge::judge(
                    &StallInput {
                        now,
                        run_started_at: run.started_at,
                        last_event_at,
                        last_commit_at: None,
                        output_bytes: super::file_len(&self.deps.paths.run_raw_path(&run.id)),
                    },
                    &thresholds,
                );
                if matches!(verdict, StallVerdict::Stalled { .. }) {
                    self.runners.cancel_and_wait(&run.id);
                    self.handled_runs.insert(run.id.clone());
                    stalled += 1;
                }
                continue;
            }
            let Some(job_id) = &run.job_id else {
                continue;
            };
            let Ok(job) = self.store.require_job(job_id) else {
                continue;
            };
            if job.state != JobState::Running {
                continue;
            }
            let last_event_at = match &run.session_id {
                Some(session) => self.store.session(session)?.map(|session| session.updated_at),
                None => None,
            };
            let last_commit_at = match (&job.worktree, &job.branch) {
                (Some(worktree), Some(branch)) => self
                    .deps
                    .git
                    .last_commit_time(std::path::Path::new(worktree), branch)
                    .ok()
                    .flatten(),
                _ => None,
            };
            let output_bytes = super::file_len(&self.deps.paths.run_raw_path(&run.id));
            let verdict = StallJudge::judge(
                &StallInput {
                    now,
                    run_started_at: run.started_at,
                    last_event_at,
                    last_commit_at,
                    output_bytes,
                },
                &thresholds,
            );
            if let StallVerdict::Stalled { silent, .. } = verdict {
                self.runners.cancel(&run.id);
                self.handled_runs.insert(run.id.clone());
                self.apply_simple(&job.id, JobEvent::Stalled { silent }, None)?;
                stalled += 1;
            }
        }
        Ok(format!("{stalled} run(s) stalled"))
    }

    /// Step 4: pending → ready and delayed rate-limit continues.
    fn step_states(&mut self, now: Timestamp) -> Result<String, CoreError> {
        let (graph, _jobs) = self.graph()?;
        let mut changed = 0;
        for id in graph.ready_jobs() {
            self.apply_simple(&id, JobEvent::NeedsSatisfied, None)?;
            changed += 1;
        }
        for (key, value) in self.store.kv_prefix("delay:")? {
            let Some(until) = super::parse_ts(&value) else {
                self.store.kv_delete(&key)?;
                continue;
            };
            if until > now {
                continue;
            }
            let job_id = JobId::parse(key.trim_start_matches("delay:"))?;
            self.store.kv_delete(&key)?;
            if let Ok(job) = self.store.require_job(&job_id)
                && job.state == JobState::Running
                && self
                    .store
                    .open_runs()?
                    .iter()
                    .all(|run| run.job_id.as_ref() != Some(&job_id))
            {
                let choice = self.pick_choice(&job)?;
                let purpose = dispatch::purpose_for_dispatch(&self.store, &job)?;
                self.start_run(&job, purpose, &choice, Vec::new())?;
                changed += 1;
            }
        }
        Ok(format!("{changed} state change(s)"))
    }

    /// Step 5: spawn gate runners and collect the ones that finished.
    fn step_gates(&mut self, _now: Timestamp) -> Result<String, CoreError> {
        let mut applied = 0;
        let finished = self
            .gate_tasks
            .iter()
            .filter(|(_, handle)| handle.is_finished())
            .map(|(job, _)| job.clone())
            .collect::<Vec<_>>();
        for job_id in finished {
            let Some(handle) = self.gate_tasks.remove(&job_id) else {
                continue;
            };
            match handle.join() {
                Ok(Ok(result)) => {
                    let job = self.store.require_job(&result.job_id)?;
                    if job.state != JobState::Gating {
                        continue;
                    }
                    if result.all_passed {
                        self.apply_simple(&job.id, JobEvent::GatesPassed, None)?;
                    } else {
                        let findings = result
                            .results
                            .iter()
                            .filter(|gate| !gate.passed)
                            .map(dispatch::gate_finding)
                            .collect::<Vec<_>>();
                        self.apply_with(&job.id, JobEvent::GatesFailed, None, findings, None)?;
                    }
                    applied += 1;
                }
                Ok(Err(error)) => self.deps.log.log(
                    Level::Error,
                    "gates",
                    &format!("gate runner for {job_id} failed: {error}"),
                ),
                Err(_) => self
                    .deps
                    .log
                    .log(Level::Error, "gates", &format!("gate runner for {job_id} panicked")),
            }
        }

        let jobs = self.store.list_jobs(&JobFilter {
            project: Some(self.project.clone()),
            states: vec![JobState::Gating],
            ..JobFilter::default()
        })?;
        let mut started = 0;
        for job in jobs {
            if self.gate_tasks.contains_key(&job.id) {
                continue;
            }
            let handle = spawn_gate_task(self.deps.clone(), self.project.clone(), job.clone())?;
            self.gate_tasks.insert(job.id.clone(), handle);
            started += 1;
        }
        Ok(format!(
            "{applied} gate result(s) applied, {started} gate runner(s) started"
        ))
    }

    /// Step 6: ensure one verify job per reviewing attempt and deliver its verdict.
    fn step_verify(&mut self, _now: Timestamp) -> Result<String, CoreError> {
        if !self.loop_active("l3-verify") {
            return Ok("l3-verify paused".to_owned());
        }
        let jobs = self.store.list_jobs(&JobFilter {
            project: Some(self.project.clone()),
            states: vec![JobState::Reviewing],
            ..JobFilter::default()
        })?;
        let mut handled = 0;
        for original in jobs {
            let verify = self.find_verify_job(&original)?;
            match verify {
                None => {
                    let evidence = self.store.gate_results_for(&original.id, original.attempts)?;
                    let diff = self.diff_summary(&original);
                    let verify = l3_verify::create_verify_job(&self.store, &self.deps, &original, &evidence, &diff)?;
                    self.apply_simple(&verify.id, JobEvent::NeedsSatisfied, None)?;
                    handled += 1;
                }
                Some(verify) if verify.state == JobState::Done => {
                    match l3_verify::on_verify_done(&self.store, &verify) {
                        Ok(event) => self.apply_simple(&original.id, event, None)?,
                        Err(error) => {
                            self.deps.log.log(
                                Level::Warn,
                                "l3",
                                &format!("verify job {} has no usable verdict: {error}", verify.id),
                            );
                            self.retry_or_park_verify(&original, &error)?;
                        }
                    }
                    handled += 1;
                }
                Some(verify) if verify.state == JobState::Failed => {
                    self.apply_with(
                        &original.id,
                        JobEvent::VerifierFailed,
                        None,
                        Vec::new(),
                        Some(format!("verifier could not produce a verdict for {}", original.id)),
                    )?;
                    handled += 1;
                }
                Some(_) => {}
            }
        }
        Ok(format!("{handled} review(s) advanced"))
    }

    /// Retries one verify job with an unusable verdict, then parks the original (T37).
    pub(crate) fn retry_or_park_verify(&mut self, original: &Job, error: &CoreError) -> Result<(), CoreError> {
        let key = (original.id.clone(), original.attempts);
        if self.verify_retries.insert(key) {
            let evidence = self.store.gate_results_for(&original.id, original.attempts)?;
            let diff = self.diff_summary(original);
            let retry = l3_verify::create_verify_retry_job(&self.store, &self.deps, original, &evidence, &diff)?;
            self.deps.log.log(
                Level::Warn,
                "l3",
                &format!(
                    "verify job for {} had no usable verdict ({error}); retrying once as {}",
                    original.id, retry.id
                ),
            );
            self.apply_simple(&retry.id, JobEvent::NeedsSatisfied, None)
        } else {
            self.apply_with(
                &original.id,
                JobEvent::VerifierFailed,
                None,
                Vec::new(),
                Some(format!("verifier could not produce a verdict for {}", original.id)),
            )
            .map(|_| ())
        }
    }

    /// Step 7: start at most one merge runner and collect its finished result.
    fn step_merge(&mut self, _now: Timestamp) -> Result<String, CoreError> {
        if let Some(handle) = self.merge_task.take() {
            if !handle.is_finished() {
                self.merge_task = Some(handle);
                return Ok("merge runner in progress".to_owned());
            }
            return match handle.join() {
                Ok(Ok(step)) => self.apply_merge_step(step),
                Ok(Err(error)) => Err(error),
                Err(_) => Err(CoreError::Command("merge runner panicked".to_owned())),
            };
        }
        let merging = self.store.list_jobs(&JobFilter {
            project: Some(self.project.clone()),
            states: vec![JobState::Merging],
            ..JobFilter::default()
        })?;
        if merging.is_empty() {
            return Ok("merge queue idle".to_owned());
        }
        let deps = self.deps.clone();
        self.merge_task = Some(thread::spawn(move || {
            let store = deps.open_store()?;
            MergeQueue::new(deps.clone()).step(&store)
        }));
        Ok("merge runner started".to_owned())
    }

    /// Applies one finished merge-queue result.
    fn apply_merge_step(&mut self, step: MergeStep) -> Result<String, CoreError> {
        match step {
            MergeStep::Idle | MergeStep::InProgress => Ok("merge queue idle".to_owned()),
            MergeStep::Merged(job) => {
                self.apply_simple(&job, JobEvent::MergeSucceeded, None)?;
                self.merged.push(job);
                Ok("merged one job".to_owned())
            }
            MergeStep::Conflict { job, files } => {
                let findings = files
                    .iter()
                    .map(|file| format!("rebase conflict in {file}; resolve it and continue the rebase"))
                    .collect::<Vec<_>>();
                self.apply_with(&job, JobEvent::MergeConflict, None, findings, None)?;
                Ok(format!("merge conflict in {}", job))
            }
            MergeStep::ChecksFailed { job } => {
                let current = self.store.require_job(&job)?;
                let results = self.store.gate_results_for(&current.id, current.attempts)?;
                let mut findings = results
                    .iter()
                    .filter(|result| !result.passed)
                    .map(dispatch::gate_finding)
                    .collect::<Vec<_>>();
                if findings.is_empty() {
                    findings.push(format!("merge checks failed for {}", current.id));
                }
                self.apply_with(&current.id, JobEvent::MergeChecksFailed, None, findings, None)?;
                Ok(format!("merge checks failed for {}", current.id))
            }
            MergeStep::Manual { job, detail } => {
                self.apply_with(&job, JobEvent::HumanPark, None, Vec::new(), Some(detail))?;
                Ok(format!("merge blocked for {}", job))
            }
        }
    }

    /// Step 8: plan and dispatch ready jobs, starting each runner thread.
    fn step_dispatch(&mut self, now: Timestamp) -> Result<String, CoreError> {
        if !self.loop_active("l2-worker") {
            return Ok("l2-worker paused".to_owned());
        }
        let (graph, jobs) = self.graph()?;
        let workers = self.store.list_workers(&self.deps.node)?;
        let free_workers = workers
            .iter()
            .filter(|worker| worker.kind == WorkerKind::Worker && worker.state == WorkerState::Idle)
            .count()
            .try_into()
            .unwrap_or(u32::MAX);
        let free_verifiers = workers
            .iter()
            .filter(|worker| worker.kind == WorkerKind::Verifier && worker.state == WorkerState::Idle)
            .count()
            .try_into()
            .unwrap_or(u32::MAX);
        let open_runs = self.store.open_runs()?;
        let mut provider_free = BTreeMap::new();
        for profile in self.deps.config.profiles.values() {
            for executor in &profile.ladder {
                let provider = executor.provider();
                if provider_free.contains_key(&provider) {
                    continue;
                }
                if let Some(config) = self.deps.config.providers.get(&provider) {
                    let running = open_runs
                        .iter()
                        .filter(|run| run.provider.as_deref() == Some(provider.as_str()))
                        .count()
                        .try_into()
                        .unwrap_or(u32::MAX);
                    provider_free.insert(provider, config.max_concurrent.saturating_sub(running));
                }
            }
        }
        let limited = self
            .store
            .active_limits(now)?
            .into_iter()
            .map(|limit| (limit.provider, limit.resets_at))
            .collect::<HashMap<_, _>>();
        for limit in self.store.active_limits(now)? {
            if let (Some(cap), Some(cap_until)) = (limit.concurrency_cap, limit.cap_until)
                && cap_until > now
            {
                provider_free
                    .entry(limit.provider)
                    .and_modify(|free| *free = (*free).min(cap));
            }
        }
        let caps = DispatchCaps {
            free_workers,
            free_verifiers,
            provider_free: provider_free.into_iter().collect(),
            limited,
            profiles: self
                .deps
                .config
                .profiles
                .clone()
                .into_iter()
                .collect::<BTreeMap<String, Profile>>(),
            now,
        };
        let choices = plan_dispatch(&graph, &jobs, &caps);
        let mut dispatched = 0;
        for choice in choices {
            let job = self.store.require_job(&choice.job_id)?;
            self.dispatched.push(job.id.clone());
            let resume = dispatch::purpose_for_dispatch(&self.store, &job)?;
            self.start_purpose_override = Some(resume);
            let applied = self.apply_with(&job.id, JobEvent::Dispatched, Some(choice), Vec::new(), None);
            self.start_purpose_override = None;
            applied?;
            dispatched += 1;
        }
        Ok(format!("{dispatched} job(s) dispatched"))
    }

    /// Step 11: fire due schedules.
    fn step_schedules(&mut self, now: Timestamp) -> Result<String, CoreError> {
        if !self.loop_active("l9-schedules") {
            return Ok("l9-schedules paused".to_owned());
        }
        let due = self.schedules.tick(&self.store, now)?;
        let mut fired = 0;
        for due_run in &due {
            match self.schedules.fire(&self.store, due_run) {
                Ok(_) => fired += 1,
                Err(error) => self.deps.log.log(
                    Level::Error,
                    "l9",
                    &format!("cannot fire schedule {}: {error}", due_run.schedule.name),
                ),
            }
        }
        Ok(format!("{fired} schedule(s) fired"))
    }

    /// Step 12: board snapshot and checkpoint.
    fn step_board(&mut self, now: Timestamp) -> Result<String, CoreError> {
        let snapshot = BoardSnapshot::collect(&self.store, &self.deps.config, now)?;
        let json = board_json(&snapshot)?;
        self.atomic_write("board.json", &json)?;

        let checkpoint_minutes = self.bounds().checkpoint_minutes.unwrap_or(120) as u64;
        let due = match self.last_checkpoint {
            None => checkpoint_minutes > 0,
            Some(last) => now.since(last) >= Duration::from_secs(checkpoint_minutes.saturating_mul(60)),
        };
        if due && checkpoint_minutes > 0 {
            let text = crate::ledger::report::checkpoint(&self.store, &self.deps.config, now)?;
            let project = super::main_repo(&self.deps, &self.store, &self.project)?;
            let dir = project.join(".vowp/checkpoints");
            std::fs::create_dir_all(&dir).map_err(|source| CoreError::Io {
                path: dir.display().to_string(),
                source,
            })?;
            let path = dir.join(format!("{}.md", now.unix_secs()));
            std::fs::write(&path, text).map_err(|source| CoreError::Io {
                path: path.display().to_string(),
                source,
            })?;
            self.last_checkpoint = Some(now);
        }
        Ok("board written".to_owned())
    }

    /// Step 13: heartbeat file and supervisor pid.
    fn step_heartbeat(&mut self, now: Timestamp) -> Result<String, CoreError> {
        self.atomic_write("heartbeat", &now.unix_secs().to_string())?;
        self.store.kv_set("supervisor_pid", &std::process::id().to_string())?;
        Ok("heartbeat written".to_owned())
    }

    /// Applies one event and executes every effect it returned.
    pub(crate) fn apply_with(
        &mut self,
        job_id: &JobId,
        event: JobEvent,
        choice: Option<DispatchChoice>,
        findings: Vec<String>,
        note: Option<String>,
    ) -> Result<Applied, CoreError> {
        let bounds = self.transition_bounds();
        let applied = apply_event(&self.store, job_id, &event, bounds, note, None)?;
        self.transitions.push((job_id.clone(), applied.from, applied.to));
        let mut findings = findings;
        for effect in applied.effects.clone() {
            self.execute_effect(job_id, &effect, choice.as_ref(), &mut findings)?;
        }
        Ok(applied)
    }

    /// Executes one state-machine effect.
    fn execute_effect(
        &mut self,
        job_id: &JobId,
        effect: &Effect,
        choice: Option<&DispatchChoice>,
        findings: &mut Vec<String>,
    ) -> Result<(), CoreError> {
        match effect {
            Effect::CreateWorktree => {
                let job = self.store.require_job(job_id)?;
                let repo = super::main_repo(&self.deps, &self.store, &self.project)?;
                dispatch::ensure_worktree(&self.deps, &self.store, &job, &repo)?;
            }
            Effect::StartRun { purpose } => {
                let job = self.store.require_job(job_id)?;
                let purpose = if *purpose == RunPurpose::Start {
                    self.start_purpose_override.take().unwrap_or(RunPurpose::Start)
                } else {
                    *purpose
                };
                let choice = match choice {
                    Some(choice) => choice.clone(),
                    None => self.pick_choice(&job)?,
                };
                if let Err(error) = self.start_run(&job, purpose, &choice, std::mem::take(findings)) {
                    self.deps
                        .log
                        .log(Level::Error, "l2", &format!("cannot start run for {}: {error}", job.id));
                    self.apply_with(job_id, JobEvent::RunEndedNoReport, None, Vec::new(), None)?;
                }
            }
            Effect::StopRun => {
                if let Some(run) = self.store.last_run_of_job(job_id)?
                    && run.ended_at.is_none()
                {
                    self.runners.cancel_and_wait(&run.id);
                }
            }
            Effect::RunGates | Effect::EnqueueMerge | Effect::UnblockDependents => {}
            Effect::IncrementContinues
            | Effect::ResetContinues
            | Effect::IncrementHangRestarts
            | Effect::IncrementAttempts
            | Effect::ResetCounters
            | Effect::SetParkedReason(_)
            | Effect::ClearParkedReason => {}
            Effect::StartVerify => {
                let job = self.store.require_job(job_id)?;
                let evidence = self.store.gate_results_for(&job.id, job.attempts)?;
                let diff = self.diff_summary(&job);
                let verify = l3_verify::create_verify_job(&self.store, &self.deps, &job, &evidence, &diff)?;
                self.apply_with(&verify.id, JobEvent::NeedsSatisfied, None, Vec::new(), None)?;
            }
            Effect::CreateFixJob { kind } => {
                let job = self.store.require_job(job_id)?;
                let child = dispatch::create_child_job(&self.deps, &self.store, &job, *kind, findings)?;
                self.apply_with(&child.id, JobEvent::NeedsSatisfied, None, Vec::new(), None)?;
            }
            Effect::CreateDecisionJob => {
                let job = self.store.require_job(job_id)?;
                let decision = dispatch::create_decision_job(&self.deps, &self.store, &job)?;
                self.apply_with(&decision.id, JobEvent::NeedsSatisfied, None, Vec::new(), None)?;
            }
            Effect::CleanupWorktree => {
                let job = self.store.require_job(job_id)?;
                if let Some(worktree) = &job.worktree {
                    let repo = super::main_repo(&self.deps, &self.store, &self.project)?;
                    let _ = self.deps.git.worktree_remove(&repo, std::path::Path::new(worktree));
                    let _ = self.deps.windows.kill_window(
                        &vowp_tmux::session_name_for_project(self.project.as_str()),
                        &super::window_name(&job.id),
                    );
                }
            }
            Effect::NotifyInbox { kind } => {
                let job = self.store.require_job(job_id)?;
                self.open_job_inbox(&job, *kind)?;
            }
            Effect::NotifyParent => self.notify_parent(job_id)?,
            Effect::WakeKeeper => {
                self.store.kv_set("wake:keeper", "1")?;
            }
            Effect::WakePlanner => {
                self.store.kv_set("wake:planner", "1")?;
            }
            Effect::DelayRun { purpose: _, until } => {
                self.store.kv_set(&format!("delay:{job_id}"), &until.as_rfc3339())?;
            }
        }
        Ok(())
    }

    /// Opens the inbox item implied by a transition effect.
    pub(crate) fn open_job_inbox(&mut self, job: &Job, kind: InboxKind) -> Result<(), CoreError> {
        let note = if job.note.is_empty() {
            String::new()
        } else {
            job.note.clone()
        };
        let (title, body, options) = match (kind, job.parked_reason) {
            (InboxKind::Decision, _) => (
                format!("decide: {}", job.id),
                format!(
                    "job {} exhausted its fix rounds. Choose retry, replan or cancel. {note}",
                    job.id
                ),
                vec!["retry".to_owned(), "replan".to_owned(), "cancel".to_owned()],
            ),
            (InboxKind::Question, Some(ParkedReason::Credits)) => (
                format!("credits exhausted: {}", job.id),
                "credits exhausted for the job's provider; top up and run `vowp inbox answer <id> resolved`".to_owned(),
                vec!["resolved".to_owned()],
            ),
            (InboxKind::Question, Some(ParkedReason::Auth)) => (
                format!("auth required: {}", job.id),
                "log in again with the harness's own login, then `vowp inbox answer <id> resolved`".to_owned(),
                vec!["resolved".to_owned()],
            ),
            (InboxKind::Question, Some(ParkedReason::ProviderBlocked)) => (
                format!("provider blocked: {}", job.id),
                "the provider is blocked; resolve it and answer `resolved`".to_owned(),
                vec!["resolved".to_owned()],
            ),
            (InboxKind::Question, _) => (
                format!("question: {}", job.id),
                if note.is_empty() {
                    format!("job {} needs a human answer", job.id)
                } else {
                    note
                },
                vec!["resolved".to_owned()],
            ),
            (InboxKind::Approval, _) => (
                format!("approval: {}", job.id),
                format!("approval requested for {} {note}", job.id),
                vec!["approve".to_owned(), "refuse".to_owned()],
            ),
            (InboxKind::Notice, _) => (
                format!("notice: {}", job.id),
                if note.is_empty() {
                    format!("job {} changed state", job.id)
                } else {
                    note
                },
                Vec::new(),
            ),
        };
        super::open_inbox(
            &self.store,
            &self.deps,
            NewInbox {
                project: Some(job.project.clone()),
                job_id: Some(job.id.clone()),
                kind,
                title,
                body,
                options,
            },
        )?;
        Ok(())
    }

    /// Delivers a finished child's event to its parent.
    fn notify_parent(&mut self, child_id: &JobId) -> Result<(), CoreError> {
        let child = self.store.require_job(child_id)?;
        let parent = self.parent_of(&child)?;
        let parent_job = self.store.require_job(&parent)?;
        match child.kind {
            JobKind::Verify => {
                if child.state == JobState::Done && parent_job.state == JobState::Reviewing {
                    match l3_verify::on_verify_done(&self.store, &child) {
                        Ok(event) => self.apply_simple(&parent, event, None)?,
                        Err(error) => {
                            self.deps.log.log(
                                Level::Warn,
                                "l3",
                                &format!("verify job {} has no usable verdict: {error}", child.id),
                            );
                            self.retry_or_park_verify(&parent_job, &error)?;
                        }
                    }
                } else if child.state == JobState::Failed {
                    self.apply_with(
                        &parent,
                        JobEvent::VerifierFailed,
                        None,
                        Vec::new(),
                        Some(format!("verifier could not produce a verdict for {parent}")),
                    )?;
                }
            }
            JobKind::Fix | JobKind::Merge => {
                if parent_job.state != JobState::Fixing {
                    return Ok(());
                }
                if child.state == JobState::Done {
                    self.apply_simple(&parent, JobEvent::FixDone { kind: child.kind }, None)?;
                } else if child.state == JobState::Failed {
                    self.apply_simple(&parent, JobEvent::FixFailed, None)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Returns the parent (fixes/verifies destination) of a child job.
    fn parent_of(&self, child: &Job) -> Result<JobId, CoreError> {
        for edge in self.store.edges_of(&child.id)? {
            if edge.src != child.id {
                continue;
            }
            if matches!(
                edge.kind,
                vowp_protocol::EdgeKind::Fixes | vowp_protocol::EdgeKind::Verifies
            ) {
                return Ok(edge.dst);
            }
        }
        Err(CoreError::Invalid(format!("job {} has no parent edge", child.id)))
    }

    /// Starts one run and records its dispatch and worker slot.
    pub(crate) fn start_run(
        &mut self,
        job: &Job,
        purpose: RunPurpose,
        choice: &DispatchChoice,
        findings: Vec<String>,
    ) -> Result<RunId, CoreError> {
        let repo = super::main_repo(&self.deps, &self.store, &self.project)?;
        let job = dispatch::ensure_worktree(&self.deps, &self.store, job, &repo)?;
        // A continue/restart replaces the previous process on the same session; join it
        // first so `SessionLog`'s lock is free.
        if let Some(previous) = self.store.last_run_of_job(&job.id)?
            && previous.ended_at.is_none()
            && self.runners.is_active(&previous.id)
        {
            self.runners.cancel_and_wait(&previous.id);
        }
        let worktree = super::job_worktree(&job)?;
        dispatch::write_brief(&self.deps, &job, &worktree)?;
        if super::latest_session(&self.store, &job.id)?.is_none() {
            let role = match job.role {
                vowp_protocol::Role::Verifier => SessionRole::Verifier,
                vowp_protocol::Role::Planner => SessionRole::Planner,
                vowp_protocol::Role::Keeper => SessionRole::Keeper,
                vowp_protocol::Role::Worker => SessionRole::Worker,
            };
            self.sessions.create_session(NewSession {
                project: Some(job.project.clone()),
                job_id: Some(job.id.clone()),
                role,
                title: job.id.as_str().to_owned(),
                cwd: worktree.clone(),
                parent: None,
                executor: Some(choice.executor.clone()),
            })?;
        }
        self.ensure_job_window(&job, &repo)?;

        let prompt = match job.kind {
            JobKind::Verify => {
                let original_id = l3_verify::verify_parent(&self.store, &job)?;
                let original = self.store.require_job(&original_id)?;
                let evidence = self.store.gate_results_for(&original.id, original.attempts)?;
                let diff = self.diff_summary(&original);
                let brief = match &original.worktree {
                    Some(worktree) => PathBuf::from(worktree).join(&original.brief),
                    None => PathBuf::from(&original.brief),
                };
                l3_verify::verify_prompt(&job, &brief.display().to_string(), &evidence, &diff)
            }
            JobKind::Fix | JobKind::Merge => {
                let findings = if findings.is_empty() {
                    job.note
                        .lines()
                        .filter(|line| !line.trim().is_empty())
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                } else {
                    findings
                };
                super::l2_worker::fix_prompt(&job, &findings)
            }
            _ => match purpose {
                RunPurpose::Continue => super::l2_worker::continue_prompt(&job),
                RunPurpose::Fix => super::l2_worker::fix_prompt(&job, &findings),
                _ => {
                    let brief = super::brief_path(&job)?.display().to_string();
                    super::l2_worker::start_prompt(&job, &brief)
                }
            },
        };
        let run_id = self.runners.spawn_job(
            job.clone(),
            choice.clone(),
            purpose,
            Some(prompt),
            self.deps.clone(),
            self.wake.clone(),
        )?;
        self.assign_worker(&job, &run_id)?;
        if let Ok(run) = self.store.require_run(&run_id) {
            self.ledger.record_dispatch(&self.store, &job, &run)?;
        }
        Ok(run_id)
    }

    /// Creates the job window when the session exists and the window does not.
    fn ensure_job_window(&self, job: &Job, repo: &std::path::Path) -> Result<(), CoreError> {
        let session = vowp_tmux::session_name_for_project(self.project.as_str());
        self.deps.windows.ensure_session(&session)?;
        let window = super::window_name(&job.id);
        if self.deps.windows.window_exists(&session, &window)? {
            return Ok(());
        }
        let spec = vowp_tmux::WindowSpec {
            argv: vec![
                "vowp".to_owned(),
                "__window".to_owned(),
                "job".to_owned(),
                job.id.as_str().to_owned(),
            ],
            cwd: Some(repo.to_path_buf()),
            env: Vec::new(),
        };
        match self.deps.windows.new_window(&session, &window, &spec) {
            Ok(()) | Err(vowp_tmux::TmuxError::WindowExists { .. }) => Ok(()),
            Err(error) => Err(CoreError::Tmux(error)),
        }
    }

    /// Marks one idle worker/verifier slot busy for the run.
    fn assign_worker(&self, job: &Job, run_id: &RunId) -> Result<(), CoreError> {
        let kind = if job.role == vowp_protocol::Role::Verifier {
            WorkerKind::Verifier
        } else {
            WorkerKind::Worker
        };
        let Some(worker) = self
            .store
            .list_workers(&self.deps.node)?
            .into_iter()
            .find(|worker| worker.kind == kind && worker.state == WorkerState::Idle)
        else {
            return Ok(());
        };
        self.store.upsert_worker(&Worker {
            node: self.deps.node.clone(),
            slot: worker.slot,
            kind,
            state: WorkerState::Busy,
            run_id: Some(run_id.clone()),
            job_id: Some(job.id.clone()),
            window: Some(super::window_name(&job.id)),
            updated_at: self.deps.clock.now(),
        })?;
        Ok(())
    }

    /// Frees the worker slot that held `run`.
    pub(crate) fn release_worker(&self, run: &RunId) -> Result<(), CoreError> {
        for worker in self.store.list_workers(&self.deps.node)? {
            if worker.run_id.as_ref() == Some(run) {
                self.store.upsert_worker(&Worker {
                    node: self.deps.node.clone(),
                    slot: worker.slot,
                    kind: worker.kind,
                    state: WorkerState::Idle,
                    run_id: None,
                    job_id: None,
                    window: None,
                    updated_at: self.deps.clock.now(),
                })?;
            }
        }
        Ok(())
    }

    /// Picks the first non-limited profile rung for a job.
    pub(crate) fn pick_choice(&self, job: &Job) -> Result<DispatchChoice, CoreError> {
        let profile = self
            .deps
            .config
            .profiles
            .get(&job.profile)
            .ok_or_else(|| CoreError::NotFound(format!("profile {}", job.profile)))?;
        let limited = super::limited_map(&self.store, self.deps.clock.now())?
            .into_iter()
            .collect::<HashMap<_, _>>();
        let (rung, executor) = profile
            .pick(&limited, self.deps.clock.now())
            .ok_or_else(|| CoreError::Invalid(format!("no available executor for {}", job.id)))?;
        Ok(DispatchChoice {
            job_id: job.id.clone(),
            executor: executor.clone(),
            rung: u32::try_from(rung).unwrap_or(u32::MAX),
            provider: executor.provider(),
            node: profile.node.clone(),
        })
    }

    /// Returns the newest verify job for the original's current attempt.
    fn find_verify_job(&self, original: &Job) -> Result<Option<Job>, CoreError> {
        let jobs = self.store.list_jobs(&JobFilter {
            project: Some(self.project.clone()),
            kind: Some(JobKind::Verify),
            ..JobFilter::default()
        })?;
        let suffix = format!("-verify-{}", original.attempts);
        let retry_suffix = format!("{suffix}-r1");
        let mut matches = Vec::new();
        for job in jobs {
            if !job.id.as_str().ends_with(&suffix) && !job.id.as_str().ends_with(&retry_suffix) {
                continue;
            }
            for edge in self.store.edges_of(&job.id)? {
                if edge.kind == vowp_protocol::EdgeKind::Verifies && edge.dst == original.id {
                    matches.push(job.clone());
                    break;
                }
            }
        }
        matches.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.as_str().cmp(right.id.as_str()))
        });
        Ok(matches.pop())
    }

    /// Builds the graph and the job list for one project.
    pub(crate) fn graph(&self) -> Result<(Graph, Vec<Job>), CoreError> {
        let jobs = self.store.list_jobs(&JobFilter {
            project: Some(self.project.clone()),
            ..JobFilter::default()
        })?;
        let edges = self.store.list_edges(Some(&self.project))?;
        let graph = Graph::new(jobs.clone(), edges)?;
        Ok((graph, jobs))
    }

    /// Returns the run ids of live runner threads.
    pub(crate) fn active_run_ids(&self) -> HashSet<RunId> {
        self.runners
            .jobs()
            .into_iter()
            .filter(|(run, _)| self.runners.is_active(run))
            .map(|(run, _)| run)
            .collect()
    }

    /// Returns true when `name` is missing or active (a paused row skips its step).
    pub(crate) fn loop_active(&self, name: &str) -> bool {
        self.store
            .loop_(name)
            .ok()
            .flatten()
            .is_none_or(|loop_| loop_.state == ActiveState::Active)
    }

    /// Returns the L1 bounds used by this tick.
    fn bounds(&self) -> LoopBounds {
        self.store
            .loop_("l1-supervisor")
            .ok()
            .flatten()
            .map(|loop_| loop_.bounds)
            .unwrap_or_default()
    }

    /// Returns the transition counters from the L1 bounds.
    pub(crate) fn transition_bounds(&self) -> TransitionBounds {
        let bounds = self.bounds();
        TransitionBounds {
            max_continues: bounds.max_continues.unwrap_or(2),
            max_hang_restarts: bounds.max_hang_restarts.unwrap_or(3),
        }
    }

    /// Returns a one-line diff summary for the verifier prompt.
    fn diff_summary(&self, job: &Job) -> String {
        let Ok(repo) = super::main_repo(&self.deps, &self.store, &self.project) else {
            return "diff unavailable".to_owned();
        };
        let Some(branch) = &job.branch else {
            return "diff unavailable (no branch)".to_owned();
        };
        match self
            .deps
            .git
            .diff_names(&repo, super::default_branch(&self.deps), branch)
        {
            Ok(files) => format!(
                "`git diff {}...{branch}` ({} file(s): {})",
                super::default_branch(&self.deps),
                files.len(),
                files.join(", ")
            ),
            Err(error) => format!("diff unavailable: {error}"),
        }
    }

    /// Writes a file under the node root atomically (temp file then rename).
    fn atomic_write(&self, name: &str, text: &str) -> Result<(), CoreError> {
        let root = self.deps.paths.root();
        std::fs::create_dir_all(root).map_err(|source| CoreError::Io {
            path: root.display().to_string(),
            source,
        })?;
        let tmp = root.join(format!(".{name}.tmp"));
        let path = root.join(name);
        std::fs::write(&tmp, text).map_err(|source| CoreError::Io {
            path: tmp.display().to_string(),
            source,
        })?;
        std::fs::rename(&tmp, &path).map_err(|source| CoreError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Ok(())
    }

    /// Returns gate results for a job's current attempt.
    pub fn gate_results(&self, job: &JobId) -> Result<Vec<GateResult>, CoreError> {
        let job = self.store.require_job(job)?;
        Ok(self.store.gate_results_for(&job.id, job.attempts)?)
    }
}

/// Renders the board snapshot as the JSON convenience copy written to `<node>/board.json`.
fn board_json(snapshot: &BoardSnapshot) -> Result<String, CoreError> {
    let running = snapshot
        .running
        .iter()
        .map(|run| {
            serde_json::json!({
                "job": run.job,
                "executor": run.executor.as_ref().map(ToString::to_string),
                "age_secs": run.age_secs,
                "last_event_age_secs": run.last_event_age_secs,
            })
        })
        .collect::<Vec<_>>();
    let value = serde_json::json!({
        "project": snapshot.project,
        "at": snapshot.at.as_rfc3339(),
        "jobs_by_state": snapshot.jobs_by_state,
        "running": running,
        "slots_busy": snapshot.slots_busy,
        "slots_total": snapshot.slots_total,
        "planner_executor": snapshot.planner_executor.as_ref().map(ToString::to_string),
        "keeper_executor": snapshot.keeper_executor.as_ref().map(ToString::to_string),
        "limits": snapshot.limits,
        "cost_today": snapshot.cost_today,
        "inbox_open": snapshot.inbox_open,
        "last_checkpoint": snapshot.last_checkpoint.map(|ts| ts.as_rfc3339()),
    });
    serde_json::to_string_pretty(&value).map_err(|error| CoreError::Invalid(error.to_string()))
}

/// Renders a panic payload as text.
fn panic_text(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "unknown panic".to_owned()
    }
}

/// Returns the snake-case request name used in logs and ledger details.
fn request_name(request: &crate::requests::Request) -> &'static str {
    use crate::requests::Request;
    match request {
        Request::ResolveJob { .. } => "resolve_job",
        Request::RunLoop { .. } => "run_loop",
        Request::RetryJob { .. } => "retry_job",
        Request::ParkJob { .. } => "park_job",
        Request::CancelJob { .. } => "cancel_job",
        Request::NudgeJob { .. } => "nudge_job",
        Request::RestartJob { .. } => "restart_job",
        Request::ReplanJob { .. } => "replan_job",
        Request::FailJob { .. } => "fail_job",
        Request::AnswerInbox { .. } => "answer_inbox",
        Request::PauseLoop { .. } => "pause_loop",
        Request::ResumeLoop { .. } => "resume_loop",
        Request::Wake => "wake",
    }
}
