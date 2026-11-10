//! Capacity targets. C0 returns the default target; E2 fills the history.

use vowp_protocol::{CapacityTarget, TargetMode, Timestamp};
use vowp_store::Store;

use crate::CoreError;

/// Returns the current worker target.
pub fn current(_store: &Store) -> Result<CapacityTarget, CoreError> {
    Ok(CapacityTarget::default())
}

/// Writes one capacity target row.
#[allow(clippy::too_many_arguments)]
pub fn set(
    _store: &Store,
    mode: TargetMode,
    target: u32,
    is_override: bool,
    by: &str,
    reason: &str,
    now: Timestamp,
) -> Result<CapacityTarget, CoreError> {
    Ok(CapacityTarget {
        mode,
        target_workers: target,
        advised_workers: None,
        is_override,
        set_by: by.to_owned(),
        set_at: now,
        reason: reason.to_owned(),
    })
}
