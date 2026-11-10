//! The opencode app host. F5 fills this module; C0 refuses to start it.

use vowp_protocol::{JoinInstructions, OrchestratorApp};

use crate::CoreError;

use super::mirror::MirrorCursor;
use super::native::{AppHandle, AppHost, HostHealth, HostRequest, TranscriptBatch};

/// The opencode app host.
#[derive(Debug, Clone, Copy, Default)]
pub struct OpencodeHost;

impl AppHost for OpencodeHost {
    fn kind(&self) -> OrchestratorApp {
        OrchestratorApp::Opencode
    }

    fn start(&self, _req: &HostRequest) -> Result<AppHandle, CoreError> {
        Err(CoreError::Invalid(
            "the opencode app host is not implemented in this build".to_owned(),
        ))
    }

    fn stop(&self, _handle: &AppHandle) -> Result<(), CoreError> {
        Ok(())
    }

    fn health(&self, _handle: &AppHandle) -> Result<HostHealth, CoreError> {
        Err(CoreError::Invalid(
            "the opencode app host is not implemented in this build".to_owned(),
        ))
    }

    fn join_instructions(&self, _handle: &AppHandle) -> Result<JoinInstructions, CoreError> {
        Err(CoreError::Invalid(
            "the opencode app host is not implemented in this build".to_owned(),
        ))
    }

    fn transcript_since(&self, _handle: &AppHandle, _cursor: &MirrorCursor) -> Result<TranscriptBatch, CoreError> {
        Err(CoreError::Invalid(
            "the opencode app host is not implemented in this build".to_owned(),
        ))
    }
}
