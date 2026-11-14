//! The Mac-side v0.4 agent client.
//!
//! Spec: `docs/design/v0.4/local-orchestrator.md` §4, §8 and §12.2 and
//! `docs/design/crates/vowp-agent.md`. The crate owns local config and the node key, speaks the
//! `vowp-protocol` agent frames over an injected transport, and never depends on a store or on
//! `vowp-core`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod client;
pub mod config;
pub mod error;
pub mod key;
pub mod status;
pub mod transport;

pub use client::{
    AGENT_VERSION, AgentClient, AgentClock, Backoff, Enrollment, HEARTBEAT_SECS, MAX_BACKOFF, PAIR_TIMEOUT,
    PairedSurface, RunOptions, RunOutcome, STABLE_CONNECTION, SurfacePairRequest, SystemAgentClock,
    validate_enrollment_code, validate_pairing_code, validate_surface_id,
};
pub use config::{
    AGENT_DIR_NAME, AgentConfig, CONFIG_FILE, STATUS_FILE, SURFACE_EXPIRY_FILE, SURFACE_TOKEN_FILE, SURFACES_DIR,
    TransportConfig,
};
pub use error::AgentError;
pub use key::{NODE_KEY_FILE, NODE_PUB_FILE, NodeKey, fingerprint_of, handshake_payload, load_public};
pub use status::{AgentStatus, ConnectionState, pending_spool_bytes, surface_token_expiry};
pub use transport::{FrameRead, FrameStream, MAX_FRAME_BYTES, SshTransport, Transport};
