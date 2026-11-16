//! Secret-bearing D6 values that stay on the local side of the boundary.
//!
//! A surface pairing code is a short-lived credential. It is held in
//! [`SecretString`] so it is zeroized, never `Serialize`s and never appears in
//! `Debug`. The Mac agent pair request follows the same rule: the code is sent on
//! stdin with `--code-file -`, never in argv, a file, a log or shared state.

use std::fmt;

use crate::engine::keychain::SecretString;
use crate::error::SpiritError;
use crate::time::Timestamp;
use crate::wire::{AgentPairOutput, ProjectName, SessionId, SurfacePairOutput, ensure_surface_id};

/// A successful home surface pairing.
///
/// Only the one-time code is secret. The surface id, node, session, expiry and the
/// exact Mac command are safe display fields; [`SurfacePairing::view`] returns those
/// without the code for anything that may be logged or serialized.
#[derive(Clone)]
pub struct SurfacePairing {
    /// Surface id.
    pub surface: String,
    /// Node the surface runs on.
    pub node: String,
    /// Session the surface mirrors.
    pub session: String,
    /// Surface kind.
    pub kind: String,
    /// Application name, when any.
    pub app: Option<String>,
    /// Pairing-code expiry.
    pub expires_at: Timestamp,
    /// Exact Mac-side `vowp agent pair` command; never carries the code.
    pub command: String,
    /// Surface revoked by `--replace`, when one was named.
    pub replaced: Option<String>,
    code: SecretString,
}

impl SurfacePairing {
    /// Wraps one raw `vowp surface pair --json` output.
    #[must_use]
    pub fn from_output(output: SurfacePairOutput) -> Self {
        Self {
            surface: output.surface,
            node: output.node,
            session: output.session,
            kind: output.kind.as_str().to_owned(),
            app: output.app,
            expires_at: output.expires_at,
            command: output.command,
            replaced: output.replaced,
            code: SecretString::new(output.code),
        }
    }

    /// Borrows the one-time code for the one UI copy action. Callers must not log it.
    #[must_use]
    pub fn code(&self) -> &SecretString {
        &self.code
    }

    /// Returns the safe view with no secret.
    #[must_use]
    pub fn view(&self) -> SurfacePairingView {
        SurfacePairingView {
            surface: self.surface.clone(),
            node: self.node.clone(),
            session: self.session.clone(),
            kind: self.kind.clone(),
            app: self.app.clone(),
            expires_at: self.expires_at,
            command: self.command.clone(),
            replaced: self.replaced.clone(),
        }
    }
}

impl fmt::Debug for SurfacePairing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SurfacePairing")
            .field("surface", &self.surface)
            .field("node", &self.node)
            .field("session", &self.session)
            .field("kind", &self.kind)
            .field("app", &self.app)
            .field("expires_at", &self.expires_at)
            .field("command", &self.command)
            .field("replaced", &self.replaced)
            .field("code", &"[redacted]")
            .finish()
    }
}

/// The safe, serializable part of a surface pairing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SurfacePairingView {
    /// Surface id.
    pub surface: String,
    /// Node the surface runs on.
    pub node: String,
    /// Session the surface mirrors.
    pub session: String,
    /// Surface kind.
    pub kind: String,
    /// Application name, when any.
    pub app: Option<String>,
    /// Pairing-code expiry.
    pub expires_at: Timestamp,
    /// Exact Mac-side `vowp agent pair` command; never carries the code.
    pub command: String,
    /// Surface revoked by `--replace`, when one was named.
    pub replaced: Option<String>,
}

/// A requested Mac-side surface pairing.
#[derive(Clone)]
pub struct AgentPairRequest {
    /// One-time pairing code; never logged or serialized.
    pub code: SecretString,
    /// Surface id printed by `vowp surface pair`.
    pub surface: String,
    /// Session id printed by `vowp surface pair`.
    pub session: String,
    /// Application name.
    pub app: Option<String>,
}

impl AgentPairRequest {
    /// Builds a request from the one-time code and the home's pairing result.
    #[must_use]
    pub fn new(
        code: SecretString,
        surface: impl Into<String>,
        session: impl Into<String>,
        app: Option<String>,
    ) -> Self {
        Self {
            code,
            surface: surface.into(),
            session: session.into(),
            app,
        }
    }

    /// Validates every value before it becomes local argv or stdin.
    pub fn validate(&self) -> Result<(), SpiritError> {
        let code = self.code.expose();
        if code.trim().is_empty() || code.len() > 4096 || code.contains('\0') {
            return Err(SpiritError::invalid_input(
                "pairing code",
                "must be a non-empty single line",
            ));
        }
        ensure_surface_id(&self.surface)?;
        SessionId::parse(&self.session)?;
        if let Some(app) = &self.app {
            if app.is_empty()
                || app.len() > 64
                || app.starts_with('-')
                || !app
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
            {
                return Err(SpiritError::invalid_input(
                    "surface app",
                    "use 1-64 letters, digits, '.', '-' or '_'",
                ));
            }
        }
        Ok(())
    }
}

impl fmt::Debug for AgentPairRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentPairRequest")
            .field("code", &"[redacted]")
            .field("surface", &self.surface)
            .field("session", &self.session)
            .field("app", &self.app)
            .finish()
    }
}

/// A paired Mac surface, without the token or its file path.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentPairing {
    /// Surface id.
    pub surface_id: String,
    /// Scopes granted to the token.
    pub scopes: Vec<String>,
    /// Token expiry.
    pub expires_at: Timestamp,
}

impl AgentPairing {
    /// Wraps the safe fields of `vowp agent pair --json`.
    #[must_use]
    pub fn from_output(output: AgentPairOutput) -> Self {
        Self {
            surface_id: output.surface_id,
            scopes: output.scopes,
            expires_at: output.expires_at,
        }
    }
}

/// Validates a project name used in a local path map.
pub fn ensure_project(project: &str) -> Result<ProjectName, SpiritError> {
    ProjectName::parse(project)
}
