//! `vowp setup` support types and phases. C0 creates the module; I1 fills the commands.
//!
//! Spec: `docs/design/v0.2/home-and-surfaces.md` §11.3.

pub mod verify;

use serde::Serialize;

/// The complete setup plan shown to the owner before anything changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SetupPlan {
    /// SSH target of the home.
    pub target: String,
    /// VOWP version the plan installs.
    pub version: String,
    /// Phases in execution order.
    pub phases: Vec<SetupPhase>,
    /// Human hand-offs the agent must stop for.
    pub handoffs: Vec<SetupHandoff>,
}

/// One setup phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SetupPhase {
    /// Stable phase id.
    pub id: String,
    /// Human title.
    pub title: String,
    /// Commands as argv vectors; no shell interpolation.
    pub commands: Vec<Vec<String>>,
    /// Files the phase touches.
    pub files: Vec<String>,
    /// Requirements (`sudo`, `network`, `login:<harness>`, ...).
    pub requires: Vec<String>,
    /// Whether re-running the phase is safe.
    pub idempotent: bool,
}

/// One human hand-off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SetupHandoff {
    /// Phase that needs the hand-off.
    pub phase: String,
    /// Kind (`sudo`, `login`, `secret`, `owner-answer`).
    pub kind: String,
    /// Question or instruction for the owner.
    pub prompt: String,
}

/// The outcome of one setup run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SetupOutcome {
    /// `ok`, `handoff` or `failed`.
    pub status: String,
    /// Phase that ran or stopped.
    pub phase: String,
    /// Paths that changed.
    pub changed: Vec<String>,
    /// Hand-off that stopped the run, when any.
    pub handoff: Option<SetupHandoff>,
    /// Verification checks.
    pub checks: Vec<SetupCheck>,
}

/// One verification check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SetupCheck {
    /// Check name.
    pub name: String,
    /// `ok`, `warn` or `fail`.
    pub status: String,
    /// Human-readable detail.
    pub detail: String,
}
