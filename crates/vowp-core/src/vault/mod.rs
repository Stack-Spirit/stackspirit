//! Secret storage, injection and redaction. C0 returns no values and redacts nothing.
//!
//! Spec: `docs/design/v0.2/home-and-surfaces.md` §9 and §13.2. G1 fills the module.

pub mod request;

use vowp_protocol::{Capsule, JobId, SecretInfo, SecretName, TurnRequest, VaultMode};
use vowp_store::Store;
use vowp_store::session_log::NewEvent;

use crate::CoreError;
use crate::deps::Deps;

pub use request::{SecretRequestServer, request_secret};

/// Returns the node's vault mode.
pub fn mode(_deps: &Deps) -> Result<VaultMode, CoreError> {
    Ok(VaultMode::Basic)
}

/// Resolves exactly the configured secret names for one harness turn.
pub fn env_for(_deps: &Deps, _req: &TurnRequest) -> Result<Vec<(String, String)>, CoreError> {
    Ok(Vec::new())
}

/// Stores one secret value.
pub fn write_secret(_deps: &Deps, _name: &SecretName, _value: &str) -> Result<SecretInfo, CoreError> {
    Err(CoreError::Invalid(
        "secret storage is not implemented in this build".to_owned(),
    ))
}

/// Removes one secret.
pub fn remove_secret(_deps: &Deps, _name: &SecretName) -> Result<bool, CoreError> {
    Err(CoreError::Invalid(
        "secret storage is not implemented in this build".to_owned(),
    ))
}

/// Redacts exact secret values from one event before it is appended.
pub fn redact_event(_deps: &Deps, ev: NewEvent) -> Result<NewEvent, CoreError> {
    Ok(ev)
}

/// Redacts exact secret values from plain text.
pub fn redact_text(_deps: &Deps, text: &str) -> String {
    text.to_owned()
}

/// Redacts exact secret values from a capsule and rewrites its file and row.
pub fn redact_capsule(_deps: &Deps, _store: &Store, capsule: &Capsule) -> Result<Capsule, CoreError> {
    Ok(capsule.clone())
}

/// Redacts exact secret values from the job artifacts of the last turn.
pub fn redact_job_artifacts(_deps: &Deps, _store: &Store, _job: Option<&JobId>) -> Result<(), CoreError> {
    Ok(())
}

/// The exact-value redactor. C0 loads nothing; G1 fills the loader.
#[derive(Debug, Clone, Default)]
pub struct Redactor {
    values: Vec<(String, String)>,
}

impl Redactor {
    /// Loads the configured secret values, longest first.
    pub fn load(_deps: &Deps) -> Result<Redactor, CoreError> {
        Ok(Redactor::default())
    }

    /// Replaces every exact occurrence with `[REDACTED:<NAME>]`.
    pub fn redact(&self, text: &str) -> String {
        let mut result = text.to_owned();
        for (name, value) in &self.values {
            if value.is_empty() {
                continue;
            }
            result = result.replace(value, &format!("[REDACTED:{name}]"));
        }
        result
    }

    /// Returns true when no secret value is loaded.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}
