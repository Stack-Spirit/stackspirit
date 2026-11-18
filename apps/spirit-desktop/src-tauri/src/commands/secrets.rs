//! The server secret store: thin adapters over VOWP's `vowp secret` vault.
//!
//! The renderer never receives a secret value back. `secrets_list` returns names, the
//! last four characters and timestamps only; `secrets_set` takes the value out of the
//! request, moves it into zeroizing [`SecretString`] storage immediately and returns only
//! `{name,last4}`; `secrets_remove` returns only the name and the removed flag.
//!
//! The value is never put in argv, the environment, a log, an error hint or a Tauri
//! event: core sends it over verified SSH stdin after VOWP's hidden prompt, and every
//! rule (name grammar, value shape, `--yes`, pty transcript handling) lives in
//! `spirit-core`.

use serde::Deserialize;
use spirit_core::engine::SecretString;
use spirit_core::vowp::{SecretListView, SecretRemoveOutcome, SecretSetOutcome, SecretSetRequest};
use tauri::State;

use crate::error::CommandResult;
use crate::state::AppState;

/// A write request from the renderer.
///
/// Deliberately no `Debug` implementation: the value must never be formatted, logged or
/// included in an error. `value` is write-only and consumed exactly once.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretSetInput {
    /// The VOWP secret name (`^[A-Z][A-Z0-9_]{0,63}$`).
    pub name: String,
    /// The secret value. Moved into zeroizing storage before any validation or SSH.
    pub value: String,
}

/// Reads `vowp --json secret ls` for the connected host.
#[tauri::command]
pub async fn secrets_list(state: State<'_, AppState>) -> CommandResult<SecretListView> {
    let client = state.client()?;
    client.secret_list().await
}

/// Stores one secret over verified SSH stdin and returns only `{name,last4}`.
#[tauri::command]
pub async fn secrets_set(
    state: State<'_, AppState>,
    mut input: SecretSetInput,
) -> CommandResult<SecretSetOutcome> {
    // Move the value into zeroizing storage immediately. The empty `String` left in
    // `input` holds no bytes of the secret.
    let value = SecretString::new(std::mem::take(&mut input.value));
    // The core request validates the name and the value shape before any SSH command is
    // built, so an invalid request never reaches argv, the environment or a log.
    let request = SecretSetRequest::new(input.name, value)?;
    let client = state.client()?;
    client.secret_set(request).await
}

/// Removes one secret through `vowp secret rm NAME --yes`.
///
/// `--yes` is the core client's responsibility and is never omitted here.
#[tauri::command]
pub async fn secrets_remove(
    state: State<'_, AppState>,
    name: String,
) -> CommandResult<SecretRemoveOutcome> {
    let client = state.client()?;
    client.secret_remove(&name).await
}
