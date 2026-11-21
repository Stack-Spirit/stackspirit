//! Pure projection of VOWP documents into view models.
//!
//! Nothing in this module performs I/O or invents missing data.

pub mod capacity;
pub mod executor;
pub mod fleet;
pub mod format;
pub mod lanes;
pub mod local;

pub use capacity::{
    BrakeReasonView, BrakeView, CapacityView, CpuView, DiskView, EstimateQuery, EstimateView,
    HostSampleView, LimitView, RamView, ResourceState, RunningCapacityView, TargetView,
    WaitingJobView, cpu_resource_state, disk_resource_state, ram_resource_state, select_estimate,
};
pub use executor::{ExecutorView, friendly_model_name};
pub use fleet::{
    FleetView, InboxItemView, JoinView, OrchestratorView, ProjectView, SessionCardView,
    SessionOwnerView, SupervisorView, SurfaceRefView, WorkerCardView,
};
pub use format::{
    format_age_secs, format_bytes_gib, format_clock, format_percent, short_id, truncate_chars,
};
pub use lanes::{JobCardView, JobLanes, QUEUE_CAP, build_lanes};
pub use local::{
    AgentPathView, AgentSurfaceView, LocalAgentView, LocalOrchestratorView, LocalSessionState,
    LocalSurfaceView,
};
