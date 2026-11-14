//! Forwards `spirit_core::SpiritEvent`s to the renderer as `spirit://state`.

use spirit_core::connection::SpiritEvent;
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
