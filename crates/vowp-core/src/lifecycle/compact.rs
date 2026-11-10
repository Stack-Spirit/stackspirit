//! Closed-session compaction. H1 fills this module.

use vowp_protocol::{CapsuleId, SessionId, Timestamp};
use vowp_store::Store;

use crate::CoreError;
use crate::deps::Deps;

/// The result of compacting one session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompactReport {
    /// Session that was compacted.
    pub session: Option<SessionId>,
    /// Bytes freed by removing the full log and blobs.
    pub bytes_freed: u64,
    /// Capsule that now carries the history.
    pub capsule_id: Option<CapsuleId>,
    /// Whether the session was skipped (not closed, or already compacted).
    pub skipped: bool,
}

/// Replaces a closed session's log with its capsule and summary.
pub fn compact_session(
    _deps: &Deps,
    _store: &Store,
    session: &SessionId,
    _now: Timestamp,
) -> Result<CompactReport, CoreError> {
    Ok(CompactReport {
        session: Some(session.clone()),
        skipped: true,
        ..CompactReport::default()
    })
}
