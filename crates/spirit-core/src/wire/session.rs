//! Session cards and the surface reference they carry.

use serde::{Deserialize, Serialize};

use crate::time::Timestamp;
use crate::wire::enums::{SessionRole, SessionState, SurfaceKind};
use crate::wire::ids::{JobId, NodeName, ProjectName, SessionId};

/// A surface id: 26-character ULID.
pub type SurfaceId = String;

/// The live lease holder of a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceRef {
    /// Surface instance id.
    pub surface_id: SurfaceId,
    /// Kind of client.
    pub kind: SurfaceKind,
    /// Human-visible holder name.
    pub holder: String,
    /// Node the holder runs on.
    pub node: NodeName,
    /// When the lease was acquired.
    pub acquired_at: Timestamp,
    /// Last heartbeat.
    pub heartbeat_at: Timestamp,
}

/// One session card in the snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSurfaceCard {
    /// Session id.
    pub id: SessionId,
    /// Project the session belongs to, when bound.
    pub project: Option<ProjectName>,
    /// Job the session belongs to, when bound.
    pub job_id: Option<JobId>,
    /// Session role, when set.
    pub role: Option<SessionRole>,
    /// Session title.
    pub title: String,
    /// Session state.
    pub state: SessionState,
    /// Executor string, when known.
    pub executor: Option<String>,
    /// Node the session runs on.
    pub node: NodeName,
    /// The surface that holds the lease, when one is live.
    pub active_surface: Option<SurfaceRef>,
    /// Time of the newest event.
    pub last_event_at: Timestamp,
    /// Structured session ownership (VOWP 0.5.1). Absent on older snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<SessionOwner>,
}

/// The read projection of a session's ownership record (VOWP `SessionOwner`).
///
/// It never carries a lease token. Every liveness field is tolerant of older or newer
/// VOWP spellings, so the raw strings are kept.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionOwner {
    /// Session the owner holds.
    pub session_id: SessionId,
    /// Surface instance id; `None` for a job run with no paired surface.
    #[serde(default)]
    pub surface_id: Option<SurfaceId>,
    /// Friendly saved surface name, for example `StackSpirit · macbook`.
    #[serde(default)]
    pub label: Option<String>,
    /// Human-visible holder name.
    pub holder: String,
    /// Kind of client that owns the session (`opencode`, `chat`, ...).
    pub kind: String,
    /// Node the owner runs on.
    pub node: NodeName,
    /// `idle`, `running`, `waiting`, or an unknown raw value.
    #[serde(default = "default_activity")]
    pub activity: String,
    /// `interactive` or `job`.
    #[serde(default = "default_mode")]
    pub mode: String,
    /// `active`, `stale`, `expired`, or an unknown raw value.
    #[serde(default = "default_state")]
    pub state: String,
    /// Monotonic ownership counter.
    #[serde(default)]
    pub epoch: u64,
    /// When the owner acquired the session.
    pub acquired_at: Timestamp,
    /// Last heartbeat.
    pub heartbeat_at: Timestamp,
    /// Expiry of a job owner.
    #[serde(default)]
    pub expires_at: Option<Timestamp>,
}

fn default_activity() -> String {
    "idle".to_owned()
}

fn default_mode() -> String {
    "interactive".to_owned()
}

fn default_state() -> String {
    "active".to_owned()
}
