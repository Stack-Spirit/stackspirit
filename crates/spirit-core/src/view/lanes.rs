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

    lanes
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
        lanes
            .running
            .push(running_card(&project_name, card, stalled));
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
                let failure = project
                    .gate_failures
                    .iter()
                    .rev()
                    .find(|f| f.job == row.job);
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
    let mut history_seen: HashSet<&str> = HashSet::new();
    for failure in project.gate_failures.iter().rev() {
        let job = failure.job.as_str();
        let live_review = queue_by_job.get(job).is_some_and(|row| {
            matches!(
                row.state,
                JobState::Gating | JobState::Reviewing | JobState::Fixing
            )
        });
        if live_review || !history_seen.insert(job) {
            continue;
        }
        lanes.history.push(gate_card(
            &project_name,
            failure,
            queue_by_job.get(job).copied(),
        ));
    }
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
    }
}
