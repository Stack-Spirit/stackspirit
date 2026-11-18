//! Orchestrator chat for the bundled StackSpirit Engine.
//!
//! This module implements the local half of `docs/ORCHESTRATOR-CHAT.md`: the safe
//! §4 DTOs, the private v1 HTTP/SSE transport, the revisioned event pump and the typed
//! §6 error map. It never spawns an engine, never reads a provider key and never exposes
//! an endpoint URL, port, workspace or native session id to a serialized chat value.

pub mod local;
pub mod model;
pub mod pump;
pub mod sse;

pub use local::{
    ChatEngine, ChatTransport, LocalChatProvider, NativeSession, map_http_status,
    map_provider_error,
};
pub use model::{
    ChatCloseReason, ChatEvent, ChatLogPage, ChatLogQuery, ChatMessage, ChatMessageOrigin,
    ChatModelView, ChatRole, ChatSendInput, ChatSendReceipt, ChatSessionId, ChatSessionLocation,
    ChatSessionState, ChatSessionView, ChatStartInput, DEFAULT_LOG_LIMIT, MAX_LOG_LIMIT,
    MAX_MESSAGE_CHARS, MAX_USER_MESSAGE_CHARS, cap_text, normalize_text, paragraphs,
    validate_project_name, validate_user_text,
};
pub use pump::{ApplyOutcome, ChatEventApplier, ChatEventBus, Mapped, SseEventMapper};
pub use sse::{
    EngineEvent, EngineMessageUpdate, EnginePartUpdate, STREAM_UNPROVEN_REASON, SseFrame,
    SseParser, StreamSupport, parse_engine_event,
};
