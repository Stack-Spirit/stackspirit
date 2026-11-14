//! Local engine boundary. D2 exposes status and safe keychain methods; D4 fills in the
//! bundled sidecar and the model catalog. No command ever forwards a key to VOWP.

use std::path::PathBuf;

use spirit_core::engine::{
    EngineChoice, EngineStatus, ExternalHarness, ExternalHarnessKind, ModelChoice,
    PathHarnessProbe, SecretString, scan_external_harnesses,
};
use spirit_core::error::{SpiritError, SpiritErrorCode};
use spirit_core::service::KeyStatus;
use spirit_core::time::Timestamp;
use tauri::State;

use crate::error::{CommandResult, keychain_error};
use crate::state::AppState;

#[tauri::command]
pub async fn engine_status(state: State<'_, AppState>) -> CommandResult<EngineStatus> {
    Ok(state.core().await.engine_status())
}

/// D4 supplies the verified catalog. Returning an empty list is honest in D2: the UI
/// must not claim an included model exists before the runtime reports it.
#[tauri::command]
pub async fn engine_models(state: State<'_, AppState>) -> CommandResult<Vec<ModelChoice>> {
    let _ = state.core().await;
    Ok(Vec::new())
}

#[tauri::command]
pub async fn select_model(
    state: State<'_, AppState>,
    model_id: String,
) -> CommandResult<EngineStatus> {
    let _ = (state, model_id);
    Err(SpiritError::new(
        SpiritErrorCode::Engine,
        "model selection is available after engine onboarding (D4)",
    ))
}

#[tauri::command]
pub async fn store_key(
    state: State<'_, AppState>,
    provider: String,
    secret: String,
) -> CommandResult<KeyStatus> {
    let secret = SecretString::new(secret);
    state
        .core()
        .await
        .store_key(&provider, &secret)
        .map_err(keychain_error)
}

#[tauri::command]
pub async fn verify_key(state: State<'_, AppState>, provider: String) -> CommandResult<KeyStatus> {
    // D2 has no model catalog or provider request. Report the honest state instead of
    // claiming the key works.
    let _ = state.core().await;
    Ok(KeyStatus {
        provider,
        state: "unverified".to_owned(),
        last_checked_at: Some(Timestamp::now()),
    })
}

#[tauri::command]
pub async fn delete_key(state: State<'_, AppState>, provider: String) -> CommandResult<KeyStatus> {
    state
        .core()
        .await
        .delete_key(&provider)
        .map_err(keychain_error)
}

#[tauri::command]
pub async fn key_status(state: State<'_, AppState>) -> CommandResult<Vec<KeyStatus>> {
    state.core().await.key_status().map_err(keychain_error)
}

#[tauri::command]
pub async fn scan_external() -> CommandResult<Vec<ExternalHarness>> {
    Ok(scan_external_harnesses(&PathHarnessProbe::from_env()))
}

#[tauri::command]
pub async fn select_external(
    state: State<'_, AppState>,
    kind: ExternalHarnessKind,
    path: String,
) -> CommandResult<EngineStatus> {
    let mut core = state.core().await;
    core.select_engine(EngineChoice::External {
        path: PathBuf::from(path),
        kind,
    });
    Ok(core.engine_status())
}

#[tauri::command]
pub async fn use_bundled(state: State<'_, AppState>) -> CommandResult<EngineStatus> {
    let mut core = state.core().await;
    core.select_engine(EngineChoice::Bundled);
    Ok(core.engine_status())
}

#[tauri::command]
pub async fn restart_engine(state: State<'_, AppState>) -> CommandResult<EngineStatus> {
    let mut core = state.core().await;
    Ok(core.restart_engine().await)
}
