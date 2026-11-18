//! VOWP transport boundary: the trait and its live/mock implementations.

pub mod agent;
pub mod asif;
pub mod client;
pub mod local;
pub mod secret;

pub use agent::{CommandLocalAgent, LocalAgentClient, MockLocalAgentClient};
pub use asif::{
    AsifExport, AsifExportOptions, AsifImportOptions, AsifLossPolicy, AsifPackage,
    ensure_package_size,
};
pub use client::{MockVowpClient, SshVowpClient, VowpClient, VowpProbe};
pub use local::{AgentPairRequest, AgentPairing, SurfacePairing, SurfacePairingView};
pub use secret::{
    SECRET_VALUE_MAX_BYTES, SecretEntry, SecretListView, SecretRemoveOutcome, SecretSetOutcome,
    SecretSetRequest, validate_secret_name,
};
