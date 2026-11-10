//! Capacity brakes. C0 keeps the brake inactive; E2 fills the conditions.

use vowp_protocol::{BrakeState, Timestamp};
use vowp_store::Store;

use crate::CoreError;

/// Evaluates the node brake.
pub fn evaluate(_store: &Store, _now: Timestamp) -> Result<BrakeState, CoreError> {
    Ok(BrakeState::default())
}
