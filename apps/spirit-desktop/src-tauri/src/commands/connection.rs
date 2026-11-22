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

/// How often live VPS capacity is re-read while connected.
const CAPACITY_REFRESH_INTERVAL: Duration = Duration::from_secs(20);

/// Reads `capacity report` and merges it into the fleet view. Any failure is ignored so
/// the connection and the existing fleet view are never affected.
pub(crate) async fn refresh_capacity_best_effort(
    manager: &spirit_core::connection::ConnectionManager,
    client: &dyn spirit_core::vowp::VowpClient,
) {
    let _ = manager.refresh_capacity(client).await;
}

async fn run_capacity_loop(
    manager: spirit_core::connection::ConnectionManager,
    client: std::sync::Arc<dyn spirit_core::vowp::VowpClient>,
) {
    loop {
        tokio::time::sleep(CAPACITY_REFRESH_INTERVAL).await;
        if manager.is_cancelled() || manager.status().state.as_str() == "disconnected" {
            break;
        }
        refresh_capacity_best_effort(&manager, client.as_ref()).await;
    }
}

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
    state.clear_local_orchestrator_state().await;

    let result = manager.connect(client.as_ref(), id.clone()).await;
    let snapshot = match result {
        Ok(snapshot) => snapshot,
        Err(error) => {
            state.set_client(None);
            state.set_active_host(None);
            state.clear_local_orchestrator_state().await;
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
    // Best effort: an older host without `capacity report` stays connected.
    refresh_capacity_best_effort(&manager, client.as_ref()).await;
    let poll_manager = manager.clone();
    let poll_client = client.clone();
    tauri::async_runtime::spawn(async move {
        poll_manager.run_poll_loop(poll_client).await;
    });
    // The poll loop re-derives the view from the last capacity report, so a slower
    // dedicated loop keeps Memory, Disk and CPU load live.
    let capacity_manager = manager.clone();
    let capacity_client = client;
    tauri::async_runtime::spawn(async move {
        run_capacity_loop(capacity_manager, capacity_client).await;
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
    state.clear_local_orchestrator_state().await;
    Ok(snapshot)
}

#[tauri::command]
pub async fn connection_status(state: State<'_, AppState>) -> CommandResult<ConnectionSnapshot> {
    Ok(state.connection_manager().await.status())
}
