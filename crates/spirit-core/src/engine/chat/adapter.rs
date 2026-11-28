//! The transport-neutral harness chat adapter boundary.
//!
//! This module is the lower half of `docs/HARNESS-CHAT.md` §4.1: it defines the trait a
//! protocol adapter implements and the safe event vocabulary the provider maps into
//! [`ChatEvent`](crate::engine::chat::ChatEvent). It carries no protocol frames: an
//! adapter receives ACP, stream-json or Codex-json data and emits only bounded, redacted
//! values.
//!
//! A native session id is core-only. [`HarnessNativeId`] deliberately has a redacting
//! `Debug`, [`HarnessSession`] never derives `Serialize`, and [`HarnessEvent`] cannot be
//! serialized. The renderer receives [`ChatSessionId`](crate::engine::chat::ChatSessionId)
//! and safe projections only.

use std::fmt;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::engine::chat::harness::HarnessChatGate;
use crate::engine::chat::model::{
    ChatModelView, ChatPermissionPrompt, ChatToolActivity, ChatUsageView,
};
use crate::error::SpiritError;
use crate::harnesses::HarnessProtocol;

/// One native session id owned by the core.
///
/// The value is useful only to the adapter and the router. It must never be serialized,
/// written to a log, shown in an error or included in a [`ChatSessionView`]. Debug output
/// is therefore redacted by construction.
///
/// [`ChatSessionView`]: crate::engine::chat::ChatSessionView
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct HarnessNativeId(String);

impl HarnessNativeId {
    /// Wraps a native protocol id.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Returns the native id for adapter-internal use only.
    ///
    /// Never copy this value into an event, an error, a log line or a serialized view.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for HarnessNativeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("HarnessNativeId(<redacted>)")
    }
}

/// The one native session an adapter has started or attached.
///
/// The harness id and protocol are safe routing metadata. `native_id` is core-only; this
/// type never derives `Serialize` and its `Debug` prints only whether a native id exists.
#[derive(Clone, PartialEq, Eq)]
pub struct HarnessSession {
    harness_id: String,
    protocol: HarnessProtocol,
    native_id: Option<HarnessNativeId>,
}

impl HarnessSession {
    /// Builds a session record.
    #[must_use]
    pub fn new(
        harness_id: impl Into<String>,
        protocol: HarnessProtocol,
        native_id: Option<HarnessNativeId>,
    ) -> Self {
        Self {
            harness_id: harness_id.into(),
            protocol,
            native_id,
        }
    }

    /// Returns the registry id this session is bound to.
    #[must_use]
    pub fn harness_id(&self) -> &str {
        &self.harness_id
    }

    /// Returns the protocol the native id belongs to.
    #[must_use]
    pub const fn protocol(&self) -> HarnessProtocol {
        self.protocol
    }

    /// Returns the native id for adapter-internal use only.
    ///
    /// `None` means the protocol has not reported one yet. Never serialize or log the
    /// returned string.
    #[must_use]
    pub fn native_id(&self) -> Option<&str> {
        self.native_id.as_ref().map(HarnessNativeId::as_str)
    }
}

impl fmt::Debug for HarnessSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HarnessSession")
            .field("harness_id", &self.harness_id)
            .field("protocol", &self.protocol)
            .field("native_id", &self.native_id.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

/// The inputs needed to create or resume one native session.
///
/// The workspace is core-only: it can contain a private path. Its `Debug` never prints
/// the path.
#[derive(Clone, PartialEq, Eq)]
pub struct HarnessStart {
    workspace: PathBuf,
}

impl HarnessStart {
    /// Builds a start request for one workspace.
    #[must_use]
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        Self {
            workspace: workspace.into(),
        }
    }

    /// Returns the workspace for the adapter only.
    #[must_use]
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }
}

impl fmt::Debug for HarnessStart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HarnessStart")
            .field("workspace", &"<redacted>")
            .finish()
    }
}

/// One user turn to hand to an adapter.
///
/// `text` is validated and capped by the adapter before it reaches a protocol. `Debug`
/// reports only the size because turn text is conversation content, not log material.
#[derive(Clone, PartialEq, Eq)]
pub struct HarnessTurn {
    text: String,
}

impl HarnessTurn {
    /// Builds a turn from user text.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }

    /// Returns the user text for the adapter only.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

impl fmt::Debug for HarnessTurn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HarnessTurn")
            .field("text_chars", &self.text.chars().count())
            .finish()
    }
}

/// The adapter accepted one turn and will stream its events.
///
/// The message id is app-generated (`msg_<ULID>`), never a native protocol id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessAccepted {
    /// The assistant message id every event for this turn uses.
    pub message_id: String,
}

/// The result of an abort.
///
/// `stop_confirmed` is false when the adapter sent its protocol cancel but did not observe
/// a confirmed stop. The caller must report that honestly instead of pretending the turn
/// stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarnessAbort {
    /// True only when the protocol confirmed the stop.
    pub stop_confirmed: bool,
}

impl HarnessAbort {
    /// A confirmed stop.
    #[must_use]
    pub const fn stopped() -> Self {
        Self {
            stop_confirmed: true,
        }
    }

    /// A cancel that was sent but not confirmed by the protocol.
    #[must_use]
    pub const fn unconfirmed() -> Self {
        Self {
            stop_confirmed: false,
        }
    }
}

/// One transport-neutral streaming event.
///
/// Variants carry safe values from [`ChatToolActivity`] and [`ChatPermissionPrompt`] or
/// app-generated ids. [`HarnessEvent::NativeSession`] is the only variant that carries a
/// native id, and its id is redacted by `HarnessNativeId`'s `Debug`. No variant stores a
/// raw protocol frame.
#[derive(Debug, Clone, PartialEq)]
pub enum HarnessEvent {
    /// The native session id the protocol reported. Core-only.
    NativeSession {
        /// Native protocol session id.
        id: HarnessNativeId,
        /// Safe display title, when the protocol reports one.
        title: Option<String>,
    },
    /// An assistant message began.
    MessageStart {
        /// App-generated assistant message id.
        message_id: String,
    },
    /// Append text to the assistant message.
    TextDelta {
        /// App-generated assistant message id.
        message_id: String,
        /// Protocol part id, when one is known.
        part_id: Option<String>,
        /// Text to append.
        text: String,
    },
    /// Replace the assistant text so far.
    TextReplace {
        /// App-generated assistant message id.
        message_id: String,
        /// Protocol part id, when one is known.
        part_id: Option<String>,
        /// The complete replacement text.
        text: String,
    },
    /// One tool-call row changed.
    Tool {
        /// Bounded, redacted tool activity.
        activity: ChatToolActivity,
    },
    /// The protocol is waiting on an owner permission decision.
    Permission {
        /// Safe permission prompt; the decision is resolved synchronously by the policy.
        prompt: ChatPermissionPrompt,
    },
    /// Token or context usage the protocol reported.
    Usage {
        /// Only fields the protocol reported; never zero-filled.
        usage: ChatUsageView,
    },
    /// The turn ended with a confirmed stop.
    Done {
        /// App-generated assistant message id.
        message_id: String,
        /// True when the adapter capped the assistant text.
        truncated: bool,
    },
    /// The turn failed with a safe typed error.
    Error {
        /// The redacted, typed failure.
        error: SpiritError,
    },
}

/// A safe permission request for the synchronous policy seam.
///
/// It carries only the redacted tool class and display patterns. Raw tool input, output,
/// error text, protocol frames and native session ids never cross this boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessPermissionRequest {
    /// The tool class, such as `shell` or `read`.
    pub permission: String,
    /// Bounded, redacted display patterns.
    pub patterns: Vec<String>,
}

/// The owner policy's answer to one permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HarnessPermissionDecision {
    /// Allow this one tool call.
    AllowOnce,
    /// Allow this and later matching calls.
    AllowAlways,
    /// Reject this one tool call.
    RejectOnce,
    /// Reject this and later matching calls.
    RejectAlways,
    /// Do not choose; the protocol cancels the turn.
    #[default]
    Cancel,
}

/// A synchronous permission policy seam.
///
/// The default denies: it returns [`HarnessPermissionDecision::Cancel`]. An adapter must
/// never auto-approve a tool call and must not perform I/O from this method.
pub trait HarnessPermissionPolicy: Send + Sync {
    /// Decides one request.
    fn decide(&self, request: &HarnessPermissionRequest) -> HarnessPermissionDecision {
        let _ = request;
        HarnessPermissionDecision::Cancel
    }
}

/// The default policy: cancel every request.
#[derive(Debug, Clone, Copy, Default)]
pub struct DenyPermissionPolicy;

impl HarnessPermissionPolicy for DenyPermissionPolicy {}

/// One adapter bound to one harness id and protocol.
///
/// The provider owns an `Arc<dyn HarnessChatAdapter>`. `start` is idempotent for the
/// adapter's single session, `send_turn` accepts one turn at a time, `abort` is a turn
/// control and `close` is idempotent. `preflight` is an inert protocol/id check and never
/// spawns a child.
#[async_trait]
pub trait HarnessChatAdapter: Send + Sync {
    /// Returns the registry id this adapter serves.
    fn harness_id(&self) -> &str;

    /// Returns the protocol this adapter speaks.
    fn protocol(&self) -> HarnessProtocol;

    /// Returns a safe gate from protocol and id alone. Never spawns or opens a socket.
    async fn preflight(&self) -> Result<HarnessChatGate, SpiritError>;

    /// Starts or attaches one native session. May spawn a long-lived child.
    async fn start(&self, input: HarnessStart) -> Result<HarnessSession, SpiritError>;

    /// Accepts one turn and streams transport-neutral events on `events`.
    ///
    /// A second call while the first turn is active returns the typed `chat_busy` error
    /// before writing to the protocol.
    async fn send_turn(
        &self,
        session: &HarnessSession,
        turn: HarnessTurn,
        events: mpsc::Sender<HarnessEvent>,
    ) -> Result<HarnessAccepted, SpiritError>;

    /// Aborts the active turn and reports whether the protocol confirmed the stop.
    async fn abort(&self, session: &HarnessSession) -> Result<HarnessAbort, SpiritError>;

    /// Reaps the child and releases the session. Idempotent.
    async fn close(&self, session: &HarnessSession) -> Result<(), SpiritError>;

    /// Returns a verified model list, or `None` when the harness cannot report one.
    ///
    /// Phase 2 adapters return `None`; there is no model picker yet.
    fn models(&self) -> Option<Vec<ChatModelView>> {
        None
    }
}
