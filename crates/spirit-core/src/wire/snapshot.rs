//! The versioned `snapshot.json` document and the `fleet status` wrapper.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::SpiritError;
use crate::time::Timestamp;
use crate::wire::capacity::CapacitySnapshot;
use crate::wire::enums::{FleetSource, HarnessKind, InboxKind, JobKind, JobState, ProjectState};
use crate::wire::ids::{InboxId, JobId, NodeName, ProjectName, SessionId};
use crate::wire::limit::Limit;
use crate::wire::session::SessionSurfaceCard;
use crate::wire::surface::OrchestratorCard;

/// Format marker written into every snapshot.
pub const SNAPSHOT_FORMAT: &str = "vowp-snapshot/1";
/// Version of the snapshot document this build supports.
pub const SNAPSHOT_VERSION: u32 = 1;

/// The supervisor card of the snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SupervisorSnapshot {
    /// Whether a supervisor process is running.
    pub running: bool,
    /// Supervisor process id, when running.
    pub pid: Option<u32>,
    /// When the supervisor started.
    pub started_at: Option<Timestamp>,
    /// Last completed tick.
    pub last_tick: Option<Timestamp>,
    /// Tick interval in seconds.
    pub tick_secs: u64,
    /// Whether the node is paused.
    pub paused: bool,
    /// Consecutive failed L1 ticks.
    pub fail_streak: u32,
    /// Binary version, for example `0.4.0`.
    pub version: String,
}

/// One running job card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunningJobCard {
    /// Job id.
    pub job: JobId,
    /// Job kind.
    pub kind: JobKind,
    /// Job role.
    pub role: crate::wire::enums::Role,
    /// Executor string, when known. `None` means a shell, gate or merge run.
    pub executor: Option<String>,
    /// Node the job runs on.
    pub node: NodeName,
    /// Age of the run in seconds.
    pub age_secs: u64,
    /// Age of the last event in seconds, when known.
    pub last_event_age_secs: Option<u64>,
    /// Session the job runs in, when any.
    pub session_id: Option<SessionId>,
    /// Input tokens spent so far. Absent in older VOWP builds.
    #[serde(default)]
    pub tokens_input: Option<u64>,
    /// Output tokens spent so far. Absent in older VOWP builds.
    #[serde(default)]
    pub tokens_output: Option<u64>,
    /// Cache-read tokens served so far. Absent in older VOWP builds.
    #[serde(default)]
    pub tokens_cache_read: Option<u64>,
    /// Context window size of the model in tokens, when known.
    #[serde(default)]
    pub context_window: Option<u64>,
    /// API cost in USD; `None` means unknown, never zero.
    #[serde(default)]
    pub cost_usd: Option<f64>,
    /// How the run is billed: `subscription`, `api`, or `free`. Unknown values pass through.
    #[serde(default)]
    pub billing_mode: Option<String>,
}

/// One blocked job card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlockedJobCard {
    /// Job id.
    pub job: JobId,
    /// Job state.
    pub state: JobState,
    /// Parked or failure reason.
    pub reason: String,
    /// One redacted detail line.
    pub detail: String,
    /// When the job entered this state.
    pub since: Timestamp,
}

/// One queue row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueueJobCard {
    /// Job id.
    pub job: JobId,
    /// Job title.
    pub title: String,
    /// Job kind.
    pub kind: JobKind,
    /// Job role.
    pub role: crate::wire::enums::Role,
    /// Dispatch priority.
    pub priority: i32,
    /// Job state.
    pub state: JobState,
    /// Age in seconds.
    pub age_secs: u64,
    /// Last admission or brake reason.
    pub waiting_reason: Option<String>,
    /// Upstream jobs this job waits on. Absent in older VOWP builds.
    #[serde(default)]
    pub needs: Vec<JobId>,
    /// Executor string the job will run on once dispatched. Absent in older VOWP builds.
    #[serde(default)]
    pub planned_executor: Option<String>,
}

/// The newest merge recorded for a project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MergeCard {
    /// Merged job.
    pub job: JobId,
    /// When the merge happened.
    pub at: Timestamp,
    /// Commit sha.
    pub commit: String,
    /// Merged branch.
    pub branch: String,
}

/// One failed gate result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateFailureCard {
    /// Job the gate ran for.
    pub job: JobId,
    /// Gate name.
    pub gate: String,
    /// When the gate failed.
    pub at: Timestamp,
    /// One redacted evidence line.
    pub evidence: String,
}

/// One open inbox row in the snapshot. It never carries `body`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboxSummary {
    /// Inbox item id.
    pub id: InboxId,
    /// Project the item belongs to, when bound.
    pub project: Option<ProjectName>,
    /// Job the item belongs to, when bound.
    pub job_id: Option<JobId>,
    /// Item kind.
    pub kind: InboxKind,
    /// Item title.
    pub title: String,
    /// When the item was created.
    pub created_at: Timestamp,
    /// Age in seconds.
    pub age_secs: u64,
}

/// One project card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectSnapshot {
    /// Project name.
    pub name: ProjectName,
    /// Registered checkout path.
    pub path: String,
    /// Registry state.
    pub state: ProjectState,
    /// Fairness weight.
    pub weight: u8,
    /// Why the project is paused.
    pub paused_reason: String,
    /// Job counts by state. This is the authority for lane counts.
    pub jobs_by_state: BTreeMap<String, u32>,
    /// Running jobs.
    pub running: Vec<RunningJobCard>,
    /// Blocked jobs.
    pub blocked: Vec<BlockedJobCard>,
    /// Queued jobs, capped at 200 by VOWP.
    pub queue: Vec<QueueJobCard>,
    /// Newest merge, when any.
    pub last_merge: Option<MergeCard>,
    /// Recent gate failures.
    pub gate_failures: Vec<GateFailureCard>,
    /// Cost spent today in USD; `None` means unknown, never zero.
    pub cost_today_usd: Option<f64>,
}

/// The whole `snapshot.json` document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Format marker, always [`SNAPSHOT_FORMAT`].
    pub schema: String,
    /// Document version, always [`SNAPSHOT_VERSION`].
    pub version: u32,
    /// Write counter; increases every write.
    pub sequence: u64,
    /// When the snapshot was generated.
    pub generated_at: Timestamp,
    /// Name of the home.
    pub home: NodeName,
    /// Supervisor card.
    pub supervisor: SupervisorSnapshot,
    /// Project cards in registry order.
    pub projects: Vec<ProjectSnapshot>,
    /// Capacity card.
    pub capacity: CapacitySnapshot,
    /// Provider limits.
    pub limits: Vec<Limit>,
    /// Open inbox rows.
    pub inbox: Vec<InboxSummary>,
    /// Recent sessions.
    pub sessions: Vec<SessionSurfaceCard>,
    /// Configured orchestrators.
    pub orchestrators: Vec<OrchestratorCard>,
}

impl Snapshot {
    /// Enforces the exact schema and version markers.
    ///
    /// A different marker is `vowp_incompatible`; StackSpirit never parses it
    /// heuristically.
    pub fn ensure_compatible(&self) -> Result<(), SpiritError> {
        if self.schema != SNAPSHOT_FORMAT {
            return Err(SpiritError::vowp_incompatible(
                "snapshot.schema",
                format!("expected {SNAPSHOT_FORMAT}, got {:?}", self.schema),
            ));
        }
        if self.version != SNAPSHOT_VERSION {
            return Err(SpiritError::vowp_incompatible(
                "snapshot.version",
                format!("expected {SNAPSHOT_VERSION}, got {}", self.version),
            ));
        }
        Ok(())
    }

    /// Returns the per-project snapshot for `name`.
    #[must_use]
    pub fn project(&self, name: &str) -> Option<&ProjectSnapshot> {
        self.projects
            .iter()
            .find(|project| project.name.as_str() == name)
    }

    /// Returns the first active project, used by the ASIF capability probe.
    #[must_use]
    pub fn first_active_project(&self) -> Option<&ProjectSnapshot> {
        self.projects
            .iter()
            .find(|project| project.state == ProjectState::Active)
    }

    /// Returns the total open-run count reported by the capacity card.
    #[must_use]
    pub const fn open_runs(&self) -> u32 {
        self.capacity.running.runs
    }

    /// Returns the count of running cards with the `worker` role.
    #[must_use]
    pub fn running_worker_cards(&self) -> u32 {
        self.projects
            .iter()
            .flat_map(|project| project.running.iter())
            .filter(|card| card.role == crate::wire::enums::Role::Worker)
            .count()
            .try_into()
            .unwrap_or(u32::MAX)
    }

    /// Returns the count of running cards with the `verifier` role.
    #[must_use]
    pub fn running_verifier_cards(&self) -> u32 {
        self.projects
            .iter()
            .flat_map(|project| project.running.iter())
            .filter(|card| card.role == crate::wire::enums::Role::Verifier)
            .count()
            .try_into()
            .unwrap_or(u32::MAX)
    }
}

/// The `vowp --json fleet status` wrapper document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FleetStatus {
    /// The full snapshot.
    pub snapshot: Snapshot,
    /// Where VOWP got the answer: `local`, `remote` or `cache`.
    pub source: FleetSource,
    /// When VOWP produced or fetched this answer.
    pub fetched_at: Timestamp,
    /// True when the answer is older than three ticks.
    pub stale: bool,
}

impl FleetStatus {
    /// Returns true when `source` is `cache`.
    #[must_use]
    pub fn source_is_cache(&self) -> bool {
        self.source == FleetSource::Cache
    }

    /// Returns the harness kind parsed from a running card's executor, if known.
    #[must_use]
    pub fn running_card_harness(card: &RunningJobCard) -> Option<HarnessKind> {
        card.executor
            .as_deref()
            .and_then(|raw| crate::wire::executor::ExecutorRef::parse(raw).harness)
    }
}
