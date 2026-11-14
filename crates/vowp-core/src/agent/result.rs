//! Result return for offloaded runs: fetch, verify, gate and hand the job to the merge queue.
//!
//! Spec: `docs/design/v0.4/local-orchestrator.md` §6.3 step 7, §6.4, §6.5, §10.6 and §13.
//!
//! The node pushes `job/<id>` with its own git credentials and reports the head commit in a
//! `RunState` frame and the terminal `TurnEnd` summary. This module is the home half: it
//! fetches the project's configured remote (never a credential from the agent), checks the
//! fetched branch head against the reported head and against the assigned base commit,
//! checks out a staging worktree outside the main checkout, runs the authoritative
//! `owns_respected` and `secret_scan` gates on the `base..head` diff, and then moves the job
//! to the normal `gating` path. It never merges, tags, deploys or pushes the default branch:
//! the merge queue remains the only writer of `main`.
//!
//! Every terminal summary of an offloaded run settles the assignment and run lease, even
//! when the result is rejected. A rejection keeps the fetched branch for inspection, marks
//! the run failed, and leaves the job retryable; a gate rejection also removes its staging
//! worktree so a retry can move the same `job/<id>` branch to the next pushed head.
//!
//! The project `runs` table has no dedicated transport/base/head columns in this phase. The
//! same facts are recorded in the run's redacted `detail` field and in one small `kv` record
//! per run (see [`AgentRunResult`]); no schema change is needed.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use vowp_graph::{JobEvent, TransitionBounds};
use vowp_protocol::{
    Evidence, Gate, JobState, LedgerEntry, LedgerKind, Level, ResultTransport, Run, RunId, RunOutcome,
};
use vowp_store::repo_offload::{AgentAssignment, AgentAssignmentState};
use vowp_store::{StateChange, Store};

use super::dispatch;
use crate::CoreError;
use crate::deps::{CommandSpec, Deps};
use crate::gates::{GateCtx, GateRunner};
use crate::ledger::Ledger;

/// Prefix of the durable per-run result record in the store's `kv` table.
pub const RESULT_KEY_PREFIX: &str = "agent-result:";

/// Timeout for the small `git` reads and ref updates this module runs directly.
const GIT_TIMEOUT: Duration = Duration::from_secs(300);

/// Most failure detail characters kept in the result record and the run detail.
const DETAIL_CAP: usize = 1200;

/// The durable, path-free result record for one agent run.
///
/// It mirrors the transport facts §6.3 asks the run row to carry. The record lives in the
/// existing `kv` table under [`RESULT_KEY_PREFIX`] plus the run id, so the store schema is
/// unchanged. `head_commit` is written as soon as the node reports one and is therefore the
/// fallback when a reconnecting summary arrives without an in-memory `RunState`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunResult {
    /// Result transport requested by the assignment: `push` or `none`.
    pub transport: String,
    /// Base commit the assignment started from.
    pub base_commit: String,
    /// Head commit the node reported, when it reported one.
    pub head_commit: Option<String>,
    /// Last spool size the node reported.
    pub spool_bytes: u64,
    /// `reported`, `returned` or `failed`.
    pub status: String,
    /// Short, path-free explanation of the last transition.
    pub detail: String,
}

/// The terminal outcome and detail a summary should use after result return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settled {
    /// Outcome to write to the run row.
    pub outcome: RunOutcome,
    /// Redacted, path-free detail to write to the run row.
    pub detail: String,
}

/// Returns the `kv` key of one run's result record.
pub fn result_key(run: &RunId) -> String {
    format!("{RESULT_KEY_PREFIX}{run}")
}

/// Loads one run's durable result record, when it has one.
pub fn load(store: &Store, run: &RunId) -> Result<Option<AgentRunResult>, CoreError> {
    let Some(text) = store.kv_get(&result_key(run))? else {
        return Ok(None);
    };
    let record = serde_json::from_str(&text)
        .map_err(|error| CoreError::Invalid(format!("invalid agent result record: {error}")))?;
    Ok(Some(record))
}

/// Writes one run's durable result record.
fn save(store: &Store, run: &RunId, record: &AgentRunResult) -> Result<(), CoreError> {
    let text = serde_json::to_string(record)
        .map_err(|error| CoreError::Command(format!("cannot encode agent result record: {error}")))?;
    store.kv_set(&result_key(run), &text)?;
    Ok(())
}

/// Records the transport facts of one `RunState` frame.
///
/// Called by the dispatcher; a terminal record is never downgraded to `reported`.
pub(crate) fn record_reported(
    store: &Store,
    assignment: &AgentAssignment,
    head_commit: Option<&str>,
    spool_bytes: u64,
) -> Result<(), CoreError> {
    let mut record = load(store, &assignment.run_id)?.unwrap_or_default();
    if record.status == "returned" || record.status == "failed" {
        return Ok(());
    }
    record.transport = transport_name(assignment);
    record.base_commit = assignment.payload.base_commit.clone();
    if let Some(head) = head_commit.map(str::trim).filter(|head| !head.is_empty()) {
        record.head_commit = Some(head.to_ascii_lowercase());
    }
    record.spool_bytes = spool_bytes;
    record.status = "reported".to_owned();
    save(store, &assignment.run_id, &record)
}

/// Settles the terminal summary of an offloaded run.
///
/// Returns the outcome and detail the caller must write with the run. A run without a live
/// offload assignment is left untouched. When the summary claims success, the pushed branch
/// is fetched and verified before the run is allowed to move to `gating`; every other path
/// keeps the job retryable and still releases the run lease and the assignment slot.
pub fn settle_terminal(
    deps: &Deps,
    store: &Store,
    run: &Run,
    reported_head: Option<&str>,
    outcome: RunOutcome,
    detail: String,
) -> Result<Settled, CoreError> {
    let Some(assignment) = live_assignment(store, &run.id)? else {
        return Ok(Settled { outcome, detail });
    };
    if Some(&assignment.payload.job) != run.job_id.as_ref() {
        return Err(CoreError::Invalid(format!(
            "run {} belongs to job {:?}, not assignment job {}",
            run.id, run.job_id, assignment.payload.job
        )));
    }
    let project = store.require_project(&assignment.project)?;
    let mut record = load(store, &run.id)?.unwrap_or_default();
    record.transport = transport_name(&assignment);
    record.base_commit = assignment.payload.base_commit.clone();
    let reported = reported_head
        .map(str::trim)
        .filter(|head| !head.is_empty())
        .map(str::to_ascii_lowercase)
        .or_else(|| record.head_commit.clone());

    let mut settled = Settled { outcome, detail };
    let mut accepted = false;
    if settled.outcome == RunOutcome::Ok {
        if record.transport != "push" {
            settled = failed(
                RunOutcome::Error,
                "git_push_failed: the assignment did not name a push remote",
            );
        } else {
            match return_pushed(deps, store, run, &assignment, reported.as_deref(), &project) {
                Ok(returned) => {
                    accepted = true;
                    record.head_commit = Some(returned.head.clone());
                    record.base_commit = returned.base.clone();
                    record.status = "returned".to_owned();
                    record.detail = format!("base={} head={}", returned.base, returned.head);
                    settled.detail = append_marker(
                        &settled.detail,
                        &format!("result: transport=push base={} head={}", returned.base, returned.head),
                    );
                }
                Err(reason) => {
                    settled = failed(RunOutcome::Error, &reason);
                }
            }
        }
    } else if settled.detail.contains("git_push_failed") {
        // The node already classified the push. Keep the stable name and drop its stderr,
        // which may contain a credential URL or an absolute local path.
        settled.detail = "git_push_failed: the agent could not push the job branch".to_owned();
    }

    if !accepted {
        record.status = "failed".to_owned();
        record.detail = cap(&settled.detail);
    }
    save(store, &run.id, &record)?;

    let job = store.require_job(&assignment.payload.job)?;
    dispatch::clear_waiting(store, &job.id)?;
    if accepted {
        if job.state == JobState::Running {
            let bounds = TransitionBounds::default();
            let applied = crate::loops::apply_event(store, &job.id, &JobEvent::ReportReady, bounds, None, None)?;
            let _ = applied.effects;
        }
    } else if job.state == JobState::Running {
        store.set_job_state(
            &job.id,
            JobState::Ready,
            &StateChange {
                note: Some(format!("result return failed: {}", cap(&settled.detail))),
                parked_reason: None,
            },
        )?;
    }

    finish_assignment(store, &assignment, accepted)?;
    dispatch::release_run_lease(store, &assignment.session_id)?;
    // A missing audit row must not turn an accepted result into a failed run; the run row
    // still carries the same facts.
    if let Err(error) = record_ledger(deps, store, run, &assignment, &record, accepted) {
        deps.log.log(
            Level::Warn,
            "result",
            &format!("result ledger for {} failed: {:?}", run.id, error.kind()),
        );
    }
    Ok(settled)
}

/// Returns the newest live assignment for a run, when the run is offloaded.
fn live_assignment(store: &Store, run: &RunId) -> Result<Option<AgentAssignment>, CoreError> {
    let mut assignments = store.assignments_for_run(run)?;
    assignments.sort_by_key(|assignment| assignment.epoch);
    Ok(assignments
        .into_iter()
        .rev()
        .find(|assignment| assignment.state.is_active()))
}

/// Returns `push` when the assignment named a remote, `none` otherwise.
fn transport_name(assignment: &AgentAssignment) -> String {
    if assignment.payload.remote_url.trim().is_empty() {
        "none".to_owned()
    } else {
        "push".to_owned()
    }
}

/// One accepted result.
struct Returned {
    /// Assigned base commit.
    base: String,
    /// Verified fetched head commit.
    head: String,
}

/// Fetches and verifies one pushed job branch, gates it and stages its worktree.
///
/// The error is a short, path-free reason for the run detail. No credential URL, git stderr
/// or absolute local path is ever included.
fn return_pushed(
    deps: &Deps,
    store: &Store,
    run: &Run,
    assignment: &AgentAssignment,
    reported: Option<&str>,
    project: &vowp_protocol::Project,
) -> Result<Returned, String> {
    let config = deps.config_now();
    if config.offload.result_transport != ResultTransport::Push {
        return Err("result_rejected: the configured result transport is not push".to_owned());
    }
    let Some(reported) = reported else {
        return Err("git_push_failed: the agent reported no head commit".to_owned());
    };
    if !is_commit_id(reported) {
        return Err("result_rejected: the reported head is not a commit id".to_owned());
    }
    let remote = assignment.payload.remote.trim();
    if remote.is_empty() {
        return Err("git_push_failed: the assignment did not name a remote".to_owned());
    }
    if remote != config.project.remote {
        return Err("result_rejected: the assignment names a remote that is not the project remote".to_owned());
    }
    let Some(expected_url) = project
        .remote_url
        .as_deref()
        .map(dispatch::strip_userinfo)
        .filter(|url| !url.is_empty())
    else {
        return Err("result_rejected: the project has no remote URL to verify".to_owned());
    };
    let repo = crate::loops::main_repo(deps, store, &assignment.project)
        .map_err(|_| "result_rejected: the project checkout is not available".to_owned())?;
    if !repo.is_dir() {
        return Err("result_rejected: the project checkout is not available".to_owned());
    }
    for push in [false, true] {
        let Some(url) = remote_url(deps, &repo, remote, push) else {
            return Err("result_rejected: the project remote could not be read".to_owned());
        };
        if dispatch::strip_userinfo(url.trim()) != expected_url {
            return Err("result_rejected: the project remote does not match the project URL".to_owned());
        }
    }

    deps.git
        .fetch(&repo, remote)
        .map_err(|_| "git_push_failed: the project remote could not be fetched".to_owned())?;
    let branch = format!("job/{}", assignment.payload.job);
    let fetched = deps
        .git
        .head_sha(&repo, &format!("{remote}/{branch}"))
        .map_err(|_| "git_push_failed: the job branch was not found on the project remote".to_owned())?;
    let fetched = fetched.trim().to_ascii_lowercase();
    if fetched != reported {
        return Err(format!(
            "result_rejected: the reported head {} is not the fetched head {}",
            short(reported),
            short(&fetched)
        ));
    }
    let base = assignment.payload.base_commit.trim().to_owned();
    if base.is_empty() {
        return Err("result_rejected: the assignment did not name a base commit".to_owned());
    }
    if deps.git.head_sha(&repo, &base).is_err() {
        return Err("result_rejected: the assigned base commit is not in the home repository".to_owned());
    }
    let backwards = deps
        .git
        .commits_between(&repo, &fetched, &base)
        .map_err(|_| "result_rejected: the base history could not be read".to_owned())?;
    if !backwards.is_empty() {
        return Err(format!(
            "result_rejected: the fetched head {} does not descend from the assigned base {}",
            short(&fetched),
            short(&base)
        ));
    }

    let worktree = staging_path(deps, run);
    if worktree.exists() {
        return Err("result_rejected: the staging worktree already exists".to_owned());
    }
    if let Some(parent) = worktree.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|_| "result_rejected: the staging directory could not be created".to_owned())?;
    }
    git_ref_update(deps, &repo, &branch, &fetched)?;
    deps.git
        .worktree_add(&repo, &worktree, &branch, &fetched)
        .map_err(|_| "result_rejected: the staging worktree could not be created".to_owned())?;

    let mut gate_job = store
        .require_job(&assignment.payload.job)
        .map_err(|_| "result_rejected: the job is not available".to_owned())?;
    gate_job.branch = Some(branch);
    gate_job.worktree = Some(worktree.display().to_string());
    // The gate helpers treat `project.default_branch` as the diff base. Pin it to the
    // assigned base commit for this evaluation so the diff is exactly `base..head` even
    // when the home's `main` advanced while the run was on the node.
    let mut gate_config = (*deps.config_now()).clone();
    gate_config.project.default_branch = base.clone();
    let mut gate_deps = deps.clone();
    gate_deps.config = Arc::new(gate_config);
    let ctx = GateCtx {
        deps: &gate_deps,
        job: &gate_job,
        run_id: Some(run.id.clone()),
        worktree: &worktree,
        repo: &repo,
        store,
        attempt: gate_job.attempts,
    };
    let runner = GateRunner::new(gate_deps.clone());
    let ledger = Ledger::new(deps.clone());
    let mut failures = Vec::new();
    for (name, gate) in [
        ("owns_respected", Gate::OwnsRespected(true)),
        ("secret_scan", Gate::SecretScan(true)),
    ] {
        let index = gate_index(&gate_job, &gate);
        let mut result = runner
            .evaluate(&ctx, index, &gate)
            .map_err(|_| "result_rejected: the gate runner failed".to_owned())?;
        let id = store
            .insert_gate_result(&result)
            .map_err(|_| "result_rejected: the gate result could not be recorded".to_owned())?;
        result.id = Some(id);
        ledger
            .record_gate(store, &result)
            .map_err(|_| "result_rejected: the gate ledger row could not be written".to_owned())?;
        if !result.passed {
            failures.push(gate_failure(name, &result.evidence));
        }
    }
    if !failures.is_empty() {
        // The branch stays for inspection; the worktree is removed so a retry can move the
        // same `job/<id>` branch to the next pushed head.
        let _ = deps.git.worktree_remove(&repo, &worktree);
        return Err(format!("result_gate_failed: {}", failures.join("; ")));
    }

    store
        .set_job_workspace(&gate_job.id, gate_job.branch.as_deref(), gate_job.worktree.as_deref())
        .map_err(|_| "result_rejected: the job workspace could not be recorded".to_owned())?;
    Ok(Returned { base, head: fetched })
}

/// Returns the staging worktree path for one run.
fn staging_path(deps: &Deps, run: &Run) -> std::path::PathBuf {
    deps.paths.root().join("agent-results").join(run.id.as_str())
}

/// Returns the gate index in the job's gate list, or `-1` when the job does not list it.
fn gate_index(job: &vowp_protocol::Job, gate: &Gate) -> i32 {
    job.gates
        .iter()
        .position(|candidate| candidate.kind() == gate.kind())
        .map(|index| i32::try_from(index).unwrap_or(i32::MAX))
        .unwrap_or(-1)
}

/// Describes one failed gate with relative paths and rule names only.
fn gate_failure(name: &str, evidence: &Evidence) -> String {
    match evidence {
        Evidence::OwnsRespected { violations, .. } => {
            let paths = violations.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
            format!("{name}: {paths}")
        }
        Evidence::SecretScan { hits, .. } => {
            let found = hits
                .iter()
                .take(5)
                .map(|hit| format!("{} at {}:{}", hit.rule, hit.path, hit.line))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{name}: {found}")
        }
        _ => format!("{name}: failed"),
    }
}

/// Runs `git remote get-url [--push] <remote>` in `repo`.
fn remote_url(deps: &Deps, repo: &Path, remote: &str, push: bool) -> Option<String> {
    let mut argv = vec!["git".to_owned(), "remote".to_owned(), "get-url".to_owned()];
    if push {
        argv.push("--push".to_owned());
    }
    argv.push(remote.to_owned());
    run_git(deps, repo, argv).ok()
}

/// Points the local job branch at `head` and reports a stable failure on conflict.
fn git_ref_update(deps: &Deps, repo: &Path, branch: &str, head: &str) -> Result<(), String> {
    let argv = vec![
        "git".to_owned(),
        "branch".to_owned(),
        "-f".to_owned(),
        branch.to_owned(),
        head.to_owned(),
    ];
    run_git(deps, repo, argv).map_err(|_| {
        "result_rejected: the job branch is checked out in another worktree or could not be updated".to_owned()
    })?;
    Ok(())
}

/// Runs a `git` argv and returns trimmed stdout.
fn run_git(deps: &Deps, repo: &Path, argv: Vec<String>) -> Result<String, CoreError> {
    let output = deps.cmd.run(&CommandSpec {
        shell: None,
        argv,
        cwd: Some(repo.to_path_buf()),
        env: Vec::new(),
        timeout: GIT_TIMEOUT,
    })?;
    if output.exit_code != Some(0) {
        return Err(CoreError::Command("a git result-return read failed".to_owned()));
    }
    Ok(output.text().trim().to_owned())
}

/// Marks the assignment terminal without moving a row that was already fenced.
fn finish_assignment(store: &Store, assignment: &AgentAssignment, accepted: bool) -> Result<(), CoreError> {
    if assignment.state.is_terminal() {
        return Ok(());
    }
    let detail = if accepted {
        "result returned to the home"
    } else {
        "result return failed; the job stays retryable"
    };
    store.set_assignment_state(
        &assignment.id,
        AgentAssignmentState::Finished,
        assignment.claimed_at,
        detail,
    )?;
    Ok(())
}

/// Appends one ledger note with the transport facts and the gate outcome.
fn record_ledger(
    deps: &Deps,
    store: &Store,
    run: &Run,
    assignment: &AgentAssignment,
    record: &AgentRunResult,
    accepted: bool,
) -> Result<(), CoreError> {
    let ledger = Ledger::new(deps.clone());
    ledger.record(
        store,
        LedgerEntry {
            id: None,
            ts: store.clock().now(),
            kind: LedgerKind::Note,
            project: Some(assignment.project.clone()),
            job_id: Some(assignment.payload.job.clone()),
            run_id: Some(run.id.clone()),
            session_id: run.session_id.clone(),
            role: None,
            provider: None,
            harness: None,
            model: None,
            cost: None,
            summary: format!(
                "result return {}: {}",
                run.id,
                if accepted { "returned" } else { "rejected" }
            ),
            detail: serde_json::json!({
                "transport": record.transport,
                "base_commit": record.base_commit,
                "head_commit": record.head_commit,
                "spool_bytes": record.spool_bytes,
                "status": record.status,
                "accepted": accepted,
                "detail": record.detail,
            }),
        },
    )?;
    Ok(())
}

/// Builds a failed settlement with stable detail.
fn failed(outcome: RunOutcome, detail: &str) -> Settled {
    Settled {
        outcome,
        detail: cap(detail),
    }
}

/// Appends `marker` to a summary detail, keeping the store's tail truncation in mind.
fn append_marker(detail: &str, marker: &str) -> String {
    let detail = detail.trim();
    if detail.is_empty() {
        marker.to_owned()
    } else {
        format!("{detail}; {marker}")
    }
}

/// Caps a detail at [`DETAIL_CAP`] characters and drops any trailing partial line noise.
fn cap(detail: &str) -> String {
    detail.chars().take(DETAIL_CAP).collect()
}

/// Returns whether `value` looks like a full or abbreviated git commit id.
fn is_commit_id(value: &str) -> bool {
    (7..=64).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Returns at most twelve characters of a commit id.
fn short(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}
