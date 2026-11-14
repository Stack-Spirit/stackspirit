//! Orchestrator cards. The current contract has **no `node` field**; StackSpirit must
//! never assume one (see `docs/VOWP-CONTRACT.md` §11.3.3).

use serde::{Deserialize, Serialize};

use crate::wire::enums::OrchestratorApp;
use crate::wire::ids::{ProjectName, SessionId};

/// Join instructions for the three ways to reach an orchestrator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JoinInstructions {
    /// Command or steps for the desktop app.
    pub desktop: String,
    /// Command or steps for the phone.
    pub phone: String,
    /// Command for `vowp chat`.
    pub chat: String,
}

/// One orchestrator as shown by `vowp orchestrator ls` and the snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrchestratorCard {
    /// Orchestrator name.
    pub name: String,
    /// Session the orchestrator uses.
    pub session_id: SessionId,
    /// Project the orchestrator is bound to, when any.
    pub project: Option<ProjectName>,
    /// App the orchestrator runs in.
    pub app: OrchestratorApp,
    /// `running`, `stopped` or `degraded`.
    pub state: String,
    /// How to join from the three surfaces.
    pub join: JoinInstructions,
}
