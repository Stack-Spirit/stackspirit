//! Home backup and restore. H2 fills this module.

use std::path::PathBuf;

use vowp_protocol::{BackupReport, Timestamp};
use vowp_store::Store;

use crate::CoreError;
use crate::deps::Deps;

/// Options for `vowp backup` and `vowp backup --restore`.
#[derive(Debug, Clone, Default)]
pub struct BackupOptions {
    /// Destination directory or single-DB file.
    pub to: Option<String>,
    /// Explicit offsite target.
    pub offsite: Option<String>,
    /// `age` recipient, when the backup is encrypted.
    pub encrypt: Option<String>,
    /// Backup directory to restore.
    pub restore: Option<PathBuf>,
    /// Fresh home to restore into.
    pub to_home: Option<PathBuf>,
    /// Overwrite an existing restore target.
    pub force: bool,
}

/// Writes one backup.
pub fn backup(_deps: &Deps, _store: &Store, opts: &BackupOptions) -> Result<BackupReport, CoreError> {
    Ok(BackupReport {
        at: Timestamp::now(),
        directory: opts.to.clone().unwrap_or_default(),
        files: Vec::new(),
        bytes: 0,
        offsite: opts.offsite.clone(),
        encrypted: opts.encrypt.is_some(),
        skipped: Vec::new(),
    })
}

/// Restores a fresh home from a backup directory.
pub fn restore(_deps: &Deps, _store: &Store, opts: &BackupOptions) -> Result<BackupReport, CoreError> {
    Ok(BackupReport {
        at: Timestamp::now(),
        directory: opts
            .restore
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        files: Vec::new(),
        bytes: 0,
        offsite: None,
        encrypted: false,
        skipped: Vec::new(),
    })
}
