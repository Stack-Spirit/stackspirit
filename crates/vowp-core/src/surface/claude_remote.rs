//! The Claude Remote Control app host. F5 fills this module.

use vowp_protocol::{JoinInstructions, OrchestratorApp};

use crate::CoreError;

use super::mirror::MirrorCursor;
use super::native::{AppHandle, AppHost, HostHealth, HostRequest, TranscriptBatch};

/// The Claude Remote Control host.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClaudeRemoteHost;

impl AppHost for ClaudeRemoteHost {
    fn kind(&self) -> OrchestratorApp {
        OrchestratorApp::ClaudeCode
    }

    fn start(&self, _req: &HostRequest) -> Result<AppHandle, CoreError> {
        Err(CoreError::Refused(
            "Claude Remote Control needs a claude.ai login on the home".to_owned(),
        ))
    }

    fn stop(&self, _handle: &AppHandle) -> Result<(), CoreError> {
        Ok(())
    }

    fn health(&self, _handle: &AppHandle) -> Result<HostHealth, CoreError> {
        Err(CoreError::Refused(
            "Claude Remote Control needs a claude.ai login on the home".to_owned(),
        ))
    }

    fn join_instructions(&self, _handle: &AppHandle) -> Result<JoinInstructions, CoreError> {
        Err(CoreError::Refused(
            "Claude Remote Control needs a claude.ai login on the home".to_owned(),
        ))
    }

    fn transcript_since(&self, _handle: &AppHandle, _cursor: &MirrorCursor) -> Result<TranscriptBatch, CoreError> {
        Err(CoreError::Refused(
            "Claude Remote Control needs a claude.ai login on the home".to_owned(),
        ))
    }
}
