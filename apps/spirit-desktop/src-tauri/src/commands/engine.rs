//! Local engine boundary. The bundled sidecar, model catalog, keychain and external
//! harness picker all live in `spirit-core`; this module only adapts them to Tauri.
//! No command ever forwards a provider key to VOWP.

use std::path::PathBuf;

use spirit_core::engine::{
    EngineChoice, EngineState, EngineStatus, ExternalHarness, ExternalHarnessKind,
    KeychainErrorClass, ModelChoice, PathHarnessProbe, ProviderKeyState, ProviderKeyStatus,
    SecretString, scan_external_harnesses, verify_provider_key,
};
use spirit_core::error::{SpiritError, SpiritErrorCode};
use spirit_core::service::KeyStatus;
use spirit_core::time::Timestamp;
use tauri::State;

use crate::error::{CommandResult, keychain_error};
use crate::state::AppState;

#[tauri::command]
pub async fn engine_status(state: State<'_, AppState>) -> CommandResult<EngineStatus> {
    Ok(state.ensure_engine_setup().await)
}

#[tauri::command]
pub async fn engine_models(state: State<'_, AppState>) -> CommandResult<Vec<ModelChoice>> {
    let status = state.core().await.engine_status();
    Ok(state.verified_engine_models(&status).await)
}

#[tauri::command]
pub async fn select_model(
    state: State<'_, AppState>,
    model_id: String,
) -> CommandResult<EngineStatus> {
    let status = state.core().await.engine_status();
    if matches!(status.choice, EngineChoice::External { .. }) {
        return Err(SpiritError::new(
            SpiritErrorCode::Conflict,
            "an external harness manages its own models",
        )
        .with_hint("Switch to the bundled engine before choosing an included model."));
    }
    if !matches!(status.state, EngineState::Ready) {
        return Err(SpiritError::new(
            SpiritErrorCode::Conflict,
            "start the StackSpirit Engine before choosing a model",
        )
        .with_hint("Restart the engine and retry."));
    }
    let model = state
        .verified_engine_models(&status)
        .await
        .into_iter()
        .find(|model| model.id == model_id)
        .ok_or_else(|| {
            SpiritError::new(
                SpiritErrorCode::NotFound,
                "that model is not available in this engine build",
            )
            .with_hint("Refresh the engine model list or add a provider key.")
        })?;
    let selected_id = model.id.clone();
    state.core().await.select_model(model);

    let mut preferences = state.load_engine_preferences();
    preferences.selected_model_id = Some(selected_id);
    preferences.onboarding_complete = true;
    state.save_engine_preferences(&preferences)?;
    let status = state.core().await.engine_status();
    Ok(state.engine_status_with_preferences(status))
}

#[tauri::command]
pub async fn store_key(
    state: State<'_, AppState>,
    provider: String,
    secret: String,
) -> CommandResult<KeyStatus> {
    let provider = provider.trim().to_owned();
    if provider.is_empty() {
        return Err(SpiritError::invalid_input(
            "provider",
            "choose a provider before saving a key",
        ));
    }
    if secret.is_empty() {
        return Err(SpiritError::invalid_input(
            "secret",
            "the provider key must not be empty",
        ));
    }
    let secret = SecretString::new(secret);
    state
        .engine_keychain()
        .set(&provider, &secret)
        .map_err(keychain_error)?;

    // A saved BYOK key completes the local setup path even though no included model
    // is selected. Persist it so a relaunch does not show the first-run flow again.
    let mut preferences = state.load_engine_preferences();
    preferences.onboarding_complete = true;
    state.save_engine_preferences(&preferences)?;

    Ok(KeyStatus {
        provider,
        state: "saved".to_owned(),
        last_checked_at: Some(Timestamp::now()),
    })
}

#[tauri::command]
pub async fn verify_key(state: State<'_, AppState>, provider: String) -> CommandResult<KeyStatus> {
    let provider = provider.trim().to_owned();
    if provider.is_empty() {
        return Err(SpiritError::invalid_input(
            "provider",
            "choose a provider before verifying a key",
        ));
    }
    let engine = state.core().await.engine_status();
    let port = if matches!(engine.state, EngineState::Ready) {
        engine.port
    } else {
        None
    };
    let runtime = state.engine_runtime();
    let status = verify_provider_key(
        state.engine_keychain(),
        runtime.as_ref(),
        port,
        &provider,
        Timestamp::now(),
    )
    .await
    .map_err(keychain_error)?;
    Ok(to_key_status(status))
}

#[tauri::command]
pub async fn delete_key(state: State<'_, AppState>, provider: String) -> CommandResult<KeyStatus> {
    let provider = provider.trim().to_owned();
    if provider.is_empty() {
        return Err(SpiritError::invalid_input(
            "provider",
            "choose a provider before deleting a key",
        ));
    }
    match state.engine_keychain().delete(&provider) {
        Ok(()) => Ok(KeyStatus {
            provider,
            state: "missing".to_owned(),
            last_checked_at: Some(Timestamp::now()),
        }),
        Err(error) if error.class == KeychainErrorClass::NotFound => Ok(KeyStatus {
            provider,
            state: "missing".to_owned(),
            last_checked_at: Some(Timestamp::now()),
        }),
        Err(error) => Err(keychain_error(error)),
    }
}

#[tauri::command]
pub async fn key_status(state: State<'_, AppState>) -> CommandResult<Vec<KeyStatus>> {
    let providers = state.engine_keychain().list().map_err(keychain_error)?;
    Ok(providers
        .into_iter()
        .map(|provider| KeyStatus {
            provider,
            state: "saved".to_owned(),
            last_checked_at: None,
        })
        .collect())
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
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err(SpiritError::invalid_input(
            "path",
            "the external harness must use an absolute path",
        ));
    }
    if !path.is_file() {
        return Err(SpiritError::new(
            SpiritErrorCode::NotFound,
            "the selected external harness does not exist",
        )
        .with_hint("Scan again or use the bundled engine."));
    }
    if !is_executable(&path) {
        return Err(SpiritError::invalid_input(
            "path",
            "the selected external harness is not executable",
        ));
    }
    let mut core = state.core().await;
    core.select_engine(EngineChoice::External {
        path: path.clone(),
        kind,
    });
    let status = core.stop_engine().await;
    drop(core);

    let mut preferences = state.load_engine_preferences();
    preferences.choice = EngineChoice::External { path, kind };
    preferences.onboarding_complete = true;
    state.save_engine_preferences(&preferences)?;
    Ok(state.engine_status_with_preferences(status))
}

#[tauri::command]
pub async fn use_bundled(state: State<'_, AppState>) -> CommandResult<EngineStatus> {
    let mut core = state.core().await;
    core.select_engine(EngineChoice::Bundled);
    let status = core.engine_status();
    drop(core);

    let mut preferences = state.load_engine_preferences();
    preferences.choice = EngineChoice::Bundled;
    state.save_engine_preferences(&preferences)?;
    Ok(state.engine_status_with_preferences(status))
}

#[tauri::command]
pub async fn restart_engine(state: State<'_, AppState>) -> CommandResult<EngineStatus> {
    {
        let mut core = state.core().await;
        if matches!(core.engine_status().choice, EngineChoice::External { .. }) {
            return Err(SpiritError::new(
                SpiritErrorCode::Conflict,
                "the selected external harness manages its own process",
            )
            .with_hint("Use the bundled engine to supervise a process from StackSpirit."));
        }
        core.restart_engine().await;
    }
    state.apply_saved_model().await;
    let status = state.core().await.engine_status();
    Ok(state.engine_status_with_preferences(status))
}

fn to_key_status(status: ProviderKeyStatus) -> KeyStatus {
    KeyStatus {
        provider: status.provider,
        state: match status.state {
            ProviderKeyState::Missing => "missing",
            ProviderKeyState::Saved => "saved",
            ProviderKeyState::Invalid => "invalid",
            ProviderKeyState::Unverified => "unverified",
        }
        .to_owned(),
        last_checked_at: status.last_checked_at,
    }
}

fn is_executable(path: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}
