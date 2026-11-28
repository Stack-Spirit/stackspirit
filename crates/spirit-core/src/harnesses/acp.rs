//! A reusable Agent Client Protocol (ACP) over-stdio client.
//!
//! The client speaks newline-delimited JSON-RPC 2.0 on the child's stdin/stdout and
//! implements the core handshake and turn framing:
//!
//! - `initialize`
//! - `session/new`
//! - `session/prompt`
//! - streaming `session/update` notifications
//! - `session/request_permission` requests
//! - `session/cancel` notifications
//!
//! It deliberately advertises no client `fs/*` or `terminal/*` capabilities: StackSpirit
//! is a chat surface, not a file or terminal proxy. It never reads a credential file and
//! never extracts or stores a provider OAuth token. Authentication stays inside the
//! harness's own process/store; this module only carries ACP protocol messages.
//!
//! The transport is split from the process so the protocol can be tested hermetically.
//! [`AcpClient::spawn`] owns a real child; [`AcpClient::from_io`] accepts any
//! `AsyncRead`/`AsyncWrite` pair, which is how the deterministic mock-process tests drive
//! a complete handshake without installing a harness.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::timeout;

use crate::error::{SpiritError, SpiritErrorCode, redact};

/// ACP protocol version implemented by this client.
pub const ACP_PROTOCOL_VERSION: u16 = 1;
/// Maximum accepted JSON-RPC line, matching the harness-chat design cap.
pub const ACP_MAX_LINE_BYTES: usize = 1024 * 1024;
/// Deadline for one handshake request or one line from the agent.
pub const ACP_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Deadline for one complete `session/prompt` turn, including tool permissions.
pub const ACP_PROMPT_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// One launch specification for [`AcpClient::spawn`].
///
/// Callers must not put an API key, OAuth token, password or other credential in
/// `program` or `args`; credentials belong in the harness's own store, the OS keychain
/// or VOWP's `vowp secret` vault. The client passes no credential itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpLaunchSpec {
    /// Executable name or absolute path.
    pub program: String,
    /// Shell-free arguments.
    pub args: Vec<String>,
    /// Working directory for the child.
    pub cwd: Option<PathBuf>,
    /// Deadline for one handshake request or one line from the agent.
    pub read_timeout: Duration,
}

impl AcpLaunchSpec {
    /// Builds a launch spec for one executable.
    #[must_use]
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            read_timeout: ACP_REQUEST_TIMEOUT,
        }
    }

    /// Adds one argument.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Sets the child working directory.
    #[must_use]
    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Overrides the per-read deadline.
    #[must_use]
    pub fn read_timeout(mut self, read_timeout: Duration) -> Self {
        self.read_timeout = read_timeout;
        self
    }
}

/// What the client may ask the agent to do.
///
/// The client advertises no filesystem or terminal callbacks on purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpFsCapabilities {
    /// Whether the client offers `fs/read_text_file` to the agent.
    pub read_text_file: bool,
    /// Whether the client offers `fs/write_text_file` to the agent.
    pub write_text_file: bool,
}

/// The client capability document sent in `initialize`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpClientCapabilities {
    /// Filesystem callbacks, always disabled.
    pub fs: AcpFsCapabilities,
    /// Terminal callbacks, always disabled.
    pub terminal: bool,
}

impl Default for AcpClientCapabilities {
    fn default() -> Self {
        Self {
            fs: AcpFsCapabilities {
                read_text_file: false,
                write_text_file: false,
            },
            terminal: false,
        }
    }
}

/// The safe, parsed result of `initialize`.
///
/// Only protocol facts and auth-method **ids** are kept. Any metadata the agent returns
/// alongside an auth method is discarded, so no token, secret or OAuth handle is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpInitializeResult {
    /// Protocol version the agent negotiated.
    pub protocol_version: u16,
    /// Whether the agent can load an existing session.
    pub supports_load_session: bool,
    /// Whether the agent accepts image prompt content.
    pub supports_image_prompts: bool,
    /// Safe auth-method ids only, for display and gating.
    pub auth_method_ids: Vec<String>,
}

impl AcpInitializeResult {
    /// True when the negotiated version matches this client.
    #[must_use]
    pub const fn compatible(&self) -> bool {
        self.protocol_version == ACP_PROTOCOL_VERSION
    }
}

/// One text/content block in an ACP prompt or update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AcpContentBlock {
    /// Plain text.
    Text {
        /// The text value.
        text: String,
    },
    /// A content type this client does not interpret.
    #[serde(other)]
    Unknown,
}

impl AcpContentBlock {
    /// Returns the text when this is a text block.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            Self::Unknown => None,
        }
    }
}

/// One streaming `session/update` payload.
///
/// Known variants are typed; an unknown `sessionUpdate` kind becomes
/// [`AcpUpdate::Unknown`] instead of being guessed as text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "sessionUpdate", rename_all = "snake_case")]
pub enum AcpUpdate {
    /// An assistant text chunk.
    AgentMessageChunk {
        /// The chunk content.
        content: AcpContentBlock,
    },
    /// An assistant reasoning/thought chunk.
    AgentThoughtChunk {
        /// The chunk content.
        content: AcpContentBlock,
    },
    /// A user message chunk echoed by the agent.
    UserMessageChunk {
        /// The chunk content.
        content: AcpContentBlock,
    },
    /// A tool call started.
    ToolCall {
        /// Native tool-call id.
        #[serde(default)]
        tool_call_id: String,
        /// Display title.
        #[serde(default)]
        title: String,
        /// Tool kind.
        #[serde(default)]
        kind: String,
        /// Current status.
        #[serde(default)]
        status: String,
    },
    /// A tool call changed.
    ToolCallUpdate {
        /// Native tool-call id.
        #[serde(default)]
        tool_call_id: String,
        /// Current status.
        #[serde(default)]
        status: String,
    },
    /// An agent plan.
    Plan {
        /// Plan entries, kept as opaque JSON for forward compatibility.
        #[serde(default)]
        entries: Vec<Value>,
    },
    /// Available commands changed.
    AvailableCommandsUpdate {
        /// Command entries, kept as opaque JSON for forward compatibility.
        #[serde(default)]
        available_commands: Vec<Value>,
    },
    /// The current mode changed.
    CurrentModeUpdate {
        /// Current mode id.
        #[serde(default)]
        current_mode_id: String,
    },
    /// A kind this client does not interpret.
    #[serde(other)]
    Unknown,
}

/// One `session/update` notification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpSessionUpdate {
    /// Native session id.
    pub session_id: String,
    /// The update payload.
    pub update: AcpUpdate,
}

impl AcpSessionUpdate {
    /// Returns the assistant text when this update is an agent message chunk.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        match &self.update {
            AcpUpdate::AgentMessageChunk { content } => content.text(),
            _ => None,
        }
    }
}

/// One permission option offered by the agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpPermissionOption {
    /// Stable option id echoed back to the agent.
    pub option_id: String,
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// `allow_once`, `allow_always`, `reject_once` or `reject_always`.
    #[serde(default)]
    pub kind: String,
}

/// One `session/request_permission` request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpPermissionRequest {
    /// Native session id.
    #[serde(default)]
    pub session_id: String,
    /// The tool call the request belongs to, kept opaque for display.
    #[serde(default)]
    pub tool_call: Value,
    /// Offered options.
    #[serde(default)]
    pub options: Vec<AcpPermissionOption>,
}

/// What the client answers to a permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AcpPermissionDecision {
    /// Allow this one tool call.
    AllowOnce,
    /// Allow this tool call and later ones of the same kind.
    AllowAlways,
    /// Reject this one tool call.
    RejectOnce,
    /// Reject this tool call and later ones of the same kind.
    RejectAlways,
    /// Do not choose; ACP reports the turn as cancelled.
    #[default]
    Cancel,
}

impl AcpPermissionDecision {
    /// The ACP option kind this decision asks for.
    #[must_use]
    pub const fn option_kind(self) -> Option<&'static str> {
        match self {
            Self::AllowOnce => Some("allow_once"),
            Self::AllowAlways => Some("allow_always"),
            Self::RejectOnce => Some("reject_once"),
            Self::RejectAlways => Some("reject_always"),
            Self::Cancel => None,
        }
    }
}

/// A consumer of streaming updates and permission requests.
///
/// The default permission answer is [`AcpPermissionDecision::Cancel`]: the client never
/// auto-approves a tool call.
pub trait AcpObserver {
    /// Receives one `session/update` in arrival order.
    fn on_update(&mut self, update: AcpSessionUpdate) {
        let _ = update;
    }

    /// Chooses an answer for one permission request.
    fn on_permission(&mut self, request: &AcpPermissionRequest) -> AcpPermissionDecision {
        let _ = request;
        AcpPermissionDecision::Cancel
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct NoopObserver;

impl AcpObserver for NoopObserver {}

/// A collector for tests and simple callers.
#[derive(Debug, Clone, Default)]
pub struct CollectingObserver {
    /// Answer used for every permission request.
    pub decision: AcpPermissionDecision,
    /// Updates received in order.
    pub updates: Vec<AcpSessionUpdate>,
    /// Permission requests received in order.
    pub permission_requests: Vec<AcpPermissionRequest>,
    /// Decisions this observer returned for `permission_requests`, in the same order.
    /// The client still downgrades a missing option kind to `cancelled`.
    pub decisions: Vec<AcpPermissionDecision>,
}

impl AcpObserver for CollectingObserver {
    fn on_update(&mut self, update: AcpSessionUpdate) {
        self.updates.push(update);
    }

    fn on_permission(&mut self, request: &AcpPermissionRequest) -> AcpPermissionDecision {
        self.permission_requests.push(request.clone());
        self.decisions.push(self.decision);
        self.decision
    }
}

/// The final result of one completed prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpPromptResult {
    /// Native session id.
    pub session_id: String,
    /// ACP stop reason, for example `end_turn` or `cancelled`.
    pub stop_reason: String,
}

/// A completed prompt plus everything observed while it streamed.
#[derive(Debug, Clone, PartialEq)]
pub struct AcpPromptOutcome {
    /// The final prompt result.
    pub result: AcpPromptResult,
    /// Streaming updates in arrival order.
    pub updates: Vec<AcpSessionUpdate>,
    /// Permission requests the agent asked during the turn.
    pub permission_requests: Vec<AcpPermissionRequest>,
}

/// One ACP child connection.
///
/// The client serializes requests: one request is in flight at a time, while agent
/// notifications and permission requests are handled inside that request's read loop.
pub struct AcpClient {
    reader: BufReader<Box<dyn AsyncRead + Send + Unpin>>,
    writer: Box<dyn AsyncWrite + Send + Unpin>,
    child: Option<Child>,
    next_id: u64,
    session_id: Option<String>,
    read_timeout: Duration,
    prompt_timeout: Duration,
}

impl std::fmt::Debug for AcpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcpClient")
            .field("session_id", &self.session_id)
            .field("has_child", &self.child.is_some())
            .finish_non_exhaustive()
    }
}

impl AcpClient {
    /// Builds a client over an existing reader/writer pair.
    ///
    /// This is the hermetic seam used by the mock-process tests.
    #[must_use]
    pub fn from_io<R, W>(reader: R, writer: W) -> Self
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        Self {
            reader: BufReader::new(Box::new(reader)),
            writer: Box::new(writer),
            child: None,
            next_id: 1,
            session_id: None,
            read_timeout: ACP_REQUEST_TIMEOUT,
            prompt_timeout: ACP_PROMPT_TIMEOUT,
        }
    }

    /// Spawns one ACP child with piped stdin/stdout and no captured stderr.
    ///
    /// The child is killed when this client is dropped and again explicitly by
    /// [`AcpClient::shutdown`].
    pub async fn spawn(spec: &AcpLaunchSpec) -> Result<Self, SpiritError> {
        validate_launch_spec(spec)?;
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        if let Some(cwd) = &spec.cwd {
            command.current_dir(cwd);
        }
        let mut child = command.spawn().map_err(|error| {
            SpiritError::new(
                SpiritErrorCode::Io,
                format!(
                    "ACP: could not start {}: {}",
                    spec.program,
                    redact(&error.to_string())
                ),
            )
        })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            SpiritError::new(SpiritErrorCode::Io, "ACP: the child has no stdin pipe")
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            SpiritError::new(SpiritErrorCode::Io, "ACP: the child has no stdout pipe")
        })?;
        Ok(Self {
            reader: BufReader::new(Box::new(stdout)),
            writer: Box::new(stdin),
            child: Some(child),
            next_id: 1,
            session_id: None,
            read_timeout: spec.read_timeout,
            prompt_timeout: ACP_PROMPT_TIMEOUT,
        })
    }

    /// Returns the native session id after `session/new` or `session/load`.
    #[must_use]
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// Sends `initialize` with StackSpirit's client identity and no fs/terminal
    /// capabilities.
    pub async fn initialize(&mut self) -> Result<AcpInitializeResult, SpiritError> {
        self.initialize_as("stackspirit", env!("CARGO_PKG_VERSION"))
            .await
    }

    /// Sends `initialize` with an explicit client identity.
    pub async fn initialize_as(
        &mut self,
        client_name: &str,
        client_version: &str,
    ) -> Result<AcpInitializeResult, SpiritError> {
        let params = json!({
            "protocolVersion": ACP_PROTOCOL_VERSION,
            "clientCapabilities": AcpClientCapabilities::default(),
            "clientInfo": { "name": client_name, "version": client_version },
        });
        let mut observer = NoopObserver;
        let value = self.request("initialize", params, &mut observer).await?;
        AcpInitializeResult::from_value(&value)
    }

    /// Sends `session/new` and remembers the returned native session id.
    pub async fn new_session(&mut self, cwd: &str) -> Result<String, SpiritError> {
        if cwd.trim().is_empty() {
            return Err(SpiritError::invalid_input("cwd", "must not be empty"));
        }
        let params = json!({ "cwd": cwd, "mcpServers": [] });
        let mut observer = NoopObserver;
        let value = self.request("session/new", params, &mut observer).await?;
        let session_id = value
            .get("sessionId")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| acp_protocol_error("session/new", "missing sessionId"))?
            .to_owned();
        self.session_id = Some(session_id.clone());
        Ok(session_id)
    }

    /// Sends one text prompt and collects every streamed update.
    ///
    /// Permission requests are cancelled by default. Use
    /// [`AcpClient::prompt_with_observer`] to allow a tool call explicitly.
    pub async fn prompt(&mut self, text: &str) -> Result<AcpPromptOutcome, SpiritError> {
        let mut observer = CollectingObserver::default();
        let result = self.prompt_with_observer(text, &mut observer).await?;
        Ok(AcpPromptOutcome {
            result,
            updates: observer.updates,
            permission_requests: observer.permission_requests,
        })
    }

    /// Sends one text prompt and streams updates and permission requests to `observer`.
    pub async fn prompt_with_observer<O: AcpObserver + ?Sized>(
        &mut self,
        text: &str,
        observer: &mut O,
    ) -> Result<AcpPromptResult, SpiritError> {
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| SpiritError::new(SpiritErrorCode::Conflict, "ACP: no active session"))?;
        let params = json!({
            "sessionId": session_id,
            "prompt": [AcpContentBlock::Text { text: text.to_owned() }],
        });
        let response = match timeout(
            self.prompt_timeout,
            self.request("session/prompt", params, observer),
        )
        .await
        {
            Ok(response) => response?,
            Err(_) => {
                // A timed-out turn leaves the JSON-RPC stream at an unknown position.
                // Reap the child rather than reusing a possibly desynchronized session.
                self.shutdown().await;
                return Err(SpiritError::new(
                    SpiritErrorCode::RemoteTimeout,
                    format!(
                        "ACP session/prompt did not finish within {}s",
                        self.prompt_timeout.as_secs()
                    ),
                )
                .retryable());
            }
        };
        let stop_reason = response
            .get("stopReason")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| acp_protocol_error("session/prompt", "missing stopReason"))?
            .to_owned();
        Ok(AcpPromptResult {
            session_id,
            stop_reason,
        })
    }

    /// Sends the `session/cancel` notification for the active session.
    pub async fn cancel(&mut self) -> Result<(), SpiritError> {
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| SpiritError::new(SpiritErrorCode::Conflict, "ACP: no active session"))?;
        self.notify("session/cancel", json!({ "sessionId": session_id }))
            .await
    }

    /// Closes stdin and reaps the child, if this client spawned one.
    pub async fn shutdown(&mut self) {
        let _ = self.writer.shutdown().await;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
    }

    async fn request<O: AcpObserver + ?Sized>(
        &mut self,
        method: &str,
        params: Value,
        observer: &mut O,
    ) -> Result<Value, SpiritError> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let message = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.write_value(&message).await?;

        let expected = Value::from(id);
        loop {
            let value = self.read_value().await?;
            if let Some(incoming_method) = value.get("method").and_then(Value::as_str) {
                let request_id = value.get("id").cloned();
                let params = value.get("params").cloned().unwrap_or(Value::Null);
                match incoming_method {
                    "session/update" => {
                        let update: AcpSessionUpdate = serde_json::from_value(params)
                            .map_err(|error| acp_protocol_error("session/update", error))?;
                        observer.on_update(update);
                    }
                    "session/request_permission" => {
                        if let Some(request_id) = request_id {
                            let parsed =
                                serde_json::from_value::<AcpPermissionRequest>(params.clone());
                            match parsed {
                                Ok(request) => {
                                    let decision = observer.on_permission(&request);
                                    let response =
                                        permission_response(request_id, &request, decision);
                                    self.write_value(&response).await?;
                                }
                                Err(error) => {
                                    // Never leave the agent waiting on a malformed request.
                                    self.write_value(&json!({
                                        "jsonrpc": "2.0",
                                        "id": request_id,
                                        "result": { "outcome": { "outcome": "cancelled" } },
                                    }))
                                    .await?;
                                    return Err(acp_protocol_error(
                                        "session/request_permission",
                                        error,
                                    ));
                                }
                            }
                        }
                    }
                    _ => {
                        if let Some(request_id) = request_id {
                            self.write_value(&json!({
                                "jsonrpc": "2.0",
                                "id": request_id,
                                "error": {
                                    "code": -32601,
                                    "message": "method not found",
                                },
                            }))
                            .await?;
                        }
                    }
                }
                continue;
            }

            if value.get("id") == Some(&expected) {
                if let Some(error) = value.get("error") {
                    return Err(jsonrpc_error(method, error));
                }
                return Ok(value.get("result").cloned().unwrap_or(Value::Null));
            }

            // A response for another id cannot be matched to a pending request in this
            // serialized client. Ignore it rather than guessing which turn it belongs to.
        }
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), SpiritError> {
        self.write_value(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
        .await
    }

    async fn write_value(&mut self, value: &Value) -> Result<(), SpiritError> {
        let mut bytes = serde_json::to_vec(value).map_err(|error| {
            SpiritError::new(
                SpiritErrorCode::Internal,
                format!("ACP: could not encode a JSON-RPC message: {error}"),
            )
        })?;
        if bytes.len() > ACP_MAX_LINE_BYTES {
            return Err(SpiritError::output_too_large(ACP_MAX_LINE_BYTES));
        }
        bytes.push(b'\n');
        self.writer
            .write_all(&bytes)
            .await
            .map_err(|error| SpiritError::io("writing ACP stdin", &error))?;
        self.writer
            .flush()
            .await
            .map_err(|error| SpiritError::io("flushing ACP stdin", &error))
    }

    async fn read_value(&mut self) -> Result<Value, SpiritError> {
        let line = timeout(
            self.read_timeout,
            read_line_bounded(&mut self.reader, ACP_MAX_LINE_BYTES),
        )
        .await
        .map_err(|_| {
            SpiritError::new(
                SpiritErrorCode::RemoteTimeout,
                format!(
                    "ACP: the harness did not answer within {}s",
                    self.read_timeout.as_secs()
                ),
            )
            .retryable()
        })??;
        let Some(line) = line else {
            return Err(SpiritError::new(
                SpiritErrorCode::Io,
                "ACP: the harness closed stdout",
            ));
        };
        serde_json::from_str(&line).map_err(|error| acp_protocol_error("stdout", error))
    }
}

impl Drop for AcpClient {
    fn drop(&mut self) {
        // `kill_on_drop(true)` covers the spawned child. Tests use in-memory halves.
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
    }
}

impl AcpInitializeResult {
    fn from_value(value: &Value) -> Result<Self, SpiritError> {
        let protocol_version = value
            .get("protocolVersion")
            .and_then(Value::as_u64)
            .and_then(|version| u16::try_from(version).ok())
            .ok_or_else(|| acp_protocol_error("initialize", "missing protocolVersion"))?;
        let capabilities = value.get("agentCapabilities");
        let supports_load_session = capabilities
            .and_then(|value| value.get("loadSession"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let supports_image_prompts = capabilities
            .and_then(|value| value.get("promptCapabilities"))
            .and_then(|value| value.get("image"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let auth_method_ids = value
            .get("authMethods")
            .and_then(Value::as_array)
            .map(|methods| {
                methods
                    .iter()
                    .filter_map(|method| method.get("id").and_then(Value::as_str))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            protocol_version,
            supports_load_session,
            supports_image_prompts,
            auth_method_ids,
        })
    }
}

fn permission_response(
    request_id: Value,
    request: &AcpPermissionRequest,
    decision: AcpPermissionDecision,
) -> Value {
    let selected = decision.option_kind().and_then(|kind| {
        request
            .options
            .iter()
            .find(|option| option.kind == kind)
            .map(|option| option.option_id.clone())
    });
    match selected {
        Some(option_id) => json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {
                "outcome": { "outcome": "selected", "optionId": option_id },
            },
        }),
        None => json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "result": { "outcome": { "outcome": "cancelled" } },
        }),
    }
}

fn validate_launch_spec(spec: &AcpLaunchSpec) -> Result<(), SpiritError> {
    if spec.program.trim().is_empty() {
        return Err(SpiritError::invalid_input("program", "must not be empty"));
    }
    if spec
        .program
        .chars()
        .chain(spec.args.iter().flat_map(|arg| arg.chars()))
        .any(char::is_control)
    {
        return Err(SpiritError::invalid_input(
            "program",
            "must not contain control characters",
        ));
    }
    Ok(())
}

async fn read_line_bounded<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    cap: usize,
) -> Result<Option<String>, SpiritError> {
    let mut line = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .await
            .map_err(|error| SpiritError::io("reading ACP stdout", &error))?;
        if available.is_empty() {
            if line.is_empty() {
                return Ok(None);
            }
            break;
        }
        if let Some(position) = available.iter().position(|byte| *byte == b'\n') {
            if line.len() + position > cap {
                return Err(SpiritError::output_too_large(cap));
            }
            line.extend_from_slice(&available[..position]);
            reader.consume(position + 1);
            break;
        }
        if line.len() + available.len() > cap {
            return Err(SpiritError::output_too_large(cap));
        }
        line.extend_from_slice(available);
        let consumed = available.len();
        reader.consume(consumed);
    }
    let text = String::from_utf8(line)
        .map_err(|_| acp_protocol_error("stdout", "line is not valid UTF-8"))?;
    Ok(Some(text))
}

fn jsonrpc_error(method: &str, error: &Value) -> SpiritError {
    let code = error.get("code").and_then(Value::as_i64).unwrap_or(0);
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("agent returned an error");
    let message: String = redact(message).chars().take(200).collect();
    SpiritError::new(
        SpiritErrorCode::Parse,
        format!("ACP {method} failed (JSON-RPC {code}): {message}"),
    )
}

fn acp_protocol_error(context: &str, detail: impl std::fmt::Display) -> SpiritError {
    let detail: String = redact(&detail.to_string()).chars().take(200).collect();
    SpiritError::new(SpiritErrorCode::Parse, format!("ACP {context}: {detail}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, duplex};

    async fn write_json(writer: &mut DuplexStream, value: Value) {
        let mut bytes = serde_json::to_vec(&value).unwrap();
        bytes.push(b'\n');
        writer.write_all(&bytes).await.unwrap();
        writer.flush().await.unwrap();
    }

    async fn read_json(reader: &mut BufReader<DuplexStream>) -> Value {
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        serde_json::from_str(line.trim()).unwrap()
    }

    fn pair() -> (AcpClient, BufReader<DuplexStream>, DuplexStream) {
        let (client_read, server_write) = duplex(64 * 1024);
        let (server_read, client_write) = duplex(64 * 1024);
        (
            AcpClient::from_io(client_read, client_write),
            BufReader::new(server_read),
            server_write,
        )
    }

    #[tokio::test]
    async fn mock_process_completes_the_handshake_and_streams_a_turn() {
        let (mut client, mut reader, mut writer) = pair();
        let server = tokio::spawn(async move {
            let initialize = read_json(&mut reader).await;
            assert_eq!(initialize["method"], "initialize");
            assert_eq!(initialize["params"]["protocolVersion"], 1);
            assert_eq!(
                initialize["params"]["clientCapabilities"]["fs"]["readTextFile"],
                false
            );
            assert_eq!(
                initialize["params"]["clientCapabilities"]["terminal"],
                false
            );
            write_json(
                &mut writer,
                json!({
                    "jsonrpc": "2.0",
                    "id": initialize["id"],
                    "result": {
                        "protocolVersion": 1,
                        "agentCapabilities": {
                            "loadSession": true,
                            "promptCapabilities": { "image": true }
                        },
                        "authMethods": [
                            { "id": "agent_login", "name": "Agent login", "metadata": "discard" }
                        ]
                    }
                }),
            )
            .await;

            let new_session = read_json(&mut reader).await;
            assert_eq!(new_session["method"], "session/new");
            assert_eq!(new_session["params"]["cwd"], "/tmp/work");
            write_json(
                &mut writer,
                json!({
                    "jsonrpc": "2.0",
                    "id": new_session["id"],
                    "result": { "sessionId": "sess-1" }
                }),
            )
            .await;

            let prompt = read_json(&mut reader).await;
            assert_eq!(prompt["method"], "session/prompt");
            assert_eq!(prompt["params"]["sessionId"], "sess-1");
            assert_eq!(prompt["params"]["prompt"][0]["type"], "text");
            assert_eq!(prompt["params"]["prompt"][0]["text"], "hello");
            for text in ["Hi ", "there"] {
                write_json(
                    &mut writer,
                    json!({
                        "jsonrpc": "2.0",
                        "method": "session/update",
                        "params": {
                            "sessionId": "sess-1",
                            "update": {
                                "sessionUpdate": "agent_message_chunk",
                                "content": { "type": "text", "text": text }
                            }
                        }
                    }),
                )
                .await;
            }
            write_json(
                &mut writer,
                json!({
                    "jsonrpc": "2.0",
                    "id": 99,
                    "method": "session/request_permission",
                    "params": {
                        "sessionId": "sess-1",
                        "toolCall": { "toolCallId": "t1", "title": "Run tests" },
                        "options": [
                            { "optionId": "allow-once", "name": "Allow once", "kind": "allow_once" },
                            { "optionId": "reject-once", "name": "Reject", "kind": "reject_once" }
                        ]
                    }
                }),
            )
            .await;

            let permission = read_json(&mut reader).await;
            assert_eq!(permission["id"], 99);
            assert_eq!(permission["result"]["outcome"]["outcome"], "selected");
            assert_eq!(permission["result"]["outcome"]["optionId"], "allow-once");

            write_json(
                &mut writer,
                json!({
                    "jsonrpc": "2.0",
                    "id": prompt["id"],
                    "result": { "stopReason": "end_turn" }
                }),
            )
            .await;
        });

        let initialized = client.initialize().await.unwrap();
        assert_eq!(initialized.protocol_version, 1);
        assert!(initialized.compatible());
        assert!(initialized.supports_load_session);
        assert!(initialized.supports_image_prompts);
        assert_eq!(initialized.auth_method_ids, vec!["agent_login"]);

        assert_eq!(client.new_session("/tmp/work").await.unwrap(), "sess-1");
        let mut observer = CollectingObserver {
            decision: AcpPermissionDecision::AllowOnce,
            ..CollectingObserver::default()
        };
        let result = client
            .prompt_with_observer("hello", &mut observer)
            .await
            .unwrap();
        assert_eq!(result.stop_reason, "end_turn");
        assert_eq!(result.session_id, "sess-1");
        assert_eq!(observer.updates.len(), 2);
        assert_eq!(observer.updates[0].text(), Some("Hi "));
        assert_eq!(observer.updates[1].text(), Some("there"));
        assert_eq!(observer.permission_requests.len(), 1);
        assert_eq!(
            observer.permission_requests[0].tool_call["toolCallId"],
            "t1"
        );
        assert_eq!(observer.decisions, vec![AcpPermissionDecision::AllowOnce]);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn mock_process_maps_json_rpc_errors_to_a_typed_failure() {
        let (mut client, mut reader, mut writer) = pair();
        let server = tokio::spawn(async move {
            let initialize = read_json(&mut reader).await;
            write_json(
                &mut writer,
                json!({
                    "jsonrpc": "2.0",
                    "id": initialize["id"],
                    "error": { "code": -32602, "message": "bad params" }
                }),
            )
            .await;
        });

        let error = client.initialize().await.unwrap_err();
        assert_eq!(error.code_str(), "parse");
        assert!(error.message.contains("bad params"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn mock_process_rejects_a_malformed_line() {
        let (mut client, _reader, mut writer) = pair();
        let server = tokio::spawn(async move {
            writer.write_all(b"{not json}\n").await.unwrap();
            writer.flush().await.unwrap();
        });

        let error = client.initialize().await.unwrap_err();
        assert_eq!(error.code_str(), "parse");
        assert!(error.message.starts_with("ACP stdout:"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn mock_process_receives_cancel_as_a_notification() {
        let (mut client, mut reader, mut writer) = pair();
        let server = tokio::spawn(async move {
            let initialize = read_json(&mut reader).await;
            write_json(
                &mut writer,
                json!({
                    "jsonrpc": "2.0",
                    "id": initialize["id"],
                    "result": { "protocolVersion": 1 }
                }),
            )
            .await;
            let new_session = read_json(&mut reader).await;
            write_json(
                &mut writer,
                json!({
                    "jsonrpc": "2.0",
                    "id": new_session["id"],
                    "result": { "sessionId": "sess-1" }
                }),
            )
            .await;
            let cancel = read_json(&mut reader).await;
            assert_eq!(cancel["method"], "session/cancel");
            assert!(cancel.get("id").is_none());
            assert_eq!(cancel["params"]["sessionId"], "sess-1");
        });

        client.initialize().await.unwrap();
        client.new_session("/tmp/work").await.unwrap();
        client.cancel().await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_missing_permission_option_cancels_instead_of_guessing() {
        let (mut client, mut reader, mut writer) = pair();
        let server = tokio::spawn(async move {
            let initialize = read_json(&mut reader).await;
            write_json(
                &mut writer,
                json!({
                    "jsonrpc": "2.0",
                    "id": initialize["id"],
                    "result": { "protocolVersion": 1 }
                }),
            )
            .await;
            let new_session = read_json(&mut reader).await;
            write_json(
                &mut writer,
                json!({
                    "jsonrpc": "2.0",
                    "id": new_session["id"],
                    "result": { "sessionId": "sess-1" }
                }),
            )
            .await;
            let prompt = read_json(&mut reader).await;
            write_json(
                &mut writer,
                json!({
                    "jsonrpc": "2.0",
                    "id": 7,
                    "method": "session/request_permission",
                    "params": {
                        "sessionId": "sess-1",
                        "options": [
                            { "optionId": "reject-once", "name": "Reject", "kind": "reject_once" }
                        ]
                    }
                }),
            )
            .await;
            let permission = read_json(&mut reader).await;
            assert_eq!(permission["result"]["outcome"]["outcome"], "cancelled");
            write_json(
                &mut writer,
                json!({
                    "jsonrpc": "2.0",
                    "id": prompt["id"],
                    "result": { "stopReason": "cancelled" }
                }),
            )
            .await;
        });

        client.initialize().await.unwrap();
        client.new_session("/tmp/work").await.unwrap();
        let mut observer = CollectingObserver {
            decision: AcpPermissionDecision::AllowOnce,
            ..CollectingObserver::default()
        };
        let result = client
            .prompt_with_observer("do it", &mut observer)
            .await
            .unwrap();
        assert_eq!(result.stop_reason, "cancelled");
        server.await.unwrap();
    }
}
