//! StackSpirit core: VOWP wire types, SSH transport, host storage, connection state,
//! deterministic views, the local engine boundary, and honest capability gates.
//!
//! The crate is headless: it has no Tauri, React, or platform-GUI dependency and never
//! reads `/home/dev/vowp` at runtime. All I/O goes through injected traits so tests stay
//! hermetic.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

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

pub use connection::{
    ConnectStage, ConnectionManager, ConnectionSnapshot, ConnectionState, LatencyStats, PollPolicy,
    PollScheduler, Revision, SpiritEvent,
};
pub use engine::gate::{AsifGate, GateSnapshot, LocalAgentState, LocalOrchestratorGate};
pub use engine::{EngineStatus, Keychain, SecretString, StackSpiritEngine};
pub use error::{SpiritError, SpiritErrorCode};
pub use host::{
    HostId, HostKeyPin, HostProfile, HostProfilePatch, HostStore, NewHostProfile, SshTarget,
};
pub use service::{KeyStatus, SpiritCore};
pub use time::{Clock, SystemClock, Timestamp};
pub use view::{CapacityView, FleetView, JobLanes};
pub use vowp::{VowpClient, VowpProbe};
pub use wire::{CapacityReportDoc, FleetQueueRow, FleetStatus, InboxItem, Snapshot};
