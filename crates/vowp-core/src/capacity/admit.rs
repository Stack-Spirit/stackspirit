//! Capacity admission. C0 always admits; E2 fills the resource checks.

use vowp_protocol::{
    AdmitDecision, AdmitRequest, BrakeState, CapacityAdvice, CapacityReport, CapacityTarget, Job, RunningCapacity,
    TargetMode, Timestamp,
};
use vowp_store::Store;

use crate::CoreError;
use crate::deps::Deps;

use super::advisor::AdviceTrigger;

/// Returns whether one run may start.
///
/// C0 always admits, which keeps v0.1 dispatch unchanged. E2 fills the checks.
pub fn admit(_deps: &Deps, _store: &Store, _req: &AdmitRequest) -> Result<AdmitDecision, CoreError> {
    Ok(AdmitDecision::Admit)
}

/// Builds the admission request for `job`.
pub fn request_for(_deps: &Deps, _store: &Store, _job: &Job) -> Result<AdmitRequest, CoreError> {
    Err(CoreError::Invalid(
        "capacity requests are not implemented in this build".to_owned(),
    ))
}

/// The node-wide capacity manager.
pub struct CapacityManager {
    _private: (),
}

impl CapacityManager {
    /// Creates a manager over `deps`.
    pub fn new(_deps: Deps) -> Self {
        Self { _private: () }
    }

    /// Samples the node and stores one row.
    pub fn sample(&self, _store: &Store, _now: Timestamp) -> Result<Option<vowp_protocol::HostSample>, CoreError> {
        Ok(None)
    }

    /// Builds the `capacity show` report.
    pub fn report(&self, _store: &Store, now: Timestamp) -> Result<CapacityReport, CoreError> {
        Ok(CapacityReport {
            generated_at: now,
            host: None,
            target: CapacityTarget::default(),
            running: RunningCapacity::default(),
            brakes: BrakeState::default(),
            reserve_ram_bytes: 0,
            min_disk_bytes: 0,
            estimates: Vec::new(),
            waiting: Vec::new(),
            limits: Vec::new(),
        })
    }

    /// Returns whether one run may start.
    pub fn admit(&self, _store: &Store, _req: &AdmitRequest, _now: Timestamp) -> Result<AdmitDecision, CoreError> {
        Ok(AdmitDecision::Admit)
    }

    /// Writes one capacity target row.
    pub fn set_target(
        &self,
        _store: &Store,
        mode: TargetMode,
        target: u32,
        is_override: bool,
        by: &str,
        reason: &str,
    ) -> Result<CapacityTarget, CoreError> {
        Ok(CapacityTarget {
            mode,
            target_workers: target,
            advised_workers: None,
            is_override,
            set_by: by.to_owned(),
            set_at: Timestamp::now(),
            reason: reason.to_owned(),
        })
    }

    /// Produces one capacity advice row, when the advisor decides to.
    pub fn advise(
        &self,
        _store: &Store,
        _now: Timestamp,
        _trigger: AdviceTrigger,
    ) -> Result<Option<CapacityAdvice>, CoreError> {
        Ok(None)
    }

    /// Recomputes every pooled usage estimate.
    pub fn recompute_estimates(&self, _store: &Store, _now: Timestamp) -> Result<u32, CoreError> {
        Ok(0)
    }
}
