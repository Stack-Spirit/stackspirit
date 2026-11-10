//! Read-only follow mode for a surface that lost the lease. F3 fills this module.

use vowp_protocol::{CancelToken, SessionId};
use vowp_store::Store;

use crate::CoreError;

/// Prints new events after `after_seq` until cancelled and returns the last sequence read.
pub fn follow(_store: &Store, _session: &SessionId, _after_seq: u64, _cancel: &CancelToken) -> Result<u64, CoreError> {
    Err(CoreError::Invalid(
        "session follow mode is not implemented in this build".to_owned(),
    ))
}
