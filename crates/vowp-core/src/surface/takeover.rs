//! Takeover of a session from another surface. F2 fills this module.

use vowp_protocol::{SessionId, Surface, TakeoverReport, Timestamp};
use vowp_store::Store;

use crate::CoreError;
use crate::deps::Deps;

/// Takes the session over for `target`.
pub fn takeover(
    _deps: &Deps,
    _store: &Store,
    _session: &SessionId,
    _target: &Surface,
    _now: Timestamp,
) -> Result<TakeoverReport, CoreError> {
    Err(CoreError::Invalid(
        "session takeover is not implemented in this build".to_owned(),
    ))
}
