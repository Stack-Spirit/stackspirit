//! Transcript mirroring and session-resume context. F5 fills this module.

use vowp_protocol::{Capsule, EventEnvelope, Lease, Session, SessionId, Timestamp};
use vowp_store::Store;

use crate::CoreError;
use crate::deps::Deps;

use super::native::AppHandle;

/// The incremental cursor of an app transcript mirror.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirrorCursor {
    /// Last app-side id mirrored.
    pub last_native_id: Option<String>,
    /// When the cursor last advanced.
    pub updated_at: Timestamp,
}

/// One mirror pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirrorReport {
    /// Events appended.
    pub events: u64,
    /// Native ids skipped as duplicates.
    pub duplicates: u64,
    /// Cursor after the pass.
    pub cursor: MirrorCursor,
}

/// Mirrors app entries into the session log once.
pub fn mirror_once(
    _deps: &Deps,
    _store: &Store,
    _session: &SessionId,
    _app: &AppHandle,
    cursor: &mut MirrorCursor,
) -> Result<MirrorReport, CoreError> {
    Ok(MirrorReport {
        events: 0,
        duplicates: 0,
        cursor: cursor.clone(),
    })
}

/// Everything a new surface needs to continue a session.
#[derive(Debug, Clone, PartialEq)]
pub struct ResumeContext {
    /// The session row.
    pub session: Session,
    /// Latest capsule, when any.
    pub capsule: Option<Capsule>,
    /// Tail events.
    pub tail: Vec<EventEnvelope>,
    /// Current lease, when one exists.
    pub lease: Option<Lease>,
}

/// Builds the resume context for a surface.
pub fn resume_context(
    _deps: &Deps,
    _store: &Store,
    _session: &SessionId,
    _tail_turns: u32,
) -> Result<ResumeContext, CoreError> {
    Err(CoreError::Invalid(
        "session resume is not implemented in this build".to_owned(),
    ))
}
