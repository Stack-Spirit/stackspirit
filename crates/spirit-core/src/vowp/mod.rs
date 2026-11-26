//! VOWP transport boundary: the trait and its live/mock implementations.

pub mod agent;
pub mod asif;
pub mod client;
pub mod files;
pub mod history;
pub mod local;
pub mod project;
pub mod secret;

pub use agent::{CommandLocalAgent, LocalAgentClient, MockLocalAgentClient};
pub use asif::{
    AsifExport, AsifExportOptions, AsifImportOptions, AsifLossPolicy, AsifPackage,
    ensure_package_size,
};
pub use client::{MockVowpClient, OrchestratorMoveTarget, SshVowpClient, VowpClient, VowpProbe};
pub use files::{
    RemoteDirListing, RemoteDownloadOutcome, RemoteEntry, RemoteEntryKind, RemoteFileOutcome,
    RemoteFiles, RemotePreview, RemotePreviewKind, RemoteUploadOutcome,
};
pub use history::{
    GcClassView, GcReportView, HISTORY_GC_OLDER_THAN, HistoryJobView, parse_gc_report,
    parse_history_jobs,
};
pub use local::{AgentPairRequest, AgentPairing, SurfacePairing, SurfacePairingView};
pub use project::{ProjectAddOutcome, ProjectAddRequest, ProjectSource, parse_project_bootstrap};
pub use secret::{
    SECRET_VALUE_MAX_BYTES, SecretEntry, SecretListView, SecretRemoveOutcome, SecretSetOutcome,
    SecretSetRequest, validate_secret_name,
};
