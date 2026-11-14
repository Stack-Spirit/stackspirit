//! Capacity documents: host samples, targets, brakes, estimates and waiting jobs.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::time::Timestamp;
use crate::wire::enums::{
    BrakeCode, BuildMode, EstimateSource, HarnessKind, JobKind, SampleSource, TargetMode, WaitCode,
};
use crate::wire::ids::{JobId, ProjectName};

/// One measurement of the whole node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostSample {
    /// When the sample was taken.
    pub at: Timestamp,
    /// Sampling interval that produced this row.
    pub interval_secs: u64,
    /// Total physical memory in bytes.
    pub mem_total_bytes: u64,
    /// Memory available to new work in bytes.
    pub mem_available_bytes: u64,
    /// Swap in use in bytes.
    pub swap_used_bytes: u64,
    /// One-minute load average.
    pub load1: f64,
    /// Five-minute load average.
    pub load5: f64,
    /// Fifteen-minute load average.
    pub load15: f64,
    /// Number of logical CPUs.
    pub cpu_count: u32,
    /// Cgroup v2 CPU quota in cores, when present.
    #[serde(default)]
    pub cpu_quota_cores: Option<f64>,
    /// Free bytes on the filesystem that holds the home.
    pub disk_free_home_bytes: u64,
    /// Free bytes on the filesystem that holds worktrees, when different.
    #[serde(default)]
    pub disk_free_worktrees_bytes: Option<u64>,
    /// `/proc/pressure/cpu` `avg10`, when present.
    #[serde(default)]
    pub pressure_cpu_avg10: Option<f64>,
    /// `/proc/pressure/memory` `avg10`, when present.
    #[serde(default)]
    pub pressure_mem_avg10: Option<f64>,
    /// `/proc/pressure/io` `avg10`, when present.
    #[serde(default)]
    pub pressure_io_avg10: Option<f64>,
    /// Read path that produced the sample.
    pub source: SampleSource,
}

/// Current running-capacity totals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunningCapacity {
    /// Number of open runs.
    pub runs: u32,
    /// p90 memory of the open runs in bytes.
    pub ram_p90_bytes: u64,
    /// p90 CPU cores of the open runs.
    pub cpu_p90_cores: f64,
}

impl Default for RunningCapacity {
    fn default() -> Self {
        Self {
            runs: 0,
            ram_p90_bytes: 0,
            cpu_p90_cores: 0.0,
        }
    }
}

/// The current node-wide worker target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapacityTarget {
    /// How the target is chosen.
    pub mode: TargetMode,
    /// Number of workers that may run.
    pub target_workers: u32,
    /// The supervisor's advised number, when known.
    #[serde(default)]
    pub advised_workers: Option<u32>,
    /// Whether the owner raised the target above the advice.
    pub is_override: bool,
    /// Who set the target: `owner`, `supervisor` or `setup`.
    pub set_by: String,
    /// When the target was set.
    pub set_at: Timestamp,
    /// Free-form reason.
    pub reason: String,
}

impl Default for CapacityTarget {
    fn default() -> Self {
        Self {
            mode: TargetMode::Auto,
            target_workers: 1,
            advised_workers: None,
            is_override: false,
            set_by: "supervisor".to_owned(),
            set_at: Timestamp::epoch(),
            reason: String::new(),
        }
    }
}

/// Why the node brake tripped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrakeReason {
    /// Machine-readable category.
    pub code: BrakeCode,
    /// One human sentence.
    pub detail: String,
}

/// The node's active brake state.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BrakeState {
    /// Whether the brake is active.
    pub active: bool,
    /// Reasons the brake is active.
    pub reasons: Vec<BrakeReason>,
    /// When the brake first became active.
    pub since: Option<Timestamp>,
}

/// Pooled resource estimate for one usage key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageEstimate {
    /// Project key; `None` is the pooled empty key.
    pub project: Option<ProjectName>,
    /// Job-kind key; `None` is the pooled empty key.
    pub job_kind: Option<JobKind>,
    /// Harness key; `None` is the pooled empty key.
    pub harness: Option<HarnessKind>,
    /// Model key; `None` is the pooled empty key.
    pub model: Option<String>,
    /// Build-mode key; `None` is the pooled `unknown` key.
    pub build_mode: Option<BuildMode>,
    /// Number of samples the estimate used.
    pub samples: u32,
    /// Median peak resident memory in bytes.
    pub ram_p50_bytes: u64,
    /// p90 peak resident memory in bytes.
    pub ram_p90_bytes: u64,
    /// Median CPU cores.
    pub cpu_p50_cores: f64,
    /// p90 CPU cores.
    pub cpu_p90_cores: f64,
    /// Median duration in milliseconds.
    pub duration_p50_ms: u64,
    /// p90 duration in milliseconds.
    pub duration_p90_ms: u64,
    /// Estimate source.
    pub source: EstimateSource,
}

/// One human-readable reason a run is waiting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaitReason {
    /// Machine-readable category.
    pub code: WaitCode,
    /// One human sentence.
    pub detail: String,
}

/// One job waiting for capacity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaitingJob {
    /// Project the job belongs to.
    pub project: ProjectName,
    /// Waiting job.
    pub job: JobId,
    /// Why it is waiting.
    pub reason: WaitReason,
    /// When it started waiting.
    pub since: Timestamp,
}

/// The capacity card embedded in `snapshot.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CapacitySnapshot {
    /// Latest host sample, when the sampler has run.
    pub measured: Option<HostSample>,
    /// Running-capacity totals.
    pub running: RunningCapacity,
    /// Current worker target.
    pub target: CapacityTarget,
    /// The supervisor's advised target, when known.
    #[serde(default)]
    pub advised_target: Option<u32>,
    /// Brake state (singular field name in the snapshot).
    pub brake: BrakeState,
    /// RAM kept free for the node in bytes.
    pub reserve_ram_bytes: u64,
    /// Disk kept free for the node in bytes.
    pub min_disk_bytes: u64,
}

/// Output of `vowp capacity report`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapacityReport {
    /// When the report was generated.
    pub generated_at: Timestamp,
    /// Latest host sample, when one exists.
    pub host: Option<HostSample>,
    /// Current target.
    pub target: CapacityTarget,
    /// Running-capacity totals.
    pub running: RunningCapacity,
    /// Brake state (plural field name in the report).
    pub brakes: BrakeState,
    /// RAM reserve in bytes.
    pub reserve_ram_bytes: u64,
    /// Disk reserve in bytes.
    pub min_disk_bytes: u64,
    /// Usage estimates included in the report.
    pub estimates: Vec<UsageEstimate>,
    /// Jobs waiting for capacity.
    pub waiting: Vec<WaitingJob>,
    /// Provider limits.
    pub limits: Vec<crate::wire::limit::Limit>,
}

/// The `capacity report` document plus the two CLI-only extras.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapacityReportDoc {
    /// The flat VOWP capacity report.
    #[serde(flatten)]
    pub report: CapacityReport,
    /// Effective provider concurrency caps (CLI extra).
    #[serde(default)]
    pub provider_caps: BTreeMap<String, u32>,
    /// Whether a config reload is pending (CLI extra).
    #[serde(default)]
    pub reload_pending: bool,
}
