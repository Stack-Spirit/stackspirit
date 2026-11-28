//! ACP implementation of the transport-neutral harness chat adapter.
//!
//! One [`AcpChatAdapter`] owns at most one long-lived [`AcpClient`] and one native ACP
//! session. `start` performs `initialize` and `session/new`; `send_turn` runs one
//! `session/prompt` at a time and maps typed `session/update` notifications to
//! [`HarnessEvent`]; `abort` sends `session/cancel` and reports an unconfirmed stop when
//! the prompt response was not observed; `close` shuts the client down idempotently.
//!
//! The adapter never exposes a raw JSON-RPC frame, a native session id or a launch
//! argument. Permission requests go through a synchronous [`HarnessPermissionPolicy`]
//! whose default cancels; nothing auto-approves.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::{Mutex, Notify, mpsc};
use tokio::time::timeout;
use ulid::Ulid;

use crate::engine::chat::adapter::{
    DenyPermissionPolicy, HarnessAbort, HarnessAccepted, HarnessChatAdapter, HarnessEvent,
    HarnessNativeId, HarnessPermissionDecision, HarnessPermissionPolicy, HarnessPermissionRequest,
    HarnessSession, HarnessStart, HarnessTurn,
};
use crate::engine::chat::harness::HarnessChatGate;
use crate::engine::chat::model::{
    ChatPermissionPrompt, ChatToolActivity, ChatToolStatus, MAX_ACTIVITY_ITEMS, MAX_MESSAGE_CHARS,
    cap_text, display_snippet, normalize_text, validate_user_text,
};
use crate::error::{SpiritError, SpiritErrorCode};
use crate::harnesses::{
    AcpClient, AcpLaunchSpec, AcpObserver, AcpPermissionDecision, AcpPermissionRequest,
    AcpSessionUpdate, AcpUpdate, HarnessManifest, HarnessProtocol,
};
use crate::time::Timestamp;

/// Deadline for one abort, including the `session/cancel` write.
const ABORT_TIMEOUT: Duration = Duration::from_secs(3);
/// Deadline for one close or one best-effort shutdown.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

/// The abort signal and completion handshake for one active turn.
struct ActiveTurn {
    abort: Notify,
    finished: Notify,
    stop_confirmed: AtomicBool,
}

impl ActiveTurn {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            abort: Notify::new(),
            finished: Notify::new(),
            stop_confirmed: AtomicBool::new(false),
        })
    }
}

/// Mutable adapter state behind one async mutex.
struct AdapterState {
    client: Option<AcpClient>,
    pending_client: Option<AcpClient>,
    session: Option<HarnessSession>,
    active: Option<Arc<ActiveTurn>>,
    native_announced: bool,
    closed: bool,
}

/// Immutable adapter configuration plus the shared state.
struct AcpAdapterInner {
    harness_id: String,
    launch: Option<AcpLaunchSpec>,
    state: Mutex<AdapterState>,
}

/// A hermetic ACP chat adapter for one harness session.
///
/// The real constructor takes an [`AcpLaunchSpec`] and spawns a child on `start`. The
/// [`AcpChatAdapter::from_client`] seam accepts an [`AcpClient::from_io`] pair so tests
/// can drive the complete lifecycle over `tokio::io::duplex` without a binary.
pub struct AcpChatAdapter {
    inner: Arc<AcpAdapterInner>,
    permission_policy: Arc<dyn HarnessPermissionPolicy>,
}

impl AcpChatAdapter {
    /// Builds an adapter that spawns `launch` on the first `start`.
    ///
    /// `launch` must not contain a credential in its program or arguments.
    #[must_use]
    pub fn new(harness_id: impl Into<String>, launch: AcpLaunchSpec) -> Self {
        Self::build(harness_id.into(), Some(launch), None)
    }

    /// Builds an adapter over an existing client. This is the hermetic test seam.
    #[must_use]
    pub fn from_client(harness_id: impl Into<String>, client: AcpClient) -> Self {
        Self::build(harness_id.into(), None, Some(client))
    }

    /// Replaces the default deny/cancel policy.
    #[must_use]
    pub fn with_permission_policy(mut self, policy: Arc<dyn HarnessPermissionPolicy>) -> Self {
        self.permission_policy = policy;
        self
    }

    fn build(
        harness_id: String,
        launch: Option<AcpLaunchSpec>,
        pending_client: Option<AcpClient>,
    ) -> Self {
        Self {
            inner: Arc::new(AcpAdapterInner {
                harness_id,
                launch,
                state: Mutex::new(AdapterState {
                    client: None,
                    pending_client,
                    session: None,
                    active: None,
                    native_announced: false,
                    closed: false,
                }),
            }),
            permission_policy: Arc::new(DenyPermissionPolicy),
        }
    }
}

impl fmt::Debug for AcpChatAdapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AcpChatAdapter")
            .field("harness_id", &self.inner.harness_id)
            .field("protocol", &HarnessProtocol::Acp)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl HarnessChatAdapter for AcpChatAdapter {
    fn harness_id(&self) -> &str {
        &self.inner.harness_id
    }

    fn protocol(&self) -> HarnessProtocol {
        HarnessProtocol::Acp
    }

    async fn preflight(&self) -> Result<HarnessChatGate, SpiritError> {
        // Protocol and id only: no launch spec is inspected, no client is spawned and no
        // workspace is touched. ACP stays not-wired until the router slice lands.
        if HarnessManifest::validate_id(&self.inner.harness_id).is_err() {
            return Ok(HarnessChatGate::HarnessMissing {
                harness_id: "invalid".to_owned(),
            });
        }
        Ok(HarnessChatGate::HarnessNotWired {
            harness_id: self.inner.harness_id.clone(),
            protocol: HarnessProtocol::Acp,
        })
    }

    async fn start(&self, input: HarnessStart) -> Result<HarnessSession, SpiritError> {
        let workspace = workspace_string(input.workspace())?;
        let mut state = self.inner.state.lock().await;
        if state.closed {
            return Err(session_lost_error());
        }
        // Idempotent: a second start while the child is alive returns the current
        // session and never initializes or creates a second one.
        if let Some(session) = state.session.clone() {
            return Ok(session);
        }
        let mut client = match (&self.inner.launch, state.pending_client.take()) {
            (Some(launch), None) => AcpClient::spawn(launch).await.map_err(|_| spawn_error())?,
            (None, Some(client)) => client,
            _ => {
                return Err(SpiritError::new(
                    SpiritErrorCode::Internal,
                    "harness_spawn_failed: no ACP client is configured",
                ));
            }
        };
        let initialized = match client.initialize().await {
            Ok(initialized) => initialized,
            Err(error) => {
                shutdown_client(&mut client).await;
                return Err(map_handshake_error(error));
            }
        };
        if !initialized.compatible() {
            shutdown_client(&mut client).await;
            return Err(harness_incompatible_error());
        }
        let native = match client.new_session(&workspace).await {
            Ok(native) => native,
            Err(error) => {
                shutdown_client(&mut client).await;
                return Err(map_handshake_error(error));
            }
        };
        let session = HarnessSession::new(
            self.inner.harness_id.clone(),
            HarnessProtocol::Acp,
            Some(HarnessNativeId::new(native)),
        );
        state.session = Some(session.clone());
        state.client = Some(client);
        state.native_announced = false;
        Ok(session)
    }

    async fn send_turn(
        &self,
        session: &HarnessSession,
        turn: HarnessTurn,
        events: mpsc::Sender<HarnessEvent>,
    ) -> Result<HarnessAccepted, SpiritError> {
        let text = validate_user_text(turn.text())?;
        let mut state = self.inner.state.lock().await;
        if state.closed {
            return Err(session_lost_error());
        }
        let current = state.session.as_ref().ok_or_else(session_lost_error)?;
        if current.harness_id() != self.inner.harness_id
            || current.protocol() != HarnessProtocol::Acp
            || current.native_id() != session.native_id()
        {
            return Err(session_lost_error());
        }
        // Checked and set under the same lock, before a byte is written: a second send
        // can never race the first prompt onto stdin.
        if state.active.is_some() {
            return Err(chat_busy_error());
        }
        let native_id = current
            .native_id()
            .ok_or_else(session_lost_error)?
            .to_owned();
        let client = state.client.take().ok_or_else(session_lost_error)?;
        let active = ActiveTurn::new();
        state.active = Some(Arc::clone(&active));
        if !state.native_announced
            && events
                .try_send(HarnessEvent::NativeSession {
                    id: HarnessNativeId::new(native_id.clone()),
                    title: None,
                })
                .is_ok()
        {
            state.native_announced = true;
        }
        drop(state);

        let message_id = format!("msg_{}", Ulid::generate());
        let task = TurnTask {
            inner: Arc::clone(&self.inner),
            client,
            active,
            native_id,
            text,
            events,
            message_id: message_id.clone(),
            policy: Arc::clone(&self.permission_policy),
        };
        tokio::spawn(task.run());
        Ok(HarnessAccepted { message_id })
    }

    async fn abort(&self, session: &HarnessSession) -> Result<HarnessAbort, SpiritError> {
        let active = {
            let state = self.inner.state.lock().await;
            if let Some(current) = state.session.as_ref()
                && (current.harness_id() != self.inner.harness_id
                    || current.protocol() != HarnessProtocol::Acp
                    || current.native_id() != session.native_id())
            {
                return Err(session_lost_error());
            }
            state.active.clone()
        };
        let Some(active) = active else {
            // Nothing is streaming; there is no turn left to stop.
            return Ok(HarnessAbort::stopped());
        };
        let finished = active.finished.notified();
        active.abort.notify_one();
        let Ok(()) = timeout(ABORT_TIMEOUT, finished).await else {
            // The cancel was requested but the turn task did not finish in the bound.
            return Ok(HarnessAbort::unconfirmed());
        };
        Ok(HarnessAbort {
            stop_confirmed: active.stop_confirmed.load(Ordering::SeqCst),
        })
    }

    async fn close(&self, _session: &HarnessSession) -> Result<(), SpiritError> {
        let (active, client, pending) = {
            let mut state = self.inner.state.lock().await;
            if state.closed
                && state.active.is_none()
                && state.client.is_none()
                && state.pending_client.is_none()
            {
                return Ok(());
            }
            state.closed = true;
            state.session = None;
            state.native_announced = false;
            (
                state.active.clone(),
                state.client.take(),
                state.pending_client.take(),
            )
        };
        if let Some(active) = active {
            active.abort.notify_one();
            let _ = timeout(CLOSE_TIMEOUT, active.finished.notified()).await;
        }
        if let Some(mut client) = client {
            shutdown_client(&mut client).await;
        }
        if let Some(mut client) = pending {
            shutdown_client(&mut client).await;
        }
        // A task that finished just after the wait may have returned its client before it
        // observed `closed`; reap that one too. The task itself shuts down when it sees
        // `closed`, so this is normally `None`.
        let leftover = { self.inner.state.lock().await.client.take() };
        if let Some(mut client) = leftover {
            shutdown_client(&mut client).await;
        }
        Ok(())
    }
}

/// One asynchronous prompt turn. It owns the client until the turn settles so `abort`
/// can signal it without touching the serialized JSON-RPC stream.
struct TurnTask {
    inner: Arc<AcpAdapterInner>,
    client: AcpClient,
    active: Arc<ActiveTurn>,
    native_id: String,
    text: String,
    events: mpsc::Sender<HarnessEvent>,
    message_id: String,
    policy: Arc<dyn HarnessPermissionPolicy>,
}

enum TurnOutcome {
    Completed(Result<crate::harnesses::AcpPromptResult, SpiritError>),
    Aborted,
}

impl TurnTask {
    async fn run(mut self) {
        let _ = self.events.try_send(HarnessEvent::MessageStart {
            message_id: self.message_id.clone(),
        });
        let mut observer = AcpEventObserver::new(
            self.events.clone(),
            self.message_id.clone(),
            self.native_id.clone(),
            Arc::clone(&self.policy),
        );
        let outcome = {
            let prompt = self.client.prompt_with_observer(&self.text, &mut observer);
            tokio::pin!(prompt);
            tokio::select! {
                biased;
                result = &mut prompt => TurnOutcome::Completed(result),
                _ = self.active.abort.notified() => TurnOutcome::Aborted,
            }
        };
        let mut terminal = None;
        let mut fatal = false;
        match outcome {
            TurnOutcome::Completed(Ok(result)) => {
                if result.session_id == self.native_id {
                    self.active.stop_confirmed.store(true, Ordering::SeqCst);
                    terminal = Some(HarnessEvent::Done {
                        message_id: self.message_id.clone(),
                        truncated: observer.truncated(),
                    });
                } else {
                    // A different native session is a lost session, never a retry or an
                    // invented id.
                    terminal = Some(HarnessEvent::Error {
                        error: session_lost_error(),
                    });
                    fatal = true;
                }
            }
            TurnOutcome::Completed(Err(error)) => {
                terminal = Some(HarnessEvent::Error {
                    error: map_turn_error(error),
                });
                fatal = true;
            }
            TurnOutcome::Aborted => {
                // The prompt future was dropped at the end of the select block, so the
                // client is free to write the cancel notification. We do not observe the
                // cancelled prompt response; `abort` reports that stop as unconfirmed.
                match timeout(ABORT_TIMEOUT, self.client.cancel()).await {
                    Ok(Ok(())) => {}
                    _ => fatal = true,
                }
            }
        }
        let events = self.events.clone();
        self.finish(fatal).await;
        if let Some(event) = terminal {
            // Emit the terminal event only after the active turn is cleared so a caller
            // that receives it can start the next turn immediately.
            let _ = events.try_send(event);
        }
    }

    async fn finish(self, fatal: bool) {
        let TurnTask {
            inner,
            mut client,
            active,
            ..
        } = self;
        let closing = { inner.state.lock().await.closed };
        if fatal || closing {
            shutdown_client(&mut client).await;
            let mut state = inner.state.lock().await;
            state.client = None;
            state.active = None;
            if fatal {
                state.session = None;
            }
        } else {
            let mut state = inner.state.lock().await;
            state.client = Some(client);
            state.active = None;
        }
        active.finished.notify_one();
    }
}

/// The synchronous ACP observer: maps updates, tool rows and permissions to bounded
/// [`HarnessEvent`] values. It never keeps or forwards a raw frame.
struct AcpEventObserver {
    events: mpsc::Sender<HarnessEvent>,
    message_id: String,
    native_id: String,
    policy: Arc<dyn HarnessPermissionPolicy>,
    text_chars: usize,
    truncated: bool,
    tool_rows: usize,
    tool_seq: usize,
    last_tool_key: Option<String>,
    tools: HashMap<String, ToolRow>,
}

#[derive(Clone)]
struct ToolRow {
    name: String,
    summary: Option<String>,
    emitted: bool,
}

impl AcpEventObserver {
    fn new(
        events: mpsc::Sender<HarnessEvent>,
        message_id: String,
        native_id: String,
        policy: Arc<dyn HarnessPermissionPolicy>,
    ) -> Self {
        Self {
            events,
            message_id,
            native_id,
            policy,
            text_chars: 0,
            truncated: false,
            tool_rows: 0,
            tool_seq: 0,
            last_tool_key: None,
            tools: HashMap::new(),
        }
    }

    fn truncated(&self) -> bool {
        self.truncated
    }

    fn next_tool_key(&mut self) -> String {
        self.tool_seq = self.tool_seq.saturating_add(1);
        format!("tool_{}", self.tool_seq)
    }

    fn send(&self, event: HarnessEvent) {
        // The observer is synchronous inside the JSON-RPC read loop. The provider owns
        // the channel and drains it; a full channel never blocks protocol reads.
        let _ = self.events.try_send(event);
    }

    fn message_delta(&mut self, text: &str) {
        let text = normalize_text(text);
        if text.is_empty() {
            return;
        }
        let remaining = MAX_MESSAGE_CHARS.saturating_sub(self.text_chars);
        if remaining == 0 {
            self.truncated = true;
            return;
        }
        let (chunk, capped) = cap_text(&text, remaining);
        if capped {
            self.truncated = true;
        }
        self.text_chars += chunk.chars().count();
        self.send(HarnessEvent::TextDelta {
            message_id: self.message_id.clone(),
            part_id: None,
            text: chunk,
        });
    }

    fn tool_call(&mut self, tool_call_id: &str, title: &str, kind: &str, status: &str) {
        // The typed ACP client reports an empty tool id when the agent uses the
        // camelCase `toolCallId` field. Fall back to an ordered core id so two tool
        // calls never collapse into one row; a later `tool_call_update` with an empty
        // id follows the same order and updates the most recent row.
        let key = if tool_call_id.trim().is_empty() {
            self.next_tool_key()
        } else {
            tool_key(tool_call_id)
        };
        self.last_tool_key = Some(key.clone());
        let existing = self.tools.get(&key).cloned();
        let name = tool_name(kind, title);
        let summary = tool_summary(title);
        let emitted = existing.as_ref().is_some_and(|row| row.emitted);
        let should_emit = emitted || self.tool_rows < MAX_ACTIVITY_ITEMS;
        if should_emit && !emitted {
            self.tool_rows += 1;
        }
        self.tools.insert(
            key.clone(),
            ToolRow {
                name: name.clone(),
                summary: summary.clone(),
                emitted: should_emit,
            },
        );
        if should_emit {
            self.send(HarnessEvent::Tool {
                activity: ChatToolActivity {
                    id: key,
                    name,
                    status: map_tool_status(status),
                    summary,
                },
            });
        }
    }

    fn tool_update(&mut self, tool_call_id: &str, status: &str) {
        let key = if tool_call_id.trim().is_empty() {
            self.last_tool_key
                .clone()
                .unwrap_or_else(|| self.next_tool_key())
        } else {
            tool_key(tool_call_id)
        };
        self.last_tool_key = Some(key.clone());
        let existing = self.tools.get(&key).cloned();
        let name = existing
            .as_ref()
            .map_or_else(|| "tool".to_owned(), |row| row.name.clone());
        let summary = existing.as_ref().and_then(|row| row.summary.clone());
        let emitted = existing.as_ref().is_some_and(|row| row.emitted);
        let should_emit = emitted || self.tool_rows < MAX_ACTIVITY_ITEMS;
        if should_emit && !emitted {
            self.tool_rows += 1;
        }
        self.tools.insert(
            key.clone(),
            ToolRow {
                name: name.clone(),
                summary: summary.clone(),
                emitted: should_emit,
            },
        );
        if should_emit {
            self.send(HarnessEvent::Tool {
                activity: ChatToolActivity {
                    id: key,
                    name,
                    status: map_tool_status(status),
                    summary,
                },
            });
        }
    }

    fn permission(&mut self, request: &AcpPermissionRequest) -> AcpPermissionDecision {
        let permission = tool_call_field(&request.tool_call, "kind")
            .or_else(|| tool_call_field(&request.tool_call, "title"))
            .map_or_else(|| "tool".to_owned(), |value| display_snippet(&value));
        let patterns = permission_patterns(&request.tool_call);
        let prompt = ChatPermissionPrompt {
            id: format!("perm_{}", Ulid::generate()),
            permission: permission.clone(),
            patterns: patterns.clone(),
            created_at: Timestamp::now(),
        };
        self.send(HarnessEvent::Permission { prompt });
        let decision = self.policy.decide(&HarnessPermissionRequest {
            permission,
            patterns,
        });
        map_permission_decision(decision)
    }
}

impl AcpObserver for AcpEventObserver {
    fn on_update(&mut self, update: AcpSessionUpdate) {
        // Only this adapter's native session may contribute events. The envelope's id is
        // never copied into an event, error or log line.
        if update.session_id != self.native_id {
            return;
        }
        match update.update {
            AcpUpdate::AgentMessageChunk { content } => {
                if let Some(text) = content.text() {
                    self.message_delta(text);
                }
            }
            AcpUpdate::ToolCall {
                tool_call_id,
                title,
                kind,
                status,
            } => self.tool_call(&tool_call_id, &title, &kind, &status),
            AcpUpdate::ToolCallUpdate {
                tool_call_id,
                status,
            } => self.tool_update(&tool_call_id, &status),
            // Reasoning is not assistant output in this phase; plans, commands, mode
            // changes and unknown kinds are ignored rather than guessed as text.
            AcpUpdate::AgentThoughtChunk { .. }
            | AcpUpdate::UserMessageChunk { .. }
            | AcpUpdate::Plan { .. }
            | AcpUpdate::AvailableCommandsUpdate { .. }
            | AcpUpdate::CurrentModeUpdate { .. }
            | AcpUpdate::Unknown => {}
        }
    }

    fn on_permission(&mut self, request: &AcpPermissionRequest) -> AcpPermissionDecision {
        if !request.session_id.is_empty() && request.session_id != self.native_id {
            // Never answer a permission prompt for another native session.
            return AcpPermissionDecision::Cancel;
        }
        self.permission(request)
    }
}

fn workspace_string(workspace: &std::path::Path) -> Result<String, SpiritError> {
    let value = workspace
        .to_str()
        .ok_or_else(|| SpiritError::invalid_input("workspace", "must be valid UTF-8"))?;
    if value.trim().is_empty() {
        return Err(SpiritError::invalid_input("workspace", "must not be empty"));
    }
    Ok(value.to_owned())
}

fn tool_key(id: &str) -> String {
    let normalized = normalize_text(id);
    if normalized.trim().is_empty() {
        return "tool".to_owned();
    }
    cap_text(&normalized, 64).0
}

fn tool_name(kind: &str, title: &str) -> String {
    let candidate = if !kind.trim().is_empty() {
        kind
    } else if !title.trim().is_empty() {
        title
    } else {
        "tool"
    };
    let snippet = display_snippet(candidate);
    if snippet.is_empty() {
        "tool".to_owned()
    } else {
        snippet
    }
}

fn tool_summary(title: &str) -> Option<String> {
    if title.trim().is_empty() {
        return None;
    }
    let snippet = display_snippet(title);
    (!snippet.is_empty()).then_some(snippet)
}

fn tool_call_field(tool_call: &Value, field: &str) -> Option<String> {
    tool_call
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn permission_patterns(tool_call: &Value) -> Vec<String> {
    let mut patterns = Vec::new();
    for field in ["title", "kind"] {
        if let Some(value) = tool_call_field(tool_call, field) {
            let snippet = display_snippet(&value);
            if !snippet.is_empty() && !patterns.contains(&snippet) {
                patterns.push(snippet);
            }
        }
    }
    patterns.truncate(MAX_ACTIVITY_ITEMS);
    patterns
}

fn map_tool_status(status: &str) -> ChatToolStatus {
    match status {
        "pending" => ChatToolStatus::Pending,
        "in_progress" | "running" => ChatToolStatus::Running,
        "completed" | "success" | "succeeded" => ChatToolStatus::Completed,
        "failed" | "error" => ChatToolStatus::Error,
        _ => ChatToolStatus::Pending,
    }
}

fn map_permission_decision(decision: HarnessPermissionDecision) -> AcpPermissionDecision {
    match decision {
        HarnessPermissionDecision::AllowOnce => AcpPermissionDecision::AllowOnce,
        HarnessPermissionDecision::AllowAlways => AcpPermissionDecision::AllowAlways,
        HarnessPermissionDecision::RejectOnce => AcpPermissionDecision::RejectOnce,
        HarnessPermissionDecision::RejectAlways => AcpPermissionDecision::RejectAlways,
        HarnessPermissionDecision::Cancel => AcpPermissionDecision::Cancel,
    }
}

async fn shutdown_client(client: &mut AcpClient) {
    let _ = timeout(CLOSE_TIMEOUT, client.shutdown()).await;
}

fn spawn_error() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Engine,
        "harness_spawn_failed: the harness process could not start",
    )
    .with_hint("Check that the harness is installed and signed in.")
}

fn harness_incompatible_error() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Gate,
        "harness_incompatible: the harness negotiated an unsupported ACP version",
    )
    .with_hint("Update the harness or StackSpirit and retry.")
}

fn harness_protocol_error() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Parse,
        "harness_protocol_error: the harness sent an invalid ACP message",
    )
}

fn harness_output_too_large_error() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Parse,
        "harness_output_too_large: the harness message exceeded the cap",
    )
}

fn harness_turn_timeout_error() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::RemoteTimeout,
        "harness_turn_timeout: the harness turn did not finish in time",
    )
    .retryable()
}

fn harness_stream_lost_error() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::RemoteTimeout,
        "harness_stream_lost: the harness connection stopped before the turn finished",
    )
    .retryable()
}

fn harness_exited_error() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Engine,
        "harness_exited: the harness closed its connection",
    )
}

fn session_lost_error() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::NotFound,
        "harness_session_lost: the harness session is not running",
    )
    .with_hint("Start the conversation again.")
}

fn chat_busy_error() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Conflict,
        "chat_busy: wait for the current reply or stop it",
    )
    .with_hint("Send one turn at a time.")
}

/// Maps an ACP handshake failure to a fixed, redacted error. No JSON-RPC detail, path,
/// native id or protocol frame crosses the adapter boundary.
fn map_handshake_error(error: SpiritError) -> SpiritError {
    match error.code {
        SpiritErrorCode::RemoteTimeout => harness_stream_lost_error(),
        SpiritErrorCode::Io => harness_exited_error(),
        SpiritErrorCode::Parse if error.message.contains("output_too_large") => {
            harness_output_too_large_error()
        }
        SpiritErrorCode::Parse => harness_protocol_error(),
        SpiritErrorCode::NotFound | SpiritErrorCode::Conflict => session_lost_error(),
        _ => harness_protocol_error(),
    }
}

/// Maps one turn failure, keeping the documented `harness_*` prefixes.
fn map_turn_error(error: SpiritError) -> SpiritError {
    match error.code {
        SpiritErrorCode::RemoteTimeout => harness_turn_timeout_error(),
        SpiritErrorCode::Io => harness_exited_error(),
        SpiritErrorCode::Parse if error.message.contains("output_too_large") => {
            harness_output_too_large_error()
        }
        SpiritErrorCode::Parse => harness_protocol_error(),
        SpiritErrorCode::NotFound | SpiritErrorCode::Conflict => session_lost_error(),
        _ => harness_protocol_error(),
    }
}
