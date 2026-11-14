//! External harness detection. Only exact binary names on `PATH` are considered; the
//! scanner never walks the disk and never edits another tool's configuration.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::engine::ExternalHarnessKind;

/// One detected external harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalHarness {
    /// Which harness it is.
    pub kind: ExternalHarnessKind,
    /// Absolute path.
    pub path: PathBuf,
    /// Version, when the probe could read one.
    pub version: Option<String>,
    /// True when the path exists and is executable.
    pub usable: bool,
}

/// Injection point for names and version probes.
pub trait HarnessProbe: Send + Sync {
    /// Finds an exact binary name on the configured search path.
    fn locate(&self, name: &str) -> Option<PathBuf>;
    /// Reads an executable's version without editing its config.
    fn version(&self, path: &Path) -> Option<String>;
}

/// A `PATH` scanner that resolves exact names in the configured directories only.
#[derive(Debug, Clone, Default)]
pub struct PathHarnessProbe {
    directories: Vec<PathBuf>,
}

impl PathHarnessProbe {
    /// Builds a probe from explicit directories.
    #[must_use]
    pub fn new(directories: Vec<PathBuf>) -> Self {
        Self { directories }
    }

    /// Builds a probe from the process `PATH`.
    #[must_use]
    pub fn from_env() -> Self {
        let directories = std::env::var_os("PATH")
            .map(|path| std::env::split_paths(&path).collect())
            .unwrap_or_default();
        Self::new(directories)
    }

    /// Returns the configured directories.
    #[must_use]
    pub fn directories(&self) -> &[PathBuf] {
        &self.directories
    }
}

impl HarnessProbe for PathHarnessProbe {
    fn locate(&self, name: &str) -> Option<PathBuf> {
        for directory in &self.directories {
            if !directory.is_absolute() {
                continue;
            }
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }

    fn version(&self, path: &Path) -> Option<String> {
        probe_version(path)
    }
}

/// Runs `<path> --version` with a bounded wait. Never edits the harness configuration.
fn probe_version(path: &Path) -> Option<String> {
    if !is_executable(path) {
        return None;
    }
    let mut child = Command::new(path)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let output = child.wait_with_output().ok()?;
                return first_safe_line(&output.stdout, &output.stderr);
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

fn first_safe_line(stdout: &[u8], stderr: &[u8]) -> Option<String> {
    let text = if stdout.is_empty() { stderr } else { stdout };
    let lossy = String::from_utf8_lossy(text);
    let line = lossy.lines().map(str::trim).find(|line| !line.is_empty())?;
    Some(
        line.chars()
            .filter(|character| !character.is_control())
            .take(120)
            .collect(),
    )
}

/// Scans the exact names `opencode`, `claude` and `claude-code`, de-duplicating paths.
#[must_use]
pub fn scan_external_harnesses(probe: &dyn HarnessProbe) -> Vec<ExternalHarness> {
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for (name, kind) in [
        ("opencode", ExternalHarnessKind::Opencode),
        ("claude", ExternalHarnessKind::ClaudeCode),
        ("claude-code", ExternalHarnessKind::ClaudeCode),
    ] {
        let Some(path) = probe.locate(name) else {
            continue;
        };
        if !seen.insert(path.clone()) {
            continue;
        }
        let version = probe.version(&path);
        found.push(ExternalHarness {
            kind,
            usable: version.is_some() || is_executable(&path),
            path,
            version,
        });
    }
    found.sort_by(|left, right| {
        left.kind
            .as_str()
            .cmp(right.kind.as_str())
            .then_with(|| left.path.cmp(&right.path))
    });
    found
}

/// Returns true when `path` is a regular file with an executable bit set.
fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn engine_external_version_probe_reads_the_binary_without_editing_it() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("claude");
        std::fs::write(&path, b"#!/bin/sh\necho 'claude 2.5.1'\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let before = std::fs::read(&path).unwrap();
        let probe = PathHarnessProbe::new(vec![directory.path().to_path_buf()]);
        assert_eq!(probe.version(&path).as_deref(), Some("claude 2.5.1"));
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn engine_external_scan_skips_relative_path_entries() {
        let probe = PathHarnessProbe::new(vec![PathBuf::from("."), PathBuf::from("bin")]);
        assert_eq!(probe.locate("opencode"), None);
    }

    #[test]
    fn engine_external_scan_matches_exact_names_only() {
        use std::collections::HashMap;

        struct Probe {
            paths: HashMap<String, PathBuf>,
        }
        impl HarnessProbe for Probe {
            fn locate(&self, name: &str) -> Option<PathBuf> {
                self.paths.get(name).cloned()
            }
            fn version(&self, _path: &Path) -> Option<String> {
                Some("1.19.5".to_owned())
            }
        }
        let probe = Probe {
            paths: HashMap::from([
                (
                    "opencode".to_owned(),
                    PathBuf::from("/opt/example/bin/opencode"),
                ),
                (
                    "opencode-extra".to_owned(),
                    PathBuf::from("/opt/example/bin/opencode-extra"),
                ),
                (
                    "claude".to_owned(),
                    PathBuf::from("/opt/example/bin/claude"),
                ),
                (
                    "claude-code".to_owned(),
                    PathBuf::from("/opt/example/bin/claude"),
                ),
            ]),
        };
        let found = scan_external_harnesses(&probe);
        assert_eq!(found.len(), 2);
        assert!(
            found
                .iter()
                .any(|item| item.kind == ExternalHarnessKind::Opencode)
        );
        assert!(
            found
                .iter()
                .all(|item| !item.path.to_string_lossy().contains("opencode-extra"))
        );
    }
}
