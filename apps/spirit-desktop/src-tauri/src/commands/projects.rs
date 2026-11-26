//! Project bootstrap: a thin adapter over VOWP's one-step `vowp project add`.
//!
//! Every rule (name slug, credential-free Git URL, folder shape, argv) lives in
//! `spirit-core`. No D6 gate applies: the call runs on the home host. The outcome never carries the home-side path VOWP prints.

use spirit_core::vowp::{GcReportView, HistoryJobView, ProjectAddOutcome, ProjectAddRequest};
use tauri::State;

use crate::error::CommandResult;
use crate::state::AppState;

/// Adds one project (and optionally a home-hosted orchestrator) on the connected host.
#[tauri::command]
pub async fn project_add(
    state: State<'_, AppState>,
    input: ProjectAddRequest,
) -> CommandResult<ProjectAddOutcome> {
    let client = state.client()?;
    let core = state.core().await;
    core.project_add(client.as_ref(), input).await
}

/// Loads the historic jobs of one project (`job ls --all`). Runs only when the user
/// clicks **Load history**.
#[tauri::command]
pub async fn project_history(
    state: State<'_, AppState>,
    project: String,
) -> CommandResult<Vec<HistoryJobView>> {
    let client = state.client()?;
    let core = state.core().await;
    core.project_history(client.as_ref(), &project).await
}

/// Previews the 1-day history clean-up; nothing is deleted.
#[tauri::command]
pub async fn project_gc_preview(
    state: State<'_, AppState>,
    project: String,
) -> CommandResult<GcReportView> {
    let client = state.client()?;
    let core = state.core().await;
    core.project_gc(client.as_ref(), &project, false, false)
        .await
}

/// Applies the 1-day history clean-up after the user confirmed the preview. VOWP never
/// touches active work, production, truth or secrets in this pass.
#[tauri::command]
pub async fn project_gc_apply(
    state: State<'_, AppState>,
    project: String,
    confirmed: bool,
) -> CommandResult<GcReportView> {
    let client = state.client()?;
    let core = state.core().await;
    core.project_gc(client.as_ref(), &project, true, confirmed)
        .await
}
