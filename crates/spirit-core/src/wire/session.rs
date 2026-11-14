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
}
