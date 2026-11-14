//! v0.4 local-node wire types: path refs, capabilities, capacity and Phase 0 frames.
//!
//! Spec: `docs/design/v0.4/local-orchestrator.md` §4.1, §6.1, §8.3, §8.5 and §11.3.
//! This module is pure data: no I/O, crypto, store or network. Absolute local paths stay on
//! the node; [`PathRef`] is the only path shape that crosses the machine boundary.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{ErrorKind, ProtocolError};
use crate::ids::{JobId, NodeName, ProjectName, SessionId};
use crate::surface::{SurfaceId, SurfaceKind};
use crate::time::Timestamp;
use crate::vault::SecretName;

/// Protocol name exchanged in the agent handshake.
pub const AGENT_PROTOCOL: &str = "vowp-agent/1";

/// Longest accepted `PathRef` wire string, in bytes.
pub const MAX_PATH_REF_LEN: usize = 1024;

/// A project-relative path the agent resolves locally.
///
/// The wire form is one short string: `project:vowp`, `worktree:vowp/<job>` or
/// `subdir:vowp/src`. Parsing rejects `..`, absolute paths, backslashes, NUL, an empty
/// project and an empty subdirectory, and normalises repeated `/` and `.` components.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "String", into = "String")]
#[schemars(with = "String")]
pub enum PathRef {
    /// The project root.
    Project {
        /// Project name.
        project: ProjectName,
    },
    /// The job worktree under the project.
    Worktree {
        /// Project name.
        project: ProjectName,
        /// Job whose worktree is referenced.
        job: JobId,
    },
    /// A directory below the project root.
    Subdir {
        /// Project name.
        project: ProjectName,
        /// Normalised project-relative path.
        rel: String,
    },
}

impl PathRef {
    /// Parses and validates a path ref wire string.
    pub fn parse(s: &str) -> Result<Self, ProtocolError> {
        let invalid = || ProtocolError::InvalidId {
            kind: "path_ref",
            value: s.to_owned(),
        };
        if s.len() > MAX_PATH_REF_LEN || s.contains('\0') || s.contains('\\') {
            return Err(invalid());
        }
        let (kind, rest) = s.split_once(':').ok_or_else(invalid)?;
        match kind {
            "project" => {
                if rest.is_empty() || rest.contains('/') {
                    return Err(invalid());
                }
                Ok(Self::Project {
                    project: ProjectName::parse(rest)?,
                })
            }
            "worktree" => {
                let (project, job) = rest.split_once('/').ok_or_else(invalid)?;
                if project.is_empty() || job.is_empty() || job.contains('/') {
                    return Err(invalid());
                }
                Ok(Self::Worktree {
                    project: ProjectName::parse(project)?,
                    job: JobId::parse(job)?,
                })
            }
            "subdir" => {
                let (project, rel) = rest.split_once('/').ok_or_else(invalid)?;
                if project.is_empty() {
                    return Err(invalid());
                }
                let rel = normalise_rel(rel).ok_or_else(invalid)?;
                Ok(Self::Subdir {
                    project: ProjectName::parse(project)?,
                    rel,
                })
            }
            _ => Err(invalid()),
        }
    }
}

/// Normalises a relative path, rejecting traversal, absolute input and empty results.
fn normalise_rel(rel: &str) -> Option<String> {
    if rel.is_empty() || rel.starts_with('/') {
        return None;
    }
    let mut parts = Vec::new();
    for part in rel.split('/') {
        match part {
            "" | "." => continue,
            ".." => return None,
            other => parts.push(other),
        }
    }
    if parts.is_empty() { None } else { Some(parts.join("/")) }
}

impl fmt::Display for PathRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Project { project } => write!(f, "project:{project}"),
            Self::Worktree { project, job } => write!(f, "worktree:{project}/{job}"),
            Self::Subdir { project, rel } => write!(f, "subdir:{project}/{rel}"),
        }
    }
}

impl From<PathRef> for String {
    fn from(value: PathRef) -> Self {
        value.to_string()
    }
}

impl TryFrom<String> for PathRef {
    type Error = ProtocolError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl FromStr for PathRef {
    type Err = ProtocolError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// What a local agent is allowed to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    /// Hosts local surfaces only.
    Surface,
    /// Runs offloaded jobs only.
    Executor,
    /// Hosts surfaces and runs offloaded jobs.
    Both,
}

/// Names-only capabilities advertised by an agent; values never cross the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AgentCapabilities {
    /// Operating system name (`darwin`, `linux`).
    pub os: String,
    /// CPU architecture (`arm64`, `x86_64`).
    pub arch: String,
    /// Agent binary version.
    pub agent_version: String,
    /// Harness name to version, for example `opencode` -> `1.2.3`.
    pub harnesses: BTreeMap<String, String>,
    /// `git --version` output, when known.
    pub git_version: Option<String>,
    /// Owner-assigned labels.
    pub labels: Vec<String>,
    /// Names of secrets the node can resolve; never values.
    pub secret_names: Vec<SecretName>,
    /// Projects mapped on the node.
    pub projects: Vec<ProjectName>,
}

/// Capacity reported in a heartbeat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AgentCapacity {
    /// Logical CPU cores.
    pub cpu_cores: u32,
    /// One-minute load average.
    pub load1: f64,
    /// Total physical memory in bytes.
    pub mem_total_bytes: u64,
    /// Memory available to new work in bytes.
    pub mem_free_bytes: u64,
    /// Free bytes on the worktree filesystem.
    pub disk_free_bytes: u64,
    /// Worker and verifier slots in use.
    pub busy_slots: u32,
    /// Maximum worker and verifier slots.
    pub max_slots: u32,
    /// Whether the node is on battery, when known.
    pub on_battery: Option<bool>,
    /// Thermal state name (`nominal`, `fair`, `serious`, `critical`), when known.
    pub thermal_state: Option<String>,
}

/// Enrollment state of a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    /// Waiting for approval.
    Pending,
    /// Enrolled and usable.
    Active,
    /// Credentials revoked.
    Revoked,
}

/// How a node is reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// The home itself.
    Home,
    /// A pre-v0.4 SSH node.
    Ssh,
    /// An enrolled `vowp agent` node.
    Agent,
}

/// Computed presence of a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NodePresence {
    /// A heartbeat arrived under 30 seconds ago.
    Online,
    /// The last heartbeat is 30 to 90 seconds old.
    Degraded,
    /// The last heartbeat is over 90 seconds old.
    Offline,
    /// The node is revoked regardless of its last heartbeat.
    Revoked,
}

/// One project path map entry supplied by an agent at enrollment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AgentPathMap {
    /// Project the map belongs to.
    pub project: ProjectName,
    /// Absolute project root on the node; stays local to the home's `node_paths` row.
    pub local_root: String,
    /// Absolute worktree root on the node, when separate.
    pub worktree_root: Option<String>,
    /// Allowed relative paths below the root.
    pub allow: Vec<String>,
}

/// Redacted node summary shown by `vowp node show` and never carrying a path or secret name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NodeCard {
    /// Node name.
    pub name: NodeName,
    /// How the node is reached.
    pub kind: NodeKind,
    /// Enrollment state.
    pub status: NodeStatus,
    /// Computed presence.
    pub presence: NodePresence,
    /// Owner-assigned labels.
    pub labels: Vec<String>,
    /// Latest capacity snapshot, when one arrived.
    pub capacity: Option<AgentCapacity>,
    /// Number of active runs on the node.
    pub runs: u32,
    /// Projects mapped on the node.
    pub mapped: Vec<ProjectName>,
    /// Agent binary version, when known.
    pub agent_version: Option<String>,
    /// Agent protocol, when known.
    pub protocol: Option<String>,
    /// Last connection or policy error, redacted by the caller.
    pub last_error: Option<String>,
}

/// Lifecycle state of a surface row.
///
/// The global schema stores this as `surfaces.state`. `pending` is a surface created for a
/// pairing code, `attached` is a live surface (and the default for pre-v0.4 rows), and
/// `revoked` is a surface whose tokens no longer work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceState {
    /// Created for a pairing code and not attached yet.
    Pending,
    /// Attached and usable.
    Attached,
    /// Revoked; no token of the surface is accepted.
    Revoked,
}

/// One capability granted to a surface token.
///
/// The wire form is one short string, for example `session.read:<id>`, `session.write:<id>`,
/// `project.read:<name>`, `fleet.read` or `tool:<name>`. The store keeps the list in
/// `surface_tokens.scopes_json`; unknown scopes are preserved verbatim so a newer home can
/// extend the vocabulary without breaking older readers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
#[schemars(with = "String")]
pub struct SurfaceScope(pub String);

impl SurfaceScope {
    /// Builds a scope from its wire string.
    pub fn new(scope: impl Into<String>) -> Self {
        Self(scope.into())
    }

    /// Returns the scope wire string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SurfaceScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A registered surface with its lifecycle state.
///
/// This is the redacted read model used by the surface repository: it carries no token or
/// token hash. The v0.2 [`crate::surface::Surface`] keeps the original field set, so rows
/// created before v0.4 remain source-compatible.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceInfo {
    /// Surface instance id.
    pub id: SurfaceId,
    /// Kind of client.
    pub kind: SurfaceKind,
    /// Node the surface runs on.
    pub node: NodeName,
    /// Human-visible holder name.
    pub holder: String,
    /// Application name, when known.
    pub app: Option<String>,
    /// Working directory, when known.
    pub cwd: Option<String>,
    /// Free-form client metadata.
    pub metadata: serde_json::Value,
    /// Lifecycle state.
    pub state: SurfaceState,
    /// When the surface first registered.
    pub created_at: Timestamp,
    /// Last heartbeat.
    pub last_seen_at: Timestamp,
    /// When the surface was revoked; `None` while it is not revoked.
    pub revoked_at: Option<Timestamp>,
}

/// How a surface asks for a session lease.
///
/// `Acquire` fails with a `Conflict` error when another surface holds a live lease;
/// `Takeover` replaces it and fences the old holder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LeaseMode {
    /// Take the lease only when it is free or stale.
    Acquire,
    /// Replace a live lease after the owner agreed.
    Takeover,
}

/// One agent protocol frame, tagged by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentFrame {
    /// Agent -> home: identity and capabilities.
    AgentHello {
        /// Node name.
        node: NodeName,
        /// Public-key id.
        key_id: String,
        /// Agent binary version.
        agent_version: String,
        /// Operating system name.
        os: String,
        /// CPU architecture.
        arch: String,
        /// Owner-assigned labels.
        labels: Vec<String>,
        /// Names-only capabilities.
        capabilities: AgentCapabilities,
    },
    /// Home -> agent: the nonce and limits for authentication.
    HomeChallenge {
        /// One-time nonce.
        nonce: String,
        /// Home identity.
        home_id: String,
        /// Connection identity.
        connection_id: String,
        /// Home server time.
        server_time: Timestamp,
        /// Heartbeat interval in seconds.
        heartbeat_secs: u64,
        /// Maximum accepted frame size in bytes.
        max_frame_bytes: u64,
    },
    /// Agent -> home: signature over the challenge.
    AgentAuth {
        /// Lowercase hex signature.
        signature: String,
    },
    /// Home -> agent: the handshake succeeded.
    HomeWelcome {
        /// Negotiated protocol.
        protocol: String,
        /// Home identity.
        home_id: String,
        /// Node name.
        node: NodeName,
        /// Connection identity.
        connection_id: String,
    },
    /// Agent -> home: redeem a one-time enrollment code.
    Enroll {
        /// One-time enrollment code; never logged.
        code: String,
        /// Lowercase hex ed25519 public key.
        public_key: String,
        /// Agent binary version.
        agent_version: String,
        /// Operating system name.
        os: String,
        /// CPU architecture.
        arch: String,
        /// Owner-assigned labels.
        labels: Vec<String>,
        /// Names-only capabilities.
        capabilities: AgentCapabilities,
        /// Initial project path map.
        paths: Vec<AgentPathMap>,
    },
    /// Home -> agent: enrollment succeeded.
    Enrolled {
        /// Home identity.
        home_id: String,
        /// Node name.
        node: NodeName,
        /// Negotiated protocol.
        protocol: String,
        /// Home server time.
        server_time: Timestamp,
        /// Heartbeat interval in seconds.
        heartbeat_secs: u64,
    },
    /// Agent -> home: redeem a one-time pairing code for a local surface.
    SurfacePair {
        /// One-time pairing code; never logged or echoed.
        code: String,
        /// Surface the owner created on the home.
        surface_id: SurfaceId,
        /// Session the surface will mirror.
        session: SessionId,
        /// Application name, for example `opencode`.
        app: String,
    },
    /// Home -> agent: the pairing succeeded and issued a token.
    SurfaceIssued {
        /// Surface the token belongs to.
        surface_id: SurfaceId,
        /// Secret token; write it 0600, never log or echo it.
        token: String,
        /// Scopes granted to the token.
        scopes: Vec<SurfaceScope>,
        /// When the token expires.
        expires_at: Timestamp,
    },
    /// Home -> agent: the surface token was revoked; detach and delete it.
    SurfaceRevoked {
        /// Surface whose token was revoked.
        surface_id: SurfaceId,
        /// Human-readable reason.
        reason: String,
    },
    /// Agent -> home: acquire or take over the session lease.
    LeaseAcquire {
        /// Surface asking for the lease.
        surface: SurfaceId,
        /// Session the lease protects.
        session: SessionId,
        /// Scoped surface token issued by `SurfaceIssued`; never logged.
        surface_token: String,
        /// Whether a live lease may be replaced.
        mode: LeaseMode,
    },
    /// Agent -> home: keep the held lease alive.
    LeaseHeartbeat {
        /// Surface holding the lease.
        surface: SurfaceId,
        /// Session the lease protects.
        session: SessionId,
        /// Plaintext lease token returned by `LeaseGrant`; never logged.
        token: String,
    },
    /// Agent -> home: release the lease on detach.
    LeaseRelease {
        /// Surface releasing the lease.
        surface: SurfaceId,
        /// Session the lease protects.
        session: SessionId,
        /// Plaintext lease token returned by `LeaseGrant`; never logged.
        token: String,
    },
    /// Home -> agent: the lease is granted for this surface.
    LeaseGrant {
        /// Plaintext lease token; shown once in the grant, never logged.
        lease_token: String,
        /// When the lease goes stale without a heartbeat.
        expires_at: Timestamp,
        /// Session sequence the new holder may continue from.
        tail_seq: u64,
    },
    /// Home -> agent: the lease was taken over or expired; the surface is read-only.
    LeaseLost {
        /// Human-readable reason.
        reason: String,
    },
    /// Home -> agent: another surface now holds the lease; stop sending events.
    LeaseRevoked {
        /// Human-readable reason.
        reason: String,
        /// Human-visible holder of the lease now.
        holder: String,
    },
    /// Agent -> home: capacity and spool state.
    Heartbeat {
        /// When the heartbeat was sent.
        at: Timestamp,
        /// Latest capacity snapshot.
        capacity: AgentCapacity,
        /// Bytes waiting in the pending spool.
        pending_spool_bytes: u64,
    },
    /// Home -> agent: liveness probe.
    Ping,
    /// Agent -> home: liveness reply.
    Pong,
    /// Home -> agent: the node credential was revoked.
    Revoked {
        /// Human-readable reason.
        reason: String,
    },
    /// Home -> agent: another connection took over.
    Superseded {
        /// Connection that superseded this one.
        connection_id: String,
    },
    /// Home -> agent: protocol or policy error.
    Error {
        /// Error class.
        kind: ErrorKind,
        /// Human-readable message.
        message: String,
        /// Whether a retry may succeed.
        retryable: bool,
    },
}
