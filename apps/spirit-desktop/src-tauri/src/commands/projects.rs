//! Project bootstrap: a thin adapter over VOWP's one-step `vowp project add`.
//!
//! Every rule (name slug, credential-free Git URL, folder shape, argv) lives in
//! `spirit-core`. No D6 gate applies: the call runs on the home host. The outcome never carries the home-side path VOWP prints.

use spirit_core::vowp::{ProjectAddOutcome, ProjectAddRequest};
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
