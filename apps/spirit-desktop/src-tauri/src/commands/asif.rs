//! Thin ASIF export/import command adapters for the D5 desktop host.
//!
//! Every VOWP operation, size cap, loss report, remote temp cleanup and idle check lives
//! in `spirit-core`. This module only:
//!
//! - refuses unless the ASIF gate and the build feature flag allow the direction,
//! - chooses a local path with the native picker or validates one supplied by the caller,
//! - reads/writes the local package, and
//! - returns a safe DTO that never carries a remote path, package bytes, token or key.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use spirit_core::engine::{AsifGate, GateSnapshot};
use spirit_core::error::{SpiritError, SpiritErrorCode};
use spirit_core::time::Timestamp;
use spirit_core::vowp::{
    AsifExport, AsifExportOptions, AsifImportOptions, AsifLossPolicy, ensure_package_size,
};
use spirit_core::wire::{AsifImportResponse, AsifLoss, SessionId};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::{DialogExt, FilePath};

use crate::error::CommandResult;
use crate::state::{AppState, default_gate_snapshot, with_build_gate_features};

/// The direction named in a gate refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AsifAction {
    Export,
    Import,
}

impl AsifAction {
    const fn label(self) -> &'static str {
        match self {
            Self::Export => "ASIF export",
            Self::Import => "ASIF import",
        }
    }

    fn enabled(self, gates: &GateSnapshot) -> bool {
        match self {
            Self::Export => gates.asif_export_enabled(),
            Self::Import => gates.asif_import_enabled(),
        }
    }
}

/// Refuses one direction with a path-free reason.
fn ensure_asif_action(gates: &GateSnapshot, action: AsifAction) -> Result<(), SpiritError> {
    if action.enabled(gates) {
        return Ok(());
    }
    Err(asif_gate_error(gates, action))
}

fn asif_gate_error(gates: &GateSnapshot, action: AsifAction) -> SpiritError {
    if !gates.features.asif_actions {
        return SpiritError::new(
            SpiritErrorCode::Gate,
            format!("{} is not implemented in this build.", action.label()),
        )
        .with_hint("This build does not expose ASIF actions.");
    }
    let error = match &gates.asif {
        AsifGate::Unknown => {
            format!(
                "{} is unavailable until the VOWP capability probe succeeds.",
                action.label()
            )
        }
        AsifGate::Probing => "The ASIF capability probe is still running.".to_owned(),
        AsifGate::WaitingForVowpAsif { missing, .. } => {
            if missing.is_empty() {
                "Waiting for VOWP ASIF.".to_owned()
            } else {
                format!("Waiting for VOWP ASIF. Missing: {}", missing.join(", "))
            }
        }
        AsifGate::Incompatible {
            detected, required, ..
        } => format!("Incompatible ASIF surface: {detected} (required {required})."),
        AsifGate::Unreachable { detail } => {
            format!("The VOWP home could not be reached: {detail}")
        }
        AsifGate::Ready { .. } => match action {
            AsifAction::Export => "ASIF export capability is not proven.".to_owned(),
            AsifAction::Import => "ASIF import capability is not proven.".to_owned(),
        },
    };
    SpiritError::new(SpiritErrorCode::Gate, error)
        .with_hint("Probe the VOWP home gates, then retry.")
}

/// Returns the gate snapshot that belongs to the current host connection.
///
/// The per-host cache is cleared on every connect, failed connect and disconnect, so a
/// stale `Ready` from another host can never enable an ASIF action. A missing or expired
/// entry is the honest "unknown" default, not a remembered capability.
async fn current_asif_gates(state: &AppState) -> GateSnapshot {
    let connection = state.connection_manager().await.status();
    if connection.state != "connected" {
        return default_gate_snapshot();
    }
    let Some(host_id) = connection.host_id else {
        return default_gate_snapshot();
    };
    state
        .gate_cache()
        .get(host_id.as_str(), Timestamp::now())
        .map(with_build_gate_features)
        .unwrap_or_else(default_gate_snapshot)
}

/// Export options as the renderer sends them. Unknown names (including a force switch)
/// are rejected instead of silently ignored.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase", default)]
pub struct AsifExportOptionsInput {
    /// Refuse blocking/integrity losses.
    pub strict: bool,
    /// Let recoverable integrity/ambiguity problems become declared losses.
    pub allow_loss: bool,
    /// Optional absolute local save path; omitted to use the native save dialog.
    pub path: Option<String>,
}

impl AsifExportOptionsInput {
    fn loss_policy(&self) -> Result<AsifLossPolicy, SpiritError> {
        loss_policy(self.strict, self.allow_loss)
    }
}

/// Import options as the renderer sends them. There is deliberately no `cwd`: the
/// renderer never sends a remote path.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase", default)]
pub struct AsifImportOptionsInput {
    /// Select one ASIF branch when the document has more than one.
    pub branch: Option<String>,
    /// Refuse blocking/integrity losses.
    pub strict: bool,
    /// Let recoverable integrity/ambiguity problems become declared losses.
    pub allow_loss: bool,
    /// Optional absolute local package path; omitted to use the native open dialog.
    pub path: Option<String>,
}

impl AsifImportOptionsInput {
    fn to_core(&self) -> Result<AsifImportOptions, SpiritError> {
        let options = AsifImportOptions {
            branch: self.branch.clone(),
            cwd: None,
            loss: loss_policy(self.strict, self.allow_loss)?,
        };
        options.validate()?;
        Ok(options)
    }
}

fn loss_policy(strict: bool, allow_loss: bool) -> Result<AsifLossPolicy, SpiritError> {
    match (strict, allow_loss) {
        (true, true) => Err(SpiritError::invalid_input(
            "ASIF options",
            "strict and allowLoss are mutually exclusive",
        )),
        (true, false) => Ok(AsifLossPolicy::Strict),
        (false, true) => Ok(AsifLossPolicy::AllowLoss),
        (false, false) => Ok(AsifLossPolicy::Default),
    }
}

/// The safe export DTO. No remote path, `report_path`, or package bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AsifExportResult {
    /// Source session.
    pub session_id: String,
    /// Local file basename only.
    pub file_name: String,
    /// Package size written locally.
    pub bytes: u64,
    /// Declared losses, as VOWP reported them.
    pub loss_count: u64,
    /// Path-free declared losses.
    pub losses: Vec<AsifLoss>,
    /// VOWP operations the ASIF document cannot carry.
    pub unsupported: Vec<String>,
}

impl AsifExportResult {
    fn from_export(export: &AsifExport, local_path: &Path) -> Result<Self, SpiritError> {
        let report = &export.summary.report;
        Ok(Self {
            session_id: export.summary.session.as_str().to_owned(),
            file_name: local_file_name(local_path)?,
            bytes: export.summary.bytes,
            loss_count: report.loss_count,
            losses: safe_losses(&report.losses),
            unsupported: report.unsupported.clone(),
        })
    }
}

/// The safe import DTO. The imported session is always reported as idle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AsifImportResult {
    /// Newly registered session.
    pub session_id: String,
    /// Declared import losses.
    pub loss_count: u64,
    /// Path-free declared losses.
    pub losses: Vec<AsifLoss>,
    /// VOWP reindex warnings, not losses.
    pub warnings: Vec<String>,
    /// VOWP operations the ASIF document cannot carry.
    pub unsupported: Vec<String>,
    /// The contract-required idle state.
    pub state: &'static str,
}

impl AsifImportResult {
    fn from_response(response: &AsifImportResponse) -> Result<Self, SpiritError> {
        response.ensure_idle()?;
        let report = &response.report;
        Ok(Self {
            session_id: response.session.id.as_str().to_owned(),
            loss_count: report.loss_count,
            losses: safe_losses(&report.losses),
            warnings: response
                .warnings
                .iter()
                .map(|warning| safe_text(warning))
                .collect(),
            unsupported: report.unsupported.clone(),
            state: "idle",
        })
    }
}

fn safe_losses(losses: &[AsifLoss]) -> Vec<AsifLoss> {
    losses
        .iter()
        .map(|loss| AsifLoss {
            kind: loss.kind.clone(),
            stage: loss.stage.clone(),
            scope: loss.scope.clone(),
            message: safe_text(&loss.message),
        })
        .collect()
}

fn safe_text(input: &str) -> String {
    spirit_core::error::redact(input)
        .chars()
        .take(1024)
        .collect()
}

fn local_file_name(path: &Path) -> Result<String, SpiritError> {
    let name = path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| SpiritError::invalid_input("local ASIF package path", "must name a file"))?;
    if name.chars().any(char::is_control) {
        return Err(SpiritError::invalid_input(
            "local ASIF package path",
            "the file name contains control characters",
        ));
    }
    Ok(name.to_owned())
}

/// Validates an absolute local package path without touching the filesystem.
fn validate_local_package_path(raw: &str) -> Result<PathBuf, SpiritError> {
    if raw.is_empty() {
        return Err(SpiritError::invalid_input(
            "local ASIF package path",
            "must not be empty",
        ));
    }
    if raw.contains('\u{0}') {
        return Err(SpiritError::invalid_input(
            "local ASIF package path",
            "must not contain a NUL byte",
        ));
    }
    let path = PathBuf::from(raw);
    if !path.is_absolute() {
        return Err(SpiritError::invalid_input(
            "local ASIF package path",
            "must be an absolute local path",
        ));
    }
    validate_package_file_name(&path)?;
    Ok(path)
}

fn validate_package_file_name(path: &Path) -> Result<(), SpiritError> {
    let name = path.file_name().and_then(std::ffi::OsStr::to_str);
    let Some(name) = name else {
        return Err(SpiritError::invalid_input(
            "local ASIF package path",
            "must name a file",
        ));
    };
    if name.is_empty() || name == "." || name == ".." {
        return Err(SpiritError::invalid_input(
            "local ASIF package path",
            "must name a file",
        ));
    }
    if name.len() > 200 {
        return Err(SpiritError::invalid_input(
            "local ASIF package path",
            "the file name must be at most 200 bytes",
        ));
    }
    if name.chars().any(char::is_control) {
        return Err(SpiritError::invalid_input(
            "local ASIF package path",
            "the file name contains control characters",
        ));
    }
    if !name.to_ascii_lowercase().ends_with(".zip") {
        return Err(SpiritError::invalid_input(
            "local ASIF package path",
            "must end in .zip",
        ));
    }
    Ok(())
}

fn validate_save_target(path: &Path) -> Result<(), SpiritError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let Some(parent) = parent else {
        return Err(SpiritError::invalid_input(
            "local ASIF package path",
            "must have a parent directory",
        ));
    };
    if !parent.is_dir() {
        return Err(SpiritError::invalid_input(
            "local ASIF save location",
            "the parent directory does not exist",
        ));
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(SpiritError::invalid_input(
            "local ASIF save location",
            "must not be a symbolic link",
        )),
        Ok(metadata) if !metadata.is_file() => Err(SpiritError::invalid_input(
            "local ASIF save location",
            "must be a regular file path",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(SpiritError::io(
            "checking the local ASIF save location",
            &error,
        )),
    }
}

fn validate_import_file(path: &Path) -> Result<u64, SpiritError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            SpiritError::invalid_input("local ASIF package", "the selected file does not exist")
        } else {
            SpiritError::io("checking the local ASIF package", &error)
        }
    })?;
    if metadata.file_type().is_symlink() {
        return Err(SpiritError::invalid_input(
            "local ASIF package",
            "must not be a symbolic link",
        ));
    }
    if !metadata.is_file() {
        return Err(SpiritError::invalid_input(
            "local ASIF package",
            "must be a regular file",
        ));
    }
    ensure_package_size(metadata.len())?;
    Ok(metadata.len())
}

fn write_local_package(path: &Path, bytes: &[u8]) -> Result<(), SpiritError> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| SpiritError::io("saving the local ASIF package", &error))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|error| SpiritError::io("securing the local ASIF package", &error))?;
    }
    file.write_all(bytes)
        .map_err(|error| SpiritError::io("writing the local ASIF package", &error))?;
    file.sync_all()
        .map_err(|error| SpiritError::io("finishing the local ASIF package", &error))?;
    Ok(())
}

fn dialog_path(file: FilePath) -> Result<PathBuf, SpiritError> {
    file.into_path().map_err(|_| {
        SpiritError::new(
            SpiritErrorCode::InvalidInput,
            "local ASIF package path: the picker returned a path that is not a local file",
        )
        .with_hint("Choose a local .zip file and retry.")
    })
}

async fn choose_save_path(
    app: &AppHandle,
    suggested_name: String,
) -> Result<Option<PathBuf>, SpiritError> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_file_name(suggested_name)
        .add_filter("ASIF package", &["zip"])
        .save_file(move |picked| {
            let _ = sender.send(picked);
        });
    let picked = receiver.await.map_err(|_| {
        SpiritError::new(
            SpiritErrorCode::Internal,
            "the save dialog closed without a result",
        )
    })?;
    picked.map(dialog_path).transpose()
}

async fn choose_import_path(app: &AppHandle) -> Result<Option<PathBuf>, SpiritError> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("ASIF package", &["zip"])
        .pick_file(move |picked| {
            let _ = sender.send(picked);
        });
    let picked = receiver.await.map_err(|_| {
        SpiritError::new(
            SpiritErrorCode::Internal,
            "the file dialog closed without a result",
        )
    })?;
    picked.map(dialog_path).transpose()
}

/// Applies the D5 build flags to a gate snapshot without weakening the probe.
///
/// The Rust function name is unique so the D4 adapter can coexist; the exposed command
/// name stays `gates_get`, which is what the desktop bridge calls.
#[tauri::command(rename = "gates_get")]
pub async fn gates_get_with_build_flags(state: State<'_, AppState>) -> CommandResult<GateSnapshot> {
    let mut snapshot =
        with_build_gate_features(crate::commands::gates::gates_get(state.clone()).await?);
    snapshot.enforce_local_orchestrator_flag();
    state.core().await.set_gates(snapshot.clone());
    Ok(snapshot)
}

/// Applies the D5/D6 build flags after a full gate probe. A waiting, incompatible,
/// unreachable, or partial gate keeps its missing action disabled, and the D6 flag is
/// on exactly for `Ready` plus a connected local agent.
#[tauri::command(rename = "probe_gates")]
pub async fn probe_gates_with_build_flags(
    state: State<'_, AppState>,
) -> CommandResult<GateSnapshot> {
    let mut snapshot =
        with_build_gate_features(crate::commands::gates::probe_gates(state.clone()).await?);
    snapshot.enforce_local_orchestrator_flag();
    state.core().await.set_gates(snapshot.clone());
    Ok(snapshot)
}

/// Exports one session to a local `.asif.zip` and returns only the safe summary.
///
/// `null` means the user cancelled the native save dialog; no export ran.
#[tauri::command]
pub async fn asif_export(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    options: AsifExportOptionsInput,
) -> CommandResult<Option<AsifExportResult>> {
    let gates = current_asif_gates(state.inner()).await;
    ensure_asif_action(&gates, AsifAction::Export)?;

    let session = SessionId::parse(&session_id)?;
    let loss = options.loss_policy()?;
    let suggested_name = format!("{}.asif.zip", session.as_str());

    let target = match options.path.as_deref() {
        Some(raw) => validate_local_package_path(raw)?,
        None => match choose_save_path(&app, suggested_name).await? {
            Some(path) => {
                validate_package_file_name(&path)?;
                path
            }
            None => return Ok(None),
        },
    };
    validate_save_target(&target)?;

    let client = state.client()?;
    let core = state.core().await;
    let export = core
        .export_asif(
            client.as_ref(),
            None,
            session.as_str(),
            AsifExportOptions::new(loss),
        )
        .await?;
    drop(core);

    let bytes = export.package.as_bytes().to_vec();
    let write_path = target.clone();
    tokio::task::spawn_blocking(move || write_local_package(&write_path, &bytes))
        .await
        .map_err(|error| {
            SpiritError::new(
                SpiritErrorCode::Internal,
                format!("saving the local ASIF package failed: {error}"),
            )
        })??;

    Ok(Some(AsifExportResult::from_export(&export, &target)?))
}

/// Imports one local `.asif.zip` as a new idle session.
///
/// `null` means the user cancelled the native open dialog; no import ran. This command
/// never starts a turn, writes a capsule, creates a lease, or dispatches a job.
#[tauri::command]
pub async fn asif_import(
    app: AppHandle,
    state: State<'_, AppState>,
    options: AsifImportOptionsInput,
) -> CommandResult<Option<AsifImportResult>> {
    let gates = current_asif_gates(state.inner()).await;
    ensure_asif_action(&gates, AsifAction::Import)?;

    let import_options = options.to_core()?;
    let path = match options.path.as_deref() {
        Some(raw) => validate_local_package_path(raw)?,
        None => match choose_import_path(&app).await? {
            Some(path) => {
                validate_package_file_name(&path)?;
                path
            }
            None => return Ok(None),
        },
    };
    let declared_len = validate_import_file(&path)?;

    let read_path = path.clone();
    let bytes = tokio::task::spawn_blocking(move || std::fs::read(read_path))
        .await
        .map_err(|error| {
            SpiritError::new(
                SpiritErrorCode::Internal,
                format!("reading the local ASIF package failed: {error}"),
            )
        })?
        .map_err(|error| SpiritError::io("reading the local ASIF package", &error))?;
    let actual_len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if actual_len != declared_len {
        return Err(SpiritError::new(
            SpiritErrorCode::Conflict,
            "the selected ASIF package changed while it was being read",
        )
        .with_hint("Choose the file again."));
    }
    ensure_package_size(actual_len)?;

    let client = state.client()?;
    let core = state.core().await;
    let response = core
        .import_asif(client.as_ref(), None, &bytes, import_options)
        .await?;
    drop(core);

    Ok(Some(AsifImportResult::from_response(&response)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use spirit_core::engine::{GateFeatureFlags, LocalAgentStatus, LocalOrchestratorGate};
    use spirit_core::time::Timestamp;
    use spirit_core::vowp::AsifPackage;
    use spirit_core::wire::{SessionState, parse_asif_export_summary, parse_asif_import_response};

    const EXPORT_FIXTURE: &str =
        include_str!("../../../../../crates/spirit-core/fixtures/vowp/asif-export-summary.json");
    const IMPORT_FIXTURE: &str =
        include_str!("../../../../../crates/spirit-core/fixtures/vowp/asif-import-response.json");

    fn gates(asif: AsifGate, asif_actions: bool) -> GateSnapshot {
        GateSnapshot {
            probed_at: Timestamp::epoch(),
            local_orchestrator: LocalOrchestratorGate::Unknown,
            asif,
            local_agent: LocalAgentStatus::default(),
            features: GateFeatureFlags {
                asif_actions,
                local_orchestrator_actions: false,
            },
        }
    }

    fn ready(export: bool, import: bool) -> AsifGate {
        AsifGate::Ready {
            export,
            import,
            detected_version: Some("0.4.0".to_owned()),
            evidence: vec!["session export --format asif: exit 3 (supported)".to_owned()],
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "stackspirit-asif-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create test directory");
        dir
    }

    #[test]
    fn a_waiting_or_partial_gate_refuses_the_missing_action() {
        let waiting = gates(
            AsifGate::WaitingForVowpAsif {
                missing: vec!["v4-asif-core".to_owned(), "v4-asif-cli".to_owned()],
                detected_version: Some("0.4.0".to_owned()),
                evidence: vec!["session export --format asif: exit 2 (unsupported)".to_owned()],
            },
            true,
        );
        let error = ensure_asif_action(&waiting, AsifAction::Export).unwrap_err();
        assert_eq!(error.code, SpiritErrorCode::Gate);
        assert!(error.message.contains("v4-asif-core"));
        assert!(ensure_asif_action(&waiting, AsifAction::Import).is_err());

        let export_only = gates(ready(true, false), true);
        assert!(ensure_asif_action(&export_only, AsifAction::Export).is_ok());
        let error = ensure_asif_action(&export_only, AsifAction::Import).unwrap_err();
        assert_eq!(error.code, SpiritErrorCode::Gate);
        assert_eq!(error.message, "ASIF import capability is not proven.");

        let import_only = gates(ready(false, true), true);
        assert!(ensure_asif_action(&import_only, AsifAction::Import).is_ok());
        let error = ensure_asif_action(&import_only, AsifAction::Export).unwrap_err();
        assert_eq!(error.message, "ASIF export capability is not proven.");

        let both = gates(ready(true, true), true);
        assert!(ensure_asif_action(&both, AsifAction::Export).is_ok());
        assert!(ensure_asif_action(&both, AsifAction::Import).is_ok());
    }

    #[test]
    fn a_disabled_build_flag_or_an_unproven_gate_refuses_both_actions() {
        let disabled = gates(ready(true, true), false);
        for action in [AsifAction::Export, AsifAction::Import] {
            let error = ensure_asif_action(&disabled, action).unwrap_err();
            assert_eq!(error.code, SpiritErrorCode::Gate);
            assert!(error.message.contains("not implemented in this build"));
        }

        for gate in [
            AsifGate::Unknown,
            AsifGate::Probing,
            AsifGate::Incompatible {
                detected: "asif/0.3".to_owned(),
                required: "asif/0.4".to_owned(),
                evidence: vec!["session export --format asif: exit 2".to_owned()],
            },
            AsifGate::Unreachable {
                detail: "vowp --version timed out".to_owned(),
            },
        ] {
            let snapshot = gates(gate, true);
            assert!(ensure_asif_action(&snapshot, AsifAction::Export).is_err());
            assert!(ensure_asif_action(&snapshot, AsifAction::Import).is_err());
        }
    }

    #[test]
    fn gate_refusals_are_path_free() {
        let snapshot = gates(
            AsifGate::WaitingForVowpAsif {
                missing: vec!["v4-asif-core".to_owned()],
                detected_version: None,
                evidence: vec![],
            },
            true,
        );
        for action in [AsifAction::Export, AsifAction::Import] {
            let error = ensure_asif_action(&snapshot, action).unwrap_err();
            let combined = format!("{} {}", error.message, error.hint.unwrap_or_default());
            assert!(!combined.contains('/'), "path leaked: {combined}");
        }
    }

    #[test]
    fn strict_and_allow_loss_are_mutually_exclusive_and_unknown_options_are_rejected() {
        let strict: AsifExportOptionsInput =
            serde_json::from_value(json!({"strict": true})).unwrap();
        assert_eq!(strict.loss_policy().unwrap(), AsifLossPolicy::Strict);

        let allow: AsifExportOptionsInput =
            serde_json::from_value(json!({"allowLoss": true})).unwrap();
        assert_eq!(allow.loss_policy().unwrap(), AsifLossPolicy::AllowLoss);

        let default: AsifExportOptionsInput = serde_json::from_value(json!({})).unwrap();
        assert_eq!(default.loss_policy().unwrap(), AsifLossPolicy::Default);

        let both: AsifExportOptionsInput =
            serde_json::from_value(json!({"strict": true, "allowLoss": true})).unwrap();
        let error = both.loss_policy().unwrap_err();
        assert_eq!(error.code, SpiritErrorCode::InvalidInput);

        assert!(
            serde_json::from_value::<AsifExportOptionsInput>(json!({"allowUnsupported": true}))
                .is_err()
        );
        assert!(
            serde_json::from_value::<AsifImportOptionsInput>(json!({"cwd": "/srv/vowp"})).is_err(),
            "the renderer must not send a remote cwd"
        );
    }

    #[test]
    fn path_and_loss_errors_never_echo_a_secret_or_full_path() {
        let marker = ["s", "k", "_", "not-a-real-token"].concat();
        let raw = format!("/tmp/{marker}/session.asif.zip");
        let path = validate_local_package_path(&raw).unwrap();
        let error = validate_save_target(&path).unwrap_err();
        let combined = format!("{} {}", error.message, error.hint.unwrap_or_default());
        assert!(!combined.contains(&marker), "{combined}");
        assert!(!combined.contains("/tmp/"), "{combined}");

        let redacted = safe_text(&format!("token={marker}"));
        assert!(!redacted.contains(&marker), "{redacted}");
    }

    #[test]
    fn local_paths_are_validated_before_any_io() {
        for raw in [
            "",
            "relative/a.asif.zip",
            "/tmp/package",
            "/tmp/package.asif",
        ] {
            assert!(validate_local_package_path(raw).is_err(), "{raw}");
        }
        assert!(validate_local_package_path("/tmp/session.asif.zip").is_ok());

        let dir = temp_dir("path");
        let package = dir.join("session.asif.zip");
        assert!(
            validate_save_target(&package).is_ok(),
            "new file is allowed"
        );
        assert!(validate_import_file(&package).is_err(), "missing file");

        std::fs::write(&package, b"PK\x03\x04preview").unwrap();
        assert_eq!(
            validate_import_file(&package).unwrap(),
            u64::try_from(b"PK\x03\x04preview".len()).unwrap()
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let link = dir.join("link.asif.zip");
            symlink(&package, &link).unwrap();
            assert!(validate_import_file(&link).is_err(), "symlink refused");
            assert!(validate_save_target(&link).is_err(), "save symlink refused");
        }

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn export_dto_carries_losses_but_no_remote_or_local_path() {
        let summary = parse_asif_export_summary(EXPORT_FIXTURE.as_bytes())
            .unwrap()
            .value;
        let export = AsifExport {
            summary,
            package: AsifPackage::new(vec![b'P'; 8]),
        };
        let result =
            AsifExportResult::from_export(&export, Path::new("/tmp/local-session.asif.zip"))
                .unwrap();
        assert_eq!(result.file_name, "local-session.asif.zip");
        assert_eq!(result.loss_count, 2);
        assert_eq!(result.losses.len(), 2);
        assert!(result.unsupported.contains(&"vowp.jobs".to_owned()));

        let serialized = serde_json::to_string(&result).unwrap();
        assert!(!serialized.contains("/tmp"), "{serialized}");
        assert!(!serialized.contains("/srv"), "{serialized}");
        assert!(!serialized.contains("path"), "{serialized}");
        assert!(!serialized.contains("package"), "{serialized}");
    }

    #[test]
    fn import_dto_is_idle_and_hides_the_remote_cwd() {
        let response = parse_asif_import_response(IMPORT_FIXTURE.as_bytes())
            .unwrap()
            .value;
        let result = AsifImportResult::from_response(&response).unwrap();
        assert_eq!(result.state, "idle");
        assert_eq!(result.loss_count, 3);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.unsupported.is_empty());

        let serialized = serde_json::to_string(&result).unwrap();
        assert!(!serialized.contains("/srv/vowp"), "{serialized}");
        assert!(!serialized.contains("\"cwd\":"), "{serialized}");
        assert!(!serialized.contains("package"), "{serialized}");

        let mut running = response;
        running.session.state = SessionState::Running;
        let error = AsifImportResult::from_response(&running).unwrap_err();
        assert_eq!(error.code, SpiritErrorCode::VowpIncompatible);
    }

    #[test]
    fn the_build_advertises_d5_and_the_d6_actions_but_readiness_gates_d6() {
        let features = crate::state::build_gate_features();
        assert_eq!(
            features.asif_actions,
            cfg!(feature = "d5"),
            "the ASIF flag must come from the D5 build feature"
        );
        assert!(
            features.local_orchestrator_actions,
            "the D6 build advertises its actions"
        );

        let snapshot = with_build_gate_features(GateSnapshot::default());
        assert_eq!(snapshot.features, features);
        assert!(
            !snapshot.local_orchestrator_enabled(),
            "an unprobed gate must never enable a D6 action"
        );
        assert_eq!(crate::state::default_gate_snapshot().features, features);
    }
}
