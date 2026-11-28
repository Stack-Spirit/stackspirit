//! Orchestrator chat for the bundled StackSpirit Engine.
//!
//! This module implements the local half of `docs/ORCHESTRATOR-CHAT.md`: the safe
//! §4 DTOs, the private v1 HTTP/SSE transport, the revisioned event pump and the typed
//! §6 error map. It never spawns an engine, never reads a provider key and never exposes
//! an endpoint URL, port, workspace or native session id to a serialized chat value.

pub mod endpoint;
pub mod harness;
pub mod local;
pub mod model;
pub mod pump;
pub mod selection;
pub mod sse;
pub mod writer;

pub use endpoint::{ChatEndpointResolver, ResolvedEndpoint, VowpEndpointResolver};
pub use harness::{HarnessChatGate, dry_run, gate_for_manifest, preflight};
pub use local::{
    ChatEngine, ChatOrchestratorProvisioner, ChatTransport, KnownOrchestratorSession,
    LocalChatProvider, NativeSession, map_http_status, map_provider_error,
};
pub use model::{
    ChatAccess, ChatAttachment, ChatCloseReason, ChatEvent, ChatLogPage, ChatLogQuery, ChatMessage,
    ChatMessageOrigin, ChatModelTier, ChatModelView, ChatOwnershipView, ChatPermissionDecision,
    ChatPermissionPrompt, ChatRetryAction, ChatRetryStatus, ChatRole, ChatSendInput,
    ChatSendReceipt, ChatSessionId, ChatSessionLocation, ChatSessionState, ChatSessionView,
    ChatStartInput, ChatToolActivity, ChatToolStatus, ChatUsageView, ChatWriterReason,
    ChatWriterState, ChatWriterView, DEFAULT_LOG_LIMIT, MAX_ACTIVITY_CHARS, MAX_ACTIVITY_ITEMS,
    MAX_ATTACHMENT_URL_BYTES, MAX_ATTACHMENTS, MAX_LOG_LIMIT, MAX_MESSAGE_CHARS,
    MAX_USER_MESSAGE_CHARS, cap_text, display_snippet, normalize_text, paragraphs,
    validate_attachments, validate_project_name, validate_user_text, validate_variant,
};
pub use pump::{
    ApplyOutcome, ChatEventApplier, ChatEventBus, Mapped, SseEventMapper, retry_status,
};
pub use selection::{
    CHAT_PREFERENCES_VERSION, ChatHarnessCapture, ChatPreferences, ChatPreferencesSource,
    ChatPreferencesStore, ChatSettingsError, JsonChatPreferences,
};
pub use sse::{
    EngineEvent, EngineMessageUpdate, EnginePartUpdate, EngineRetryAction, EngineSessionStatus,
    STREAM_UNPROVEN_REASON, SseFrame, SseParser, StreamSupport, parse_engine_event,
    parse_session_status,
};
pub use writer::{ChatWriter, ChatWriterConfig, ChatWriterError, WriterTarget};
