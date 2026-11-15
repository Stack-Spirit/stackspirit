//! ASIF export and import over the public VOWP CLI.
//!
//! Export is the two-step path from `VOWP-CONTRACT.md` §11.2.2: write a package to a
//! private remote temp directory, print the JSON summary, read the package with a hard
//! size cap, verify the byte count, then remove the directory. Export never uses
//! `--out -` with `--json` and never uses the hidden `--asif-stream`.
//!
//! Import streams the local package to `vowp ... session import - --format asif` over
//! SSH stdin. The imported session is idle data; this module never runs a turn, writes a
//! capsule, creates a lease or dispatches a job.
//!
//! The renderer receives only safe data: the summary (without `path`/`report_path`) and
//! the parsed report. Remote paths are scrubbed from every typed error.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{SpiritError, SpiritErrorCode};
use crate::ssh::SshRemote;
use crate::ssh::remote_file::{RemoteTempDir, scrub_paths};
use crate::vowp::client::check_command_output;
use crate::wire::{
    ASIF_ARCHIVE_CAP_BYTES, ASIF_JSON_CAP_BYTES, AsifExportSummary, AsifImportResponse, AsifReport,
    ProjectName, SessionId, WireWarning, parse_asif_export_summary, parse_asif_import_response,
};

/// The prefix every ASIF remote temp directory must carry.
const TEMP_PREFIX: &str = "stackspirit-asif";

/// A package transfer is bounded by size, not by the 30 s control-write timeout.
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(120);

/// How declared ASIF losses are treated.
///
/// The enum makes `--strict` and `--allow-loss` mutually exclusive by construction.
/// `--allow-unsupported` has no variant and is never sent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AsifLossPolicy {
    /// VOWP's default: declared omissions and normalizations succeed and are reported.
    #[default]
    Default,
    /// Refuse `Blocking`/`Integrity` losses; inherent omissions stay non-fatal.
    Strict,
    /// Let recoverable integrity/ambiguity problems become declared losses.
    AllowLoss,
}

impl AsifLossPolicy {
    /// Returns the one CLI flag this policy adds, if any.
    #[must_use]
    pub const fn flag(self) -> Option<&'static str> {
        match self {
            Self::Default => None,
            Self::Strict => Some("--strict"),
            Self::AllowLoss => Some("--allow-loss"),
        }
    }
}

/// Export options. There is deliberately no `allow_unsupported` field, and an unknown
/// option name is rejected instead of being silently ignored.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AsifExportOptions {
    /// Strict/allow-loss policy.
    #[serde(default)]
    pub loss: AsifLossPolicy,
}

impl AsifExportOptions {
    /// Builds export options from one loss policy.
    #[must_use]
    pub const fn new(loss: AsifLossPolicy) -> Self {
        Self { loss }
    }
}

/// Import options. `branch` and `cwd` are the only extra selectors VOWP documents; an
/// unknown option name (including `allow_unsupported`) is rejected.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AsifImportOptions {
    /// Select one ASIF branch when the document has more than one.
    #[serde(default)]
    pub branch: Option<String>,
    /// Remote working directory written to `session.created`.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Strict/allow-loss policy.
    #[serde(default)]
    pub loss: AsifLossPolicy,
}

/// Rejects a package length above the adapter's 272 MiB archive cap.
///
/// The desktop checks a selected local file with this before reading it, and the
/// remote helper checks the summary and `stat` size with it before the transfer.
pub fn ensure_package_size(len: u64) -> Result<(), SpiritError> {
    if len > ASIF_ARCHIVE_CAP_BYTES {
        Err(SpiritError::transfer_too_large(
            "the ASIF package",
            ASIF_ARCHIVE_CAP_BYTES,
        ))
    } else {
        Ok(())
    }
}

impl AsifImportOptions {
    /// Validates the optional selector strings before they become remote argv elements.
    pub fn validate(&self) -> Result<(), SpiritError> {
        if let Some(branch) = &self.branch {
            if branch.is_empty()
                || branch.len() > 200
                || branch.starts_with('-')
                || branch.chars().any(|c| c.is_control() || c.is_whitespace())
            {
                return Err(SpiritError::invalid_input(
                    "asif branch",
                    "must be 1-200 non-whitespace characters and must not start with '-'",
                ));
            }
        }
        if let Some(cwd) = &self.cwd {
            if cwd.len() > 4096
                || cwd == "/"
                || !cwd.starts_with('/')
                || cwd.starts_with('-')
                || cwd.chars().any(|c| c.is_control())
            {
                return Err(SpiritError::invalid_input(
                    "asif cwd",
                    "must be an absolute remote directory path of at most 4096 bytes",
                ));
            }
        }
        Ok(())
    }
}

/// An ASIF package. It has no `Serialize` implementation so package bytes cannot be
/// accidentally included in a bridge response or a view.
#[derive(Clone, PartialEq, Eq)]
pub struct AsifPackage(Vec<u8>);

impl AsifPackage {
    /// Wraps package bytes received from a capped transfer.
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Borrows the package bytes for a local save.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Returns the package length.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns true for an empty package.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for AsifPackage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("AsifPackage")
            .field(&format_args!("{} bytes", self.0.len()))
            .finish()
    }
}

/// A successful export: the safe summary plus the package for a local save.
#[derive(Clone, PartialEq)]
pub struct AsifExport {
    /// Safe summary; it has no remote `path` or `report_path`.
    pub summary: AsifExportSummary,
    /// Package bytes, bounded by the 272 MiB archive cap.
    pub package: AsifPackage,
}

impl AsifExport {
    /// Returns the suggested local file name. It is derived from the session id.
    #[must_use]
    pub fn file_name(&self) -> String {
        format!("{}.asif.zip", self.summary.session)
    }

    /// Borrows the declared report.
    #[must_use]
    pub fn report(&self) -> &AsifReport {
        &self.summary.report
    }
}

impl fmt::Debug for AsifExport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsifExport")
            .field("summary", &self.summary)
            .field("package_bytes", &self.package.len())
            .finish()
    }
}

/// Exports one session to a private remote temp file and reads it back.
pub(crate) async fn export_asif(
    remote: &SshRemote,
    home: Option<&str>,
    project: &str,
    session: &str,
    options: AsifExportOptions,
) -> Result<(AsifExport, Vec<WireWarning>), SpiritError> {
    let project = ProjectName::parse(project)?;
    let session = SessionId::parse(session)?;
    let dir = RemoteTempDir::create(remote.clone(), TEMP_PREFIX).await?;
    let temp_path = dir.path().to_owned();
    let package_path = match dir.child(&format!("{session}.asif.zip")) {
        Ok(path) => path,
        Err(error) => {
            let _ = dir.remove().await;
            return Err(error);
        }
    };

    let result = export_with_dir(
        remote,
        home,
        &project,
        &session,
        &options,
        &dir,
        &package_path,
    )
    .await;
    let cleanup = dir.remove().await;
    let result = match (result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    };
    result.map_err(|error| scrub_paths(error, &[package_path.as_str(), temp_path.as_str()]))
}

async fn export_with_dir(
    remote: &SshRemote,
    home: Option<&str>,
    project: &ProjectName,
    session: &SessionId,
    options: &AsifExportOptions,
    dir: &RemoteTempDir,
    package_path: &str,
) -> Result<(AsifExport, Vec<WireWarning>), SpiritError> {
    let mut argv = base_argv(home);
    argv.extend([
        "--project".to_owned(),
        project.as_str().to_owned(),
        "--json".to_owned(),
        "session".to_owned(),
        "export".to_owned(),
        session.as_str().to_owned(),
        "--format".to_owned(),
        "asif".to_owned(),
        "--out".to_owned(),
        package_path.to_owned(),
    ]);
    if let Some(flag) = options.loss.flag() {
        argv.push(flag.to_owned());
    }

    let output = remote
        .run_with_cap(&argv, TRANSFER_TIMEOUT, ASIF_JSON_CAP_BYTES)
        .await?;
    let output = check_command_output(output)?;
    let parsed = parse_asif_export_summary(&output.stdout)?;
    if !parsed.value.is_for_session(session) {
        return Err(SpiritError::vowp_incompatible(
            "asif export summary",
            "the summary names a different session",
        ));
    }
    ensure_package_size(parsed.value.bytes)?;

    let bytes = dir
        .read_file(package_path, ASIF_ARCHIVE_CAP_BYTES, TRANSFER_TIMEOUT)
        .await?;
    let actual = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if actual != parsed.value.bytes {
        return Err(SpiritError::new(
            SpiritErrorCode::Remote,
            "the exported ASIF package byte count does not match the summary",
        )
        .retryable()
        .with_hint("Retry the export; StackSpirit never keeps a mismatched package."));
    }

    Ok((
        AsifExport {
            summary: parsed.value,
            package: AsifPackage::new(bytes),
        },
        parsed.warnings,
    ))
}

/// Imports one local ASIF package by streaming it to VOWP's stdin.
pub(crate) async fn import_asif(
    remote: &SshRemote,
    home: Option<&str>,
    project: &str,
    package: &[u8],
    options: AsifImportOptions,
) -> Result<(AsifImportResponse, Vec<WireWarning>), SpiritError> {
    let scrub_cwd = options.cwd.clone().unwrap_or_default();
    let result = import_with_options(remote, home, project, package, &options).await;
    result.map_err(|error| {
        if scrub_cwd.is_empty() {
            error
        } else {
            scrub_paths(error, &[scrub_cwd.as_str()])
        }
    })
}

async fn import_with_options(
    remote: &SshRemote,
    home: Option<&str>,
    project: &str,
    package: &[u8],
    options: &AsifImportOptions,
) -> Result<(AsifImportResponse, Vec<WireWarning>), SpiritError> {
    let project = ProjectName::parse(project)?;
    options.validate()?;
    let package_len = u64::try_from(package.len()).unwrap_or(u64::MAX);
    ensure_package_size(package_len)?;

    let mut argv = base_argv(home);
    argv.extend([
        "--project".to_owned(),
        project.as_str().to_owned(),
        "--json".to_owned(),
        "session".to_owned(),
        "import".to_owned(),
        "-".to_owned(),
        "--format".to_owned(),
        "asif".to_owned(),
    ]);
    if let Some(branch) = &options.branch {
        argv.extend(["--branch".to_owned(), branch.clone()]);
    }
    if let Some(cwd) = &options.cwd {
        argv.extend(["--cwd".to_owned(), cwd.clone()]);
    }
    if let Some(flag) = options.loss.flag() {
        argv.push(flag.to_owned());
    }

    let output = remote
        .run_with_stdin(
            &argv,
            package.to_vec(),
            TRANSFER_TIMEOUT,
            ASIF_JSON_CAP_BYTES,
        )
        .await?;
    let output = check_command_output(output)?;
    let parsed = parse_asif_import_response(&output.stdout)?;
    parsed.value.ensure_idle()?;
    Ok((parsed.value, parsed.warnings))
}

fn base_argv(home: Option<&str>) -> Vec<String> {
    let mut argv = vec!["vowp".to_owned()];
    if let Some(home) = home {
        argv.extend(["--home".to_owned(), home.to_owned()]);
    }
    argv
}
