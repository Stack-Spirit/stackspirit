//! StackSpirit core: VOWP wire types, SSH transport, host storage, connection state,
//! deterministic views, the local engine boundary, and honest capability gates.
//!
//! The crate is headless: it has no Tauri, React, or platform-GUI dependency and never
//! reads `/home/user/vowp` at runtime. All I/O goes through injected traits so tests stay
//! hermetic.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod cluster;
pub mod connection;
pub mod engine;
pub mod error;
#[cfg(feature = "fixtures")]
pub mod fixtures;
pub mod host;
pub mod service;
pub mod ssh;
pub mod time;
pub mod view;
pub mod vowp;
pub mod wire;

pub use cluster::{
    BackupPolicy, CLUSTER_BACKUP_SCHEDULE, ClusterAgentState, ClusterCapacityTotals, ClusterConfig,
    ClusterFormRequest, ClusterFormed, ClusterHealth, ClusterHostHealth, ClusterHostRole,
    ClusterProject, ClusterService, ClusterTotalsSource, ClusterWorker, ClusterWorkerProject,
    ClusterWorkerRole, HostProfileId, PROMOTE_CONFIRMATION_TOKEN, PromoteOutcome, PromoteRequest,
    enable_pooled_compute,
};
pub use connection::{
    ConnectStage, ConnectionManager, ConnectionSnapshot, ConnectionState, LatencyStats, PollPolicy,
    PollScheduler, Revision, SpiritEvent,
};
pub use engine::gate::{
    AsifGate, GateSnapshot, LocalAgentState, LocalAgentStatus, LocalOrchestratorGate,
    local_orchestrator_enabled,
};
pub use engine::local::{
    LocalNodeState, LocalOrchestratorAction, LocalOrchestratorGuard, LocalOrchestratorPrerequisite,
    LocalOrchestratorState, LocalPathMapStatus,
};
pub use engine::{EngineStatus, Keychain, SecretString, StackSpiritEngine};
pub use error::{SpiritError, SpiritErrorCode};
pub use host::{
    HostId, HostKeyPin, HostProfile, HostProfilePatch, HostStore, NewHostProfile, SshTarget,
};
pub use service::{KeyStatus, SpiritCore, SpiritService};
pub use time::{Clock, SystemClock, Timestamp};
pub use view::{
    CapacityView, FleetView, JobLanes, LocalAgentView, LocalOrchestratorView, LocalSessionState,
    LocalSurfaceView,
};
pub use vowp::{
    AgentPairRequest, AgentPairing, AsifExport, AsifExportOptions, AsifImportOptions,
    AsifLossPolicy, AsifPackage, LocalAgentClient, SurfacePairing, SurfacePairingView, VowpClient,
    VowpProbe, ensure_package_size,
};
pub use wire::{
    AGENT_PROTOCOL, ASIF_ARCHIVE_CAP_BYTES, ASIF_FORMAT, ASIF_JSON_CAP_BYTES, AgentPathMap,
    AgentStatusDocument, AsifCounts, AsifExportSummary, AsifImportResponse, AsifLoss, AsifReport,
    AsifSessionRef, CapacityReportDoc, FleetQueueRow, FleetStatus, InboxItem, NodeCard, PathRef,
    Snapshot, SurfaceEntry, SurfacePairRequest, SurfaceRotateRequest, parse_asif_export_summary,
    parse_asif_import_response,
};
