//! One-time-password onboarding: app-owned key status/creation and the key install.
//!
//! The renderer never supplies a key path and never receives one from [`AppKeyView`]:
//! this module resolves the key inside the app-data directory. The one-time password is
//! wrapped in [`SecretString`] as soon as the command starts, so it is zeroized on every
//! return path and can never appear in a log, error, or saved profile.
//!
//! Every network rule (pin verification, the askpass bootstrap, the key-only check and
//! the optional `PasswordAuthentication no` step) lives in `spirit-core`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use spirit_core::engine::SecretString;
use spirit_core::error::{SpiritError, SpiritErrorCode};
use spirit_core::host::{HostDraft, HostKeyPin, HostProfile, HostProfilePatch, NewHostProfile};
use spirit_core::ssh::{
    AppKeyOptions, PasswordAuthOutcome, PasswordInstallRequest, create_app_key,
    install_public_key_with_password, unlock_app_key,
};
use tauri::State;

use crate::error::CommandResult;
use crate::state::AppState;

/// Stable app-key name. The private half lives in the app-data SSH directory.
const APP_KEY_NAME: &str = "stackspirit-onboarding";

/// The safe app-key view returned to the renderer.
///
/// It deliberately has no filesystem path: the private and public key locations stay
/// inside the host process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppKeyView {
    /// True when the app-private private-key file exists.
    pub exists: bool,
    /// `SHA256:...` fingerprint of the existing key.
    pub fingerprint: Option<String>,
    /// One-line OpenSSH public key of the existing key.
    pub public_key: Option<String>,
    /// Ids of saved profiles that point at the app-owned key.
    ///
    /// Only ids cross the bridge. The key path stays inside the host process.
    #[serde(default)]
    pub installed_host_ids: Vec<String>,
}

/// A request to install the app key with the server's one-time password.
///
/// Deliberately no `Debug` implementation: the password field must never be formatted.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OnboardingInstallInput {
    /// Host fields the owner entered; `identity_file` is replaced by the app key path.
    pub draft: HostDraft,
    /// The host-key pin the owner already confirmed out of band.
    pub host_key: HostKeyPin,
    /// One-time login password. Consumed once and then forgotten.
    pub password: String,
    /// Ask for `PasswordAuthentication no` when the server supports a safe apply.
    pub disable_password_auth: bool,
}

/// The result of one install. The one-time password is already gone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OnboardingInstallOutcome {
    /// True only after a fresh key-only login verified.
    pub installed: bool,
    /// `SHA256:...` fingerprint of the installed public key.
    pub fingerprint: String,
    /// True only when the optional drop-in is effective and sshd reloaded.
    pub password_login_disabled: bool,
    /// Detail of the optional password-login step.
    pub password_auth: PasswordAuthOutcome,
    /// The saved host profile that now points at the app key.
    pub profile: HostProfile,
}

/// Returns the app-private path. The renderer never sees or supplies this value.
fn app_key_path(state: &AppState) -> PathBuf {
    state.app_keys_dir().join(APP_KEY_NAME)
}

/// Returns the ids of saved profiles that point at the app-owned onboarding key.
///
/// A profile that was saved before an install failed is included on purpose: its
/// pin is already trusted and the Settings retry may finish the key install.
fn installed_host_ids(state: &AppState, key_path: &Path) -> CommandResult<Vec<String>> {
    let hosts = state.hosts().list()?;
    Ok(hosts
        .into_iter()
        .filter(|host| host.identity_file.as_deref() == Some(key_path))
        .map(|host| host.id.0)
        .collect())
}

/// Reads the existing key without ever exposing a path.
///
/// A key file without a usable keychain entry is a typed error, not a pretend
/// `exists: false`: the file is there and the owner needs to know why it cannot be used.
fn existing_key_view(state: &AppState, path: &Path) -> CommandResult<AppKeyView> {
    if !path.is_file() {
        return Ok(AppKeyView {
            exists: false,
            fingerprint: None,
            public_key: None,
            installed_host_ids: Vec::new(),
        });
    }
    let unlocked = unlock_app_key(path, state.engine_keychain())?;
    Ok(AppKeyView {
        exists: true,
        fingerprint: Some(unlocked.fingerprint),
        public_key: Some(unlocked.public_key),
        installed_host_ids: Vec::new(),
    })
}

/// Saves the pinned profile before an install runs.
///
/// A profile for the same SSH destination is updated in place, so retrying a failed
/// install never creates duplicates. A changed fingerprint is refused instead of being
/// silently re-pinned: the existing explicit trust flow must confirm it first. The app
/// key path replaces any renderer-supplied identity file.
fn save_host_profile(
    state: &AppState,
    draft: &HostDraft,
    pin: &HostKeyPin,
) -> CommandResult<HostProfile> {
    let existing = state
        .hosts()
        .list()?
        .into_iter()
        .find(|host| host.ssh == draft.ssh);
    let Some(host) = existing else {
        return state.hosts().add(NewHostProfile {
            draft: draft.clone(),
            host_key: pin.clone(),
        });
    };

    if host.host_key != *pin {
        return Err(
            SpiritError::host_key_mismatch(&host.host_key.sha256, &pin.sha256)
                .with_hint("Re-pin this saved server explicitly, then retry the app-key install."),
        );
    }

    state.hosts().update(
        &host.id,
        HostProfilePatch {
            label: Some(draft.label.clone()),
            ssh: Some(draft.ssh.clone()),
            identity_file: Some(draft.identity_file.clone()),
            vowp_home: Some(draft.vowp_home.clone()),
            default_project: Some(draft.default_project.clone()),
        },
    )
}

/// Reports whether the app-owned key already exists, without creating it.
#[tauri::command]
pub async fn onboarding_key_status(state: State<'_, AppState>) -> CommandResult<AppKeyView> {
    let state = state.inner();
    let path = app_key_path(state);
    let mut view = existing_key_view(state, &path)?;
    view.installed_host_ids = installed_host_ids(state, &path)?;
    Ok(view)
}

/// Creates the app-owned ed25519 key, or returns the existing one.
#[tauri::command]
pub async fn onboarding_create_key(state: State<'_, AppState>) -> CommandResult<AppKeyView> {
    let state = state.inner();
    let path = app_key_path(state);
    if path.is_file() {
        let mut view = existing_key_view(state, &path)?;
        view.installed_host_ids = installed_host_ids(state, &path)?;
        return Ok(view);
    }
    match create_app_key(
        state.app_keys_dir(),
        APP_KEY_NAME,
        state.engine_keychain(),
        &AppKeyOptions::new(),
    ) {
        Ok(key) => Ok(AppKeyView {
            exists: true,
            fingerprint: Some(key.fingerprint),
            public_key: Some(key.public_key),
            installed_host_ids: installed_host_ids(state, &path)?,
        }),
        // A concurrent create won the race; the existing key is the idempotent answer.
        Err(error) if error.code == SpiritErrorCode::Conflict => {
            let mut view = existing_key_view(state, &path)?;
            view.installed_host_ids = installed_host_ids(state, &path)?;
            Ok(view)
        }
        Err(error) => Err(error),
    }
}

/// Installs the app key with a one-time password and returns the saved host profile.
///
/// The confirmed profile is saved **before** the install starts, so a failed password
/// hop leaves a visible, pinned profile that can be retried without redoing the
/// fingerprint step. On failure only the typed install error is returned.
#[tauri::command]
pub async fn onboarding_install_key(
    state: State<'_, AppState>,
    mut input: OnboardingInstallInput,
) -> CommandResult<OnboardingInstallOutcome> {
    let state = state.inner();
    // Move the secret into zeroizing storage immediately. The empty `String` left in
    // `input` holds no bytes of the password.
    let password = SecretString::new(std::mem::take(&mut input.password));
    let key_path = app_key_path(state);
    if !key_path.is_file() {
        return Err(SpiritError::not_found("app SSH key")
            .with_hint("Create the app SSH key before installing it on a server."));
    }

    let mut draft = input.draft;
    draft.identity_file = Some(key_path.clone());
    draft.validate()?;
    input.host_key.validate()?;
    let profile = save_host_profile(state, &draft, &input.host_key)?;

    let options = state.ssh_options(Some(key_path.clone()));
    // The secret lives only inside this block and is zeroized when the request drops,
    // including on the error path.
    let outcome = {
        let request = PasswordInstallRequest {
            ssh: draft.ssh,
            host_key: input.host_key,
            password,
            private_key_path: key_path,
            disable_password_auth: input.disable_password_auth,
        };
        install_public_key_with_password(state.runner(), &request, &options).await?
    };

    Ok(OnboardingInstallOutcome {
        installed: outcome.installed,
        fingerprint: outcome.fingerprint,
        password_login_disabled: outcome.password_login_disabled,
        password_auth: outcome.password_auth,
        profile,
    })
}
