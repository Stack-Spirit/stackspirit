//! The deterministic fleet view model consumed by the bridge and UI.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::connection::LatencyStats;
use crate::host::profile::HostId;
use crate::time::Timestamp;
use crate::view::capacity::CapacityView;
use crate::view::executor::ExecutorView;
use crate::view::lanes::{JobLanes, build_lanes};
use crate::wire::{
    CapacityReportDoc, FleetStatus, InboxItem, InboxSummary, OrchestratorCard, ProjectSnapshot,
    SessionOwner, SessionSurfaceCard, Snapshot, SurfaceRef,
};

/// The whole cockpit view for one revision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FleetView {
    /// Monotonic revision.
    pub revision: u64,
    /// Connected host id.
    pub host_id: HostId,
    /// When VOWP produced or fetched the answer.
    pub fetched_at: Timestamp,
    /// When the snapshot was generated.
    pub generated_at: Timestamp,
    /// `local`, `remote` or `cache`.
    pub source: String,
    /// True when the data is stale and mutations are disabled.
    pub stale: bool,
    /// Home node name.
    pub home: String,
    /// Supervisor card.
    pub supervisor: SupervisorView,
    /// Project cards.
    pub projects: Vec<ProjectView>,
    /// Capacity rail.
    pub capacity: CapacityView,
    /// Running cards, stalled first.
    pub running: Vec<WorkerCardView>,
    /// Job lanes.
    pub lanes: JobLanes,
    /// Inbox summaries (bodies are absent; use `fleet inbox --all`).
    pub inbox: Vec<InboxItemView>,
    /// Session cards.
    pub sessions: Vec<SessionCardView>,
    /// Structured session owners (VOWP 0.5.1), one per owned session; empty on older
    /// snapshots. Keyed by `session_id`.
    #[serde(default)]
    pub session_owners: Vec<SessionOwnerView>,
    /// Orchestrator cards. The current contract has no `node` field.
    pub orchestrators: Vec<OrchestratorView>,
    /// Round-trip latency stats.
    pub latency: LatencyStats,
}

/// Supervisor card for the top bar and VPS root.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SupervisorView {
    /// Whether a supervisor process is running.
    pub running: bool,
    /// Process id.
    pub pid: Option<u32>,
    /// Whether the node is paused.
    pub paused: bool,
    /// Last completed tick.
    pub last_tick_at: Option<Timestamp>,
    /// Age of the last tick in seconds.
    pub last_tick_age_secs: Option<u64>,
    /// Tick interval.
    pub tick_secs: u64,
    /// Consecutive failed ticks.
    pub fail_streak: u32,
    /// VOWP version.
    pub version: String,
}

/// One project summary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectView {
    /// Project name.
    pub name: String,
    /// `active`, `paused`, `archived`, or an unknown raw value.
    pub state: String,
    /// Why the project is paused.
    pub paused_reason: String,
    /// Authoritative counts by state.
    pub jobs_by_state: BTreeMap<String, u32>,
    /// Cost today, when known.
    pub cost_today_usd: Option<f64>,
}

/// One running card in the worker grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerCardView {
    /// Job id.
    pub job_id: String,
    /// Owning project.
    pub project: String,
    /// Job kind.
    pub kind: String,
    /// Executor role.
    pub role: String,
    /// `active`, `stalled`, or the raw unknown state.
    pub state: String,
    /// Whether a matching queue row says `stalled`.
    pub stalled: bool,
    /// Executor, or `None` for a shell/gate/merge run.
    pub executor: Option<ExecutorView>,
    /// Executor node.
    pub node: String,
    /// Age of the run.
    pub age_secs: u64,
    /// Age of the last event.
    pub last_event_age_secs: Option<u64>,
    /// Session id.
    pub session_id: Option<String>,
}

/// Inbox item view. The snapshot summary has no body; the full drawer uses
/// [`InboxItemView::from_item`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboxItemView {
    /// Item id.
    pub id: String,
    /// Project, when bound.
    pub project: Option<String>,
    /// Job, when bound.
    pub job_id: Option<String>,
    /// Item kind.
    pub kind: String,
    /// Short title.
    pub title: String,
    /// Full body, when fetched with `--all`.
    pub body: Option<String>,
    /// Suggested answers.
    pub options: Vec<String>,
    /// `open`, `answered`, `dismissed`, or an unknown raw value.
    pub state: String,
    /// Answer, when answered.
    pub answer: Option<String>,
    /// Answerer.
    pub answered_by: Option<String>,
    /// Creation time.
    pub created_at: Timestamp,
    /// Answer time.
    pub answered_at: Option<Timestamp>,
    /// Age in seconds.
    pub age_secs: u64,
}

/// One session card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionCardView {
    /// Session id.
    pub id: String,
    /// Project, when bound.
    pub project: Option<String>,
    /// Job, when bound.
    pub job_id: Option<String>,
    /// Session role.
    pub role: Option<String>,
    /// Title.
    pub title: String,
    /// Session state.
    pub state: String,
    /// Executor, when known.
    pub executor: Option<ExecutorView>,
    /// Node owning the log.
    pub node: String,
    /// Live lease holder.
    pub active_surface: Option<SurfaceRefView>,
    /// Newest event time.
    pub last_event_at: Timestamp,
}

/// The structured owner of a session, with a ready display label.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionOwnerView {
    /// Surface id, when the owner has a paired surface.
    pub surface_id: Option<String>,
    /// Display name: the saved label, else the holder.
    pub label: String,
    /// Holder name.
    pub holder: String,
    /// Client kind.
    pub kind: String,
    /// Owner node.
    pub node: String,
    /// `idle`, `running`, `waiting`, or an unknown raw value.
    pub activity: String,
    /// `interactive` or `job`.
    pub mode: String,
    /// `active`, `stale`, `expired`, or an unknown raw value.
    pub state: String,
    /// Ownership epoch.
    pub epoch: u64,
    /// Acquisition time.
    pub acquired_at: Timestamp,
    /// Last heartbeat.
    pub heartbeat_at: Timestamp,
    /// Job expiry.
    pub expires_at: Option<Timestamp>,
    /// Session id.
    pub session_id: String,
}

/// The live lease holder of a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceRefView {
    /// Surface id.
    pub surface_id: String,
    /// Client kind.
    pub kind: String,
    /// Holder name.
    pub holder: String,
    /// Holder node.
    pub node: String,
    /// Lease acquisition.
    pub acquired_at: Timestamp,
    /// Last heartbeat.
    pub heartbeat_at: Timestamp,
}

/// One orchestrator card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrchestratorView {
    /// Name.
    pub name: String,
    /// Session id.
    pub session_id: String,
    /// Project, when bound.
    pub project: Option<String>,
    /// `opencode`, `claude_code`, `chat`, or an unknown value.
    pub app: String,
    /// `running`, `stopped`, `degraded`, or an unknown value.
    pub state: String,
    /// Join instructions.
    pub join: JoinView,
    /// Node that runs the app; absent for a home-hosted v0.2 orchestrator.
    pub node: Option<String>,
}

impl OrchestratorView {
    /// Projects one `vowp orchestrator ls --json` card.
    #[must_use]
    pub fn from_card(card: &OrchestratorCard) -> Self {
        Self {
            name: card.name.clone(),
            session_id: card.session_id.to_string(),
            project: card.project.as_ref().map(ToString::to_string),
            app: card.app.as_str().to_owned(),
            state: card.state.clone(),
            join: JoinView {
                desktop: card.join.desktop.clone(),
                phone: card.join.phone.clone(),
                chat: card.join.chat.clone(),
            },
            node: card.node.as_ref().map(ToString::to_string),
        }
    }
}

/// The three join paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinView {
    /// Desktop command or steps.
    pub desktop: String,
    /// Phone steps.
    pub phone: String,
    /// `vowp chat` command.
    pub chat: String,
}

impl FleetView {
    /// Projects a parsed `fleet status` (and optional capacity report) into the view.
    ///
    /// The function is pure: the same inputs produce byte-identical views.
    #[must_use]
    pub fn from_status(
        host_id: HostId,
        status: &FleetStatus,
        report: Option<&CapacityReportDoc>,
        latency: LatencyStats,
        revision: u64,
        now: Timestamp,
    ) -> Self {
        let snapshot = &status.snapshot;
        let freshness =
            Duration::from_secs(snapshot.supervisor.tick_secs.saturating_mul(3).max(180));
        let stale =
            status.stale || status.source_is_cache() || now.since(status.fetched_at) > freshness;
        let supervisor = supervisor_view(snapshot, now);
        let projects = snapshot.projects.iter().map(project_view).collect();
        let running = build_running(snapshot);
        let lanes = build_lanes(snapshot);
        let inbox = snapshot.inbox.iter().map(inbox_summary_view).collect();
        let sessions = snapshot.sessions.iter().map(session_view).collect();
        let orchestrators = snapshot
            .orchestrators
            .iter()
            .map(OrchestratorView::from_card)
            .collect();
        let capacity = CapacityView::derive(
            &snapshot.capacity,
            &snapshot.limits,
            report,
            snapshot.generated_at,
        );
        Self {
            revision,
            host_id,
            fetched_at: status.fetched_at,
            generated_at: snapshot.generated_at,
            source: status.source.as_str().to_owned(),
            stale,
            home: snapshot.home.as_str().to_owned(),
            supervisor,
            projects,
            capacity,
            running,
            lanes,
            inbox,
            sessions,
            session_owners: snapshot
                .sessions
                .iter()
                .filter_map(|session| session.owner.as_ref().map(owner_view))
                .collect(),
            orchestrators,
            latency,
        }
    }

    /// Returns true when all mutations must be disabled.
    #[must_use]
    pub fn mutations_disabled(&self) -> bool {
        self.stale
    }
}

fn supervisor_view(snapshot: &Snapshot, now: Timestamp) -> SupervisorView {
    let supervisor = &snapshot.supervisor;
    SupervisorView {
        running: supervisor.running,
        pid: supervisor.pid,
        paused: supervisor.paused,
        last_tick_at: supervisor.last_tick,
        last_tick_age_secs: supervisor.last_tick.map(|tick| now.since(tick).as_secs()),
        tick_secs: supervisor.tick_secs,
        fail_streak: supervisor.fail_streak,
        version: supervisor.version.clone(),
    }
}

fn project_view(project: &ProjectSnapshot) -> ProjectView {
    ProjectView {
        name: project.name.as_str().to_owned(),
        state: project.state.as_str().to_owned(),
        paused_reason: project.paused_reason.clone(),
        jobs_by_state: project.jobs_by_state.clone(),
        cost_today_usd: project.cost_today_usd,
    }
}

fn build_running(snapshot: &Snapshot) -> Vec<WorkerCardView> {
    let mut cards = Vec::new();
    for project in &snapshot.projects {
        let project_name = project.name.as_str();
        let queue_by_job: BTreeMap<&str, &crate::wire::QueueJobCard> = project
            .queue
            .iter()
            .map(|row| (row.job.as_str(), row))
            .collect();
        for card in &project.running {
            let stalled = queue_by_job
                .get(card.job.as_str())
                .is_some_and(|row| row.state == crate::wire::JobState::Stalled);
            cards.push(WorkerCardView {
                job_id: card.job.as_str().to_owned(),
                project: project_name.to_owned(),
                kind: card.kind.as_str().to_owned(),
                role: card.role.as_str().to_owned(),
                state: if stalled { "stalled" } else { "active" }.to_owned(),
                stalled,
                executor: card.executor.as_deref().map(ExecutorView::parse),
                node: card.node.as_str().to_owned(),
                age_secs: card.age_secs,
                last_event_age_secs: card.last_event_age_secs,
                session_id: card.session_id.as_ref().map(ToString::to_string),
            });
        }
    }
    cards.sort_by(|left, right| {
        right
            .stalled
            .cmp(&left.stalled)
            .then_with(|| right.age_secs.cmp(&left.age_secs))
            .then_with(|| left.job_id.cmp(&right.job_id))
    });
    cards
}

fn inbox_summary_view(summary: &InboxSummary) -> InboxItemView {
    InboxItemView {
        id: summary.id.to_string(),
        project: summary.project.as_ref().map(ToString::to_string),
        job_id: summary.job_id.as_ref().map(ToString::to_string),
        kind: summary.kind.as_str().to_owned(),
        title: summary.title.clone(),
        body: None,
        options: Vec::new(),
        state: "open".to_owned(),
        answer: None,
        answered_by: None,
        created_at: summary.created_at,
        answered_at: None,
        age_secs: summary.age_secs,
    }
}

impl InboxItemView {
    /// Builds a full view from `fleet inbox --all`.
    #[must_use]
    pub fn from_item(item: &InboxItem, age_secs: u64) -> Self {
        Self {
            id: item.id.to_string(),
            project: item.project.as_ref().map(ToString::to_string),
            job_id: item.job_id.as_ref().map(ToString::to_string),
            kind: item.kind.as_str().to_owned(),
            title: item.title.clone(),
            body: Some(item.body.clone()),
            options: item.options.clone(),
            state: item.state.as_str().to_owned(),
            answer: item.answer.clone(),
            answered_by: item.answered_by.clone(),
            created_at: item.created_at,
            answered_at: item.answered_at,
            age_secs,
        }
    }
}

fn session_view(session: &SessionSurfaceCard) -> SessionCardView {
    SessionCardView {
        id: session.id.to_string(),
        project: session.project.as_ref().map(ToString::to_string),
        job_id: session.job_id.as_ref().map(ToString::to_string),
        role: session.role.as_ref().map(ToString::to_string),
        title: session.title.clone(),
        state: session.state.as_str().to_owned(),
        executor: session.executor.as_deref().map(ExecutorView::parse),
        node: session.node.as_str().to_owned(),
        active_surface: session.active_surface.as_ref().map(surface_view),
        last_event_at: session.last_event_at,
    }
}

fn surface_view(surface: &SurfaceRef) -> SurfaceRefView {
    SurfaceRefView {
        surface_id: surface.surface_id.clone(),
        kind: surface.kind.as_str().to_owned(),
        holder: surface.holder.clone(),
        node: surface.node.as_str().to_owned(),
        acquired_at: surface.acquired_at,
        heartbeat_at: surface.heartbeat_at,
    }
}

fn owner_view(owner: &SessionOwner) -> SessionOwnerView {
    let label = owner
        .label
        .as_deref()
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .unwrap_or(&owner.holder)
        .to_owned();
    SessionOwnerView {
        surface_id: owner.surface_id.clone(),
        label,
        holder: owner.holder.clone(),
        kind: owner.kind.clone(),
        node: owner.node.as_str().to_owned(),
        activity: owner.activity.clone(),
        mode: owner.mode.clone(),
        state: owner.state.clone(),
        epoch: owner.epoch,
        acquired_at: owner.acquired_at,
        heartbeat_at: owner.heartbeat_at,
        expires_at: owner.expires_at,
        session_id: owner.session_id.to_string(),
    }
}
