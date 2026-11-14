//! External harness detection. Only exact binary names on `PATH` are considered; the
//! scanner never walks the disk and never edits another tool's configuration.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }

    fn version(&self, _path: &Path) -> Option<String> {
        // Version probing needs to spawn the binary. The D1 core keeps this behind the
        // injected trait; D4 supplies a process-backed implementation.
        None
    }
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
