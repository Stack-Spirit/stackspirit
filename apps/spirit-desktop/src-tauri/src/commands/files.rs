//! Finder-style file commands for the connected server. Thin adapters: every rule
//! (path validation, argv-only quoting, secret masking, the protected-path seatbelt,
//! caps and the never-overwrite guarantee) lives in `spirit_core::vowp::files`.
//!
//! The renderer never sends a local path: uploads and downloads open the native picker
//! here, so a compromised page cannot read or write arbitrary local files. File
//! contents are never logged, and errors never carry a path.

use std::path::PathBuf;

use serde::Deserialize;
use spirit_core::error::{SpiritError, SpiritErrorCode};
use spirit_core::vowp::{
    RemoteDirListing, RemoteDownloadOutcome, RemoteFileOutcome, RemoteFiles, RemotePreview,
    RemoteUploadOutcome,
};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::{DialogExt, FilePath};

use crate::error::CommandResult;
use crate::state::AppState;

/// Builds the file service for the connected host.
fn remote_files(state: &AppState) -> CommandResult<RemoteFiles> {
    let host = state.active_host().ok_or_else(|| {
        SpiritError::new(
            SpiritErrorCode::Conflict,
            "connect to a host before browsing its files",
        )
    })?;
    // Fleet commands require a live client; reuse the same guard.
    state.client()?;
    let options = state.ssh_options(host.identity_file.clone());
    Ok(RemoteFiles::new(spirit_core::ssh::SshRemote::new(
        state.runner(),
        host.ssh.clone(),
        options,
    )))
}

/// Input for `files_preview`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewInput {
    /// Absolute or `~` path.
    pub path: String,
    /// Explicit local reveal of a secret-like file.
    #[serde(default)]
    pub reveal: bool,
}

/// Input for `files_move`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoveInput {
    /// Source path.
    pub from: String,
    /// Exact destination path.
    pub to: String,
    /// The user accepted the live-path seatbelt.
    #[serde(default)]
    pub confirm_protected: bool,
}

/// Input for `files_delete`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteInput {
    /// Path to delete.
    pub path: String,
    /// Allow deleting a non-empty folder.
    #[serde(default)]
    pub recursive: bool,
    /// The user accepted the live-path seatbelt.
    #[serde(default)]
    pub confirm_protected: bool,
}

/// Lists one directory.
#[tauri::command]
pub async fn files_list(
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<RemoteDirListing> {
    remote_files(&state)?.list_dir(&path).await
}

/// Reads a bounded preview; a secret-like file stays masked unless `reveal` is true.
#[tauri::command]
pub async fn files_preview(
    state: State<'_, AppState>,
    input: PreviewInput,
) -> CommandResult<RemotePreview> {
    remote_files(&state)?
        .read_preview(&input.path, input.reveal)
        .await
}

/// Creates a folder.
#[tauri::command]
pub async fn files_mkdir(
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<RemoteFileOutcome> {
    remote_files(&state)?.mkdir(&path).await
}

/// Renames or moves a path.
#[tauri::command]
pub async fn files_move(
    state: State<'_, AppState>,
    input: MoveInput,
) -> CommandResult<RemoteFileOutcome> {
    remote_files(&state)?
        .rename_or_move(&input.from, &input.to, input.confirm_protected)
        .await
}

/// Deletes a path.
#[tauri::command]
pub async fn files_delete(
    state: State<'_, AppState>,
    input: DeleteInput,
) -> CommandResult<RemoteFileOutcome> {
    remote_files(&state)?
        .delete(&input.path, input.recursive, input.confirm_protected)
        .await
}

fn dialog_path(file: FilePath) -> Result<PathBuf, SpiritError> {
    file.into_path().map_err(|_| {
        SpiritError::new(
            SpiritErrorCode::InvalidInput,
            "local path: the picker returned a path that is not a local file",
        )
    })
}

/// Opens the native file picker, then uploads the chosen file into `remote_dir`.
/// `null` means the user cancelled.
#[tauri::command]
pub async fn files_upload(
    app: AppHandle,
    state: State<'_, AppState>,
    remote_dir: String,
) -> CommandResult<Option<RemoteUploadOutcome>> {
    let files = remote_files(&state)?;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_file(move |picked| {
        let _ = sender.send(picked);
    });
    let picked = receiver.await.map_err(|_| {
        SpiritError::new(
            SpiritErrorCode::Internal,
            "the file dialog closed without a result",
        )
    })?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let local = dialog_path(picked)?;
    files.upload_file(&local, &remote_dir).await.map(Some)
}

/// Opens the native folder picker, then downloads `remote_path` into it.
/// `null` means the user cancelled.
#[tauri::command]
pub async fn files_download(
    app: AppHandle,
    state: State<'_, AppState>,
    remote_path: String,
) -> CommandResult<Option<RemoteDownloadOutcome>> {
    let files = remote_files(&state)?;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |picked| {
        let _ = sender.send(picked);
    });
    let picked = receiver.await.map_err(|_| {
        SpiritError::new(
            SpiritErrorCode::Internal,
            "the folder dialog closed without a result",
        )
    })?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let local = dialog_path(picked)?;
    files.download_file(&remote_path, &local).await.map(Some)
}
