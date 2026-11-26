//! On-demand project history: `job ls --all` and the safe 1-day `gc` pass.
//!
//! These are explicit user actions; nothing here runs on connect or render. The
//! commands own no VOWP rule: validation, argv and parsing live in `spirit-core`.
//!
//! History is a **read** that must stay usable while the desktop still has a live
//! client but the fleet poll has gone stale. `SpiritCore::project_history` refuses
//! unless the connection state is exactly `connected`, which made a History tab that
//! still rendered live data answer every click with `not_found: no host is connected`.
//! The command therefore talks to the same validated client directly, exactly like
//! the fleet and inbox commands do. A client only exists between a successful connect
//! and a disconnect/failed connect, so this never targets the wrong host.

use spirit_core::error::{SpiritError, SpiritErrorCode};
use spirit_core::vowp::history::validate_history_project;
use spirit_core::vowp::{GcReportView, HistoryJobView, VowpClient};
use tauri::State;

use crate::error::CommandResult;
use crate::state::AppState;

/// Loads the historic jobs of one project (`job ls --all --json`).
pub(crate) async fn load_history(
    client: &dyn VowpClient,
    project: &str,
) -> CommandResult<Vec<HistoryJobView>> {
    validate_history_project(project)?;
    client.project_history(project).await
}

/// Previews the 1-day clean-up; nothing is deleted.
pub(crate) async fn preview_gc(
    client: &dyn VowpClient,
    project: &str,
) -> CommandResult<GcReportView> {
    validate_history_project(project)?;
    client.project_gc(project, false).await
}

/// Applies the 1-day clean-up. `confirmed` is re-checked here and in the UI so a
/// caller can never delete without the owner having seen the preview.
pub(crate) async fn apply_gc(
    client: &dyn VowpClient,
    project: &str,
    confirmed: bool,
) -> CommandResult<GcReportView> {
    validate_history_project(project)?;
    if !confirmed {
        return Err(SpiritError::new(
            SpiritErrorCode::InvalidInput,
            "clean-up needs an explicit confirmation",
        ));
    }
    client.project_gc(project, true).await
}

/// Runs only when the user clicks **Load history**.
#[tauri::command]
pub async fn project_history(
    state: State<'_, AppState>,
    project: String,
) -> CommandResult<Vec<HistoryJobView>> {
    let client = state.client()?;
    load_history(client.as_ref(), &project).await
}

/// Previews the 1-day history clean-up; nothing is deleted.
#[tauri::command]
pub async fn project_gc_preview(
    state: State<'_, AppState>,
    project: String,
) -> CommandResult<GcReportView> {
    let client = state.client()?;
    preview_gc(client.as_ref(), &project).await
}

/// Applies the 1-day history clean-up after the owner confirmed the preview.
#[tauri::command]
pub async fn project_gc_apply(
    state: State<'_, AppState>,
    project: String,
    confirmed: bool,
) -> CommandResult<GcReportView> {
    let client = state.client()?;
    apply_gc(client.as_ref(), &project, confirmed).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use spirit_core::fixtures::{Scenario, capacity_report, fleet_status};
    use spirit_core::vowp::{GcClassView, MockVowpClient};

    fn job(job_id: &str, state: &str) -> HistoryJobView {
        HistoryJobView {
            job_id: job_id.to_owned(),
            project: "demo".to_owned(),
            title: format!("Job {job_id}"),
            kind: "build".to_owned(),
            role: "worker".to_owned(),
            state: state.to_owned(),
            updated_at: Some("2026-11-26T10:00:00Z".to_owned()),
        }
    }

    fn gc(dry_run: bool) -> GcReportView {
        GcReportView {
            dry_run,
            classes: vec![GcClassView {
                class: "run_output".to_owned(),
                files: 2,
                bytes: 2048,
                eligible_files: 2,
                eligible_bytes: 2000,
            }],
            refused: 0,
            deleted_files: if dry_run { 0 } else { 2 },
            freed_bytes: if dry_run { 0 } else { 2000 },
            compacted_sessions: 0,
            eligible_files: 2,
            eligible_bytes: 2000,
            errors: Vec::new(),
        }
    }

    fn mock() -> MockVowpClient {
        MockVowpClient::new(
            fleet_status(Scenario::Idle).expect("fleet status"),
            capacity_report(Scenario::Idle).expect("capacity"),
        )
        .with_history_jobs(vec![job("ss-1", "done"), job("ss-2", "cancelled")])
        .with_gc(gc(true), gc(false))
    }

    #[tokio::test]
    async fn the_three_actions_reach_the_client_with_their_arguments() {
        let mock = mock();

        let jobs = load_history(&mock, "demo").await.expect("history");
        assert_eq!(jobs.len(), 2);
        assert_eq!(mock.history_calls(), ["demo"]);

        let preview = preview_gc(&mock, "demo").await.expect("preview");
        assert!(preview.dry_run);
        assert_eq!(preview.eligible_files, 2);

        let applied = apply_gc(&mock, "demo", true).await.expect("apply");
        assert!(!applied.dry_run);
        assert_eq!((applied.deleted_files, applied.freed_bytes), (2, 2000));
        assert_eq!(
            mock.gc_calls(),
            [("demo".to_owned(), false), ("demo".to_owned(), true)]
        );
    }

    #[tokio::test]
    async fn apply_without_confirmation_never_reaches_the_client() {
        let mock = mock();
        let error = apply_gc(&mock, "demo", false).await.expect_err("refused");
        assert_eq!(error.code, SpiritErrorCode::InvalidInput);
        assert!(mock.gc_calls().is_empty());
    }

    #[tokio::test]
    async fn an_invalid_project_never_reaches_the_client() {
        let mock = mock();
        for error in [
            load_history(&mock, "-bad name").await.expect_err("load"),
            preview_gc(&mock, "-bad name").await.expect_err("preview"),
            apply_gc(&mock, "-bad name", true).await.expect_err("apply"),
        ] {
            assert_eq!(error.code, SpiritErrorCode::InvalidInput);
        }
        assert!(mock.history_calls().is_empty());
        assert!(mock.gc_calls().is_empty());
    }

    #[tokio::test]
    async fn a_backend_error_reaches_the_command_caller() {
        // No gc report seeded: the mock returns a typed not_found, which the command
        // must surface instead of returning an empty "success".
        let mock = MockVowpClient::new(
            fleet_status(Scenario::Idle).expect("fleet status"),
            capacity_report(Scenario::Idle).expect("capacity"),
        );
        let error = preview_gc(&mock, "demo").await.expect_err("error");
        assert_eq!(error.code, SpiritErrorCode::NotFound);
    }

    /// Guards the one link the TypeScript bridge cannot see: the commands must be
    /// present in `tauri::generate_handler!`, or every click rejects at the IPC layer.
    #[test]
    fn history_commands_are_registered_in_the_tauri_handler() {
        let source = include_str!("../lib.rs");
        for command in [
            "commands::history::project_history",
            "commands::history::project_gc_preview",
            "commands::history::project_gc_apply",
        ] {
            assert!(
                source.contains(&format!("{command},")),
                "{command} is missing from tauri::generate_handler! in lib.rs"
            );
        }
    }
}
