//! Surfaces and leases. C0 keeps every lease check a no-op; F2 fills the behaviour.

pub mod claude_remote;
pub mod follow;
pub mod lease;
pub mod mirror;
pub mod native;
pub mod opencode_host;
pub mod orchestrator;
pub mod takeover;

use vowp_protocol::Timestamp;
use vowp_store::Store;

use crate::CoreError;
use crate::deps::Deps;

pub use lease::{AcquireLease, LeaseGuard, check_lease, check_lease_with, new_lease_token};
pub use mirror::ResumeContext;
pub use takeover::takeover;

/// Removes leases whose heartbeat is older than the stale window.
///
/// C0 removes nothing, which keeps v0.1 behaviour unchanged. F2 fills the sweep.
pub fn sweep_leases(_deps: &Deps, _store: &Store, _now: Timestamp) -> Result<u32, CoreError> {
    Ok(0)
}
