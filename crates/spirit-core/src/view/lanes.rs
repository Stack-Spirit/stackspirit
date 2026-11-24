//! Deterministic job-lane mapping from snapshot cards and `jobs_by_state`.
//!
//! Counts always come from `jobs_by_state`; card lists are only what VOWP included.
//! Cards are de-duplicated by `(project, job id)` with running > blocked > queue.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::time::Timestamp;
use crate::view::executor::ExecutorView;
use crate::wire::{
    BlockedJobCard, GateFailureCard, JobState, ProjectSnapshot, QueueJobCard, RunningJobCard,
    Snapshot,
};

/// VOWP caps `queue` at 200 rows.
pub const QUEUE_CAP: usize = 200;

/// One job card in a lane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobCardView {
    /// Job id.
    pub job_id: String,
    /// Owning project.
    pub project: String,
    /// Title, only when a queue row provides one.
    pub title: Option<String>,
    /// Job kind.
    pub kind: String,
    /// Executor role.
    pub role: String,
    /// Current state string. Unknown values are preserved.
    pub state: String,
    /// Priority from a queue row; `0` when the source card has no priority.
    pub priority: i32,
    /// Age in the current state.
    pub age_secs: u64,
    /// Blocked/parked reason.
    pub reason: Option<String>,
    /// Redacted blocked/parked detail.
    pub detail: Option<String>,
    /// Gate name for a gate-failure card.
    pub gate: Option<String>,
    /// Gate failure time.
    pub gate_at: Option<Timestamp>,
    /// Gate evidence line.
    pub gate_evidence: Option<String>,
    /// Parsed executor, when a running card carries one.
    pub executor: Option<ExecutorView>,
    /// Session id, when a running card carries one.
    pub session_id: Option<String>,
    /// Upstream job ids this job waits on (explicit `needs`, else derived for
    /// `-verify-N` / `-fix-N` child jobs). Empty when unknown.
    #[serde(default)]
    pub needs: Vec<String>,
    /// Executor a queued job is planned to run on. `None` for older VOWP snapshots.
    #[serde(default)]
    pub planned_executor: Option<ExecutorView>,
    /// Input tokens spent, when a running card reports them.
    #[serde(default)]
    pub tokens_input: Option<u64>,
    /// Output tokens spent, when a running card reports them.
    #[serde(default)]
    pub tokens_output: Option<u64>,
    /// Cache-read tokens, when a running card reports them.
    #[serde(default)]
    pub tokens_cache_read: Option<u64>,
    /// Model context window in tokens, when known.
    #[serde(default)]
    pub context_window: Option<u64>,
    /// API cost in USD; `None` is unknown, never zero.
    #[serde(default)]
    pub cost_usd: Option<f64>,
    /// `subscription`, `api`, or `free`; unknown values pass through.
    #[serde(default)]
    pub billing_mode: Option<String>,
}

impl JobCardView {
    /// Returns true when the card's state is not a known VOWP state.
    #[must_use]
    pub fn is_unknown_state(&self) -> bool {
        !JobState::parse(&self.state).is_known()
    }
}

/// The five lanes plus authoritative counts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobLanes {
    /// Lock-holding cards from `ProjectSnapshot.running`.
    pub running: Vec<JobCardView>,
    /// `pending`/`ready` queue cards.
    pub ready: Vec<JobCardView>,
    /// Gate-failure cards; counts also include gating/reviewing/fixing.
    pub review_fix: Vec<JobCardView>,
    /// Parked/failed cards, blocked cards, and unknown states: stopped jobs that need
    /// an owner decision. Cancelled jobs are history, never attention.
    pub parked_blocked: Vec<JobCardView>,
    /// Done/replanned counts; cards are summarized, not live.
    pub done: Vec<JobCardView>,
    /// Historical work, kept out of the active lanes: cancelled jobs and gate failures of
    /// jobs that are no longer live. Shown only behind the History tab.
    #[serde(default)]
    pub history: Vec<JobCardView>,
    /// Lane counts keyed by `running`, `ready`, `review_fix`, `parked_blocked`, `done`.
    pub counts: BTreeMap<String, u32>,
    /// True when any project's queue reached the 200-row cap.
    pub queue_truncated: bool,
    /// Number of queue rows included by VOWP across projects.
    pub queue_shown: u32,
    /// The documented queue cap.
    pub queue_cap: u32,
}

/// Builds all lanes from a snapshot.
#[must_use]
pub fn build_lanes(snapshot: &Snapshot) -> JobLanes {
    let mut lanes = JobLanes {
        running: Vec::new(),
        ready: Vec::new(),
        review_fix: Vec::new(),
        parked_blocked: Vec::new(),
        done: Vec::new(),
        history: Vec::new(),
        counts: BTreeMap::new(),
        queue_truncated: false,
        queue_shown: 0,
        queue_cap: u32::try_from(QUEUE_CAP).unwrap_or(u32::MAX),
    };
    for key in [
        "running",
        "ready",
        "review_fix",
        "parked_blocked",
        "done",
        "cancelled",
    ] {
        lanes.counts.insert(key.to_owned(), 0);
    }

    for project in &snapshot.projects {
        accumulate_counts(project, &mut lanes.counts);
        if project.queue.len() >= QUEUE_CAP {
            lanes.queue_truncated = true;
        }
        lanes.queue_shown = lanes
            .queue_shown
            .saturating_add(u32::try_from(project.queue.len()).unwrap_or(u32::MAX));
        build_project_cards(project, &mut lanes);
    }
    derive_child_needs(&mut lanes);

    lanes
}

/// Splits `<parent>-verify-N` / `<parent>-fix-N` into `(parent, is_fix, N)`.
fn child_parts(job: &str) -> Option<(&str, bool, u32)> {
    for (marker, is_fix) in [("-verify-", false), ("-fix-", true)] {
        if let Some(at) = job.rfind(marker) {
            let round = job[at + marker.len()..].parse::<u32>().ok()?;
            return Some((&job[..at], is_fix, round));
        }
    }
    None
}

/// Chains Build -> Review -> Fix for child sessions whose `needs` is empty:
/// `<parent>-verify-N` needs `<parent>-fix-N` (N > 0, when present) else `<parent>`, and
/// `<parent>-fix-N` needs `<parent>-verify-(N-1)` when present, else `<parent>`.
fn derive_child_needs(lanes: &mut JobLanes) {
    let known: HashSet<(String, String)> = [
        &lanes.running,
        &lanes.ready,
        &lanes.review_fix,
        &lanes.parked_blocked,
    ]
    .into_iter()
    .flatten()
    .map(|card| (card.project.clone(), card.job_id.clone()))
    .collect();
    let has = |project: &str, job: &str| known.contains(&(project.to_owned(), job.to_owned()));
    for card in lanes
        .running
        .iter_mut()
        .chain(lanes.ready.iter_mut())
        .chain(lanes.review_fix.iter_mut())
        .chain(lanes.parked_blocked.iter_mut())
    {
        if !card.needs.is_empty() {
            continue;
        }
        let Some((parent, is_fix, round)) = child_parts(&card.job_id) else {
            continue;
        };
        let project = card.project.as_str();
        // Rounds run verify-0, fix-1, verify-1, fix-2, ...
        let prior_verify = format!("{parent}-verify-{}", round.saturating_sub(1));
        let same_fix = format!("{parent}-fix-{round}");
        let upstream = if is_fix && round > 0 && has(project, &prior_verify) {
            prior_verify
        } else if !is_fix && round > 0 && has(project, &same_fix) {
            same_fix
        } else {
            parent.to_owned()
        };
        card.needs = vec![upstream];
    }
}

fn accumulate_counts(project: &ProjectSnapshot, counts: &mut BTreeMap<String, u32>) {
    for (state, count) in &project.jobs_by_state {
        if JobState::parse(state) == JobState::Cancelled {
            let entry = counts.entry("cancelled".to_owned()).or_default();
            *entry = entry.saturating_add(*count);
            continue;
        }
        let lane = match JobState::parse(state) {
            JobState::Running | JobState::Stalled | JobState::Merging => "running",
            JobState::Pending | JobState::Ready => "ready",
            JobState::Gating | JobState::Reviewing | JobState::Fixing => "review_fix",
            JobState::Parked | JobState::Failed | JobState::Cancelled => "parked_blocked",
            JobState::Done | JobState::Replanned => "done",
            // Unknown states are surfaced in the parked lane; never silently dropped.
            JobState::Unknown(_) => "parked_blocked",
        };
        let entry = counts.entry(lane.to_owned()).or_default();
        *entry = entry.saturating_add(*count);
    }
}

fn build_project_cards(project: &ProjectSnapshot, lanes: &mut JobLanes) {
    let project_name = project.name.as_str().to_owned();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let queue_by_job: BTreeMap<&str, &QueueJobCard> = project
        .queue
        .iter()
        .map(|row| (row.job.as_str(), row))
        .collect();

    for card in &project.running {
        let key = (project_name.clone(), card.job.as_str().to_owned());
        if !seen.insert(key) {
            continue;
        }
        let stalled = queue_by_job
            .get(card.job.as_str())
            .is_some_and(|row| row.state == JobState::Stalled);
        // VOWP lists gating/reviewing/fixing jobs in `running` (they hold the lock), not in
        // `queue`. A gate failure on a running job is live review work: the job goes to
        // `review_fix` only (never both lanes), enriched from its card with the newest failure.
        if let Some(failure) = newest_failure(project, &card.job) {
            let mut review = running_card(&project_name, card, stalled);
            review.state = if stalled { "stalled" } else { "fixing" }.to_owned();
            review.gate = Some(failure.gate.clone());
            review.gate_at = Some(failure.at);
            review.gate_evidence = Some(failure.evidence.clone());
            lanes.review_fix.push(review);
        } else {
            lanes
                .running
                .push(running_card(&project_name, card, stalled));
        }
    }

    for card in &project.blocked {
        let key = (project_name.clone(), card.job.as_str().to_owned());
        if !seen.insert(key) {
            continue;
        }
        lanes.parked_blocked.push(blocked_card(&project_name, card));
    }

    for row in &project.queue {
        let key = (project_name.clone(), row.job.as_str().to_owned());
        if seen.contains(&key) {
            continue;
        }
        match &row.state {
            JobState::Pending | JobState::Ready => {
                seen.insert(key);
                lanes.ready.push(queue_card(&project_name, row));
            }
            JobState::Parked | JobState::Failed => {
                seen.insert(key);
                lanes.parked_blocked.push(queue_card(&project_name, row));
            }
            JobState::Cancelled => {
                seen.insert(key);
                lanes.history.push(queue_card(&project_name, row));
            }
            JobState::Gating | JobState::Reviewing | JobState::Fixing => {
                // Live review work: enriched with its gate failure when one exists.
                seen.insert(key);
                let failure = newest_failure(project, &row.job);
                lanes.review_fix.push(match failure {
                    Some(failure) => gate_card(&project_name, failure, Some(row)),
                    None => queue_card(&project_name, row),
                });
            }
            JobState::Done | JobState::Replanned => {
                // Done cards are summarized by count only.
            }
            JobState::Unknown(_) => {
                seen.insert(key);
                lanes.parked_blocked.push(queue_card(&project_name, row));
            }
            JobState::Running | JobState::Stalled | JobState::Merging => {
                // Already represented by the running card list, if VOWP included one.
            }
        }
    }

    // Gate failures of jobs that are not live review work are history. One card per job
    // (latest failure), enriched from the queue row; never an `unknown · unknown` row.
    let live_jobs: HashSet<&str> = project
        .running
        .iter()
        .map(|card| card.job.as_str())
        .chain(project.blocked.iter().map(|card| card.job.as_str()))
        .collect();
    let mut history_seen: HashSet<&str> = HashSet::new();
    for failure in newest_first_failures(project) {
        let job = failure.job.as_str();
        let live_review = queue_by_job.get(job).is_some_and(|row| {
            matches!(
                row.state,
                JobState::Gating | JobState::Reviewing | JobState::Fixing
            )
        });
        if live_review || live_jobs.contains(job) || !history_seen.insert(job) {
            continue;
        }
        lanes.history.push(gate_card(
            &project_name,
            failure,
            queue_by_job.get(job).copied(),
        ));
    }
}

/// The failure of `job` with the greatest timestamp, regardless of input order.
fn newest_failure<'a>(
    project: &'a ProjectSnapshot,
    job: &crate::wire::JobId,
) -> Option<&'a GateFailureCard> {
    project
        .gate_failures
        .iter()
        .filter(|f| &f.job == job)
        .max_by_key(|f| f.at)
}

/// All failures sorted newest first (stable for equal timestamps).
fn newest_first_failures(project: &ProjectSnapshot) -> Vec<&GateFailureCard> {
    let mut all: Vec<&GateFailureCard> = project.gate_failures.iter().collect();
    all.sort_by_key(|f| std::cmp::Reverse(f.at));
    all
}

fn running_card(project: &str, card: &RunningJobCard, stalled: bool) -> JobCardView {
    JobCardView {
        job_id: card.job.as_str().to_owned(),
        project: project.to_owned(),
        title: None,
        kind: card.kind.as_str().to_owned(),
        role: card.role.as_str().to_owned(),
        state: if stalled { "stalled" } else { "active" }.to_owned(),
        priority: 0,
        age_secs: card.age_secs,
        reason: None,
        detail: None,
        gate: None,
        gate_at: None,
        gate_evidence: None,
        executor: card.executor.as_deref().map(ExecutorView::parse),
        session_id: card.session_id.as_ref().map(ToString::to_string),
        needs: Vec::new(),
        planned_executor: None,
        tokens_input: card.tokens_input,
        tokens_output: card.tokens_output,
        tokens_cache_read: card.tokens_cache_read,
        context_window: card.context_window,
        cost_usd: card.cost_usd,
        billing_mode: card.billing_mode.clone(),
    }
}

fn blocked_card(project: &str, card: &BlockedJobCard) -> JobCardView {
    JobCardView {
        job_id: card.job.as_str().to_owned(),
        project: project.to_owned(),
        title: None,
        kind: "unknown".to_owned(),
        role: "unknown".to_owned(),
        state: card.state.as_str().to_owned(),
        priority: 0,
        age_secs: 0,
        reason: Some(card.reason.clone()),
        detail: Some(card.detail.clone()),
        gate: None,
        gate_at: None,
        gate_evidence: None,
        executor: None,
        session_id: None,
        needs: Vec::new(),
        planned_executor: None,
        tokens_input: None,
        tokens_output: None,
        tokens_cache_read: None,
        context_window: None,
        cost_usd: None,
        billing_mode: None,
    }
}

fn queue_card(project: &str, row: &QueueJobCard) -> JobCardView {
    JobCardView {
        job_id: row.job.as_str().to_owned(),
        project: project.to_owned(),
        title: Some(row.title.clone()),
        kind: row.kind.as_str().to_owned(),
        role: row.role.as_str().to_owned(),
        state: row.state.as_str().to_owned(),
        priority: row.priority,
        age_secs: row.age_secs,
        reason: row.waiting_reason.clone(),
        detail: None,
        gate: None,
        gate_at: None,
        gate_evidence: None,
        executor: None,
        session_id: None,
        needs: row.needs.iter().map(|id| id.as_str().to_owned()).collect(),
        planned_executor: row.planned_executor.as_deref().map(ExecutorView::parse),
        tokens_input: None,
        tokens_output: None,
        tokens_cache_read: None,
        context_window: None,
        cost_usd: None,
        billing_mode: None,
    }
}

fn gate_card(
    project: &str,
    failure: &GateFailureCard,
    queue_row: Option<&QueueJobCard>,
) -> JobCardView {
    JobCardView {
        job_id: failure.job.as_str().to_owned(),
        project: project.to_owned(),
        title: queue_row.map(|row| row.title.clone()),
        kind: queue_row.map_or_else(|| "unknown".to_owned(), |row| row.kind.as_str().to_owned()),
        role: queue_row.map_or_else(|| "unknown".to_owned(), |row| row.role.as_str().to_owned()),
        state: queue_row.map_or_else(|| "unknown".to_owned(), |row| row.state.as_str().to_owned()),
        priority: queue_row.map_or(0, |row| row.priority),
        age_secs: queue_row.map_or(0, |row| row.age_secs),
        reason: None,
        detail: None,
        gate: Some(failure.gate.clone()),
        gate_at: Some(failure.at),
        gate_evidence: Some(failure.evidence.clone()),
        executor: None,
        session_id: None,
        needs: queue_row.map_or_else(Vec::new, |row| {
            row.needs.iter().map(|id| id.as_str().to_owned()).collect()
        }),
        planned_executor: queue_row
            .and_then(|row| row.planned_executor.as_deref())
            .map(ExecutorView::parse),
        tokens_input: None,
        tokens_output: None,
        tokens_cache_read: None,
        context_window: None,
        cost_usd: None,
        billing_mode: None,
    }
}
