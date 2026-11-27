//! Safe chat types mirrored from `docs/ORCHESTRATOR-CHAT.md` §4.
//!
//! Everything in this module may cross to the renderer. It deliberately has no
//! endpoint URL, port, credential, native session id, workspace or binary path. The
//! transport keeps those values private in [`super::local`].
//!
//! Cooperative ownership is display-only here: [`ChatOwnershipView`] carries the safe
//! owner metadata and [`ChatAccess`] says whether this caller may write. Neither type
//! carries surface identity, the mirrored log id, an epoch, a lease timestamp or a token.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::{SpiritError, SpiritErrorCode};
use crate::time::Timestamp;

/// The per-message text cap from `docs/ORCHESTRATOR-CHAT.md` §2.2.5.
pub const MAX_MESSAGE_CHARS: usize = 32_000;

/// The largest accepted user turn.
pub const MAX_USER_MESSAGE_CHARS: usize = 32_000;

/// The default log page size from §2.2.4.
pub const DEFAULT_LOG_LIMIT: usize = 50;

/// The hard log page cap from §2.2.4.
pub const MAX_LOG_LIMIT: usize = 200;

/// The longest accepted project name.
const MAX_PROJECT_NAME_CHARS: usize = 64;

/// A shell chat routing key.
///
/// The wire form is `setup`, `global`, or `project:<PROJECT_NAME>`. It is not an
/// OpenCode native session id and not a VOWP session id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum ChatSessionId {
    /// The local first-run setup assistant.
    Setup,
    /// The global orchestrator session.
    Global,
    /// One project orchestrator session.
    Project(String),
}

impl ChatSessionId {
    /// Parses and validates the wire form.
    pub fn parse(value: &str) -> Result<Self, SpiritError> {
        match value {
            "setup" => Ok(Self::Setup),
            "global" => Ok(Self::Global),
            other => {
                let Some(name) = other.strip_prefix("project:") else {
                    return Err(SpiritError::invalid_input(
                        "session",
                        "expected setup, global or project:<name>",
                    ));
                };
                validate_project_name(name)?;
                Ok(Self::Project(name.to_owned()))
            }
        }
    }

    /// Returns the wire string.
    #[must_use]
    pub fn as_wire(&self) -> String {
        match self {
            Self::Setup => "setup".to_owned(),
            Self::Global => "global".to_owned(),
            Self::Project(name) => format!("project:{name}"),
        }
    }

    /// Returns the project name for a project session.
    #[must_use]
    pub fn project_name(&self) -> Option<&str> {
        match self {
            Self::Project(name) => Some(name),
            Self::Setup | Self::Global => None,
        }
    }
}

impl fmt::Display for ChatSessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_wire())
    }
}

impl From<ChatSessionId> for String {
    fn from(value: ChatSessionId) -> Self {
        value.as_wire()
    }
}

impl TryFrom<String> for ChatSessionId {
    type Error = SpiritError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

/// Validates a VOWP-style project slug. The renderer only ever sends a name.
pub fn validate_project_name(name: &str) -> Result<(), SpiritError> {
    if name.is_empty() || name.chars().count() > MAX_PROJECT_NAME_CHARS {
        return Err(SpiritError::invalid_input(
            "project",
            "must be 1 to 64 characters",
        ));
    }
    if name == "." || name == ".." {
        return Err(SpiritError::invalid_input(
            "project",
            "must be a project name, not a path",
        ));
    }
    let valid = name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'));
    if !valid {
        return Err(SpiritError::invalid_input(
            "project",
            "may contain only letters, digits, '-', '_' and '.'",
        ));
    }
    if !name
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphanumeric())
    {
        return Err(SpiritError::invalid_input(
            "project",
            "must start with a letter or digit",
        ));
    }
    Ok(())
}

/// Where the transport actually runs a session. Never a user toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatSessionLocation {
    /// The bundled StackSpirit Engine on this Mac.
    Local,
    /// A VOWP-reported orchestrator reached through the server path.
    Server,
}

/// Whether this caller may write to a local orchestrator.
///
/// This is a display/report value, not a takeover primitive. A `read_only` session keeps
/// streaming and reading its mirrored log; the composer is what the UI must disable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatAccess {
    /// The bundled engine, or a local orchestrator whose lease this caller holds.
    Owner,
    /// Another surface holds the cooperative lease; this caller may only read.
    #[default]
    ReadOnly,
}

/// Display-only cooperative ownership reported by VOWP.
///
/// The values describe another writer so the UI can name it and make the previous writer
/// read-only. They are cooperative metadata, not an enforcement mechanism, and never
/// include a surface id, a session/log id, an epoch, a lease timestamp or a token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatOwnershipView {
    /// Surface kind such as `opencode`, or a newer safe kind string from VOWP.
    pub kind: String,
    /// Human label for the holder, when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Holder description, when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holder: Option<String>,
    /// Node that holds the lease, when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    /// Cooperative ownership mode, when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Owner state, when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// Short activity description, when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<String>,
}

/// The chat state machine from §4.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatSessionState {
    /// No native session has been opened in this app run.
    Idle,
    /// Attaching, waiting for health, or reconnecting.
    Connecting,
    /// A turn is in flight.
    Streaming,
    /// Attached and idle; the composer is enabled.
    Ready,
    /// The owner stopped the turn or the engine was stopped cleanly.
    Stopped,
    /// A safe typed error is attached.
    Error,
}

/// Who produced a message. The real path never constructs [`Self::Preview`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatMessageOrigin {
    /// Typed by the owner.
    User,
    /// Produced by the model.
    Model,
    /// App-owned copy or a card, never model text.
    App,
    /// Preview-only demo text; forbidden in the real path.
    Preview,
}

/// The chat role on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    /// The owner.
    User,
    /// The orchestrator model.
    Assistant,
}

/// Why a model is usable without visiting Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatModelTier {
    /// Verified free with the bundled engine; no provider key required.
    Included,
    /// Requires a provider key already stored in the OS keychain.
    Byok,
    /// Chosen by the VOWP orchestrator, not by this app.
    Orchestrator,
}

/// The model selected for the session, without a provider key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatModelView {
    /// Engine-native id, `provider/model`.
    pub id: String,
    /// Display label.
    pub label: String,
    /// Provider id.
    pub provider: String,
    /// Included, BYOK or orchestrator-owned.
    pub tier: ChatModelTier,
    /// Context window, when the engine reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_tokens: Option<u64>,
    /// Reasoning-effort variants the engine declares for this model. Absent when the model
    /// has no reasoning support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variants: Option<Vec<String>>,
}

/// Token and context usage reported by the OpenCode session.
///
/// Every counter is optional: `null` means the engine did not report it, never zero.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChatUsageView {
    /// Context window occupancy of the latest assistant turn, when reported.
    pub context_tokens_used: Option<u64>,
    /// The selected model's context window, when known.
    pub context_limit: Option<u64>,
    /// Input tokens reported across assistant messages.
    pub input_tokens: Option<u64>,
    /// Output tokens reported across assistant messages.
    pub output_tokens: Option<u64>,
    /// Prompt-cache read tokens reported across assistant messages.
    pub cache_read_tokens: Option<u64>,
    /// User turns in the conversation.
    pub turns: u64,
    /// Provider-reported cost in USD, when at least one assistant message reported one.
    pub cost_usd: Option<f64>,
}

/// One safe chat message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Stable across reconnects; a native message id or `msg_<ULID>`.
    pub id: String,
    /// User or assistant.
    pub role: ChatRole,
    /// Plain-text paragraphs. Never HTML, markdown or a tool dump.
    pub text: Vec<String>,
    /// Creation time as canonical RFC 3339 UTC with millis.
    pub created_at: Timestamp,
    /// Who produced the message.
    pub origin: ChatMessageOrigin,
    /// True while assistant text is still arriving.
    pub streaming: bool,
    /// Compact, redacted tool activity for an assistant turn. Absent when the message
    /// carries no tool parts (or an event could not know them), so a shell update never
    /// erases tool rows an earlier update delivered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ChatToolActivity>>,
}

/// Lifecycle of one tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatToolStatus {
    /// Queued, not started.
    Pending,
    /// Executing.
    Running,
    /// Finished successfully.
    Completed,
    /// Failed.
    Error,
}

/// One compact tool-call row. It never carries raw input, output or error text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatToolActivity {
    /// The native part id.
    pub id: String,
    /// The tool name (for example `bash`), capped and redacted.
    pub name: String,
    /// Lifecycle status.
    pub status: ChatToolStatus,
    /// A short redacted hint such as the command or path, when one is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// One permission request OpenCode is waiting on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatPermissionPrompt {
    /// The native permission request id (`per_...`). Needed to reply; not a secret.
    pub id: String,
    /// The requested action, such as `bash` or `external_directory`.
    pub permission: String,
    /// The requested patterns, redacted and capped for display.
    pub patterns: Vec<String>,
    /// When the core first saw the request.
    pub created_at: Timestamp,
}

/// Why OpenCode is retrying a turn, when it names a provider action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatRetryAction {
    /// Stable reason such as `free_tier_limit` or `rate_limit`.
    pub reason: String,
    /// Short title suitable for a notice heading.
    pub title: String,
    /// One or two calm sentences from the provider.
    pub message: String,
    /// Provider id such as `opencode`.
    pub provider: String,
}

/// One active `session.status` retry, mirrored into the safe view.
///
/// OpenCode can sleep for hours before its next attempt (the free-tier limit is the
/// common case). The composer must show this instead of a permanent “Replying…” line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatRetryStatus {
    /// 0-based attempt counter from OpenCode.
    pub attempt: u32,
    /// A short, redacted provider message; never raw error JSON.
    pub message: String,
    /// Epoch milliseconds of the next attempt, when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<u64>,
    /// Provider action such as the free-tier upgrade prompt, when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<ChatRetryAction>,
}

/// How the owner answers a permission prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatPermissionDecision {
    /// Allow this one request.
    Once,
    /// Allow this and matching future requests in the session.
    Always,
    /// Refuse the request.
    Reject,
}

impl ChatPermissionDecision {
    /// The OpenCode wire word.
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Always => "always",
            Self::Reject => "reject",
        }
    }
}

/// Longest display string for a tool summary or permission pattern.
pub const MAX_ACTIVITY_CHARS: usize = 160;
/// Most tool rows and permission patterns kept per message or prompt.
pub const MAX_ACTIVITY_ITEMS: usize = 64;

/// Redacts and shortens untrusted text for a one-line display.
#[must_use]
pub fn display_snippet(input: &str) -> String {
    let redacted = crate::error::redact(input);
    let one_line = redacted.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= MAX_ACTIVITY_CHARS {
        return one_line;
    }
    let mut cut: String = one_line.chars().take(MAX_ACTIVITY_CHARS - 1).collect();
    cut.push('…');
    cut
}

impl ChatMessage {
    /// Builds a message from plain text, splitting and capping paragraphs.
    #[must_use]
    pub fn from_text(
        id: impl Into<String>,
        role: ChatRole,
        origin: ChatMessageOrigin,
        text: &str,
        created_at: Timestamp,
    ) -> Self {
        let (normalized, _) = cap_text(&normalize_text(text), MAX_MESSAGE_CHARS);
        Self {
            id: id.into(),
            role,
            text: paragraphs(&normalized),
            created_at,
            origin,
            streaming: false,
            tools: None,
        }
    }

    /// Builds a user message.
    #[must_use]
    pub fn user(id: impl Into<String>, text: &str, created_at: Timestamp) -> Self {
        Self::from_text(
            id,
            ChatRole::User,
            ChatMessageOrigin::User,
            text,
            created_at,
        )
    }

    /// Builds a model message.
    #[must_use]
    pub fn model(id: impl Into<String>, text: &str, created_at: Timestamp) -> Self {
        Self::from_text(
            id,
            ChatRole::Assistant,
            ChatMessageOrigin::Model,
            text,
            created_at,
        )
    }

    /// Replaces the text with a capped, normalized copy.
    ///
    /// Returns true when the cap truncated the text.
    pub fn replace_text(&mut self, text: &str) -> bool {
        let (normalized, truncated) = cap_text(&normalize_text(text), MAX_MESSAGE_CHARS);
        self.text = paragraphs(&normalized);
        truncated
    }

    /// Appends streamed text and returns true when the cap truncated it.
    ///
    /// A delta only appends, matching `docs/ORCHESTRATOR-CHAT.md` §2.2.5.
    pub fn append_delta(&mut self, delta: &str) -> bool {
        let delta = normalize_text(delta);
        if delta.is_empty() {
            return false;
        }
        let mut joined = self.text.join("\n\n");
        joined.push_str(&delta);
        let (joined, truncated) = cap_text(&joined, MAX_MESSAGE_CHARS);
        self.text = paragraphs(&joined);
        truncated
    }

    /// Returns the joined plain text.
    #[must_use]
    pub fn joined_text(&self) -> String {
        self.text.join("\n\n")
    }

    /// Returns the character count of the joined text.
    #[must_use]
    pub fn char_count(&self) -> usize {
        self.text
            .iter()
            .map(|paragraph| paragraph.chars().count())
            .sum()
    }
}

/// One safe chat session view. It is derived from the transport, not a toggle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatSessionView {
    /// Shell routing key.
    pub session: ChatSessionId,
    /// The actual transport location.
    pub location: ChatSessionLocation,
    /// State machine value.
    pub state: ChatSessionState,
    /// True when a native session is attached and usable.
    pub attached: bool,
    /// False when the endpoint has no usable SSE; the UI says so.
    pub streaming_supported: bool,
    /// Selected model, when one is selected.
    pub model: Option<ChatModelView>,
    /// Verified models this session's engine can use right now.
    ///
    /// `None` means the transport cannot report a verified list (an orchestrator owns
    /// its own config). An empty list is an honest "no usable model".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_models: Option<Vec<ChatModelView>>,
    /// Token and context usage reported by the engine, or `None` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<ChatUsageView>,
    /// When the session was first attached in this app run.
    pub started_at: Option<Timestamp>,
    /// The last event time.
    pub last_event_at: Option<Timestamp>,
    /// A safe error, when the state is `error` or a stop was unconfirmed.
    pub error: Option<SpiritError>,
    /// Permission requests OpenCode is waiting on for this session, oldest first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_permissions: Option<Vec<ChatPermissionPrompt>>,
    /// An active `session.status` retry, when OpenCode is sleeping before another try.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry: Option<ChatRetryStatus>,
    /// Display-only cooperative owner, when VOWP reports one.
    ///
    /// The bundled engine is the app's own writer and reports `None` here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ownership: Option<ChatOwnershipView>,
    /// Whether this caller may write; VOWP orchestrators default to `read_only`.
    #[serde(default)]
    pub access: ChatAccess,
}

/// Why a session closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatCloseReason {
    /// The owner stopped the turn.
    Stopped,
    /// The bundled engine stopped.
    EngineStopped,
    /// The server host disconnected.
    HostDisconnected,
    /// The native session was deleted or recreated.
    SessionLost,
    /// The turn failed.
    Failed,
}

/// One event from the chat event bus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatEvent {
    /// A session metadata change.
    Session {
        /// Monotonic per-bridge revision.
        revision: u64,
        /// The full safe view.
        session: Box<ChatSessionView>,
    },
    /// An authoritative message replacement.
    Message {
        /// Monotonic per-bridge revision.
        revision: u64,
        /// Shell routing key.
        session: ChatSessionId,
        /// The message to upsert by id.
        message: ChatMessage,
    },
    /// An append-only text delta.
    Delta {
        /// Monotonic per-bridge revision.
        revision: u64,
        /// Shell routing key.
        session: ChatSessionId,
        /// The assistant message.
        message_id: String,
        /// The OpenCode text part, when known.
        part_id: Option<String>,
        /// The text to append.
        text: String,
    },
    /// The turn is complete.
    Done {
        /// Monotonic per-bridge revision.
        revision: u64,
        /// Shell routing key.
        session: ChatSessionId,
        /// The assistant message id, or an empty string when no text arrived.
        message_id: String,
        /// True when the core capped the text.
        truncated: bool,
    },
    /// A safe error.
    Error {
        /// Monotonic per-bridge revision.
        revision: u64,
        /// Shell routing key.
        session: ChatSessionId,
        /// The typed error.
        error: SpiritError,
    },
    /// The session detached.
    Closed {
        /// Monotonic per-bridge revision.
        revision: u64,
        /// Shell routing key.
        session: ChatSessionId,
        /// Why it closed.
        reason: ChatCloseReason,
    },
}

impl ChatEvent {
    /// Returns the monotonic revision.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        match self {
            Self::Session { revision, .. }
            | Self::Message { revision, .. }
            | Self::Delta { revision, .. }
            | Self::Done { revision, .. }
            | Self::Error { revision, .. }
            | Self::Closed { revision, .. } => *revision,
        }
    }

    /// Returns the shell session key.
    #[must_use]
    pub fn session(&self) -> &ChatSessionId {
        match self {
            Self::Session { session, .. } => &session.session,
            Self::Message { session, .. }
            | Self::Delta { session, .. }
            | Self::Done { session, .. }
            | Self::Error { session, .. }
            | Self::Closed { session, .. } => session,
        }
    }

    /// Returns the stable wire kind.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Session { .. } => "session",
            Self::Message { .. } => "message",
            Self::Delta { .. } => "delta",
            Self::Done { .. } => "done",
            Self::Error { .. } => "error",
            Self::Closed { .. } => "closed",
        }
    }
}

/// What starts a chat session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatStartInput {
    /// Shell routing key.
    pub session: ChatSessionId,
    /// The project name for `project:<name>`; null for setup and global.
    #[serde(default)]
    pub project: Option<String>,
    /// A transport hint. Setup is always local.
    pub location: ChatSessionLocation,
    /// Reuse the stored native session when true (default).
    #[serde(default)]
    pub resume: Option<bool>,
}

impl ChatStartInput {
    /// Returns the resume flag, defaulting to true.
    #[must_use]
    pub fn resumes(&self) -> bool {
        self.resume.unwrap_or(true)
    }
}

/// One user turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatSendInput {
    /// Shell routing key.
    pub session: ChatSessionId,
    /// The user text.
    pub text: String,
    /// Selected reasoning-effort variant, forwarded to OpenCode as `variant`.
    #[serde(default)]
    pub variant: Option<String>,
    /// Image/file attachments forwarded as OpenCode `file` parts.
    #[serde(default)]
    pub attachments: Vec<ChatAttachment>,
}

/// One attachment on a user turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatAttachment {
    /// File name shown to the model.
    pub filename: String,
    /// MIME type, e.g. `image/png`.
    pub mime: String,
    /// `data:<mime>;base64,...` URI.
    pub url: String,
}

/// Maximum attachments per turn.
pub const MAX_ATTACHMENTS: usize = 8;
/// Maximum bytes of one attachment data URI.
pub const MAX_ATTACHMENT_URL_BYTES: usize = 8 * 1024 * 1024;

/// Validates attachments: bounded count and size, `data:` URIs only.
pub fn validate_attachments(items: &[ChatAttachment]) -> Result<(), SpiritError> {
    let bad = |msg: &str| {
        SpiritError::new(
            SpiritErrorCode::InvalidInput,
            format!("invalid_attachment: {msg}"),
        )
    };
    if items.len() > MAX_ATTACHMENTS {
        return Err(bad("too many attachments"));
    }
    for item in items {
        if !item.url.starts_with("data:") || item.mime.is_empty() || item.filename.is_empty() {
            return Err(bad("attachment must be a named data URI with a mime type"));
        }
        if item.url.len() > MAX_ATTACHMENT_URL_BYTES {
            return Err(bad("attachment is too large"));
        }
    }
    Ok(())
}

/// Validates a variant token (engine-declared names are short identifiers).
pub fn validate_variant(variant: &str) -> Result<(), SpiritError> {
    if variant.is_empty()
        || variant.len() > 32
        || !variant
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(SpiritError::new(
            SpiritErrorCode::InvalidInput,
            "invalid_variant: unknown reasoning effort",
        ));
    }
    Ok(())
}

/// The immediate receipt for an accepted turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatSendReceipt {
    /// Shell routing key.
    pub session: ChatSessionId,
    /// The user message as stored.
    pub message: ChatMessage,
    /// True once the request was accepted locally.
    pub accepted: bool,
}

/// A backward log query.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatLogQuery {
    /// Opaque cursor from a previous page; null starts at the newest page.
    #[serde(default)]
    pub cursor: Option<String>,
    /// 1..=200; defaults to 50.
    #[serde(default)]
    pub limit: Option<usize>,
}

impl ChatLogQuery {
    /// Returns the clamped page size.
    #[must_use]
    pub fn limit(&self) -> usize {
        self.limit
            .unwrap_or(DEFAULT_LOG_LIMIT)
            .clamp(1, MAX_LOG_LIMIT)
    }
}

/// One page of older messages, oldest-first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatLogPage {
    /// Messages, oldest-first.
    pub messages: Vec<ChatMessage>,
    /// Opaque cursor for the next older page, or null at the start.
    pub next: Option<String>,
    /// True when the local cache may have dropped messages.
    pub truncated: bool,
}

/// Validates and normalizes one user turn.
///
/// The returned text has no surrounding whitespace. Empty text and text over
/// [`MAX_USER_MESSAGE_CHARS`] are typed `invalid_input` errors; the core never silently
/// truncates a user's message.
pub fn validate_user_text(text: &str) -> Result<String, SpiritError> {
    let normalized = normalize_text(text);
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return Err(SpiritError::new(
            SpiritErrorCode::InvalidInput,
            "invalid_text: enter a message before sending",
        ));
    }
    if trimmed.chars().count() > MAX_USER_MESSAGE_CHARS {
        return Err(SpiritError::new(
            SpiritErrorCode::InvalidInput,
            "invalid_text: enter a shorter message",
        )
        .with_hint(format!("The limit is {MAX_USER_MESSAGE_CHARS} characters.")));
    }
    Ok(trimmed.to_owned())
}

/// Normalizes line endings and strips unsafe control characters.
///
/// `\n` and `\t` are preserved; every other control character is removed.
#[must_use]
pub fn normalize_text(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut characters = input.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\r' => {
                if characters.peek() != Some(&'\n') {
                    output.push('\n');
                }
            }
            '\n' | '\t' => output.push(character),
            character if character.is_control() => {}
            character => output.push(character),
        }
    }
    output
}

/// Caps text by characters and reports whether it was truncated.
#[must_use]
pub fn cap_text(input: &str, max_chars: usize) -> (String, bool) {
    if input.chars().count() <= max_chars {
        return (input.to_owned(), false);
    }
    (input.chars().take(max_chars).collect(), true)
}

/// Splits plain text into paragraphs on blank lines.
#[must_use]
pub fn paragraphs(input: &str) -> Vec<String> {
    if input.is_empty() {
        return Vec::new();
    }
    input.split("\n\n").map(str::to_owned).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_ids_round_trip_and_reject_paths() {
        for value in ["setup", "global", "project:meadow"] {
            let session = ChatSessionId::parse(value).unwrap();
            assert_eq!(session.as_wire(), value);
            let json = serde_json::to_string(&session).unwrap();
            assert_eq!(json, format!("\"{value}\""));
        }
        for value in [
            "project:",
            "project:..",
            "project:a/b",
            "project:../escape",
            "project:a\\b",
            "project:a:b",
            "other",
        ] {
            assert!(ChatSessionId::parse(value).is_err(), "{value} must fail");
        }
    }

    #[test]
    fn text_is_normalized_capped_and_split() {
        assert_eq!(normalize_text("a\r\nb\rc\0d"), "a\nb\ncd");
        assert_eq!(normalize_text("bad\u{7}text"), "badtext");
        let (capped, truncated) = cap_text("abcdef", 3);
        assert_eq!(capped, "abc");
        assert!(truncated);
        assert_eq!(paragraphs("one\n\ntwo"), vec!["one", "two"]);
    }

    #[test]
    fn deltas_only_append_and_stop_at_the_cap() {
        let mut message = ChatMessage::model("m1", "Hello", Timestamp::epoch());
        assert!(!message.append_delta(" world"));
        assert_eq!(message.joined_text(), "Hello world");
        assert!(!message.append_delta("\n\nSecond"));
        assert_eq!(message.text, vec!["Hello world", "Second"]);
        let huge = "x".repeat(MAX_MESSAGE_CHARS + 10);
        assert!(message.append_delta(&huge));
        assert!(message.char_count() <= MAX_MESSAGE_CHARS);
    }

    #[test]
    fn user_text_rejects_empty_and_over_the_cap() {
        let empty = validate_user_text(" \n\t ").unwrap_err();
        assert_eq!(empty.code, SpiritErrorCode::InvalidInput);
        assert!(empty.message.contains("invalid_text"));
        let long = "x".repeat(MAX_USER_MESSAGE_CHARS + 1);
        let error = validate_user_text(&long).unwrap_err();
        assert_eq!(error.code, SpiritErrorCode::InvalidInput);
        assert!(error.message.contains("invalid_text"));
        assert!(error.hint.is_some());
        assert_eq!(validate_user_text("  hello  ").unwrap(), "hello".to_owned());
    }

    #[test]
    fn serialized_views_have_no_transport_fields() {
        let view = ChatSessionView {
            session: ChatSessionId::Setup,
            location: ChatSessionLocation::Local,
            state: ChatSessionState::Ready,
            attached: true,
            streaming_supported: false,
            model: Some(ChatModelView {
                id: "opencode/big-pickle".to_owned(),
                label: "Big Pickle".to_owned(),
                provider: "opencode".to_owned(),
                tier: ChatModelTier::Included,
                context_tokens: Some(128_000),
                variants: None,
            }),
            available_models: Some(Vec::new()),
            usage: Some(ChatUsageView {
                context_tokens_used: Some(2_400),
                context_limit: Some(128_000),
                input_tokens: Some(1_200),
                output_tokens: Some(300),
                cache_read_tokens: Some(800),
                turns: 1,
                cost_usd: None,
            }),
            started_at: None,
            last_event_at: None,
            error: None,
            pending_permissions: None,
            retry: None,
            ownership: None,
            access: ChatAccess::ReadOnly,
        };
        let json = serde_json::to_string(&view).unwrap();
        assert!(!json.contains("\"url\""));
        assert!(!json.contains("\"port\":"));
        assert!(!json.contains("\"token\""));
        assert!(!json.contains("\"directory\""));
        assert!(!json.contains("\"native_session_id\""));
        assert!(json.contains("\"access\":\"read_only\""));
        assert!(!json.contains("\"ownership\""));
    }

    #[test]
    fn ownership_serializes_only_display_fields() {
        let ownership = ChatOwnershipView {
            kind: "opencode".to_owned(),
            label: Some("Mac Studio".to_owned()),
            holder: Some("opencode".to_owned()),
            node: Some("mac-studio".to_owned()),
            mode: Some("cooperative".to_owned()),
            state: Some("attached".to_owned()),
            activity: Some("thinking".to_owned()),
        };
        let value = serde_json::to_value(&ownership).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "kind": "opencode",
                "label": "Mac Studio",
                "holder": "opencode",
                "node": "mac-studio",
                "mode": "cooperative",
                "state": "attached",
                "activity": "thinking",
            })
        );
        for forbidden in [
            "surface_id",
            "session_id",
            "epoch",
            "lease",
            "expires_at",
            "token",
        ] {
            assert!(value.get(forbidden).is_none(), "{forbidden}");
        }
    }

    #[test]
    fn retry_status_serializes_its_safe_fields_only() {
        let retry = ChatRetryStatus {
            attempt: 2,
            message: "Free usage limit reached".to_owned(),
            next: Some(1_763_673_600_000),
            action: Some(ChatRetryAction {
                reason: "free_tier_limit".to_owned(),
                title: "Free limit reached".to_owned(),
                message: "Subscribe to OpenCode Go".to_owned(),
                provider: "opencode".to_owned(),
            }),
        };
        let value = serde_json::to_value(&retry).unwrap();
        assert_eq!(value["attempt"], 2);
        assert_eq!(value["next"], 1_763_673_600_000_u64);
        assert_eq!(value["action"]["reason"], "free_tier_limit");
        assert!(value.get("url").is_none());
        assert!(value.get("token").is_none());
    }

    #[test]
    fn events_serialize_with_the_contract_kind_tag() {
        let event = ChatEvent::Delta {
            revision: 7,
            session: ChatSessionId::Setup,
            message_id: "m1".to_owned(),
            part_id: None,
            text: "hi".to_owned(),
        };
        let value: serde_json::Value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["kind"], "delta");
        assert_eq!(value["revision"], 7);
        assert_eq!(value["session"], "setup");
        assert!(value["part_id"].is_null());
    }
}
