//! Runner threads: one per active harness run.
//!
//! Spec: `docs/design/crates/vowp-core.md` §7 (`RunnerPool`, `WakeHandle`) and
//! `docs/design/loops.md` §1. A runner opens its own store connection through the session
//! service, writes the run row and its events, then wakes L1. L1 never joins a live runner;
//! it only reaps threads that have finished.

use std::collections::HashSet;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use vowp_graph::DispatchChoice;
use vowp_protocol::{
    CancelToken, ExecutorRef, Job, JobId, Level, RunId, RunPurpose, RunRole, SessionRole, TurnRequest,
};

use crate::CoreError;
use crate::deps::Deps;
use crate::session::service::SessionService;

use super::l2;

/// Time L1 is willing to wait for the session service to insert a run row.
const RUN_HANDOFF_TIMEOUT: Duration = Duration::from_secs(5);

/// Cloneable wake-up handle for L1: `wake()` sends `()` on an `mpsc::Sender<()>`.
#[derive(Clone)]
pub struct WakeHandle(Sender<()>);

impl WakeHandle {
    /// Wakes the supervisor loop; never blocks.
    pub fn wake(&self) {
        let _ = self.0.send(());
    }

    /// Creates the paired wake handle and receiver.
    pub fn channel() -> (WakeHandle, Receiver<()>) {
        let (sender, receiver) = channel();
        (WakeHandle(sender), receiver)
    }
}

/// One live runner thread.
struct RunnerEntry {
    run_id: RunId,
    job_id: Option<JobId>,
    cancel: CancelToken,
    join: Option<JoinHandle<()>>,
    finished: Arc<Mutex<bool>>,
}

/// Pool of runner threads, keyed by the run row the session service created.
#[derive(Default)]
pub struct RunnerPool {
    entries: Vec<RunnerEntry>,
}

impl RunnerPool {
    /// Creates an empty pool.
    pub fn new() -> Self {
        Self::default()
    }

    /// Spawns the L2 start run for `job` on `choice`.
    ///
    /// This is the compatibility entry point named in the crate doc; callers that need a
    /// fix/verify/continue prompt use [`RunnerPool::spawn_turn`].
    pub fn spawn(
        &mut self,
        job: Job,
        choice: DispatchChoice,
        deps: Deps,
        wake: WakeHandle,
    ) -> Result<RunId, CoreError> {
        let purpose = super::l2_worker::purpose_for(&job);
        self.spawn_job(job, choice, purpose, None, deps, wake)
    }

    /// Spawns one turn for `job` using `choice` and `purpose`.
    pub fn spawn_job(
        &mut self,
        job: Job,
        choice: DispatchChoice,
        purpose: RunPurpose,
        prompt_override: Option<String>,
        deps: Deps,
        wake: WakeHandle,
    ) -> Result<RunId, CoreError> {
        let worktree = super::job_worktree(&job)?;
        let store = deps.open_store()?;
        let session_id = match super::latest_session(&store, &job.id)? {
            Some(session) => session,
            None => {
                let role = match job.role {
                    vowp_protocol::Role::Verifier => SessionRole::Verifier,
                    vowp_protocol::Role::Planner => SessionRole::Planner,
                    vowp_protocol::Role::Keeper => SessionRole::Keeper,
                    vowp_protocol::Role::Worker => SessionRole::Worker,
                };
                let service = SessionService::new(deps.clone());
                service.create_session(vowp_protocol::NewSession {
                    project: Some(job.project.clone()),
                    job_id: Some(job.id.clone()),
                    role,
                    title: job.id.as_str().to_owned(),
                    cwd: worktree.clone(),
                    parent: None,
                    executor: Some(choice.executor.clone()),
                })?
            }
        };
        let prompt = match prompt_override {
            Some(prompt) => prompt,
            None => match purpose {
                RunPurpose::Continue => l2::continue_prompt(&job),
                RunPurpose::Fix => l2::fix_prompt(&job, &[]),
                _ => {
                    let brief = super::brief_path(&job)
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|_| job.brief.clone());
                    l2::start_prompt(&job, &brief)
                }
            },
        };
        let baseline = store
            .runs_of_job(&job.id)?
            .into_iter()
            .map(|run| run.id)
            .collect::<HashSet<_>>();
        let cancel = CancelToken::default();
        let finished = Arc::new(Mutex::new(false));

        let thread_job = job.clone();
        let thread_choice = choice.clone();
        let thread_cancel = cancel.clone();
        let thread_finished = Arc::clone(&finished);
        let thread_wake = wake.clone();
        let thread_session = session_id.clone();
        let thread_deps = deps.clone();
        let thread_prompt = prompt.clone();

        let join = thread::spawn(move || {
            let service = SessionService::new(thread_deps.clone());
            let role = match thread_job.role {
                vowp_protocol::Role::Verifier => RunRole::Verifier,
                vowp_protocol::Role::Planner => RunRole::Planner,
                vowp_protocol::Role::Keeper => RunRole::Keeper,
                vowp_protocol::Role::Worker => RunRole::Worker,
            };
            let request = TurnRequest {
                session_id: thread_session,
                executor: ExecutorRef {
                    executor: thread_choice.executor.clone(),
                    node: thread_choice.node.clone(),
                },
                purpose: thread_purpose(&thread_job, purpose),
                role,
                job_id: Some(thread_job.id.clone()),
                prompt: thread_prompt,
                profile: Some(thread_job.profile.clone()),
                rung: thread_choice.rung,
                window: Some(super::window_name(&thread_job.id)),
                timeout: None,
            };
            match service.run_turn(request, &thread_cancel) {
                Ok(result) => {
                    thread_deps.log.log(
                        Level::Info,
                        "l2",
                        &format!("run {} finished: {:?}", result.run_id, result.summary.outcome),
                    );
                }
                Err(error) => {
                    thread_deps.log.log(
                        Level::Error,
                        "l2",
                        &format!("run for {} failed to start: {error}", thread_job.id),
                    );
                }
            }
            if let Ok(mut done) = thread_finished.lock() {
                *done = true;
            }
            thread_wake.wake();
        });

        let run_id = wait_for_run(&deps, &job.id, &baseline, &finished, &join)?;
        self.entries.push(RunnerEntry {
            run_id: run_id.clone(),
            job_id: Some(job.id),
            cancel,
            join: Some(join),
            finished,
        });
        Ok(run_id)
    }

    /// Cancels one run's thread, if it is still tracked.
    pub fn cancel(&self, run: &RunId) {
        if let Some(entry) = self.entries.iter().find(|entry| &entry.run_id == run) {
            entry.cancel.cancel();
        }
    }

    /// Cancels every tracked run.
    pub fn cancel_all(&self) {
        for entry in &self.entries {
            entry.cancel.cancel();
        }
    }

    /// Cancels one run and joins its thread, so the session log lock is released before a
    /// follow-up run on the same session starts (T6/T8/T10).
    pub fn cancel_and_wait(&mut self, run: &RunId) {
        if let Some(index) = self.entries.iter().position(|entry| &entry.run_id == run) {
            let mut entry = self.entries.remove(index);
            entry.cancel.cancel();
            if let Some(join) = entry.join.take() {
                let _ = join.join();
            }
        }
    }

    /// Returns the number of runner threads that have not finished yet.
    pub fn active(&self) -> usize {
        self.entries.iter().filter(|entry| !is_finished(entry)).count()
    }

    /// Returns true when `run` has a tracked, unfinished thread.
    pub fn is_active(&self, run: &RunId) -> bool {
        self.entries
            .iter()
            .find(|entry| &entry.run_id == run)
            .is_some_and(|entry| !is_finished(entry))
    }

    /// Reaps finished threads and returns the run ids that completed since the last call.
    pub fn poll(&mut self) -> Vec<RunId> {
        let mut finished_runs = Vec::new();
        let mut remaining = Vec::new();
        for mut entry in self.entries.drain(..) {
            if is_finished(&entry) {
                if let Some(join) = entry.join.take() {
                    let _ = join.join();
                }
                finished_runs.push(entry.run_id);
            } else {
                remaining.push(entry);
            }
        }
        self.entries = remaining;
        finished_runs
    }

    /// Returns the job ids of the tracked runs, for diagnostics.
    pub fn jobs(&self) -> Vec<(RunId, JobId)> {
        self.entries
            .iter()
            .filter_map(|entry| entry.job_id.clone().map(|job| (entry.run_id.clone(), job)))
            .collect()
    }
}

/// Returns true when the thread signalled completion.
fn is_finished(entry: &RunnerEntry) -> bool {
    entry.finished.lock().map(|done| *done).unwrap_or(true)
}

/// Keeps the thread's job kind in the run purpose: fix/merge runs report `Fix`.
fn thread_purpose(job: &Job, purpose: RunPurpose) -> RunPurpose {
    if matches!(job.kind, vowp_protocol::JobKind::Fix | vowp_protocol::JobKind::Merge) {
        RunPurpose::Fix
    } else {
        purpose
    }
}

/// Waits briefly for the session service to insert the new run row.
fn wait_for_run(
    deps: &Deps,
    job: &JobId,
    baseline: &HashSet<RunId>,
    finished: &Arc<Mutex<bool>>,
    join: &JoinHandle<()>,
) -> Result<RunId, CoreError> {
    let deadline = Instant::now() + RUN_HANDOFF_TIMEOUT;
    loop {
        let store = deps.open_store()?;
        for run in store.runs_of_job(job)? {
            if !baseline.contains(&run.id) {
                return Ok(run.id);
            }
        }
        let done = finished.lock().map(|done| *done).unwrap_or(true);
        if done || join.is_finished() {
            // One last read closes the race between the thread finishing and the row write.
            if let Some(run) = store.last_run_of_job(job)?
                && !baseline.contains(&run.id)
            {
                return Ok(run.id);
            }
            return Err(CoreError::Command(format!("run for {job} did not start")));
        }
        if Instant::now() >= deadline {
            return Err(CoreError::Command(format!("timed out waiting for run of {job}")));
        }
        thread::sleep(Duration::from_millis(5));
    }
}
