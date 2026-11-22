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

/// Refreshes capacity every `interval` until `token` (the cancel token of the connection
/// this loop was started for) is cancelled. Disconnect and any later connect both cancel
/// it, so a loop never outlives its connection or reads for a different host.
async fn run_capacity_loop(
    manager: spirit_core::connection::ConnectionManager,
    client: std::sync::Arc<dyn spirit_core::vowp::VowpClient>,
    mut token: spirit_core::connection::CancellationToken,
    interval: Duration,
) {
    loop {
        tokio::select! {
            () = tokio::time::sleep(interval) => {}
            () = token.cancelled() => break,
        }
        if token.is_cancelled() {
            break;
        }
        refresh_capacity_best_effort(&manager, client.as_ref()).await;
    }
}

/// Post-connect work: one immediate capacity read, then the poll and capacity loops,
/// both bound to this connection's cancel token.
fn start_connection_tasks(
    manager: &spirit_core::connection::ConnectionManager,
    client: &std::sync::Arc<dyn spirit_core::vowp::VowpClient>,
    interval: Duration,
) {
    let token = manager.cancel_token();
    let poll_manager = manager.clone();
    let poll_client = client.clone();
    tauri::async_runtime::spawn(async move {
        poll_manager.run_poll_loop(poll_client).await;
    });
    let capacity_manager = manager.clone();
    let capacity_client = client.clone();
    tauri::async_runtime::spawn(async move {
        run_capacity_loop(capacity_manager, capacity_client, token, interval).await;
    });
}

/// Best-effort first capacity read after a successful connect, then start the loops.
pub(crate) async fn finish_connect(
    manager: &spirit_core::connection::ConnectionManager,
    client: &std::sync::Arc<dyn spirit_core::vowp::VowpClient>,
    interval: Duration,
) {
    // Best effort: an older host without `capacity report` stays connected.
    refresh_capacity_best_effort(manager, client.as_ref()).await;
    start_connection_tasks(manager, client, interval);
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
    finish_connect(&manager, &client, CAPACITY_REFRESH_INTERVAL).await;

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

#[cfg(test)]
mod tests {
    use super::*;
    use spirit_core::connection::ConnectionManager;
    use spirit_core::fixtures::{Scenario, capacity_report, fleet_status};
    use spirit_core::host::HostId;
    use spirit_core::vowp::{MockVowpClient, VowpClient};
    use std::sync::Arc;

    fn mock() -> MockVowpClient {
        MockVowpClient::new(
            fleet_status(Scenario::Idle).expect("status"),
            capacity_report(Scenario::Idle).expect("capacity"),
        )
    }

    async fn connected(client: &MockVowpClient) -> ConnectionManager {
        let manager = ConnectionManager::default();
        manager
            .connect(client, HostId("h1".to_owned()))
            .await
            .expect("connect");
        manager
    }

    #[tokio::test]
    async fn finish_connect_reads_capacity_immediately_and_on_the_loop() {
        let mock = mock();
        let manager = connected(&mock).await;
        let client: Arc<dyn VowpClient> = Arc::new(mock.clone());
        finish_connect(&manager, &client, Duration::from_millis(20)).await;
        assert!(mock.capacity_calls() >= 1, "connect reads capacity once");
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(mock.capacity_calls() >= 3, "loop keeps reading capacity");
        manager.disconnect();
    }

    #[tokio::test]
    async fn capacity_loop_stops_when_its_connection_is_replaced() {
        let mock = mock();
        let manager = connected(&mock).await;
        let client: Arc<dyn VowpClient> = Arc::new(mock.clone());
        let old_token = manager.cancel_token();
        let handle = tokio::spawn(run_capacity_loop(
            manager.clone(),
            client,
            old_token,
            Duration::from_millis(20),
        ));
        // The "Disconnect and connect" flow: disconnect, then connect again.
        manager.disconnect();
        manager
            .connect(&mock, HostId("h2".to_owned()))
            .await
            .expect("reconnect");
        tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("old loop exits")
            .expect("join");
        let before = mock.capacity_calls();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(mock.capacity_calls(), before, "no reads from a stale loop");
        manager.disconnect();
    }
}
