//! `fleet queue` rows and the action/target write inputs.

use serde::{Deserialize, Serialize};

use crate::wire::enums::{JobKind, JobState, Role, TargetMode};
use crate::wire::ids::{JobId, ProjectName};

/// One `vowp --json fleet queue` row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FleetQueueRow {
    /// Owning project.
    pub project: ProjectName,
    /// Job id.
    pub job: JobId,
    /// Human title. Unlike running/blocked snapshot cards, the queue has one.
    pub title: String,
    /// Job kind.
    pub kind: JobKind,
    /// Executor role.
    pub role: Role,
    /// Dispatch priority.
    pub priority: i32,
    /// Current state.
    pub state: JobState,
    /// Age in this state.
    pub age_secs: u64,
    /// Last admission or brake reason.
    pub waiting_reason: Option<String>,
}

/// A job action exposed to the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobAction {
    /// Retry a failed or parked job.
    Retry,
    /// Park a job until a human intervenes.
    Park,
    /// Cancel a job.
    Cancel,
}

impl JobAction {
    /// Returns the CLI verb.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Retry => "retry",
            Self::Park => "park",
            Self::Cancel => "cancel",
        }
    }
}

/// The validated input for `capacity set`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetTargetInput {
    /// Requested worker count.
    pub target: u32,
    /// Target mode.
    pub mode: TargetMode,
    /// Whether the caller confirmed raising above VOWP's advice.
    #[serde(rename = "override")]
    pub override_: bool,
    /// Trimmed reason, at most 200 characters.
    pub reason: String,
}

impl SetTargetInput {
    /// Validates the local rules from `VOWP-CONTRACT.md` §9.1.
    pub fn validate(&self) -> Result<(), crate::error::SpiritError> {
        if self.reason.chars().count() > 200 {
            return Err(crate::error::SpiritError::invalid_input(
                "reason",
                "must be at most 200 characters",
            ));
        }
        if self.reason.contains('\u{0}') {
            return Err(crate::error::SpiritError::invalid_input(
                "reason",
                "must not contain a NUL byte",
            ));
        }
        Ok(())
    }
}
