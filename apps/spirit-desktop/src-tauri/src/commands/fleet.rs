//! Fleet read models, raw documents, inbox and job/capacity mutations.

use serde::Serialize;
use spirit_core::error::{SpiritError, SpiritErrorCode};
use spirit_core::time::Timestamp;
use spirit_core::view::{FleetView, InboxItemView, TargetView};
use spirit_core::wire::{CapacityReportDoc, FleetStatus, JobAction, SetTargetInput, WireWarning};
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
    manager.poll_once(client.as_ref()).await?.ok_or_else(|| {
        SpiritError::new(
            SpiritErrorCode::Conflict,
            "no host is connected, so there is no fleet view to refresh",
        )
    })
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
pub async fn job_action(
    state: State<'_, AppState>,
    job_id: String,
    action: JobAction,
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
    client.job_action(&project, &job_id, action).await?;
    let refreshed = manager.poll_once(client.as_ref()).await.is_ok();
    Ok(ActionResult {
        job_id,
        action,
        request: None,
        refreshed,
    })
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
