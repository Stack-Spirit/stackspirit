//! Chat command adapters for the bundled StackSpirit Engine.
//!
//! Every rule (session map, state machine, HTTP/SSE transport, event pump, log paging,
//! the harness gate and the typed error map) lives in `spirit_core::engine::chat`. These
//! commands only resolve the one `LocalChatProvider` from [`AppState`] and forward the
//! safe request. No reply contains an endpoint URL, port, credential, native session id,
//! workspace, binary path or settings path; none is logged here.
//!
//! `global` and `project:<name>` stay gated in core until VOWP's orchestrator endpoint
//! resolves; these adapters only hand the ensured VOWP universal session to core.
//!
//! `chat_start` dispatches through the harness router, so an app-owned `setup` start is
//! checked against the persisted default before the delegate can create a session. The
//! settings commands expose only a registry id and the typed gate.

use serde::{Deserialize, Serialize};
use spirit_core::engine::chat::{
    CHAT_PREFERENCES_VERSION, ChatLogPage, ChatLogQuery, ChatPermissionDecision, ChatPreferences,
    ChatSendInput, ChatSendReceipt, ChatSessionId, ChatSessionLocation, ChatSessionView,
    ChatStartInput, ChatTransport, ChatWriterError, HarnessChatGate, LocalChatProvider,
    gate_for_manifest, preflight,
};
use spirit_core::error::{SpiritError, SpiritErrorCode};
use spirit_core::harnesses::HarnessManifest;
use tauri::State;

use crate::error::CommandResult;
use crate::state::AppState;

/// Ensures the engine and creates or resumes the native session, idempotently.
///
/// This is the only session-creation path: the router gates an app-owned `setup` start
/// through the persisted harness default and records the per-session capture. VOWP
/// `global`/`project` starts bypass the default in core.
#[tauri::command]
pub async fn chat_start(
    state: State<'_, AppState>,
    input: ChatStartInput,
) -> CommandResult<ChatSessionView> {
    let chat = state.chat_harness();
    chat.start(input).await
}

/// The safe default-harness read model for Settings.
///
/// It carries the persisted registry id and the tagged gate. It never carries a
/// settings path, endpoint, port, token, native id or user-facing prose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatHarnessSettingsView {
    /// The stored id, or `None` when unset or unreadable. With a `ready` gate, `None`
    /// is the documented OpenCode default.
    pub default_harness_id: Option<String>,
    /// The same preflight the next app-owned setup start uses.
    pub gate: HarnessChatGate,
}

/// The id payload for `chat_set_default_harness`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatSetDefaultHarnessInput {
    /// A validated registry id such as `opencode` or `goose`.
    pub harness_id: String,
}

/// Reads the persisted default harness and its typed gate.
///
/// A corrupt or future `chat.json` yields `default_harness_id: None` with the
/// `chat_settings_unreadable` gate. The file is never rewritten here.
#[tauri::command]
pub fn chat_harness_settings(state: State<'_, AppState>) -> CommandResult<ChatHarnessSettingsView> {
    Ok(chat_harness_settings_view(state.inner()))
}

/// Saves the default harness used by new app-owned setup conversations.
///
/// Only `ready` and `harness_not_wired` selections are accepted. A non-OpenCode live
/// transport does not exist yet, so `harness_not_wired` is selectable for planning and
/// is still refused by `chat_start`. Missing, worker-only and unsupported-protocol ids
/// are typed refusals. A corrupt or future settings file is never overwritten.
#[tauri::command]
pub fn chat_set_default_harness(
    state: State<'_, AppState>,
    input: ChatSetDefaultHarnessInput,
) -> CommandResult<ChatHarnessSettingsView> {
    HarnessManifest::validate_id(&input.harness_id)?;

    let registry = state.chat_registry();
    let manifest = match registry.get(&input.harness_id) {
        Ok(Some(manifest)) => manifest,
        // A skipped custom file is unresolvable by design; it gets the same
        // `harness_missing` gate as an unknown id, with no parser or path detail.
        Ok(None) | Err(_) => return Err(harness_missing_error()),
    };

    match gate_for_manifest(&manifest) {
        HarnessChatGate::Ready { .. } | HarnessChatGate::HarnessNotWired { .. } => {}
        gate @ (HarnessChatGate::HarnessWorkerOnly { .. }
        | HarnessChatGate::HarnessProtocolUnsupported { .. }) => {
            return Err(harness_gate_error(&gate));
        }
        // `gate_for_manifest` cannot produce these, but never save an unusable id if
        // that ever changes.
        _ => return Err(harness_missing_error()),
    }

    let preferences = state.chat_preferences();
    // Refuse to overwrite a corrupt or future file instead of silently resetting it.
    preferences.load().map_err(|problem| problem.spirit())?;
    preferences.save(&ChatPreferences {
        version: CHAT_PREFERENCES_VERSION,
        default_harness_id: Some(input.harness_id),
    })?;

    Ok(chat_harness_settings_view(state.inner()))
}

/// Builds the safe settings view from the one persisted store.
fn chat_harness_settings_view(state: &AppState) -> ChatHarnessSettingsView {
    match state.chat_preferences().load() {
        Ok(preferences) => ChatHarnessSettingsView {
            default_harness_id: preferences.default_harness_id.clone(),
            gate: preflight(state.chat_registry().as_ref(), &preferences),
        },
        Err(problem) => ChatHarnessSettingsView {
            default_harness_id: None,
            gate: HarnessChatGate::ChatSettingsUnreadable { problem },
        },
    }
}

/// The stable typed refusal for an id that no registry entry resolves.
fn harness_missing_error() -> SpiritError {
    SpiritError::new(SpiritErrorCode::NotFound, "harness_missing")
        .with_hint("Choose a harness that is registered on this Mac, or use OpenCode.")
}

/// The stable typed refusal for a gate that cannot back the embedded chat.
fn harness_gate_error(gate: &HarnessChatGate) -> SpiritError {
    SpiritError::new(SpiritErrorCode::Gate, gate.reason())
        .with_hint("Choose OpenCode or another embedded-chat harness in Settings.")
}

/// One-click Start for a project orchestrator. Core attaches VOWP's local endpoint or
/// falls back to a project session on the bundled engine.
#[tauri::command]
pub async fn chat_provision_orchestrator(
    state: State<'_, AppState>,
    project: String,
) -> CommandResult<ChatSessionView> {
    // Create the real VOWP universal session on the VPS first when a host and an enrolled
    // local node are connected; otherwise core falls back to the bundled engine.
    let known =
        crate::commands::local_orchestrator::ensure_project_orchestrator(state.inner(), &project)
            .await?;
    let chat = state.chat();
    chat.provision_orchestrator_with(&project, known).await
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
    chat.send_turn(
        &input.session,
        &input.text,
        input.variant.as_deref(),
        &input.attachments,
    )
    .await
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

/// Takes the cooperative VOWP writer lease for an attached local orchestrator session.
///
/// This is Phase 2 plumbing from `docs/SESSION-TAKEOVER.md` §2.8: the bridge exposes it,
/// but the read-only chat notice keeps no action and no UI calls it yet.
///
/// `takeover` must be `true` only after the owner explicitly confirmed replacing a live
/// holder; absent or `false` attaches only a free or stale lease. The renderer never
/// supplies the orchestrator, holder, label, home, surface or token. Setup, engine-backed
/// and server-gated sessions are rejected here. The reply is the safe `ChatSessionView`;
/// it never carries a lease token, surface id, endpoint URL, port, native session id,
/// epoch or path.
#[tauri::command]
pub async fn chat_take_over(
    state: State<'_, AppState>,
    session: ChatSessionId,
    takeover: Option<bool>,
) -> CommandResult<ChatSessionView> {
    let chat = state.chat();
    ensure_local_writer_session(&chat, &session).await?;
    let view = chat.take_over(&session, takeover.unwrap_or(false)).await?;
    if view.location != ChatSessionLocation::Local {
        // Defense in depth: never return a server view for a writer action, and do not
        // leave a lease behind if a future transport reported one after the guard.
        let _ = chat.release_writer(&session).await;
        return Err(chat_gated_writer_error());
    }
    state.track_chat_writer(&session);
    Ok(view)
}

/// Releases this app's writer lease and returns the refreshed safe view.
///
/// Release is not Stop: a running turn may continue on the orchestrator. A session this
/// app never attached is an idempotent no-op. Setup, engine-backed and server-gated
/// sessions are rejected. The reply carries only `ChatSessionView`.
#[tauri::command]
pub async fn chat_release_writer(
    state: State<'_, AppState>,
    session: ChatSessionId,
) -> CommandResult<ChatSessionView> {
    let chat = state.chat();
    if matches!(session, ChatSessionId::Setup) {
        return Err(ChatWriterError::HarnessNotWired.into());
    }
    // No attach probe here: a lost endpoint must not block releasing a live child, and an
    // engine-backed session must not start the engine just to be refused. Core owns the
    // setup/engine-backed rejection and reaps any child it holds.
    let view = chat.release_writer(&session).await?;
    if view.location != ChatSessionLocation::Local {
        return Err(chat_gated_writer_error());
    }
    state.forget_chat_writer(&session);
    Ok(view)
}

/// Validates that a writer action can target this session and that it is local.
///
/// `spirit-core` owns the full session rules. This adapter adds the two boundaries the
/// renderer must not cross: `setup` is app-owned, and a server-gated transport has no
/// VOWP writer lease to take.
async fn ensure_local_writer_session(
    chat: &LocalChatProvider,
    session: &ChatSessionId,
) -> CommandResult<()> {
    if matches!(session, ChatSessionId::Setup) {
        return Err(ChatWriterError::HarnessNotWired.into());
    }
    let view = chat.attach(session).await?;
    if view.location != ChatSessionLocation::Local {
        return Err(chat_gated_writer_error());
    }
    Ok(())
}

/// The typed, token-free refusal for a writer action on a non-local session.
fn chat_gated_writer_error() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Gate,
        "chat_gated: the session writer is only available for a local VOWP orchestrator",
    )
    .with_hint("Connect to the VOWP home that hosts the orchestrator, then retry.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use spirit_core::engine::chat::{
        ChatAccess, ChatOwnershipView, ChatSettingsError, ChatWriterReason, ChatWriterState,
        ChatWriterView,
    };
    use spirit_core::harnesses::HarnessProtocol;

    /// The command boundary must keep the same safe shape as the core view.
    #[test]
    fn writer_view_serializes_without_lease_identity() {
        let view = ChatSessionView {
            session: ChatSessionId::Global,
            location: ChatSessionLocation::Local,
            state: spirit_core::engine::chat::ChatSessionState::Ready,
            attached: true,
            streaming_supported: true,
            model: None,
            available_models: None,
            usage: None,
            started_at: None,
            last_event_at: None,
            error: None,
            pending_permissions: None,
            retry: None,
            ownership: Some(ChatOwnershipView {
                kind: "desktop_ui".to_owned(),
                label: Some("StackSpirit · macbook".to_owned()),
                holder: Some("stackspirit".to_owned()),
                node: Some("macbook".to_owned()),
                mode: Some("interactive".to_owned()),
                state: Some("active".to_owned()),
                activity: Some("idle".to_owned()),
            }),
            access: ChatAccess::Owner,
            writer: Some(ChatWriterView {
                state: ChatWriterState::Held,
                can_take_over: false,
                can_release: true,
                reason: None,
            }),
        };
        let serialized = serde_json::to_string(&view).expect("the safe view serializes");
        for forbidden in [
            "\"token\"",
            "\"surface_id\"",
            "\"native_session_id\"",
            "\"url\"",
            "\"port\"",
            "\"epoch\"",
            "\"expires_at\"",
            "http://",
            "https://",
            "127.0.0.1",
            "/Users/",
            "/home/",
        ] {
            assert!(
                !serialized.contains(forbidden),
                "the command view leaked {forbidden}"
            );
        }

        let lost = ChatWriterView {
            state: ChatWriterState::Lost,
            can_take_over: false,
            can_release: false,
            reason: Some(ChatWriterReason::LeaseLost),
        };
        let value = serde_json::to_value(lost).expect("the writer view serializes");
        assert_eq!(
            value,
            serde_json::json!({
                "state": "lost",
                "can_take_over": false,
                "can_release": false,
                "reason": "lease_lost",
            })
        );
    }

    #[test]
    fn settings_view_serializes_the_tagged_gate_without_unsafe_fields() {
        let fresh = ChatHarnessSettingsView {
            default_harness_id: None,
            gate: HarnessChatGate::Ready {
                harness_id: "opencode".to_owned(),
                protocol: HarnessProtocol::OpencodeHttp,
            },
        };
        assert_eq!(
            serde_json::to_value(&fresh).expect("the settings view serializes"),
            serde_json::json!({
                "default_harness_id": null,
                "gate": {
                    "state": "ready",
                    "harness_id": "opencode",
                    "protocol": "opencode-http",
                },
            })
        );

        let selected = ChatHarnessSettingsView {
            default_harness_id: Some("claude-code".to_owned()),
            gate: HarnessChatGate::HarnessNotWired {
                harness_id: "claude-code".to_owned(),
                protocol: HarnessProtocol::StreamJson,
            },
        };
        let serialized = serde_json::to_string(&selected).expect("the settings view serializes");
        assert!(serialized.contains("\"state\":\"harness_not_wired\""));
        assert!(serialized.contains("\"protocol\":\"stream-json\""));
        for forbidden in [
            "\"path\"",
            "\"token\"",
            "\"endpoint\"",
            "\"port\"",
            "\"workspace\"",
            "\"native_session_id\"",
            "/Users/",
            "/home/",
        ] {
            assert!(
                !serialized.contains(forbidden),
                "the settings view leaked {forbidden}"
            );
        }
    }

    #[test]
    fn unreadable_settings_serializes_the_typed_problem() {
        let view = ChatHarnessSettingsView {
            default_harness_id: None,
            gate: HarnessChatGate::ChatSettingsUnreadable {
                problem: ChatSettingsError::UnsupportedVersion {
                    found: 2,
                    supported: CHAT_PREFERENCES_VERSION,
                },
            },
        };
        assert_eq!(
            serde_json::to_value(&view).expect("the settings view serializes"),
            serde_json::json!({
                "default_harness_id": null,
                "gate": {
                    "state": "chat_settings_unreadable",
                    "problem": {
                        "reason": "unsupported_version",
                        "found": 2,
                        "supported": CHAT_PREFERENCES_VERSION,
                    },
                },
            })
        );
    }
}
