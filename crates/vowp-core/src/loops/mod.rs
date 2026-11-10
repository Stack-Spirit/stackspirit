//! Supervisor loops that keep the job graph moving.
//!
//! Spec: `docs/design/loops.md` §1–3, §5–6 and §12, and
//! `docs/design/crates/vowp-core.md` §7.
//!
//! This module owns the L1 tick, the L2 worker start/continue/fix prompts, L3 verify
//! scheduling, the merge queue, dispatch, reconcile and the runner-thread pool. The shared
//! helpers here apply one job event exactly like `requests::apply_direct`: counters, parked
//! reason and the ledger `state` row (`{"from","to","why"}`) are written in one transaction.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::json;
use vowp_graph::{Effect, JobEvent, TransitionBounds, transition_with};
use vowp_protocol::{
    InboxId, Job, JobId, JobState, LedgerEntry, LedgerKind, ParkedReason, ProjectName, SessionId, Timestamp,
};
use vowp_store::{SessionFilter, StateChange, Store};

/// The project-registry view used by the node supervisor and `vowp project`.
///
/// The split schema has `projects.state`, `weight`, `paused_reason` and `archived_at`
/// columns. C1 left the repository file empty for the feature jobs, and this job's owned
/// paths do not include the store crate, so the same view is kept in the project's
/// `config_json` under a reserved key. Every reader (project commands, the node supervisor,
/// `project ls`) goes through this module, so the behaviour is consistent. Removing a
/// project marks the row; the row itself remains for the session text that names it.
pub mod registry {
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use vowp_protocol::{Project, ProjectEntry, ProjectName, ProjectState, Timestamp};
    use vowp_store::Store;

    use crate::CoreError;

    /// Reserved key inside `projects.config_json`.
    const META: &str = "vowp_registry";

    /// Registry fields that are not part of the v0.1 `Project` shape.
    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    struct Meta {
        #[serde(default = "default_state")]
        state: String,
        #[serde(default = "default_weight")]
        weight: u8,
        #[serde(default)]
        paused_reason: String,
        #[serde(default)]
        archived_at: Option<Timestamp>,
        #[serde(default)]
        removed: bool,
    }

    /// Default state for a row written by `vowp init` or a v0.1 migration.
    fn default_state() -> String {
        "active".to_owned()
    }

    /// Default fairness weight.
    fn default_weight() -> u8 {
        1
    }

    /// Returns the registry metadata stored on one project row.
    fn meta_of(project: &Project) -> Meta {
        project
            .config
            .get(META)
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default()
    }

    /// Converts one `projects` row into its registry view.
    fn entry_of(project: &Project) -> ProjectEntry {
        let meta = meta_of(project);
        ProjectEntry {
            name: project.name.clone(),
            path: project.path.clone(),
            remote_url: project.remote_url.clone(),
            default_branch: project.default_branch.clone(),
            state: state_from_text(&meta.state),
            weight: meta.weight.clamp(1, 100),
            paused_reason: meta.paused_reason,
            archived_at: meta.archived_at,
            created_at: project.created_at,
            updated_at: project.updated_at,
        }
    }

    /// Maps a registry state onto its stable text.
    fn state_to_text(state: ProjectState) -> &'static str {
        match state {
            ProjectState::Active => "active",
            ProjectState::Paused => "paused",
            ProjectState::Archived => "archived",
        }
    }

    /// Maps stored text onto [`ProjectState`]; unknown text is active.
    fn state_from_text(text: &str) -> ProjectState {
        match text {
            "paused" => ProjectState::Paused,
            "archived" => ProjectState::Archived,
            _ => ProjectState::Active,
        }
    }

    /// Stores metadata back in `config_json` and touches `updated_at`.
    fn write_meta(store: &Store, mut project: Project, meta: &Meta) -> Result<(), CoreError> {
        if !project.config.is_object() {
            project.config = json!({});
        }
        let value = serde_json::to_value(meta).map_err(|error| CoreError::Invalid(error.to_string()))?;
        project
            .config
            .as_object_mut()
            .ok_or_else(|| CoreError::Invalid("project config is not an object".to_owned()))?
            .insert(META.to_owned(), value);
        project.updated_at = store.clock().now();
        store.upsert_project(&project)?;
        Ok(())
    }

    /// Returns one live registry entry, or `None` when it is absent or removed.
    pub fn project_entry(store: &Store, name: &ProjectName) -> Result<Option<ProjectEntry>, CoreError> {
        let Some(project) = store.project(name)? else {
            return Ok(None);
        };
        let meta = meta_of(&project);
        if meta.removed {
            return Ok(None);
        }
        Ok(Some(entry_of(&project)))
    }

    /// Lists live registry entries, ordered by name.
    pub fn project_entries(store: &Store) -> Result<Vec<ProjectEntry>, CoreError> {
        let mut entries = Vec::new();
        for project in store.list_projects()? {
            if !meta_of(&project).removed {
                entries.push(entry_of(&project));
            }
        }
        Ok(entries)
    }

    /// Returns the live entry whose checkout is `path`.
    pub fn project_by_path(store: &Store, path: &std::path::Path) -> Result<Option<ProjectEntry>, CoreError> {
        for entry in project_entries(store)? {
            let candidate = std::path::Path::new(&entry.path);
            if candidate == path
                || std::fs::canonicalize(candidate)
                    .ok()
                    .zip(std::fs::canonicalize(path).ok())
                    .is_some_and(|(left, right)| left == right)
            {
                return Ok(Some(entry));
            }
        }
        Ok(None)
    }

    /// Sets one project's state and paused reason.
    pub fn set_project_state(
        store: &Store,
        name: &ProjectName,
        state: ProjectState,
        reason: &str,
    ) -> Result<ProjectEntry, CoreError> {
        let project = store
            .project(name)?
            .ok_or_else(|| CoreError::NotFound(format!("project {name}")))?;
        let mut meta = meta_of(&project);
        meta.state = state_to_text(state).to_owned();
        meta.paused_reason = reason.to_owned();
        meta.archived_at = (state == ProjectState::Archived).then(|| store.clock().now());
        meta.removed = false;
        write_meta(store, project, &meta)?;
        project_entry(store, name)?.ok_or_else(|| CoreError::NotFound(format!("project {name}")))
    }

    /// Sets one project's fairness weight (`1..=100`).
    pub fn set_project_weight(store: &Store, name: &ProjectName, weight: u8) -> Result<(), CoreError> {
        if !(1..=100).contains(&weight) {
            return Err(CoreError::Invalid(format!(
                "project weight must be 1..=100, got {weight}"
            )));
        }
        let project = store
            .project(name)?
            .ok_or_else(|| CoreError::NotFound(format!("project {name}")))?;
        let mut meta = meta_of(&project);
        meta.weight = weight;
        write_meta(store, project, &meta)
    }

    /// Marks one project removed. The row stays so session text keeps its name.
    pub fn remove_project_record(store: &Store, name: &ProjectName) -> Result<(), CoreError> {
        if name.as_str() == vowp_store::RESERVED_NODE_PROJECT {
            return Err(CoreError::Invalid(
                "cannot remove the reserved _node project".to_owned(),
            ));
        }
        let project = store
            .project(name)?
            .ok_or_else(|| CoreError::NotFound(format!("project {name}")))?;
        let mut meta = meta_of(&project);
        meta.removed = true;
        meta.paused_reason = "removed".to_owned();
        write_meta(store, project, &meta)
    }
}

use crate::CoreError;
use crate::deps::Deps;

pub mod digest;
pub mod dispatch;
pub mod l2_worker;
pub mod l3_verify;
pub mod l4_planner;
pub mod l5_keeper;
pub mod l6_failover;
pub mod l7_inbox;
pub mod l8_watchdog;
pub mod merge_queue;
pub mod prompts;
pub mod reconcile;
pub mod runner;
pub mod supervisor;

pub use runner::{RunnerPool, SessionRunPool, WakeHandle};
pub use supervisor::{
    NodeReconcileReport, NodeSupervisor, NodeTickReport, ProjectSupervisor, ReconcileReport, StepReport, Supervisor,
    TickReport,
};

/// Prepares a newly registered project: loop rows and worker slots.
///
/// `deps.config.project.name` must be the registered project. The store binding follows
/// the per-project rule in the v0.2 spec (§3.8), so a handle bound to another project is
/// never used for these writes.
pub fn prepare_project(deps: &Deps, project: &ProjectName) -> Result<(), CoreError> {
    if &deps.config.project.name != project {
        return Err(CoreError::Invalid(format!(
            "deps project {} does not match {project}",
            deps.config.project.name
        )));
    }
    let store = deps.open_store()?;
    reconcile::ensure_loops(&store, deps)?;
    store.ensure_worker_slots(
        &deps.node,
        vowp_protocol::WorkerKind::Worker,
        deps.config.node.max_workers,
    )?;
    store.ensure_worker_slots(
        &deps.node,
        vowp_protocol::WorkerKind::Verifier,
        deps.config.node.max_verifiers,
    )?;
    Ok(())
}

/// L6 failover helpers (spec: `loops.md` §8).
pub mod l6 {
    pub use super::l6_failover::{Failover, FailoverAction, ProbeVerdict, is_limit, provider_of, row_kind_for};
}

/// L7 inbox helpers (spec: `loops.md` §9).
pub mod l7 {
    pub use super::l7_inbox::{AnswerEffect, NewInboxItem, on_answer, open_inbox, record_answer};
}

/// L8 watchdog helpers (spec: `loops.md` §10).
pub mod l8 {
    pub use super::l8_watchdog::{CRON_CHECK_SH, STALE_AFTER_SECS, SYSTEMD_UNIT, heartbeat_age, write_heartbeat};
}

/// L2 prompt templates (spec: `loops.md` §3).
pub mod l2 {
    pub use super::l2_worker::{continue_prompt, fix_prompt, start_prompt};
}

/// L3 verify helpers (spec: `loops.md` §5).
pub mod l3 {
    pub use super::l3_verify::{on_verify_done, on_verify_done_with_findings, parse_verdict, verify_prompt};
}

/// One job event that was applied to a row.
#[derive(Debug, Clone)]
pub(crate) struct Applied {
    /// State before the transition.
    pub from: JobState,
    /// State after the transition.
    pub to: JobState,
    /// Effects the caller still has to perform.
    pub effects: Vec<Effect>,
}

/// Applies one job event and writes the state row, counters and ledger entry atomically.
///
/// This is the supervisor's copy of `requests::apply_direct`'s job path; both produce the
/// same ledger `state` detail shape `{"from","to","why"}`.
pub(crate) fn apply_event(
    store: &Store,
    job_id: &JobId,
    event: &JobEvent,
    bounds: TransitionBounds,
    note: Option<String>,
    parked_override: Option<ParkedReason>,
) -> Result<Applied, CoreError> {
    let job = store.require_job(job_id)?;
    let outcome = transition_with(&job, event, &bounds)?;
    let why = event_name(event);

    let mut attempts = job.attempts;
    let mut continues = job.continues;
    let mut hang_restarts = job.hang_restarts;
    let mut counters_changed = false;
    let mut parked_reason = parked_override;
    for effect in &outcome.effects {
        match effect {
            Effect::ResetCounters => {
                attempts = 0;
                continues = 0;
                hang_restarts = 0;
                counters_changed = true;
            }
            Effect::ResetContinues => {
                continues = 0;
                counters_changed = true;
            }
            Effect::IncrementAttempts => {
                attempts = attempts.saturating_add(1);
                counters_changed = true;
            }
            Effect::IncrementContinues => {
                continues = continues.saturating_add(1);
                counters_changed = true;
            }
            Effect::IncrementHangRestarts => {
                hang_restarts = hang_restarts.saturating_add(1);
                counters_changed = true;
            }
            Effect::SetParkedReason(reason) => parked_reason = Some(*reason),
            _ => {}
        }
    }

    let from = store.transaction(|tx| {
        let from = tx.set_job_state(
            job_id,
            outcome.to,
            &StateChange {
                note: note.clone(),
                parked_reason,
            },
        )?;
        if counters_changed {
            tx.set_job_counters(job_id, Some(attempts), Some(continues), Some(hang_restarts))?;
        }
        let entry = LedgerEntry {
            id: None,
            ts: tx.clock().now(),
            kind: LedgerKind::State,
            project: Some(job.project.clone()),
            job_id: Some(job_id.clone()),
            run_id: None,
            session_id: None,
            role: None,
            provider: None,
            harness: None,
            model: None,
            cost: None,
            summary: format!("{job_id}: {} -> {} ({why})", from.as_str(), outcome.to.as_str()),
            detail: json!({
                "from": from.as_str(),
                "to": outcome.to.as_str(),
                "why": why,
            }),
        };
        tx.append_ledger(&entry)?;
        Ok(from)
    })?;

    Ok(Applied {
        from,
        to: outcome.to,
        effects: outcome.effects,
    })
}

/// Returns the canonical event name used in ledger `state` summaries.
pub(crate) fn event_name(event: &JobEvent) -> String {
    match event {
        JobEvent::NeedsSatisfied => "NeedsSatisfied".to_owned(),
        JobEvent::Dispatched => "Dispatched".to_owned(),
        JobEvent::ReportReady => "ReportReady".to_owned(),
        JobEvent::RunEndedNoReport => "RunEndedNoReport".to_owned(),
        JobEvent::RunRateLimited { .. } => "RunRateLimited".to_owned(),
        JobEvent::Stalled { silent } => format!("Stalled({silent})"),
        JobEvent::KeeperNudge { .. } => "KeeperNudge".to_owned(),
        JobEvent::KeeperRestart => "KeeperRestart".to_owned(),
        JobEvent::KeeperReplan => "KeeperReplan".to_owned(),
        JobEvent::KeeperFail => "KeeperFail".to_owned(),
        JobEvent::GatesPassed => "GatesPassed".to_owned(),
        JobEvent::GatesFailed => "GatesFailed".to_owned(),
        JobEvent::VerifyPassed => "VerifyPassed".to_owned(),
        JobEvent::VerifyFailed => "VerifyFailed".to_owned(),
        JobEvent::FixDone { kind } => format!("FixDone({kind:?})"),
        JobEvent::FixFailed => "FixFailed".to_owned(),
        JobEvent::VerifierFailed => "VerifierFailed".to_owned(),
        JobEvent::MergeSucceeded => "MergeSucceeded".to_owned(),
        JobEvent::MergeConflict => "MergeConflict".to_owned(),
        JobEvent::MergeChecksFailed => "MergeChecksFailed".to_owned(),
        JobEvent::Blocked { .. } => "Blocked".to_owned(),
        JobEvent::HumanPark => "HumanPark".to_owned(),
        JobEvent::Resolved => "Resolved".to_owned(),
        JobEvent::Retry => "Retry".to_owned(),
        JobEvent::Cancel => "Cancel".to_owned(),
    }
}

/// Writes a ledger `state` row for a loop request (`pause_loop`, `resume_loop`, `run_loop`,
/// `wake`) as required by loops.md §1 step 0.
pub(crate) fn record_loop_request(
    store: &Store,
    project: &ProjectName,
    loop_name: &str,
    from: &str,
    to: &str,
    why: &str,
) -> Result<(), CoreError> {
    let entry = LedgerEntry {
        id: None,
        ts: store.clock().now(),
        kind: LedgerKind::State,
        project: Some(project.clone()),
        job_id: None,
        run_id: None,
        session_id: None,
        role: None,
        provider: None,
        harness: None,
        model: None,
        cost: None,
        summary: format!("loop {loop_name}: {from} -> {to} ({why})"),
        detail: json!({"from": from, "to": to, "why": why}),
    };
    store.append_ledger(&entry)?;
    Ok(())
}

/// Transport-neutral inbox item input (the L7 public type).
pub use l7_inbox::NewInboxItem as NewInbox;

/// Opens one inbox item and writes its Markdown copy under `<node>/inbox/`.
pub(crate) fn open_inbox(store: &Store, deps: &Deps, item: NewInbox) -> Result<InboxId, CoreError> {
    l7_inbox::open_inbox(store, deps, item)
}

/// Returns the newest session for `job`, if one exists.
pub(crate) fn latest_session(store: &Store, job: &JobId) -> Result<Option<SessionId>, CoreError> {
    let sessions = store.list_sessions(&SessionFilter {
        job_id: Some(job.clone()),
        limit: Some(1),
        ..SessionFilter::default()
    })?;
    Ok(sessions.into_iter().next().map(|session| session.id))
}

/// Returns the project's main checkout.
pub(crate) fn main_repo(deps: &Deps, store: &Store, project: &ProjectName) -> Result<PathBuf, CoreError> {
    if let Some(path) = &deps.config.project.path {
        return Ok(PathBuf::from(path));
    }
    let row = store
        .project(project)?
        .ok_or_else(|| CoreError::NotFound(format!("project {project}")))?;
    Ok(PathBuf::from(row.path))
}

/// Returns the worktree root, defaulting to `<repo>/../<repo-name>-wt`.
pub(crate) fn worktree_root(deps: &Deps, repo: &Path) -> PathBuf {
    if let Some(root) = &deps.config.project.worktree_root {
        return PathBuf::from(root);
    }
    let name = repo
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".to_owned());
    repo.parent().unwrap_or(repo).join(format!("{name}-wt"))
}

/// Returns the path of `job`'s worktree.
pub(crate) fn job_worktree(job: &Job) -> Result<PathBuf, CoreError> {
    job.worktree
        .as_ref()
        .map(PathBuf::from)
        .ok_or_else(|| CoreError::Invalid(format!("job {} has no worktree", job.id)))
}

/// Returns the absolute path of the job brief.
pub(crate) fn brief_path(job: &Job) -> Result<PathBuf, CoreError> {
    Ok(job_worktree(job)?.join(&job.brief))
}

/// Returns the tmux window name for a job.
pub(crate) fn window_name(job: &JobId) -> String {
    vowp_tmux::window_name_for_job(job.as_str())
}

/// Returns the project's default branch.
pub(crate) fn default_branch(deps: &Deps) -> &str {
    &deps.config.project.default_branch
}

/// Returns the provider limits that are in force at `now`, keyed by provider.
///
/// A quota/rate row whose reset time has passed stays blocking while its probe-back is
/// still owed: the preferred rung returns only after a successful probe (loops.md §8.5).
pub(crate) fn limited_map(store: &Store, now: Timestamp) -> Result<BTreeMap<String, Option<Timestamp>>, CoreError> {
    let mut limited = BTreeMap::new();
    for limit in store.active_limits(now)? {
        limited.insert(limit.provider, limit.resets_at);
    }
    for (key, value) in store.kv_prefix("probe_back:")? {
        if value != "1" {
            continue;
        }
        let provider = key.trim_start_matches("probe_back:");
        if store.limit_for(provider)?.is_some() {
            limited.insert(provider.to_owned(), None);
        }
    }
    for (key, _) in store.kv_prefix("probe_pending:")? {
        limited.insert(key.trim_start_matches("probe_pending:").to_owned(), None);
    }
    Ok(limited)
}

/// Clips `text` to at most `max` characters.
pub(crate) fn clip_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut clipped: String = text.chars().take(max.saturating_sub(1)).collect();
    clipped.push('…');
    clipped
}

/// Returns the number of bytes in `path`, or zero when it is absent.
pub(crate) fn file_len(path: &Path) -> u64 {
    std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

/// Parses an RFC 3339 timestamp written by a loop, ignoring malformed values.
pub(crate) fn parse_ts(value: &str) -> Option<Timestamp> {
    Timestamp::parse(value).ok()
}

/// Converts a filesystem modification time to a protocol timestamp.
pub(crate) fn modified_at(path: &Path) -> Option<Timestamp> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let millis = modified.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis();
    Some(Timestamp::from_unix_millis(i64::try_from(millis).ok()?))
}
