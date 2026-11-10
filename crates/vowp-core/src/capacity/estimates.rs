//! Usage estimates from measured runs. E1 fills this module.

use vowp_protocol::{AdmitRequest, EstimateSource, Timestamp, UsageEstimate};
use vowp_store::Store;

use crate::CoreError;

/// Recomputes every pooled usage estimate.
pub fn recompute(_store: &Store, _now: Timestamp) -> Result<u32, CoreError> {
    Ok(0)
}

/// Returns the estimate admission should use for `req`.
pub fn estimate_for(_store: &Store, req: &AdmitRequest) -> Result<UsageEstimate, CoreError> {
    Ok(UsageEstimate {
        project: Some(req.project.clone()),
        job_kind: Some(req.kind),
        harness: req.harness,
        model: req.model.clone(),
        build_mode: None,
        samples: 0,
        ram_p50_bytes: 0,
        ram_p90_bytes: 0,
        cpu_p50_cores: 0.0,
        cpu_p90_cores: 0.0,
        duration_p50_ms: 0,
        duration_p90_ms: 0,
        source: EstimateSource::Default,
    })
}
