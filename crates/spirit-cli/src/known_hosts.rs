//! App-private `known_hosts` installation for `host trust`.
//!
//! The helper replaces only the target's lines and preserves every other host. The
//! core writer uses an atomic rename with mode 0600.

use std::path::Path;

use spirit_core::error::SpiritError;
use spirit_core::host::HostKeyPin;
use spirit_core::ssh::known_hosts::{known_hosts_line, write_known_hosts};
use spirit_core::ssh::{ScannedHostKey, SshTarget};

use crate::error::CliError;

/// Installs the scanned key that matches `pin`.
///
/// A pin that does not match any scanned key is a hard conflict; the function never
/// auto-accepts the scan.
pub(crate) fn install(
    path: &Path,
    target: &SshTarget,
    scanned: &[ScannedHostKey],
    pin: &HostKeyPin,
) -> Result<(), CliError> {
    let key = scanned
        .iter()
        .find(|key| key.sha256 == pin.sha256)
        .ok_or_else(|| {
            CliError::new(SpiritError::host_key_mismatch(
                &pin.sha256,
                scanned.first().map_or("no keys", |key| key.sha256.as_str()),
            ))
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
    write_known_hosts(path, &lines)?;
    Ok(())
}
