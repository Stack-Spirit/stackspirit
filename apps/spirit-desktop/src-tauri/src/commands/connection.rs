//! Connection lifecycle: one active host, connect sequence, polling and disconnect.

use std::time::Duration;

use spirit_core::connection::ConnectionSnapshot;
use spirit_core::error::{SpiritError, SpiritErrorCode};
use spirit_core::host::{HostId, verify_pin_against_scan};
use spirit_core::ssh::scan_host_keys;
use spirit_core::time::Timestamp;
use tauri::State;

use crate::error::CommandResult;
use crate::state::AppState;

#[tauri::command]
pub async fn connect_host(
    state: State<'_, AppState>,
    host_id: String,
) -> CommandResult<ConnectionSnapshot> {
    let id = HostId(host_id);
    let manager = state.connection_manager().await;
    let current_state = manager.status().state;
    if let Some(active) = state.active_host() {
        let switching = matches!(
            current_state.as_str(),
            "connected" | "connecting" | "stale" | "disconnecting"
        );
        if active.id != id && switching {
            return Err(SpiritError::new(
                SpiritErrorCode::Conflict,
                "disconnect from the current host before connecting another",
            ));
        }
    }
    let host = state
        .hosts()
        .get(&id)?
        .ok_or_else(|| SpiritError::not_found("host profile"))?;
    if host.host_key.validate().is_err() {
        return Err(SpiritError::new(
            spirit_core::error::SpiritErrorCode::HostKeyMismatch,
            "the host profile has no valid pinned fingerprint",
        ));
    }

    let client = state.client_for(&host);
    state.set_client(Some(client.clone()));
    state.set_active_host(Some(host.clone()));

    let result = manager.connect(client.as_ref(), id.clone()).await;
    let snapshot = match result {
        Ok(snapshot) => snapshot,
        Err(error) => {
            state.set_client(None);
            // `ssh` reports a generic host-key failure. Re-scan the host so the UI can
            // show both fingerprints, but only when the pin itself no longer matches.
            if error.code == SpiritErrorCode::HostKeyMismatch {
                if let Ok(scan) =
                    scan_host_keys(state.runner().as_ref(), &host.ssh, Duration::from_secs(8)).await
                {
                    verify_pin_against_scan(&host.host_key, &scan.keys)?;
                }
            }
            return Err(error);
        }
    };

    let _ = state.hosts().mark_connected(&id, Timestamp::now());
    let poll_manager = manager.clone();
    let poll_client = client;
    tauri::async_runtime::spawn(async move {
        poll_manager.run_poll_loop(poll_client).await;
    });

    Ok(snapshot)
}

#[tauri::command]
pub async fn disconnect_host(
    state: State<'_, AppState>,
    host_id: String,
) -> CommandResult<ConnectionSnapshot> {
    if let Some(active) = state.active_host() {
        if active.id.0 != host_id {
            return Err(SpiritError::new(
                spirit_core::error::SpiritErrorCode::Conflict,
                "a different host is connected",
            ));
        }
    }
    let manager = state.connection_manager().await;
    let snapshot = manager.disconnect();
    state.set_client(None);
    state.set_active_host(None);
    Ok(snapshot)
}

#[tauri::command]
pub async fn connection_status(state: State<'_, AppState>) -> CommandResult<ConnectionSnapshot> {
    Ok(state.connection_manager().await.status())
}
