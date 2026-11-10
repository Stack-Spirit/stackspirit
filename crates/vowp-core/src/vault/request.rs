//! The one-time secret paste page. G1 fills this module.

use std::time::Duration;

use vowp_protocol::{SecretInfo, SecretName, SecretRequest, Timestamp};

use crate::CoreError;
use crate::deps::Deps;

/// A running one-time paste page.
pub struct SecretRequestServer {
    _private: (),
}

impl SecretRequestServer {
    /// Binds `127.0.0.1:0`, serves exactly one POST and exits.
    pub fn start(_deps: &Deps, _name: &SecretName, _now: Timestamp) -> Result<SecretRequest, CoreError> {
        Err(CoreError::Invalid(
            "secret requests are not implemented in this build".to_owned(),
        ))
    }

    /// Waits for the owner's value and returns the stored metadata.
    pub fn wait(self, _timeout: Duration) -> Result<SecretInfo, CoreError> {
        Err(CoreError::Invalid(
            "secret requests are not implemented in this build".to_owned(),
        ))
    }
}

/// Issues one one-time request document.
pub fn request_secret(_deps: &Deps, _name: &SecretName, _now: Timestamp) -> Result<SecretRequest, CoreError> {
    Err(CoreError::Invalid(
        "secret requests are not implemented in this build".to_owned(),
    ))
}
