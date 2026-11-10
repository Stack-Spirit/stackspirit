//! Capacity advice. C0 never advises; E3 fills the keeper-profile advice turn.

use vowp_protocol::{CapacityAdvice, Timestamp};
use vowp_store::Store;

use crate::CoreError;
use crate::deps::Deps;

/// Why the advisor is considering a new target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdviceTrigger {
    /// The thirty-minute timer elapsed.
    Timer,
    /// A project was registered.
    NewProject,
    /// A brake tripped.
    Brake,
    /// A run was OOM-killed.
    Oom,
    /// The node stayed idle for long enough.
    Idle,
}

/// Produces one advice row when the trigger calls for it.
pub fn maybe_advise(
    _deps: &Deps,
    _store: &Store,
    _now: Timestamp,
    _trigger: AdviceTrigger,
) -> Result<Option<CapacityAdvice>, CoreError> {
    Ok(None)
}
