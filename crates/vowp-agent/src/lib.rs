//! The Mac-side v0.4 agent client.
//!
//! Spec: `docs/design/v0.4/local-orchestrator.md` §4, §8 and §12.2 and
//! `docs/design/crates/vowp-agent.md`. The crate owns local config and the node key, speaks the
//! `vowp-protocol` agent frames over an injected transport, and never depends on a store or on
//! `vowp-core`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod chat;
pub mod client;
pub mod config;
pub mod error;
pub mod executor;
pub mod key;
pub mod spool;
pub mod status;
pub mod transport;
pub mod worktree;

pub use chat::{
    CHAT_EPOCH, ChatClient, ChatTurnRequest, ChatTurnStatus, EVENT_POLL, LEASE_HEARTBEAT_EVERY, MAX_RECONNECTS,
};
pub use client::{
    AGENT_VERSION, AgentClient, AgentClock, Backoff, Enrollment, HEARTBEAT_SECS, MAX_BACKOFF, PAIR_TIMEOUT,
    PairedSurface, RunOptions, RunOutcome, STABLE_CONNECTION, SurfacePairRequest, SystemAgentClock,
    validate_enrollment_code, validate_pairing_code, validate_surface_id,
};
pub use config::{
    AGENT_DIR_NAME, AgentConfig, CONFIG_FILE, DEFAULT_WORKTREE_DIR, PairedToken, STATUS_FILE, SURFACE_EXPIRY_FILE,
    SURFACE_SESSION_FILE, SURFACE_TOKEN_FILE, SURFACES_DIR, TransportConfig, paired_token_for_session,
};
pub use error::AgentError;
pub use executor::{
    AcceptedRun, EVENT_WINDOW, HandlerReject, PushStatus, QUARANTINE_FILE, RunHandler, RunSink, RunSinkError,
    RunSummary,
};
pub use key::{NODE_KEY_FILE, NODE_PUB_FILE, NodeKey, fingerprint_of, handshake_payload, load_public};
pub use spool::{ACKED_FILE, DEFAULT_SPOOL_CAP_BYTES, EVENTS_FILE, SPOOL_DIR, Spool, SpoolAppend, spool_dir};
pub use status::{AgentStatus, ConnectionState, pending_spool_bytes, surface_token_expiry};
pub use transport::{FrameRead, FrameStream, MAX_FRAME_BYTES, SshTransport, Transport};
pub use worktree::{
    DEFAULT_DISK_RESERVE_BYTES, GitOutput, GitRunner, JOB_BRANCH_PREFIX, PathMap, PushResult, STDERR_TAIL_BYTES,
    SystemGit, Worktree, WorktreeError, WorktreeManager, WorktreeSpec, strip_remote_credentials,
};
