//! Lease acquisition, heartbeat and checks. C0 makes every check a no-op; F2 fills it.

use std::thread::JoinHandle;

use vowp_protocol::{SessionId, Surface};
use vowp_store::Store;

use crate::CoreError;
use crate::deps::Deps;

pub use vowp_protocol::surface::new_lease_token;

/// The request to acquire a session lease.
#[derive(Debug, Clone, PartialEq)]
pub struct AcquireLease {
    /// Session to attach to.
    pub session: SessionId,
    /// The surface asking for the lease.
    pub surface: Surface,
    /// Whether an interactive owner already agreed to take over.
    pub takeover: bool,
}

/// A held lease with its heartbeat thread.
pub struct LeaseGuard {
    _deps: Deps,
    _session: SessionId,
    token: String,
    _thread: Option<JoinHandle<()>>,
}

impl LeaseGuard {
    /// Acquires the session lease.
    pub fn acquire(_deps: &Deps, _store: &Store, _req: &AcquireLease) -> Result<LeaseGuard, CoreError> {
        Err(CoreError::Invalid(
            "session leases are not implemented in this build".to_owned(),
        ))
    }

    /// Returns the lease token.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Releases the lease and stops the heartbeat thread.
    pub fn release(self) -> Result<(), CoreError> {
        Ok(())
    }
}

/// Checks that the caller holds the live lease of `session`.
///
/// A session without a lease (a job session) passes. C0 always passes, which keeps v0.1
/// behaviour unchanged; F2 fills the token comparison.
pub fn check_lease(_deps: &Deps, _store: &Store, _session: &SessionId) -> Result<(), CoreError> {
    Ok(())
}

/// Checks an explicit lease token; `None` means "no lease required".
pub fn check_lease_with(_store: &Store, _session: &SessionId, _token: Option<&str>) -> Result<(), CoreError> {
    Ok(())
}
