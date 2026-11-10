//! Per-run resource scopes. E1 fills this module; C0 returns no scope.

use std::collections::BTreeMap;
use std::path::Path;

use vowp_protocol::{BuildMode, RunId, RunUsage};
use vowp_store::Store;

use crate::CoreError;
use crate::deps::Deps;

/// One sampled scope measurement.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScopeSample {
    /// Peak resident memory in bytes.
    pub peak_rss_bytes: u64,
    /// Sampled anonymous memory at the peak.
    pub anon_peak_bytes: Option<u64>,
    /// Sampled file-backed pages at the peak.
    pub file_peak_bytes: Option<u64>,
    /// Sampled kernel memory at the peak.
    pub kernel_peak_bytes: Option<u64>,
    /// CPU time in microseconds.
    pub cpu_usec: u64,
    /// Mean CPU cores used.
    pub mean_cpu_cores: f64,
    /// Maximum CPU cores observed.
    pub max_cpu_cores: f64,
    /// Whether the kernel OOM killer ended the run.
    pub oom_killed: bool,
}

/// The resource scope of one run.
pub struct RunScope {
    _private: (),
}

impl RunScope {
    /// Starts measuring the process tree rooted at `root_pid`.
    pub fn begin(_deps: &Deps, _run: &RunId, _root_pid: u32) -> Result<RunScope, CoreError> {
        Err(CoreError::Invalid(
            "run scopes are not implemented in this build".to_owned(),
        ))
    }

    /// Reads the current measurements.
    pub fn sample(&self) -> Result<ScopeSample, CoreError> {
        Err(CoreError::Invalid(
            "run scopes are not implemented in this build".to_owned(),
        ))
    }

    /// Stops measuring and writes the run_usage row.
    pub fn finish(self, _store: &Store, _build_mode: BuildMode) -> Result<RunUsage, CoreError> {
        Err(CoreError::Invalid(
            "run scopes are not implemented in this build".to_owned(),
        ))
    }
}

/// The filesystem reads the sampler and scope need.
pub trait ProcFs: Send + Sync {
    /// Reads one pseudo-file as UTF-8.
    fn read(&self, path: &str) -> Result<String, CoreError>;
    /// Returns the direct children of `pid`.
    fn children(&self, pid: u32) -> Result<Vec<u32>, CoreError>;
    /// Returns the free bytes of the filesystem that holds `path`.
    fn disk_free(&self, path: &Path) -> Result<u64, CoreError>;
}

/// The real `/proc` and filesystem implementation.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemProcFs;

impl ProcFs for SystemProcFs {
    fn read(&self, path: &str) -> Result<String, CoreError> {
        std::fs::read_to_string(path).map_err(|source| CoreError::Io {
            path: path.to_owned(),
            source,
        })
    }

    fn children(&self, pid: u32) -> Result<Vec<u32>, CoreError> {
        let dir = format!("/proc/{pid}/task");
        let tasks = std::fs::read_dir(&dir).map_err(|source| CoreError::Io {
            path: dir.clone(),
            source,
        })?;
        let mut children = Vec::new();
        for task in tasks {
            let task = task.map_err(|source| CoreError::Io {
                path: dir.clone(),
                source,
            })?;
            let path = task.path().join("children");
            let text = std::fs::read_to_string(&path).map_err(|source| CoreError::Io {
                path: path.display().to_string(),
                source,
            })?;
            children.extend(text.split_whitespace().filter_map(|value| value.parse::<u32>().ok()));
        }
        Ok(children)
    }

    fn disk_free(&self, path: &Path) -> Result<u64, CoreError> {
        Err(CoreError::Invalid(format!(
            "disk sampling is not implemented in this build ({})",
            path.display()
        )))
    }
}

/// A fixture-backed [`ProcFs`] for tests.
#[derive(Debug, Clone, Default)]
pub struct FakeProcFs {
    /// Pseudo-file contents keyed by path.
    pub files: BTreeMap<String, String>,
    /// Process children keyed by parent pid.
    pub children: BTreeMap<u32, Vec<u32>>,
    /// Free disk bytes keyed by filesystem path.
    pub disk: BTreeMap<String, u64>,
}

impl ProcFs for FakeProcFs {
    fn read(&self, path: &str) -> Result<String, CoreError> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| CoreError::NotFound(path.to_owned()))
    }

    fn children(&self, pid: u32) -> Result<Vec<u32>, CoreError> {
        Ok(self.children.get(&pid).cloned().unwrap_or_default())
    }

    fn disk_free(&self, path: &Path) -> Result<u64, CoreError> {
        self.disk
            .get(&path.display().to_string())
            .copied()
            .ok_or_else(|| CoreError::NotFound(path.display().to_string()))
    }
}
