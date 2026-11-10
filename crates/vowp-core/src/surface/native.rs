//! The app-host trait and its shared types. F5 fills the hosts; tests use a fake.

use std::path::PathBuf;

use vowp_protocol::{OrchestratorApp, SessionId, Timestamp};

use crate::CoreError;

use super::mirror::MirrorCursor;

/// One request to start an app host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRequest {
    /// Orchestrator name.
    pub name: String,
    /// Session the app serves.
    pub session: SessionId,
    /// Port to bind, when the caller picked one.
    pub port: Option<u16>,
    /// Working directory for the app.
    pub workdir: PathBuf,
    /// Project-local app config file.
    pub config: PathBuf,
}

/// A running app host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppHandle {
    /// App kind.
    pub kind: OrchestratorApp,
    /// Process id.
    pub pid: u32,
    /// Port the app listens on.
    pub port: Option<u16>,
    /// When the host started.
    pub started_at: Timestamp,
    /// Working directory.
    pub workdir: PathBuf,
}

/// Health of one app host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostHealth {
    /// Whether the app answered.
    pub ok: bool,
    /// Human-readable detail.
    pub detail: String,
}

/// Kind of one transcript entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptKind {
    /// A user message.
    UserMessage,
    /// An assistant message.
    AssistantMessage,
    /// A tool call.
    ToolCall,
    /// A tool result.
    ToolResult,
    /// A free-form note.
    Note,
}

/// One transcript entry read from an app.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptEntry {
    /// App-side id used for idempotency.
    pub native_id: String,
    /// Kind of entry.
    pub kind: TranscriptKind,
    /// Timestamp.
    pub ts: Timestamp,
    /// Message text, when any.
    pub text: Option<String>,
    /// Tool name, when any.
    pub tool: Option<String>,
    /// Tool input, when any.
    pub input: Option<serde_json::Value>,
    /// Tool output, when any.
    pub output: Option<String>,
    /// File path, when the entry names one.
    pub path: Option<String>,
}

/// One batch of transcript entries plus the cursor after the batch.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptBatch {
    /// Entries read since the cursor.
    pub entries: Vec<TranscriptEntry>,
    /// Cursor after the batch.
    pub cursor: MirrorCursor,
}

/// One host implementation (opencode, Claude or a fake).
pub trait AppHost: Send + Sync {
    /// Returns the app kind.
    fn kind(&self) -> OrchestratorApp;
    /// Starts the app.
    fn start(&self, req: &HostRequest) -> Result<AppHandle, CoreError>;
    /// Stops the app.
    fn stop(&self, handle: &AppHandle) -> Result<(), CoreError>;
    /// Checks whether the app is healthy.
    fn health(&self, handle: &AppHandle) -> Result<HostHealth, CoreError>;
    /// Returns the join instructions for the three surfaces.
    fn join_instructions(&self, handle: &AppHandle) -> Result<vowp_protocol::JoinInstructions, CoreError>;
    /// Reads transcript entries after `cursor`.
    fn transcript_since(&self, handle: &AppHandle, cursor: &MirrorCursor) -> Result<TranscriptBatch, CoreError>;
}
