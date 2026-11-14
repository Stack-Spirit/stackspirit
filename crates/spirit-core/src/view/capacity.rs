//! Deterministic projection of VOWP capacity documents into the rail view.
//!
//! Every value carries its source. The view never invents a missing sample, estimate,
//! waiting reason or provider cap.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::time::Timestamp;
use crate::wire::{
    BrakeState, CapacityReportDoc, CapacitySnapshot, CapacityTarget, HostSample, Limit,
    UsageEstimate, WaitingJob,
};

/// Resource severity used only for gauge colour. VOWP's brake and waiting data remain
/// authoritative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceState {
    /// Comfortably above the reserve.
    Ok,
    /// Close to the reserve.
    Warn,
    /// Below the reserve; VOWP may stop admitting runs.
    Critical,
    /// No sample or not enough information.
    Unknown,
}

/// One host sample in the rail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostSampleView {
    /// Sample time.
    pub at: Timestamp,
    /// Sampling interval.
    pub interval_secs: u64,
    /// Total physical memory.
    pub mem_total_bytes: u64,
    /// Available memory.
    pub mem_available_bytes: u64,
    /// Swap in use.
    pub swap_used_bytes: u64,
    /// One-minute load.
    pub load1: f64,
    /// Five-minute load.
    pub load5: f64,
    /// Fifteen-minute load.
    pub load15: f64,
    /// Logical CPUs.
    pub cpu_count: u32,
    /// Cgroup CPU quota, when present.
    pub cpu_quota_cores: Option<f64>,
    /// Free home bytes.
    pub disk_free_home_bytes: u64,
    /// Free worktree bytes, when different.
    pub disk_free_worktrees_bytes: Option<u64>,
    /// CPU pressure avg10.
    pub pressure_cpu_avg10: Option<f64>,
    /// Memory pressure avg10.
    pub pressure_mem_avg10: Option<f64>,
    /// I/O pressure avg10.
    pub pressure_io_avg10: Option<f64>,
    /// Sample source.
    pub source: String,
}

/// Running-capacity totals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunningCapacityView {
    /// Open runs.
    pub runs: u32,
    /// p90 memory across open runs.
    pub ram_p90_bytes: u64,
    /// p90 CPU cores across open runs.
    pub cpu_p90_cores: f64,
}

/// The current target plus VOWP's advice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetView {
    /// `auto` or `fixed`.
    pub mode: String,
    /// Current allowed workers.
    pub target_workers: u32,
    /// Advised workers, using the documented precedence.
    pub advised_workers: Option<u32>,
    /// Whether the owner raised the target above advice.
    pub is_override: bool,
    /// Who set the target.
    pub set_by: String,
    /// When it was set.
    pub set_at: Timestamp,
    /// Free-form reason.
    pub reason: String,
}

/// One brake reason.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrakeReasonView {
    /// Brake code.
    pub code: String,
    /// Human detail.
    pub detail: String,
}

/// The brake banner data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrakeView {
    /// Whether the brake is active.
    pub active: bool,
    /// Every reason, verbatim.
    pub reasons: Vec<BrakeReasonView>,
    /// When the brake first became active.
    pub since: Option<Timestamp>,
}

/// Disk gauge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiskView {
    /// Free home bytes.
    pub free_home_bytes: u64,
    /// Free worktree bytes, when reported separately.
    pub free_worktrees_bytes: Option<u64>,
    /// The reserve floor in bytes.
    pub floor_bytes: u64,
    /// `snapshot`, `capacity_report`, or `none`.
    pub source: String,
    /// Gauge state.
    pub state: ResourceState,
}

/// RAM gauge and per-worker estimate fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RamView {
    /// Available bytes.
    pub available_bytes: u64,
    /// Reserve bytes.
    pub reserve_bytes: u64,
    /// p90 across open runs, not a per-worker estimate.
    pub running_p90_bytes: u64,
    /// Selected per-worker p90, when one exists.
    pub per_worker_p90_bytes: Option<u64>,
    /// `measured`, `pooled`, or `default`.
    pub per_worker_source: Option<String>,
    /// Sample count behind the estimate.
    pub per_worker_samples: Option<u32>,
    /// Advisory extra slots by RAM, capped at 99.
    pub estimated_extra_slots: Option<u32>,
    /// Gauge state.
    pub state: ResourceState,
}

/// CPU gauge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CpuView {
    /// One-minute load.
    pub load1: f64,
    /// Logical cores.
    pub cpu_count: u32,
    /// `load1 / cpu_count`, clamped for display only.
    pub load_ratio: f64,
    /// Pressure avg10, when reported.
    pub pressure_avg10: Option<f64>,
    /// Gauge state.
    pub state: ResourceState,
}

/// One usage estimate row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EstimateView {
    /// Project key.
    pub project: Option<String>,
    /// Job-kind key.
    pub job_kind: Option<String>,
    /// Harness key.
    pub harness: Option<String>,
    /// Model key.
    pub model: Option<String>,
    /// Build-mode key.
    pub build_mode: Option<String>,
    /// Samples behind the estimate.
    pub samples: u32,
    /// Median peak RSS.
    pub ram_p50_bytes: u64,
    /// p90 peak RSS.
    pub ram_p90_bytes: u64,
    /// Median CPU cores.
    pub cpu_p50_cores: f64,
    /// p90 CPU cores.
    pub cpu_p90_cores: f64,
    /// Median duration in milliseconds.
    pub duration_p50_ms: u64,
    /// p90 duration in milliseconds.
    pub duration_p90_ms: u64,
    /// `measured`, `pooled`, or `default`.
    pub source: String,
}

/// One waiting job row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaitingJobView {
    /// Project.
    pub project: String,
    /// Job.
    pub job: String,
    /// Wait code.
    pub code: String,
    /// Human detail, verbatim.
    pub detail: String,
    /// Waiting since.
    pub since: Timestamp,
}

/// One provider limit row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LimitView {
    /// Provider or harness key.
    pub provider: String,
    /// Reporting harness, when known.
    pub harness: Option<String>,
    /// `quota`, `credits`, `rate`, `context`, `auth`, `unavailable`.
    pub kind: String,
    /// When it lifts; `None` means a human must resolve it.
    pub resets_at: Option<Timestamp>,
    /// Redacted detail.
    pub detail: String,
    /// First observed.
    pub since: Timestamp,
    /// Last recovery probe.
    pub last_probe_at: Option<Timestamp>,
    /// Consecutive failed probes.
    pub probe_fail_streak: u32,
    /// Temporary concurrency cap.
    pub concurrency_cap: Option<u32>,
    /// Cap expiry.
    pub cap_until: Option<Timestamp>,
}

/// The capacity rail view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapacityView {
    /// Report generation time, or the snapshot time when no report is loaded.
    pub generated_at: Timestamp,
    /// Chosen host sample time.
    pub measured_at: Option<Timestamp>,
    /// True only after a capacity report succeeded.
    pub report_loaded: bool,
    /// Chosen host sample.
    pub host: Option<HostSampleView>,
    /// Running capacity.
    pub running: RunningCapacityView,
    /// Target and advice.
    pub target: TargetView,
    /// Advised workers using the documented precedence.
    pub advised_workers: Option<u32>,
    /// Brake state.
    pub brake: BrakeView,
    /// RAM reserve.
    pub reserve_ram_bytes: u64,
    /// Disk floor.
    pub min_disk_bytes: u64,
    /// Disk gauge.
    pub disk: DiskView,
    /// RAM gauge.
    pub ram: RamView,
    /// CPU gauge.
    pub cpu: CpuView,
    /// Usage estimates; empty when the report is not loaded.
    pub estimates: Vec<EstimateView>,
    /// Waiting jobs; empty when the report is not loaded.
    pub waiting: Vec<WaitingJobView>,
    /// Provider limits.
    pub limits: Vec<LimitView>,
    /// Effective provider caps (report only).
    pub provider_caps: BTreeMap<String, u32>,
    /// Whether a config reload is pending.
    pub reload_pending: bool,
}

/// Query used to select a per-worker estimate for a visible job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EstimateQuery<'a> {
    /// Visible job's project.
    pub project: Option<&'a str>,
    /// Visible job's kind.
    pub job_kind: Option<&'a str>,
    /// Visible job's harness.
    pub harness: Option<&'a str>,
    /// Visible job's model.
    pub model: Option<&'a str>,
    /// Visible job's build mode.
    pub build_mode: Option<&'a str>,
}

impl CapacityView {
    /// Derives the rail from the snapshot and an optional capacity report.
    ///
    /// When the report has a host sample it wins over the snapshot sample and
    /// `disk.source` says `capacity_report`; values are never averaged.
    #[must_use]
    pub fn derive(
        snapshot: &CapacitySnapshot,
        snapshot_limits: &[Limit],
        report: Option<&CapacityReportDoc>,
        snapshot_generated_at: Timestamp,
    ) -> Self {
        let report_doc = report.map(|doc| &doc.report);
        let chosen_host = report_doc
            .and_then(|doc| doc.host.as_ref())
            .or(snapshot.measured.as_ref());
        let host_source = if report_doc.and_then(|doc| doc.host.as_ref()).is_some() {
            "capacity_report"
        } else if snapshot.measured.is_some() {
            "snapshot"
        } else {
            "none"
        };
        let generated_at = report_doc.map_or(snapshot_generated_at, |doc| doc.generated_at);
        let running = report_doc.map_or(&snapshot.running, |doc| &doc.running);
        let target = report_doc.map_or(&snapshot.target, |doc| &doc.target);
        let brake = report_doc.map_or(&snapshot.brake, |doc| &doc.brakes);
        let reserve_ram_bytes =
            report_doc.map_or(snapshot.reserve_ram_bytes, |doc| doc.reserve_ram_bytes);
        let min_disk_bytes = report_doc.map_or(snapshot.min_disk_bytes, |doc| doc.min_disk_bytes);
        let advised_workers = target.advised_workers.or(snapshot.advised_target);
        let host = chosen_host.map(host_view);
        let disk_free_home = chosen_host.map_or(0, |sample| sample.disk_free_home_bytes);
        let disk = DiskView {
            free_home_bytes: disk_free_home,
            free_worktrees_bytes: chosen_host.and_then(|sample| sample.disk_free_worktrees_bytes),
            floor_bytes: min_disk_bytes,
            source: host_source.to_owned(),
            state: disk_resource_state(chosen_host.is_some(), disk_free_home, min_disk_bytes),
        };
        let mem_available = chosen_host.map_or(0, |sample| sample.mem_available_bytes);
        let (load1, cpu_count, pressure) = chosen_host.map_or((0.0, 0, None), |sample| {
            (sample.load1, sample.cpu_count, sample.pressure_cpu_avg10)
        });
        let load_ratio = if cpu_count == 0 {
            0.0
        } else {
            load1 / f64::from(cpu_count)
        };
        let ram = RamView {
            available_bytes: mem_available,
            reserve_bytes: reserve_ram_bytes,
            running_p90_bytes: running.ram_p90_bytes,
            per_worker_p90_bytes: None,
            per_worker_source: None,
            per_worker_samples: None,
            estimated_extra_slots: None,
            state: ram_resource_state(chosen_host.is_some(), mem_available, reserve_ram_bytes),
        };
        let cpu = CpuView {
            load1,
            cpu_count,
            load_ratio,
            pressure_avg10: pressure,
            state: cpu_resource_state(chosen_host.is_some(), load1, cpu_count, pressure),
        };
        let estimates = report_doc.map_or_else(Vec::new, |doc| {
            doc.estimates.iter().map(estimate_view).collect()
        });
        let waiting = report_doc.map_or_else(Vec::new, |doc| {
            doc.waiting.iter().map(waiting_view).collect()
        });
        let limits = if report_doc.is_some_and(|doc| !doc.limits.is_empty()) {
            report_doc
                .map(|doc| doc.limits.iter().map(limit_view).collect())
                .unwrap_or_default()
        } else {
            snapshot_limits.iter().map(limit_view).collect()
        };
        let provider_caps = report.map_or_else(BTreeMap::new, |doc| doc.provider_caps.clone());
        let reload_pending = report.is_some_and(|doc| doc.reload_pending);
        let measured_at = chosen_host.map(|sample| sample.at);
        Self {
            generated_at,
            measured_at,
            report_loaded: report.is_some(),
            host,
            running: RunningCapacityView {
                runs: running.runs,
                ram_p90_bytes: running.ram_p90_bytes,
                cpu_p90_cores: running.cpu_p90_cores,
            },
            target: target_view(target, advised_workers),
            advised_workers,
            brake: brake_view(brake),
            reserve_ram_bytes,
            min_disk_bytes,
            disk,
            ram,
            cpu,
            estimates,
            waiting,
            limits,
            provider_caps,
            reload_pending,
        }
    }

    /// Returns the RAM gauge with a per-worker estimate selected for `query`.
    #[must_use]
    pub fn ram_with_estimate(&self, query: &EstimateQuery<'_>) -> RamView {
        let selected = select_estimate(&self.estimates, query);
        let mut ram = self.ram.clone();
        if let Some(estimate) = selected {
            ram.per_worker_p90_bytes = Some(estimate.ram_p90_bytes);
            ram.per_worker_source = Some(estimate.source.clone());
            ram.per_worker_samples = Some(estimate.samples);
            let headroom = ram.available_bytes.saturating_sub(ram.reserve_bytes);
            if let Some(slots) = headroom.checked_div(estimate.ram_p90_bytes) {
                ram.estimated_extra_slots = Some(u32::try_from(slots.min(99)).unwrap_or(99));
            }
        }
        ram
    }
}

/// Selects a per-worker estimate using the documented precedence:
/// exact → pooled (kind + harness) → default (kind + harness) → largest measured.
#[must_use]
pub fn select_estimate<'a>(
    estimates: &'a [EstimateView],
    query: &EstimateQuery<'_>,
) -> Option<&'a EstimateView> {
    let exact = query.project.and_then(|_| {
        estimates.iter().find(|estimate| {
            estimate.project.as_deref() == query.project
                && estimate.job_kind.as_deref() == query.job_kind
                && estimate.harness.as_deref() == query.harness
                && estimate.model.as_deref() == query.model
                && estimate.build_mode.as_deref() == query.build_mode
        })
    });
    if exact.is_some() {
        return exact;
    }
    let pooled = estimates.iter().find(|estimate| {
        estimate.source == "pooled"
            && estimate.project.is_none()
            && estimate.model.is_none()
            && estimate.build_mode.is_none()
            && estimate.job_kind.as_deref() == query.job_kind
            && estimate.harness.as_deref() == query.harness
    });
    if pooled.is_some() {
        return pooled;
    }
    let default = estimates.iter().find(|estimate| {
        estimate.source == "default"
            && estimate.job_kind.as_deref() == query.job_kind
            && estimate.harness.as_deref() == query.harness
    });
    if default.is_some() {
        return default;
    }
    estimates
        .iter()
        .filter(|estimate| estimate.samples > 0)
        .max_by_key(|estimate| estimate.ram_p90_bytes)
}

/// Disk threshold policy from `docs/PLAN.md` §5.4.
#[must_use]
pub fn disk_resource_state(sample_present: bool, free: u64, floor: u64) -> ResourceState {
    if !sample_present {
        return ResourceState::Unknown;
    }
    let free_f = free as f64;
    let floor_f = floor as f64;
    if free_f < floor_f {
        ResourceState::Critical
    } else if free_f < floor_f * 1.25 {
        ResourceState::Warn
    } else {
        ResourceState::Ok
    }
}

/// RAM threshold policy from `docs/PLAN.md` §5.4.
#[must_use]
pub fn ram_resource_state(sample_present: bool, available: u64, reserve: u64) -> ResourceState {
    if !sample_present {
        return ResourceState::Unknown;
    }
    let available_f = available as f64;
    let reserve_f = reserve as f64;
    if available_f < reserve_f {
        ResourceState::Critical
    } else if available_f < reserve_f * 2.0 {
        ResourceState::Warn
    } else {
        ResourceState::Ok
    }
}

/// CPU threshold policy from `docs/PLAN.md` §5.4. The worse of load and pressure wins.
#[must_use]
pub fn cpu_resource_state(
    sample_present: bool,
    load1: f64,
    cpu_count: u32,
    pressure: Option<f64>,
) -> ResourceState {
    if !sample_present || cpu_count == 0 {
        return ResourceState::Unknown;
    }
    let ratio = load1 / f64::from(cpu_count);
    let load_state = if ratio < 0.70 {
        ResourceState::Ok
    } else if ratio <= 1.00 {
        ResourceState::Warn
    } else {
        ResourceState::Critical
    };
    let pressure_state = pressure.map_or(ResourceState::Ok, |value| {
        if value < 10.0 {
            ResourceState::Ok
        } else if value <= 25.0 {
            ResourceState::Warn
        } else {
            ResourceState::Critical
        }
    });
    worse(load_state, pressure_state)
}

const fn worse(left: ResourceState, right: ResourceState) -> ResourceState {
    use ResourceState::{Critical, Ok, Unknown, Warn};
    match (left, right) {
        (Unknown, _) | (_, Unknown) => Unknown,
        (Critical, _) | (_, Critical) => Critical,
        (Warn, _) | (_, Warn) => Warn,
        (Ok, Ok) => Ok,
    }
}

fn host_view(sample: &HostSample) -> HostSampleView {
    HostSampleView {
        at: sample.at,
        interval_secs: sample.interval_secs,
        mem_total_bytes: sample.mem_total_bytes,
        mem_available_bytes: sample.mem_available_bytes,
        swap_used_bytes: sample.swap_used_bytes,
        load1: sample.load1,
        load5: sample.load5,
        load15: sample.load15,
        cpu_count: sample.cpu_count,
        cpu_quota_cores: sample.cpu_quota_cores,
        disk_free_home_bytes: sample.disk_free_home_bytes,
        disk_free_worktrees_bytes: sample.disk_free_worktrees_bytes,
        pressure_cpu_avg10: sample.pressure_cpu_avg10,
        pressure_mem_avg10: sample.pressure_mem_avg10,
        pressure_io_avg10: sample.pressure_io_avg10,
        source: sample.source.as_str().to_owned(),
    }
}

fn target_view(target: &CapacityTarget, advised_workers: Option<u32>) -> TargetView {
    TargetView {
        mode: target.mode.as_str().to_owned(),
        target_workers: target.target_workers,
        advised_workers,
        is_override: target.is_override,
        set_by: target.set_by.clone(),
        set_at: target.set_at,
        reason: target.reason.clone(),
    }
}

fn brake_view(brake: &BrakeState) -> BrakeView {
    BrakeView {
        active: brake.active,
        reasons: brake
            .reasons
            .iter()
            .map(|reason| BrakeReasonView {
                code: reason.code.as_str().to_owned(),
                detail: reason.detail.clone(),
            })
            .collect(),
        since: brake.since,
    }
}

fn estimate_view(estimate: &UsageEstimate) -> EstimateView {
    EstimateView {
        project: estimate.project.as_ref().map(ToString::to_string),
        job_kind: estimate.job_kind.as_ref().map(ToString::to_string),
        harness: estimate.harness.as_ref().map(ToString::to_string),
        model: estimate.model.clone(),
        build_mode: estimate.build_mode.as_ref().map(ToString::to_string),
        samples: estimate.samples,
        ram_p50_bytes: estimate.ram_p50_bytes,
        ram_p90_bytes: estimate.ram_p90_bytes,
        cpu_p50_cores: estimate.cpu_p50_cores,
        cpu_p90_cores: estimate.cpu_p90_cores,
        duration_p50_ms: estimate.duration_p50_ms,
        duration_p90_ms: estimate.duration_p90_ms,
        source: estimate.source.as_str().to_owned(),
    }
}

fn waiting_view(waiting: &WaitingJob) -> WaitingJobView {
    WaitingJobView {
        project: waiting.project.as_str().to_owned(),
        job: waiting.job.as_str().to_owned(),
        code: waiting.reason.code.as_str().to_owned(),
        detail: waiting.reason.detail.clone(),
        since: waiting.since,
    }
}

fn limit_view(limit: &Limit) -> LimitView {
    LimitView {
        provider: limit.provider.clone(),
        harness: limit.harness.as_ref().map(ToString::to_string),
        kind: limit.kind.as_str().to_owned(),
        resets_at: limit.resets_at,
        detail: limit.detail.clone(),
        since: limit.since,
        last_probe_at: limit.last_probe_at,
        probe_fail_streak: limit.probe_fail_streak,
        concurrency_cap: limit.concurrency_cap,
        cap_until: limit.cap_until,
    }
}
