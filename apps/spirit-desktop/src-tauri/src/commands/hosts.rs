//! Host CRUD, host-key scanning and trust, and the add-wizard probe.

use std::path::Path;
use std::time::Duration;

use spirit_core::error::SpiritError;
use spirit_core::host::{
    HostDraft, HostId, HostKeyPin, HostProbe, HostProbeInput, HostProfile, HostProfilePatch,
    NewHostProfile, probe_host as probe_host_with_runner, verify_pin_against_scan,
};
use spirit_core::ssh::known_hosts::{known_hosts_line, write_known_hosts};
use spirit_core::ssh::{HostKeyScan, ScannedHostKey, SshTarget, scan_host_keys};
use tauri::State;

use crate::error::CommandResult;
use crate::state::AppState;

const KEY_SCAN_TIMEOUT: Duration = Duration::from_secs(8);

#[tauri::command]
pub fn list_hosts(state: State<'_, AppState>) -> CommandResult<Vec<HostProfile>> {
    state.hosts().list()
}

#[tauri::command]
pub fn add_host(state: State<'_, AppState>, input: NewHostProfile) -> CommandResult<HostProfile> {
    // `HostStore` validates the draft and the confirmed pin, and refuses a profile
    // without one. The app-private known_hosts entry is installed by `probe_host`
    // after the user confirms the same fingerprint.
    state.hosts().add(input)
}

#[tauri::command]
pub fn update_host(
    state: State<'_, AppState>,
    id: String,
    patch: HostProfilePatch,
) -> CommandResult<HostProfile> {
    state.hosts().update(&HostId(id), patch)
}

#[tauri::command]
pub fn remove_host(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    state.hosts().remove(&HostId(id))
}

#[tauri::command]
pub async fn scan_fingerprint(
    state: State<'_, AppState>,
    draft: HostDraft,
) -> CommandResult<HostKeyScan> {
    draft.validate()?;
    scan_host_keys(state.runner().as_ref(), &draft.ssh, KEY_SCAN_TIMEOUT).await
}

#[tauri::command]
pub async fn trust_fingerprint(
    state: State<'_, AppState>,
    id: String,
    pin: HostKeyPin,
) -> CommandResult<HostProfile> {
    let host_id = HostId(id);
    let profile = state
        .hosts()
        .get(&host_id)?
        .ok_or_else(|| SpiritError::not_found("host profile"))?;
    let scan = scan_host_keys(state.runner().as_ref(), &profile.ssh, KEY_SCAN_TIMEOUT).await?;
    verify_pin_against_scan(&pin, &scan.keys)?;
    install_known_hosts(state.known_hosts(), &profile.ssh, &scan.keys, &pin)?;
    state.hosts().repin(&host_id, pin)
}

#[tauri::command]
pub async fn probe_host(
    state: State<'_, AppState>,
    draft: HostDraft,
    pin: Option<HostKeyPin>,
) -> CommandResult<HostProbe> {
    draft.validate()?;

    // A confirmed pin is installed only after a fresh scan proves it still matches.
    // An unreachable host is left for the probe to report as a failing stage.
    if let Some(pin) = &pin {
        if let Ok(scan) =
            scan_host_keys(state.runner().as_ref(), &draft.ssh, KEY_SCAN_TIMEOUT).await
        {
            verify_pin_against_scan(pin, &scan.keys)?;
            install_known_hosts(state.known_hosts(), &draft.ssh, &scan.keys, pin)?;
        }
    }

    let input = HostProbeInput {
        target: draft.ssh,
        options: state.ssh_options(draft.identity_file),
        vowp_home: draft.vowp_home.filter(|value| !value.is_empty()),
        pin,
    };
    Ok(probe_host_with_runner(state.runner(), &input).await)
}

/// Replaces the target's lines in the app-private `known_hosts`, preserving every
/// other host. The file is written atomically with mode 0600 by core.
fn install_known_hosts(
    path: &Path,
    target: &SshTarget,
    scanned: &[ScannedHostKey],
    pin: &HostKeyPin,
) -> CommandResult<()> {
    let key = scanned
        .iter()
        .find(|key| key.sha256 == pin.sha256)
        .ok_or_else(|| {
            SpiritError::host_key_mismatch(
                &pin.sha256,
                scanned.first().map_or("no keys", |key| key.sha256.as_str()),
            )
        })?;

    let pattern = target.known_hosts_pattern();
    let prefix = format!("{pattern} ");
    let mut lines = Vec::new();
    if path.exists() {
        let existing = std::fs::read_to_string(path)
            .map_err(|error| SpiritError::io("reading known_hosts", &error))?;
        for line in existing.lines() {
            if line.trim().is_empty() || line.starts_with(&prefix) {
                continue;
            }
            lines.push(format!("{line}\n"));
        }
    }
    lines.push(known_hosts_line(target, key));
    write_known_hosts(path, &lines)
}
