//! App-private `known_hosts` helpers.
//!
//! The file is mode 0600 on Unix and is written atomically. StackSpirit never uses the
//! user's global `known_hosts` and never auto-accepts a key.

use std::fs;
use std::io::Write;
use std::path::Path;

use crate::error::SpiritError;
use crate::ssh::SshTarget;
use crate::ssh::fingerprint::ScannedHostKey;

/// Returns the `known_hosts` host pattern for a target.
#[must_use]
pub fn known_hosts_pattern(target: &SshTarget) -> String {
    target.known_hosts_pattern()
}

/// Builds one `known_hosts` line for a scanned key.
#[must_use]
pub fn known_hosts_line(target: &SshTarget, key: &ScannedHostKey) -> String {
    format!(
        "{} {} {}\n",
        known_hosts_pattern(target),
        key.algorithm,
        key.public_key_base64
    )
}

/// Atomically writes the given lines to the app-private `known_hosts` with mode 0600.
pub fn write_known_hosts(path: &Path, lines: &[String]) -> Result<(), SpiritError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| SpiritError::io("creating app data dir", &err))?;
    }
    let temporary = path.with_extension("known_hosts.tmp");
    {
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|err| SpiritError::io("creating known_hosts", &err))?;
        for line in lines {
            file.write_all(line.as_bytes())
                .map_err(|err| SpiritError::io("writing known_hosts", &err))?;
        }
        file.sync_all()
            .map_err(|err| SpiritError::io("syncing known_hosts", &err))?;
    }
    fs::rename(&temporary, path).map_err(|err| SpiritError::io("renaming known_hosts", &err))?;
    Ok(())
}

/// Returns true when the file contains the exact key line.
pub fn contains_key(
    path: &Path,
    target: &SshTarget,
    key: &ScannedHostKey,
) -> Result<bool, SpiritError> {
    if !path.exists() {
        return Ok(false);
    }
    let text =
        fs::read_to_string(path).map_err(|err| SpiritError::io("reading known_hosts", &err))?;
    let expected = known_hosts_line(target, key);
    Ok(text.lines().any(|line| format!("{line}\n") == expected))
}

/// Replaces every pin for `target` with one confirmed key.
///
/// Other hosts and comments are preserved. The write goes through
/// [`write_known_hosts`], so the file is replaced atomically with mode 0600 and
/// there is never a moment where a second pin for the same target survives.
pub(crate) fn upsert_pinned_key(
    path: &Path,
    target: &SshTarget,
    key: &ScannedHostKey,
) -> Result<(), SpiritError> {
    let pattern = known_hosts_pattern(target);
    let mut lines = Vec::new();
    match fs::read_to_string(path) {
        Ok(text) => {
            for line in text.lines() {
                let candidates = line.split_whitespace().next().unwrap_or_default();
                let same_target = candidates.split(',').any(|entry| entry == pattern);
                if !same_target {
                    lines.push(format!("{line}\n"));
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(SpiritError::io("reading known_hosts", &error)),
    }
    lines.push(known_hosts_line(target, key));
    write_known_hosts(path, &lines)
}

/// Returns true when the file carries a key for the target with the pinned SHA-256.
pub fn contains_pinned_fingerprint(
    path: &Path,
    target: &SshTarget,
    sha256: &str,
    scan: &[ScannedHostKey],
) -> Result<bool, SpiritError> {
    let Some(key) = scan.iter().find(|key| key.sha256 == sha256) else {
        return Ok(false);
    };
    contains_key(path, target, key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scanned(body: &str) -> ScannedHostKey {
        ScannedHostKey {
            algorithm: "ssh-ed25519".to_owned(),
            sha256: "SHA256:test".to_owned(),
            public_key_base64: body.to_owned(),
            preferred: true,
        }
    }

    #[test]
    fn upsert_replaces_only_the_same_target_and_keeps_other_hosts() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("known_hosts");
        let target = SshTarget::new("owner", "192.0.2.10", 22).unwrap();
        let other = SshTarget::new("owner", "198.51.100.7", 22).unwrap();
        write_known_hosts(&path, &[known_hosts_line(&other, &scanned("OTHERKEY"))]).unwrap();

        upsert_pinned_key(&path, &target, &scanned("FIRSTKEY")).unwrap();
        upsert_pinned_key(&path, &target, &scanned("SECONDKEY")).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(
            text.lines()
                .filter(|line| line.starts_with("192.0.2.10 "))
                .count(),
            1,
            "a re-pin must not leave the old target line behind"
        );
        assert!(text.contains("192.0.2.10 ssh-ed25519 SECONDKEY"));
        assert!(!text.contains("FIRSTKEY"));
        assert!(text.contains("198.51.100.7 ssh-ed25519 OTHERKEY"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
