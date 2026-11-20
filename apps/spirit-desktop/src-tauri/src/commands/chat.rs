//! Chat command adapters for the bundled StackSpirit Engine.
//!
//! Every rule (session map, state machine, HTTP/SSE transport, event pump, log paging
//! and the typed error map) lives in `spirit_core::engine::chat`. These commands only
//! resolve the one `LocalChatProvider` from [`AppState`] and forward the safe request.
//! No reply contains an endpoint URL, port, credential, native session id, workspace or
//! binary path; none is logged here.
//!
//! `global` and `project:<name>` stay gated in core until VOWP 0.5's orchestrator
//! endpoint (or the owner-approved forward) lands; these adapters do not special-case
//! them.

use spirit_core::engine::chat::{
    ChatLogPage, ChatLogQuery, ChatPermissionDecision, ChatSendInput, ChatSendReceipt,
    ChatSessionId, ChatSessionView, ChatStartInput,
};
use tauri::State;

use crate::error::CommandResult;
use crate::state::AppState;

/// Ensures the engine and creates or resumes the native session, idempotently.
#[tauri::command]
pub async fn chat_start(
    state: State<'_, AppState>,
    input: ChatStartInput,
) -> CommandResult<ChatSessionView> {
    let chat = state.chat();
    chat.start(&input).await
}

/// Reuses the stored native session. Core never creates a session here.
#[tauri::command]
pub async fn chat_attach(
    state: State<'_, AppState>,
    session: ChatSessionId,
) -> CommandResult<ChatSessionView> {
    let chat = state.chat();
    chat.attach(&session).await
}

/// Accepts one user turn and returns the immediate receipt. Assistant text arrives on
/// `spirit://chat`.
#[tauri::command]
pub async fn chat_send(
    state: State<'_, AppState>,
    input: ChatSendInput,
) -> CommandResult<ChatSendReceipt> {
    let chat = state.chat();
    chat.send(&input.session, &input.text).await
}

/// Switches the setup assistant's model. The core validates the session and the engine
/// adapter persists the explicit owner choice; no key or endpoint crosses this boundary.
#[tauri::command]
pub async fn chat_set_model(
    state: State<'_, AppState>,
    session: ChatSessionId,
    model_id: String,
) -> CommandResult<ChatSessionView> {
    let chat = state.chat();
    chat.set_model(&session, &model_id).await
}

/// Aborts the current turn when supported and returns the stopped view.
#[tauri::command]
pub async fn chat_stop(
    state: State<'_, AppState>,
    session: ChatSessionId,
) -> CommandResult<ChatSessionView> {
    let chat = state.chat();
    chat.stop(&session).await
}

/// Reads one backward conversation page from the active native log.
#[tauri::command]
pub async fn chat_log(
    state: State<'_, AppState>,
    session: ChatSessionId,
    query: Option<ChatLogQuery>,
) -> CommandResult<ChatLogPage> {
    let chat = state.chat();
    let query = query.unwrap_or_default();
    chat.log(&session, &query).await
}

/// Answers one pending OpenCode permission request from an owner click.
///
/// Core refuses ids it does not know and never approves anything on its own.
#[tauri::command]
pub async fn chat_reply_permission(
    state: State<'_, AppState>,
    session: ChatSessionId,
    permission_id: String,
    decision: ChatPermissionDecision,
) -> CommandResult<ChatSessionView> {
    let chat = state.chat();
    chat.reply_permission(&session, &permission_id, decision)
        .await
}
