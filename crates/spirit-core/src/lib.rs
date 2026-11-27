//! StackSpirit core: VOWP wire types, SSH transport, host storage, connection state,
//! deterministic views, the local engine boundary, and honest capability gates.
//!
//! The crate is headless: it has no Tauri, React, or platform-GUI dependency and never
//! reads `/home/user/vowp` at runtime. All I/O goes through injected traits so tests stay
//! hermetic.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod cloud;
pub mod cluster;
pub mod connection;
pub mod engine;
pub mod error;
#[cfg(feature = "fixtures")]
pub mod fixtures;
pub mod harnesses;
pub mod host;
pub mod service;
pub mod ssh;
pub mod time;
pub mod view;
pub mod vowp;
pub mod wire;

pub use cloud::{
    CLOUD_SESSION_FILE, CloudAccount, CloudClient, CloudError, CloudErrorCode, CloudMachine,
    CloudMethod, CloudRequest, CloudResponse, CloudSession, CloudSessionStore, CloudTransport,
    DEFAULT_API_BASE_URL, JsonCloudSessionStore, ManagedPlanCatalog, RelayTicket,
    RelayTicketRequest,
};
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
pub use harnesses::{
    BUILTIN_HARNESS_IDS, DEFAULT_HARNESS_ID, HarnessAuthMode, HarnessAuthState, HarnessBinaryState,
    HarnessConnectPlan, HarnessConnectPlanView, HarnessListView, HarnessManifest, HarnessProbe,
    HarnessProbeReport, HarnessProbeTarget, HarnessProtocol, HarnessReadiness, HarnessRegistry,
    HarnessTargetKind, LoopbackForwardPlan, build_connect_plan, builtin_harnesses,
    default_harness_dir, loopback_forward_plan, probe_harness,
};
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
