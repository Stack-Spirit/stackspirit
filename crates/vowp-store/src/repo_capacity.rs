//! Capacity repository (filled by E1/E2/E3).
//!
//! The tables exist from day one (`host_samples`, `run_usage`, `usage_estimates`,
//! `capacity_targets`, `capacity_advice`). The frozen signatures from
//! `docs/design/v0.2/home-and-surfaces.md` §13.3 are:
//!
//! ```text
//! pub fn insert_host_sample(&self, s: &HostSample) -> Result<(), StoreError>;
//! pub fn host_samples_since(&self, since: Timestamp) -> Result<Vec<HostSample>, StoreError>;
//! pub fn prune_host_samples(&self, before: Timestamp, minute_before: Timestamp) -> Result<u64, StoreError>;
//! pub fn upsert_run_usage(&self, u: &RunUsage) -> Result<(), StoreError>;
//! pub fn run_usage(&self, run: &RunId) -> Result<Option<RunUsage>, StoreError>;
//! pub fn run_usage_for_group(&self, q: &UsageQuery, limit: u32) -> Result<Vec<RunUsage>, StoreError>;
//! pub fn upsert_usage_estimate(&self, e: &UsageEstimate) -> Result<(), StoreError>;
//! pub fn usage_estimate(&self, k: &EstimateKey) -> Result<Option<UsageEstimate>, StoreError>;
//! pub fn list_usage_estimates(&self, project: Option<&ProjectName>) -> Result<Vec<UsageEstimate>, StoreError>;
//! pub fn insert_capacity_target(&self, t: &CapacityTarget) -> Result<i64, StoreError>;
//! pub fn latest_capacity_target(&self) -> Result<Option<CapacityTarget>, StoreError>;
//! pub fn insert_capacity_advice(&self, a: &CapacityAdvice) -> Result<(), StoreError>;
//! pub fn capacity_advice(&self, id: &str) -> Result<Option<CapacityAdvice>, StoreError>;
//! pub fn latest_open_advice(&self) -> Result<Option<CapacityAdvice>, StoreError>;
//! pub fn finish_capacity_advice(&self, id: &str, state: AdviceState, by: &str, now: Timestamp)
//!     -> Result<(), StoreError>;
//! ```
//!
//! The referenced wire types live in `vowp-protocol/src/capacity.rs`, which the C0 skeleton
//! job creates in the same wave. This file stays empty until those types land so the C1
//! branch compiles and passes its gates without depending on an unmerged sibling job.
