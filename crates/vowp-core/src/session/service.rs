//! The session service: create a session, run one turn, write a capsule, switch executors.
//! Spec: `docs/design/crates/vowp-core.md` §3 and `docs/design/sessions.md` §1–5.
//!
//! `run_turn` owns the event framing an adapter must not emit: `session.executor` attach,
//! `turn.started` and `user.message` before the harness events, and `session.executor` detach
//! after the run's `turn.ended`. Capsules and seeds live in the sibling modules; this file
//! wires them into the service and keeps the store's index rows in step with the log.

use std::path::Path;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use vowp_protocol::{
    Actor, AttachAction, CancelToken, Capsule, CapsuleEvent, CapsuleTrigger, CostEvent, ErrorEvent, ErrorKind,
    EventBody, EventEnvelope, EventSource, Executor, ExecutorRef, HarnessKind, Limit, LimitEvent, LimitRowKind,
    NewSession, NodeName, Run, RunOutcome, ServiceError, Session, SessionExecutor, SessionId, SessionOps, SessionState,
    Switch, SwitchEvent, SwitchId, SwitchRequest, SwitchState, Timestamp, TurnEnded, TurnRequest, TurnResult,
    TurnStarted, UserMessage,
};
use vowp_store::session_files::{AdapterMapping, add_adapter_mapping, write_capsule_file};
use vowp_store::session_log::{AppendedEvent, NewEvent, SessionLog, index_row};
use vowp_store::{RunFinish, Store, StoreError};

use crate::deps::Deps;
use crate::error::CoreError;
use crate::session::capsule::CapsuleWriter;
use crate::session::seed::seed_prompt;

/// Poll interval for adapter events; cancellation is checked at this cadence.
const EVENT_POLL: Duration = Duration::from_millis(200);

/// Safety margin added once when a provider limit is written to `limits`.
const LIMIT_MARGIN: Duration = Duration::from_secs(120);

/// Startup recovery counters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    /// `started` switch rows finished as completed.
    pub switches_completed: u32,
    /// `started` switch rows finished as aborted.
    pub switches_aborted: u32,
    /// Dangling runs closed as crashed.
    pub runs_closed: u32,
    /// Session locks found and cleared (always zero: locks are files released with the process).
    pub sessions_unlocked: u32,
}

/// The session service. Cloning shares the injected [`Deps`].
#[derive(Clone)]
pub struct SessionService {
    deps: Deps,
}

impl SessionService {
    /// Creates a service over `deps`.
    pub fn new(deps: Deps) -> Self {
        Self { deps }
    }

    /// Returns the injected dependencies.
    pub fn deps(&self) -> &Deps {
        &self.deps
    }

    /// Creates a session: index row, log directory and the `session.created` event.
    pub fn create_session(&self, req: NewSession) -> Result<SessionId, CoreError> {
        let store = self.deps.open_store()?;
        let id = SessionId::new();
        let now = self.deps.clock.now();
        let session = Session {
            id: id.clone(),
            project: req.project.clone(),
            job_id: req.job_id.clone(),
            role: Some(req.role),
            title: req.title.clone(),
            parent_session_id: req.parent.clone(),
            node: self.deps.node.clone(),
            cwd: req.cwd.to_string_lossy().into_owned(),
            state: SessionState::Idle,
            executor: req.executor.clone(),
            last_seq: 0,
            last_capsule_id: None,
            log_bytes: 0,
            created_at: now,
            updated_at: now,
        };
        store.upsert_session(&session)?;

        let log = SessionLog::create(&self.deps.paths, &id, self.deps.clock.clone())?;
        let body = EventBody::SessionCreated(vowp_protocol::SessionCreated {
            title: req.title,
            cwd: session.cwd.clone(),
            project: req.project,
            job_id: req.job_id,
            role: Some(req.role),
            parent_session_id: req.parent,
        });
        self.append(&store, &log, HarnessKind::Vowp, body, None, None)?;
        log.sync()?;
        Ok(id)
    }

    /// Runs one turn, blocking until the adapter finishes.
    ///
    /// Framing, event forwarding, run/limits/ledger rows and the native-session mapping are
    /// exactly the numbered algorithm in `crates/vowp-core.md` §3.
    pub fn run_turn(&self, req: TurnRequest, cancel: &CancelToken) -> Result<TurnResult, CoreError> {
        let store = self.deps.open_store()?;
        let session = store.require_session(&req.session_id)?;
        self.check_cwd(&session.cwd)?;
        if cancel.is_cancelled() {
            return Err(CoreError::Cancelled);
        }
        let executor = req.executor.executor.clone();
        let node = req.executor.node.clone().unwrap_or_else(|| self.deps.node.clone());
        let adapter = self.deps.adapters.get(&executor).ok_or_else(|| {
            CoreError::Adapter(vowp_protocol::AdapterError::Unavailable(
                "harness not installed".to_owned(),
            ))
        })?;

        let log = SessionLog::open(&self.deps.paths, &req.session_id, self.deps.clock.clone())?;
        let prior_events = SessionLog::open_read(&self.deps.paths, &req.session_id)?.read_all()?;
        let turn = prior_events
            .iter()
            .filter(|event| event.event_type == vowp_protocol::EventType::TurnStarted)
            .count()
            .saturating_add(1) as u32;

        let native = if adapter.capabilities().can_resume_native {
            store.native_for(&req.session_id, executor.harness)?
        } else {
            None
        };
        let prompt = match (&native, store.latest_capsule(&req.session_id)?) {
            (None, Some(capsule)) => {
                let tail = tail_of_complete_turns(&prior_events, self.deps.config.capsule.tail_turns);
                seed_prompt(&capsule, &tail, self.deps.config.capsule.tail_turns, &req.prompt)
            }
            _ => req.prompt.clone(),
        };
        let handoff = native.is_none() && store.latest_capsule(&req.session_id)?.is_some();

        let run_id = vowp_protocol::RunId::new();
        let run = Run {
            id: run_id.clone(),
            job_id: req.job_id.clone().or_else(|| session.job_id.clone()),
            session_id: Some(req.session_id.clone()),
            role: req.role,
            purpose: req.purpose,
            profile: req.profile.clone(),
            rung: req.rung,
            executor: Some(executor.clone()),
            provider: Some(executor.provider()),
            node: node.clone(),
            native_session_id: native.clone(),
            pid: None,
            window: req.window.clone(),
            cwd: Some(session.cwd.clone()),
            started_at: self.deps.clock.now(),
            ended_at: None,
            exit_code: None,
            outcome: None,
            limit_kind: None,
            cost: vowp_protocol::Cost::unknown(),
            detail: String::new(),
        };
        store.insert_run(&run)?;

        let attach = SessionExecutor {
            action: AttachAction::Attach,
            executor: executor.clone(),
            node: node.clone(),
            native_session_id: native.clone(),
            run_id: Some(run_id.clone()),
            reason: None,
        };
        self.append(
            &store,
            &log,
            executor.harness,
            EventBody::SessionExecutor(attach),
            Some(node.clone()),
            native.clone(),
        )?;
        self.append(
            &store,
            &log,
            executor.harness,
            EventBody::TurnStarted(TurnStarted { turn }),
            Some(node.clone()),
            native.clone(),
        )?;
        self.append(
            &store,
            &log,
            executor.harness,
            EventBody::UserMessage(UserMessage {
                text: prompt.clone(),
                attachments: Vec::new(),
                note: handoff.then(|| "handoff".to_owned()),
            }),
            Some(node.clone()),
            native.clone(),
        )?;
        store.set_session_state(&req.session_id, SessionState::Running, Some(&executor))?;

        let start_input = vowp_protocol::StartInput {
            session_id: req.session_id.clone(),
            executor: executor.clone(),
            cwd: Path::new(&session.cwd).to_path_buf(),
            prompt,
            resume: native.clone(),
            new_native_id: None,
            env: self.start_env(&executor),
            permission_mode: self
                .harness_config(&executor)
                .and_then(|config| config.permission_mode.clone()),
            system_note: None,
            event_log_hint: Some(self.deps.paths.run_raw_path(&run_id)),
            timeout: req.timeout,
        };
        let mut adapter_run = match adapter.start(start_input) {
            Ok(run) => run,
            Err(error) => {
                self.finish_failed_start(&store, &log, &req, &run_id, turn, &executor, &node, &error)?;
                return Err(CoreError::Adapter(error));
            }
        };
        store.update_run_attach(&run_id, adapter_run.pid(), None, req.window.as_deref(), None)?;

        let mut emitted_cost = false;
        let mut emitted_limit = false;
        let mut emitted_error = false;
        let mut terminal: Option<vowp_protocol::HarnessEvent> = None;
        let mut interrupted = false;
        loop {
            match adapter_run.events().recv_timeout(EVENT_POLL) {
                Ok(event) => {
                    match &event.body {
                        EventBody::Cost(_) => emitted_cost = true,
                        EventBody::Limit(_) => emitted_limit = true,
                        EventBody::Error(_) => emitted_error = true,
                        EventBody::TurnEnded(_) => {
                            terminal = Some(event);
                            break;
                        }
                        _ => {}
                    }
                    self.append_harness(&store, &log, event, &executor, &node, native.as_deref())?;
                }
                Err(RecvTimeoutError::Timeout) => {
                    if cancel.is_cancelled() && !interrupted {
                        interrupted = true;
                        let _ = adapter_run.interrupt();
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }

        let summary = match adapter_run.wait() {
            Ok(summary) => summary,
            Err(error) => {
                let finish = RunFinish {
                    outcome: RunOutcome::Error,
                    exit_code: None,
                    limit_kind: None,
                    cost: vowp_protocol::Cost::unknown(),
                    native_session_id: None,
                    detail: error.to_string(),
                };
                let _ = store.finish_run(&run_id, &finish);
                let _ = store.set_session_state(&req.session_id, SessionState::Idle, Some(&executor));
                return Err(CoreError::Adapter(error));
            }
        };

        if summary.cost.usd.is_some() && !emitted_cost {
            self.append(
                &store,
                &log,
                executor.harness,
                EventBody::Cost(CostEvent {
                    cost: summary.cost.clone(),
                    run_id: Some(run_id.clone()),
                }),
                Some(node.clone()),
                summary.native_session_id.clone(),
            )?;
        }
        if let Some(limit) = &summary.limit {
            if !emitted_limit {
                self.append(
                    &store,
                    &log,
                    executor.harness,
                    EventBody::Limit(limit.clone()),
                    Some(node.clone()),
                    summary.native_session_id.clone(),
                )?;
            }
        } else if !emitted_error && matches!(summary.outcome, RunOutcome::Error | RunOutcome::Crash) {
            self.append(
                &store,
                &log,
                executor.harness,
                EventBody::Error(ErrorEvent {
                    kind: ErrorKind::AdapterFailed,
                    message: summary
                        .result_text
                        .clone()
                        .unwrap_or_else(|| "adapter ended without a result".to_owned()),
                    retryable: summary.outcome == RunOutcome::Crash,
                }),
                Some(node.clone()),
                summary.native_session_id.clone(),
            )?;
        }

        let duration_ms = terminal.as_ref().and_then(|event| match &event.body {
            EventBody::TurnEnded(ended) => ended.duration_ms,
            _ => None,
        });
        self.append(
            &store,
            &log,
            executor.harness,
            EventBody::TurnEnded(TurnEnded {
                turn,
                stop_reason: summary.stop_reason,
                duration_ms,
            }),
            Some(node.clone()),
            summary.native_session_id.clone(),
        )?;
        let detach_reason = match summary.limit {
            Some(_) => "limit",
            None if summary.outcome == RunOutcome::Crash => "crash",
            None => "turn_done",
        };
        self.append(
            &store,
            &log,
            executor.harness,
            EventBody::SessionExecutor(SessionExecutor {
                action: AttachAction::Detach,
                executor: executor.clone(),
                node: node.clone(),
                native_session_id: summary.native_session_id.clone(),
                run_id: Some(run_id.clone()),
                reason: Some(detach_reason.to_owned()),
            }),
            Some(node.clone()),
            summary.native_session_id.clone(),
        )?;
        log.sync()?;

        if let Some(native) = &summary.native_session_id {
            store.upsert_native(&req.session_id, executor.harness, native, &node, log.last_seq())?;
            add_adapter_mapping(
                &self.deps.paths,
                &req.session_id,
                AdapterMapping {
                    harness: executor.harness,
                    native_session_id: native.clone(),
                    node: node.clone(),
                    first_seq: log.last_seq(),
                    created_at: self.deps.clock.now(),
                },
            )?;
        }

        let state = if matches!(
            summary.outcome,
            RunOutcome::Limit | RunOutcome::Credits | RunOutcome::Auth | RunOutcome::Rate | RunOutcome::Context
        ) {
            SessionState::Blocked
        } else {
            SessionState::Idle
        };
        store.set_session_state(&req.session_id, state, Some(&executor))?;

        let finish = RunFinish {
            outcome: summary.outcome,
            exit_code: summary.exit_code,
            limit_kind: summary.limit.as_ref().map(|limit| limit.kind),
            cost: summary.cost.clone(),
            native_session_id: summary.native_session_id.clone(),
            detail: summary
                .limit
                .as_ref()
                .map(|limit| limit.message.clone())
                .or_else(|| summary.result_text.clone())
                .unwrap_or_default(),
        };
        store.finish_run(&run_id, &finish)?;

        if let Some(limit) = &summary.limit {
            self.record_limit(&store, limit, executor.harness)?;
        } else if summary.outcome == RunOutcome::Ok {
            store.clear_limit(&executor.provider())?;
        }
        self.record_cost_ledger(&store, &run, &summary)?;

        Ok(TurnResult { run_id, summary })
    }

    /// Writes a capsule through [`CapsuleWriter`].
    pub fn write_capsule(&self, session: &SessionId, trigger: CapsuleTrigger) -> Result<Capsule, CoreError> {
        CapsuleWriter::new(self.deps.clone()).write(session, trigger)
    }

    /// Startup recovery: finish or abort switches left in `started`, close dangling runs.
    pub fn recover(&self, store: &Store) -> Result<RecoveryReport, CoreError> {
        let mut report = RecoveryReport::default();
        for switch in store.open_switches()? {
            match self.recover_switch(store, &switch)? {
                true => report.switches_completed += 1,
                false => report.switches_aborted += 1,
            }
        }
        for run in store.open_runs()? {
            let finish = RunFinish {
                outcome: RunOutcome::Crash,
                exit_code: None,
                limit_kind: None,
                cost: vowp_protocol::Cost::unknown(),
                native_session_id: None,
                detail: "orphaned by supervisor restart".to_owned(),
            };
            store.finish_run(&run.id, &finish)?;
            report.runs_closed += 1;
        }
        Ok(report)
    }

    /// Switches a session to another executor (a pure switch: the seed is delivered next turn).
    pub fn switch(&self, req: &SwitchRequest) -> Result<Switch, CoreError> {
        let store = self.deps.open_store()?;
        let session = store.require_session(&req.session_id)?;
        if session.state == SessionState::Switching {
            return Err(CoreError::Store(StoreError::Conflict(
                "session is already switching".to_owned(),
            )));
        }
        let previous_state = session.state;
        let previous_executor = session.executor.clone();
        let at_seq = SessionLog::open_read(&self.deps.paths, &req.session_id)?.last_seq()?;
        store.set_session_state(&req.session_id, SessionState::Switching, previous_executor.as_ref())?;

        let switch = Switch {
            id: SwitchId::new(),
            session_id: req.session_id.clone(),
            from_executor: previous_executor.clone(),
            to_executor: req.to.executor.clone(),
            from_node: previous_executor.as_ref().map(|_| session.node.clone()),
            to_node: req.to.node.clone().unwrap_or_else(|| self.deps.node.clone()),
            capsule_id: None,
            reason: req.reason,
            state: SwitchState::Started,
            error: None,
            started_at: self.deps.clock.now(),
            ended_at: None,
            at_seq,
        };
        store.insert_switch(&switch)?;

        let target = self.deps.adapters.get(&req.to.executor);
        let probe_ok = target.as_ref().is_some_and(|adapter| adapter.probe().ok);
        if !probe_ok {
            return self.abort_switch(&store, switch, previous_state, previous_executor, "target not runnable");
        }

        let capsule_id = match self.write_capsule(&req.session_id, CapsuleTrigger::Switch) {
            Ok(capsule) => Some(capsule.capsule_id),
            Err(error) => match store.latest_capsule(&req.session_id)? {
                Some(capsule) => Some(capsule.capsule_id),
                None => {
                    return self.abort_switch(&store, switch, previous_state, previous_executor, &error.to_string());
                }
            },
        };

        let log = SessionLog::open(&self.deps.paths, &req.session_id, self.deps.clock.clone())?;
        if let Some(executor) = &previous_executor {
            self.append(
                &store,
                &log,
                executor.harness,
                EventBody::SessionExecutor(SessionExecutor {
                    action: AttachAction::Detach,
                    executor: executor.clone(),
                    node: session.node.clone(),
                    native_session_id: None,
                    run_id: None,
                    reason: Some("switch".to_owned()),
                }),
                Some(session.node.clone()),
                None,
            )?;
        }
        store.set_session_state(&req.session_id, SessionState::Idle, None)?;
        self.append(
            &store,
            &log,
            req.to.executor.harness,
            EventBody::SessionExecutor(SessionExecutor {
                action: AttachAction::Attach,
                executor: req.to.executor.clone(),
                node: switch.to_node.clone(),
                native_session_id: None,
                run_id: None,
                reason: None,
            }),
            Some(switch.to_node.clone()),
            None,
        )?;
        let mut completed = switch.clone();
        completed.capsule_id = capsule_id.clone();
        self.append(
            &store,
            &log,
            req.to.executor.harness,
            EventBody::Switch(SwitchEvent {
                switch_id: switch.id.clone(),
                from: previous_executor.map(|executor| ExecutorRef {
                    executor,
                    node: Some(session.node.clone()),
                }),
                to: req.to.clone(),
                capsule_id,
                reason: req.reason,
            }),
            Some(switch.to_node.clone()),
            None,
        )?;
        log.sync()?;
        store.set_session_state(&req.session_id, SessionState::Idle, Some(&req.to.executor))?;
        store.finish_switch(&switch.id, SwitchState::Completed, None)?;
        completed.state = SwitchState::Completed;
        completed.ended_at = Some(self.deps.clock.now());
        Ok(completed)
    }

    /// Aborts a switch before the target is attached.
    fn abort_switch(
        &self,
        store: &Store,
        switch: Switch,
        previous_state: SessionState,
        previous_executor: Option<Executor>,
        detail: &str,
    ) -> Result<Switch, CoreError> {
        let log = SessionLog::open(&self.deps.paths, &switch.session_id, self.deps.clock.clone())?;
        self.append(
            store,
            &log,
            HarnessKind::Vowp,
            EventBody::Error(ErrorEvent {
                kind: ErrorKind::AdapterUnavailable,
                message: detail.to_owned(),
                retryable: false,
            }),
            None,
            None,
        )?;
        log.sync()?;
        store.finish_switch(&switch.id, SwitchState::Aborted, Some(detail))?;
        store.set_session_state(&switch.session_id, previous_state, previous_executor.as_ref())?;
        let mut aborted = switch;
        aborted.state = SwitchState::Aborted;
        aborted.error = Some(detail.to_owned());
        aborted.ended_at = Some(self.deps.clock.now());
        Err(CoreError::SwitchAborted(detail.to_owned()))
    }

    /// Resolves one `started` switch after a restart; returns true when completed.
    fn recover_switch(&self, store: &Store, switch: &Switch) -> Result<bool, CoreError> {
        let events = SessionLog::open_read(&self.deps.paths, &switch.session_id)?.read_all()?;
        if events.iter().any(|event| {
            matches!(
                event.body(),
                Ok(EventBody::Switch(payload)) if payload.switch_id == switch.id
            )
        }) {
            store.finish_switch(&switch.id, SwitchState::Completed, None)?;
            return Ok(true);
        }
        let last_attach = events.iter().rev().find_map(|event| match event.body() {
            Ok(EventBody::SessionExecutor(payload)) if payload.action == AttachAction::Attach => Some(payload),
            _ => None,
        });
        if last_attach.is_some_and(|payload| payload.executor == switch.to_executor) {
            let log = SessionLog::open(&self.deps.paths, &switch.session_id, self.deps.clock.clone())?;
            self.append(
                store,
                &log,
                switch.to_executor.harness,
                EventBody::Switch(SwitchEvent {
                    switch_id: switch.id.clone(),
                    from: switch.from_executor.clone().map(|executor| ExecutorRef {
                        executor,
                        node: switch.from_node.clone(),
                    }),
                    to: ExecutorRef {
                        executor: switch.to_executor.clone(),
                        node: Some(switch.to_node.clone()),
                    },
                    capsule_id: switch.capsule_id.clone(),
                    reason: switch.reason,
                }),
                Some(switch.to_node.clone()),
                None,
            )?;
            log.sync()?;
            store.finish_switch(&switch.id, SwitchState::Completed, None)?;
            return Ok(true);
        }
        store.finish_switch(&switch.id, SwitchState::Aborted, Some("interrupted by restart"))?;
        let blocked = events.iter().rev().find_map(|event| match event.body() {
            Ok(EventBody::TurnEnded(payload)) if payload.stop_reason == vowp_protocol::StopReason::Limit => Some(true),
            _ => None,
        });
        let state = if blocked == Some(true) {
            SessionState::Blocked
        } else {
            SessionState::Idle
        };
        let executor = store.session(&switch.session_id)?.and_then(|session| session.executor);
        store.set_session_state(&switch.session_id, state, executor.as_ref())?;
        Ok(false)
    }

    /// Checks `cwd` against the project path, worktree root and `[node].cwd_allow`.
    fn check_cwd(&self, cwd: &str) -> Result<(), CoreError> {
        let path = Path::new(cwd);
        let mut roots: Vec<&str> = Vec::new();
        if let Some(project) = &self.deps.config.project.path {
            roots.push(project);
        }
        if let Some(root) = &self.deps.config.project.worktree_root {
            roots.push(root);
        }
        roots.extend(self.deps.config.node.cwd_allow.iter().map(String::as_str));
        if roots.is_empty() {
            return Ok(());
        }
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        for root in roots {
            let root_path = Path::new(root);
            let root_path = std::fs::canonicalize(root_path).unwrap_or_else(|_| root_path.to_path_buf());
            if path.starts_with(&root_path) {
                return Ok(());
            }
        }
        Err(CoreError::Refused(format!(
            "working directory {cwd} is outside the allow-list"
        )))
    }

    /// Returns the configured harness entry for `executor`.
    fn harness_config(&self, executor: &Executor) -> Option<&vowp_protocol::HarnessConfig> {
        let key = match executor.harness {
            HarnessKind::ClaudeCode => "claude-code",
            HarnessKind::Opencode => "opencode",
            HarnessKind::Cli => executor.model.as_str(),
            HarnessKind::Vowp => return None,
        };
        self.deps
            .config
            .harnesses
            .get(key)
            .or_else(|| self.deps.config.harnesses.get(&format!("cli:{key}")))
    }

    /// Resolves `[harnesses.*].env_pass` names from the process environment.
    fn start_env(&self, executor: &Executor) -> Vec<(String, String)> {
        let Some(config) = self.harness_config(executor) else {
            return Vec::new();
        };
        config
            .env_pass
            .iter()
            .filter_map(|name| std::env::var(name).ok().map(|value| (name.clone(), value)))
            .collect()
    }

    /// Appends a framing event and indexes it.
    fn append(
        &self,
        store: &Store,
        log: &SessionLog,
        harness: HarnessKind,
        body: EventBody,
        node: Option<NodeName>,
        native: Option<String>,
    ) -> Result<AppendedEvent, CoreError> {
        self.append_event(store, log, harness, body, None, node, native)
    }

    /// Appends one event with an optional harness-reported timestamp and raw value.
    #[allow(clippy::too_many_arguments)]
    fn append_event(
        &self,
        store: &Store,
        log: &SessionLog,
        harness: HarnessKind,
        body: EventBody,
        ts: Option<Timestamp>,
        node: Option<NodeName>,
        native: Option<String>,
    ) -> Result<AppendedEvent, CoreError> {
        let source = EventSource {
            harness,
            model: None,
            node,
            native_session_id: native,
        };
        let appended = log.append(NewEvent {
            actor: actor_for(&body),
            source,
            body,
            raw: None,
            ts,
        })?;
        store.index_event(&index_row(&appended))?;
        Ok(appended)
    }

    /// Appends one adapter event.
    fn append_harness(
        &self,
        store: &Store,
        log: &SessionLog,
        event: vowp_protocol::HarnessEvent,
        executor: &Executor,
        node: &NodeName,
        native: Option<&str>,
    ) -> Result<(), CoreError> {
        let source = EventSource {
            harness: executor.harness,
            model: Some(executor.model.clone()),
            node: Some(node.clone()),
            native_session_id: native.map(str::to_owned),
        };
        let appended = log.append(NewEvent {
            actor: event.actor,
            source,
            body: event.body,
            raw: event.raw,
            ts: event.ts,
        })?;
        store.index_event(&index_row(&appended))?;
        Ok(())
    }

    /// Records a provider limit with the single two-minute safety margin.
    fn record_limit(&self, store: &Store, event: &LimitEvent, harness: HarnessKind) -> Result<(), CoreError> {
        let resets_at = event.resets_at.map(|resets| resets.plus(LIMIT_MARGIN));
        store.upsert_limit(&Limit {
            provider: event.provider.clone(),
            harness: Some(harness),
            kind: limit_row_kind(event.kind),
            resets_at,
            detail: event.message.clone(),
            since: self.deps.clock.now(),
            last_probe_at: None,
            probe_fail_streak: 0,
            concurrency_cap: None,
            cap_until: None,
        })?;
        Ok(())
    }

    /// Appends the run's cost ledger row.
    fn record_cost_ledger(
        &self,
        store: &Store,
        run: &Run,
        summary: &vowp_protocol::RunSummary,
    ) -> Result<(), CoreError> {
        let mut detail = serde_json::json!({"billed": true, "basis": "receipt"});
        if run
            .executor
            .as_ref()
            .is_some_and(|executor| executor.harness == HarnessKind::ClaudeCode)
        {
            detail = serde_json::json!({"billed": false, "basis": "list"});
        }
        store.append_ledger(&vowp_protocol::LedgerEntry {
            id: None,
            ts: self.deps.clock.now(),
            kind: vowp_protocol::LedgerKind::Cost,
            project: None,
            job_id: run.job_id.clone(),
            run_id: Some(run.id.clone()),
            session_id: run.session_id.clone(),
            role: Some(run.role),
            provider: run.provider.clone(),
            harness: run.executor.as_ref().map(|executor| executor.harness),
            model: run.executor.as_ref().map(|executor| executor.model.clone()),
            cost: Some(summary.cost.clone()),
            summary: format!("run {} finished: {:?}", run.id, summary.outcome),
            detail,
        })?;
        Ok(())
    }

    /// Appends an error + terminal event for a run whose adapter could not start.
    #[allow(clippy::too_many_arguments)]
    fn finish_failed_start(
        &self,
        store: &Store,
        log: &SessionLog,
        req: &TurnRequest,
        run_id: &vowp_protocol::RunId,
        turn: u32,
        executor: &Executor,
        node: &NodeName,
        error: &vowp_protocol::AdapterError,
    ) -> Result<(), CoreError> {
        self.append(
            store,
            log,
            executor.harness,
            EventBody::Error(ErrorEvent {
                kind: ErrorKind::AdapterUnavailable,
                message: error.to_string(),
                retryable: false,
            }),
            Some(node.clone()),
            None,
        )?;
        self.append(
            store,
            log,
            executor.harness,
            EventBody::TurnEnded(TurnEnded {
                turn,
                stop_reason: vowp_protocol::StopReason::Error,
                duration_ms: None,
            }),
            Some(node.clone()),
            None,
        )?;
        self.append(
            store,
            log,
            executor.harness,
            EventBody::SessionExecutor(SessionExecutor {
                action: AttachAction::Detach,
                executor: executor.clone(),
                node: node.clone(),
                native_session_id: None,
                run_id: Some(run_id.clone()),
                reason: Some("crash".to_owned()),
            }),
            Some(node.clone()),
            None,
        )?;
        log.sync()?;
        store.set_session_state(&req.session_id, SessionState::Idle, Some(executor))?;
        store.finish_run(
            run_id,
            &RunFinish {
                outcome: RunOutcome::Error,
                exit_code: None,
                limit_kind: None,
                cost: vowp_protocol::Cost::unknown(),
                native_session_id: None,
                detail: error.to_string(),
            },
        )?;
        Ok(())
    }
}

impl SessionOps for SessionService {
    fn create_session(&self, req: NewSession) -> Result<SessionId, ServiceError> {
        SessionService::create_session(self, req).map_err(ServiceError::from)
    }

    fn run_turn(&self, req: TurnRequest, cancel: &CancelToken) -> Result<TurnResult, ServiceError> {
        SessionService::run_turn(self, req, cancel).map_err(ServiceError::from)
    }

    fn switch(&self, req: SwitchRequest) -> Result<Switch, ServiceError> {
        SessionService::switch(self, &req).map_err(ServiceError::from)
    }

    fn write_capsule(&self, session: &SessionId, trigger: CapsuleTrigger) -> Result<Capsule, ServiceError> {
        SessionService::write_capsule(self, session, trigger).map_err(ServiceError::from)
    }
}

/// Returns the canonical actor for a framing event.
fn actor_for(body: &EventBody) -> Actor {
    match body {
        EventBody::UserMessage(_) => Actor::User,
        EventBody::AssistantMessage(_)
        | EventBody::AssistantReasoning(_)
        | EventBody::ToolCall(_)
        | EventBody::ToolResult(_)
        | EventBody::FileChange(_)
        | EventBody::Plan(_) => Actor::Assistant,
        _ => Actor::System,
    }
}

/// Converts an event limit kind to its `limits` row kind.
fn limit_row_kind(kind: vowp_protocol::LimitKind) -> LimitRowKind {
    match kind {
        vowp_protocol::LimitKind::Quota => LimitRowKind::Quota,
        vowp_protocol::LimitKind::Credits => LimitRowKind::Credits,
        vowp_protocol::LimitKind::Rate => LimitRowKind::Rate,
        vowp_protocol::LimitKind::Context => LimitRowKind::Context,
        vowp_protocol::LimitKind::Auth => LimitRowKind::Auth,
    }
}

/// Selects the events of the last `turns` complete turns from `events`.
pub(crate) fn tail_of_complete_turns(events: &[EventEnvelope], turns: u32) -> Vec<EventEnvelope> {
    if turns == 0 {
        return Vec::new();
    }
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut start: Option<usize> = None;
    for (index, event) in events.iter().enumerate() {
        match event.body() {
            Ok(EventBody::TurnStarted(_)) => start = Some(index),
            Ok(EventBody::TurnEnded(_)) => {
                if let Some(begin) = start.take() {
                    ranges.push((begin, index));
                }
            }
            _ => {}
        }
    }
    if ranges.is_empty() {
        return if events.is_empty() { Vec::new() } else { events.to_vec() };
    }
    let first = ranges.len().saturating_sub(turns as usize);
    let (begin, _) = ranges[first];
    let (_, end) = ranges[ranges.len() - 1];
    events[begin..=end].to_vec()
}

/// Convenience used by the capsule writer: count turns and sum costs in one event slice.
pub(crate) fn telemetry(events: &[EventEnvelope]) -> vowp_protocol::CapsuleTelemetry {
    let mut telemetry = vowp_protocol::CapsuleTelemetry::default();
    let mut usd = None;
    for event in events {
        match event.body() {
            Ok(EventBody::TurnEnded(_)) => telemetry.turns = telemetry.turns.saturating_add(1),
            Ok(EventBody::Cost(payload)) => {
                telemetry.tokens_in = telemetry.tokens_in.saturating_add(payload.cost.tokens.input);
                telemetry.tokens_out = telemetry.tokens_out.saturating_add(payload.cost.tokens.output);
                if let Some(value) = payload.cost.usd {
                    usd = Some(usd.unwrap_or(0.0) + value);
                }
            }
            _ => {}
        }
    }
    telemetry.usd = usd;
    telemetry
}

/// Selects the last assistant message text from a session log.
pub(crate) fn last_assistant_text(
    paths: &vowp_store::NodePaths,
    session: &SessionId,
) -> Result<Option<String>, CoreError> {
    let events = SessionLog::open_read(paths, session)?.read_all()?;
    Ok(events.iter().rev().find_map(|event| match event.body() {
        Ok(EventBody::AssistantMessage(payload)) => Some(payload.text),
        _ => None,
    }))
}

/// Appends the `capsule` event to a summarised session and syncs the log.
pub(crate) fn append_capsule_event(deps: &Deps, store: &Store, capsule: &Capsule) -> Result<(), CoreError> {
    let log = SessionLog::open(&deps.paths, &capsule.session_id, deps.clock.clone())?;
    let appended = log.append(NewEvent {
        actor: Actor::System,
        source: EventSource {
            harness: HarnessKind::Vowp,
            model: None,
            node: Some(deps.node.clone()),
            native_session_id: None,
        },
        body: EventBody::Capsule(CapsuleEvent {
            capsule_id: capsule.capsule_id.clone(),
            version: capsule.version,
            trigger: capsule.trigger,
            at_seq: capsule.at_seq,
        }),
        raw: None,
        ts: None,
    })?;
    store.index_event(&index_row(&appended))?;
    log.sync()?;
    Ok(())
}

/// Writes the capsule file and inserts its row.
pub(crate) fn persist_capsule(deps: &Deps, store: &Store, capsule: &Capsule) -> Result<(), CoreError> {
    let path = write_capsule_file(&deps.paths, capsule)?;
    store.insert_capsule(capsule, &path.to_string_lossy())?;
    append_capsule_event(deps, store, capsule)
}

/// Builds the `CapsuleTrigger` wire name for prompts.
pub(crate) fn trigger_name(trigger: CapsuleTrigger) -> &'static str {
    match trigger {
        CapsuleTrigger::Switch => "switch",
        CapsuleTrigger::Manual => "manual",
        CapsuleTrigger::TurnEnd => "turn_end",
        CapsuleTrigger::Limit => "limit",
        CapsuleTrigger::Context => "context",
        CapsuleTrigger::Export => "export",
        CapsuleTrigger::JobEnd => "job_end",
    }
}
