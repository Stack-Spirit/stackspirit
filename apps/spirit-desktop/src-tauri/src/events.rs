//! Forwards core events to the renderer.
//!
//! `spirit_core::SpiritEvent` goes to `spirit://state`; chat deltas go to
//! `spirit://chat` on their own channel and never mix into the fleet event. Chat event
//! bodies contain model and user text, so this module never logs a payload.

use spirit_core::connection::SpiritEvent;
use spirit_core::engine::ChatEvent;
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast::error::RecvError;

/// Starts one forwarder task. It stops when the broadcast channel closes or the window
/// is gone; a lagged receiver resumes from the newest event.
pub fn spawn_event_forwarder(
    app: AppHandle,
    mut receiver: tokio::sync::broadcast::Receiver<SpiritEvent>,
) {
    tauri::async_runtime::spawn(async move {
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    if app.emit("spirit://state", &event).is_err() {
                        break;
                    }
                }
                Err(RecvError::Lagged(skipped)) => {
                    tracing::warn!(
                        skipped,
                        "UI event receiver lagged; resuming at newest event"
                    );
                }
                Err(RecvError::Closed) => break,
            }
        }
    });
}

/// Starts the one chat event forwarder.
///
/// Chat events carry model and user text, so a lag is logged as a count only and the
/// receiver resumes at the newest event. The payload is emitted unchanged; the renderer
/// replaces its messages from `chat.log` after a revision gap.
pub fn spawn_chat_event_forwarder(
    app: AppHandle,
    mut receiver: tokio::sync::broadcast::Receiver<ChatEvent>,
) {
    tauri::async_runtime::spawn(async move {
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    if app.emit("spirit://chat", &event).is_err() {
                        break;
                    }
                }
                Err(RecvError::Lagged(skipped)) => {
                    tracing::warn!(
                        skipped,
                        "chat event receiver lagged; resuming at newest event"
                    );
                }
                Err(RecvError::Closed) => break,
            }
        }
    });
}
