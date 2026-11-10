//! Orchestrator sessions and app hosts. F4 fills this module.

use std::path::PathBuf;

use vowp_protocol::{ExecutorRef, OrchestratorApp, OrchestratorCard, ProjectName, SessionId};

use crate::CoreError;
use crate::deps::Deps;

use super::native::AppHandle;

/// A request to create an orchestrator.
#[derive(Debug, Clone, PartialEq)]
pub struct NewOrchestrator {
    /// Orchestrator name.
    pub name: String,
    /// Project to bind, when any.
    pub project: Option<ProjectName>,
    /// App to run.
    pub app: OrchestratorApp,
    /// Executor to use, when chosen.
    pub executor: Option<ExecutorRef>,
}

/// One orchestrator on disk.
#[derive(Debug, Clone, PartialEq)]
pub struct OrchestratorInfo {
    /// Snapshot card.
    pub card: OrchestratorCard,
    /// Orchestrator directory.
    pub dir: PathBuf,
    /// Session id.
    pub session: SessionId,
    /// Running host, when any.
    pub host: Option<AppHandle>,
}

/// The orchestrator manager.
pub struct Orchestrator {
    _deps: Deps,
}

impl Orchestrator {
    /// Creates a manager over `deps`.
    pub fn new(deps: Deps) -> Self {
        Self { _deps: deps }
    }

    /// Creates and starts an orchestrator.
    pub fn create(&self, _req: &NewOrchestrator) -> Result<OrchestratorInfo, CoreError> {
        Err(CoreError::Invalid(
            "orchestrators are not implemented in this build".to_owned(),
        ))
    }

    /// Lists every orchestrator.
    pub fn list(&self) -> Result<Vec<OrchestratorInfo>, CoreError> {
        Ok(Vec::new())
    }

    /// Returns one orchestrator.
    pub fn info(&self, _name: &str) -> Result<OrchestratorInfo, CoreError> {
        Err(CoreError::NotFound("orchestrator".to_owned()))
    }

    /// Stops one orchestrator.
    pub fn stop(&self, _name: &str) -> Result<(), CoreError> {
        Err(CoreError::NotFound("orchestrator".to_owned()))
    }

    /// Resumes one orchestrator.
    pub fn resume(&self, _name: &str) -> Result<OrchestratorInfo, CoreError> {
        Err(CoreError::NotFound("orchestrator".to_owned()))
    }

    /// Resumes every orchestrator whose state is `running`.
    pub fn resume_all(&self) -> Result<Vec<OrchestratorInfo>, CoreError> {
        Ok(Vec::new())
    }
}
