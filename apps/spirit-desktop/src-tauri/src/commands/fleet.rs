//! Fleet read models, raw documents, inbox and job/capacity mutations.

use serde::Serialize;
use spirit_core::error::{SpiritError, SpiritErrorCode};
use spirit_core::time::Timestamp;
use spirit_core::view::{FleetView, InboxItemView, TargetView};
use spirit_core::vowp::client::{SessionTakeoverRequest, SessionTakeoverResult};
use spirit_core::wire::{
    CapacityReportDoc, FleetStatus, JobAction, SessionId, SetTargetInput, WireWarning,
};
use tauri::State;

use crate::error::CommandResult;
use crate::state::AppState;

/// The parsed wire documents plus every warning kept by the connection manager.
#[derive(Debug, Serialize)]
pub struct RawDocument {
    status: Option<FleetStatus>,
    capacity: Option<CapacityReportDoc>,
    status_warnings: Vec<WireWarning>,
    capacity_warnings: Vec<WireWarning>,
    fetched_at: Option<Timestamp>,
    generated_at: Option<Timestamp>,
}

/// The result of a job action.
#[derive(Debug, Serialize)]
pub struct ActionResult {
    job_id: String,
    action: JobAction,
    request: Option<String>,
    refreshed: bool,
}

#[tauri::command]
pub async fn fleet_get(state: State<'_, AppState>) -> CommandResult<Option<FleetView>> {
    Ok(state.connection_manager().await.fleet_view())
}

#[tauri::command]
pub async fn fleet_refresh(state: State<'_, AppState>) -> CommandResult<FleetView> {
    let client = state.client()?;
    let manager = state.connection_manager().await;
    refresh_fleet(&manager, client.as_ref()).await
}

async fn refresh_fleet(
    manager: &spirit_core::connection::ConnectionManager,
    client: &dyn spirit_core::vowp::VowpClient,
) -> CommandResult<FleetView> {
    let polled = manager.poll_once(client).await?;
    if polled.is_none() {
        return Err(SpiritError::new(
            SpiritErrorCode::Conflict,
            "no host is connected, so there is no fleet view to refresh",
        ));
    }
    // Best effort: live Memory, Disk and CPU load. Falls back to the polled view.
    crate::commands::connection::refresh_capacity_best_effort(manager, client).await;
    Ok(manager.fleet_view().or(polled).expect("polled view exists"))
}

#[tauri::command]
pub async fn fleet_raw_document(state: State<'_, AppState>) -> CommandResult<Option<RawDocument>> {
    let manager = state.connection_manager().await;
    let status = manager.last_status();
    let capacity = manager.last_capacity_report();
    if status.is_none() && capacity.is_none() {
        return Ok(None);
    }
    Ok(Some(RawDocument {
        fetched_at: status.as_ref().map(|status| status.fetched_at),
        generated_at: status.as_ref().map(|status| status.snapshot.generated_at),
        status_warnings: manager.last_status_warnings(),
        capacity_warnings: manager.last_capacity_warnings(),
        status,
        capacity,
    }))
}

#[tauri::command]
pub async fn fleet_inbox(
    state: State<'_, AppState>,
    all: bool,
) -> CommandResult<Vec<InboxItemView>> {
    let client = state.client()?;
    let now = Timestamp::now();
    let items = client.fleet_inbox(all).await?;
    Ok(items
        .iter()
        .map(|item| InboxItemView::from_item(item, now.since(item.created_at).as_secs()))
        .collect())
}

#[tauri::command]
pub async fn answer_inbox(
    state: State<'_, AppState>,
    id: String,
    answer: String,
) -> CommandResult<InboxItemView> {
    let client = state.client()?;
    let item = client.answer_inbox(None, &id, &answer).await?;
    Ok(InboxItemView::from_item(
        &item,
        Timestamp::now().since(item.created_at).as_secs(),
    ))
}

#[tauri::command]
pub async fn dismiss_inbox(state: State<'_, AppState>, id: String) -> CommandResult<InboxItemView> {
    let client = state.client()?;
    let item = client.dismiss_inbox(None, &id).await?;
    Ok(InboxItemView::from_item(
        &item,
        Timestamp::now().since(item.created_at).as_secs(),
    ))
}

#[tauri::command]
pub async fn job_action(
    state: State<'_, AppState>,
    job_id: String,
    action: JobAction,
    reason: Option<String>,
) -> CommandResult<ActionResult> {
    let client = state.client()?;
    let manager = state.connection_manager().await;
    let project = manager
        .fleet_view()
        .and_then(|view| project_for_job(&view, &job_id))
        .ok_or_else(|| {
            SpiritError::new(
                SpiritErrorCode::NotFound,
                format!("job {job_id} is not in the current snapshot"),
            )
            .with_hint("Refresh the fleet view and try again.")
        })?;
    client
        .job_action(&project, &job_id, action, reason.as_deref())
        .await?;
    let refreshed = manager.poll_once(client.as_ref()).await.is_ok();
    Ok(ActionResult {
        job_id,
        action,
        request: None,
        refreshed,
    })
}

#[tauri::command]
pub async fn set_job_executor(
    state: State<'_, AppState>,
    project: String,
    job_id: String,
    executor: String,
) -> CommandResult<()> {
    let client = state.client()?;
    let manager = state.connection_manager().await;
    client
        .set_job_executor(&project, &job_id, &executor)
        .await?;
    let _ = manager.poll_once(client.as_ref()).await;
    Ok(())
}

#[tauri::command]
pub async fn set_target(
    state: State<'_, AppState>,
    input: SetTargetInput,
) -> CommandResult<TargetView> {
    let client = state.client()?;
    let manager = state.connection_manager().await;
    input.validate()?;
    let target = client
        .set_target(
            input.target,
            input.mode.clone(),
            input.override_,
            &input.reason,
        )
        .await?;
    let _ = manager.poll_once(client.as_ref()).await;
    Ok(TargetView {
        mode: target.mode.as_str().to_owned(),
        target_workers: target.target_workers,
        advised_workers: target.advised_workers,
        is_override: target.is_override,
        set_by: target.set_by,
        set_at: target.set_at,
        reason: target.reason,
    })
}

fn project_for_job(view: &FleetView, job_id: &str) -> Option<String> {
    if let Some(card) = view.running.iter().find(|card| card.job_id == job_id) {
        return Some(card.project.clone());
    }
    for lane in [
        &view.lanes.running,
        &view.lanes.ready,
        &view.lanes.review_fix,
        &view.lanes.parked_blocked,
        &view.lanes.done,
    ] {
        if let Some(card) = lane.iter().find(|card| card.job_id == job_id) {
            return Some(card.project.clone());
        }
    }
    None
}

/// Reads a page of one session's event log, read-only (`vowp session tail --from`).
#[tauri::command]
pub async fn session_tail(
    state: State<'_, AppState>,
    project: String,
    session_id: String,
    after_seq: Option<u64>,
) -> CommandResult<spirit_core::vowp::client::session_tail::SessionTailView> {
    let client = state.client()?;
    client
        .session_tail(&project, &session_id, after_seq, None)
        .await
}

/// Takes a session over for StackSpirit.
///
/// The renderer supplies only the session id. The local node and surface label come
/// from the host-side local-node state, and the idle-versus-running confirmation is a
/// UI decision made before this call. The project comes from the fleet view because
/// VOWP rejects `session takeover` without project context when the SSH command starts
/// outside a checkout; the project-less global orchestrator uses `_node`. A VOWP
/// without the command returns a typed `not_supported` error and no ownership is faked.
#[tauri::command]
pub async fn session_takeover(
    state: State<'_, AppState>,
    session_id: String,
) -> CommandResult<SessionTakeoverResult> {
    SessionId::parse(&session_id)?;
    let client = state.client()?;
    let project = state
        .connection_manager()
        .await
        .fleet_view()
        .and_then(|view| view.project_for_session(&session_id))
        .ok_or_else(|| {
            SpiritError::new(
                SpiritErrorCode::NotFound,
                format!("session {session_id} is not in the current snapshot"),
            )
            .with_hint("Refresh the fleet view and try again.")
        })?;
    let node = resolve_local_node(&state, client.as_ref(), Some(project.as_str())).await?;
    let label = Some(format!("StackSpirit \u{b7} {node}"));
    client
        .session_takeover(SessionTakeoverRequest {
            session: session_id,
            node,
            label,
            project,
        })
        .await
}

/// The local node name for `--node`, never supplied by the renderer.
///
/// The chat screen never opens Settings, so the cached node card can be empty after a
/// connect. Fall back to the probed gate's ready node and read its card, exactly as the
/// local-orchestrator overview does.
async fn resolve_local_node(
    state: &AppState,
    client: &dyn spirit_core::vowp::VowpClient,
    project: Option<&str>,
) -> Result<String, SpiritError> {
    // Release the core guard before `current_gates`: it locks the same mutex via
    // `connection_manager()`, so holding it across the call would deadlock.
    {
        let core = state.core().await;
        if let Some(node) = core.local_orchestrator_state().node {
            return Ok(node.name);
        }
    }
    let gates = current_gates(state).await;
    let core = state.core().await;
    if let spirit_core::engine::LocalOrchestratorGate::Ready {
        node: Some(name), ..
    } = &gates.local_orchestrator
    {
        if let Ok(card) = core.refresh_local_node(client, project, name).await {
            return Ok(card.name.to_string());
        }
    }
    Err(SpiritError::new(
        SpiritErrorCode::Gate,
        "chat_gated: this computer is not an enrolled local node yet, so it cannot take a session over",
    )
    .with_hint("Pair this computer as a local node in Settings, then try again."))
}

/// The gate snapshot for the current connection, with build flags applied.
///
/// A missing or expired cache entry is the honest "unknown" default.
async fn current_gates(state: &AppState) -> spirit_core::engine::GateSnapshot {
    let connection = state.connection_manager().await.status();
    if connection.state != "connected" {
        return crate::state::default_gate_snapshot();
    }
    let Some(host_id) = connection.host_id else {
        return crate::state::default_gate_snapshot();
    };
    let mut snapshot = state
        .gate_cache()
        .get(host_id.as_str(), Timestamp::now())
        .map(crate::state::with_build_gate_features)
        .unwrap_or_else(crate::state::default_gate_snapshot);
    snapshot.enforce_local_orchestrator_flag();
    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fleet_refresh_reads_capacity() {
        use spirit_core::fixtures::{Scenario, capacity_report, fleet_status};
        let mock = spirit_core::vowp::MockVowpClient::new(
            fleet_status(Scenario::Idle).expect("status"),
            capacity_report(Scenario::Idle).expect("capacity"),
        );
        let manager = spirit_core::connection::ConnectionManager::default();
        manager
            .connect(&mock, spirit_core::host::HostId("h1".to_owned()))
            .await
            .expect("connect");
        let before = mock.capacity_calls();
        refresh_fleet(&manager, &mock).await.expect("refresh");
        assert_eq!(mock.capacity_calls(), before + 1);
        manager.disconnect();
    }

    /// Regression: the fallback used to hold the core mutex while `current_gates`
    /// locked it again, so an empty node card hung forever.
    #[tokio::test]
    async fn resolve_local_node_without_a_card_returns_gate_error_instead_of_hanging() {
        let dir = std::env::temp_dir().join(format!("ss92-resolve-node-{}", std::process::id()));
        let (state, _events) = AppState::new(dir.clone()).expect("state");
        let client =
            spirit_core::fixtures::FixtureVowpClient::new(spirit_core::fixtures::Scenario::Idle)
                .expect("fixture client");
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            resolve_local_node(&state, &client, Some("stackspirit")),
        )
        .await
        .expect("resolve_local_node must not deadlock");
        let error = result.expect_err("no enrolled node");
        assert_eq!(error.code, SpiritErrorCode::Gate);
        assert!(error.message.contains("chat_gated"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
