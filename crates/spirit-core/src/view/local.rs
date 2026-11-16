//! Honest D6 views: local surfaces, the Mac agent, path maps and local orchestrators.
//!
//! Every field here is safe to serialize to the renderer. Absolute Mac paths, pairing
//! codes, surface tokens, token files and key fingerprints are absent by construction.
//! A surface that is revoked or expired is shown as such and is never written to on the
//! strength of an old credential.

use serde::{Deserialize, Serialize};

use crate::engine::local::LocalNodeState;
use crate::time::Timestamp;
use crate::view::fleet::{JoinView, OrchestratorView, SessionCardView};
use crate::wire::{
    AGENT_PROTOCOL, AgentConnectionState, AgentPathMap, AgentStatusDocument, AgentSurfaceStatus,
    NodePresence, SurfaceEntry,
};

/// The local surface/session state shown to the owner.
///
/// The six failure states from `D6-READINESS.md` §4 are explicit: `disconnected`,
/// `degraded`, `reconnecting`, `revoked`, `superseded` and `stopped`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalSessionState {
    /// The local surface is attached and can show the session.
    Active,
    /// The session exists but has no work running.
    Idle,
    /// The session is running a turn.
    Running,
    /// The session is blocked on an owner decision.
    Blocked,
    /// The session is switching surfaces.
    Switching,
    /// The session is closed.
    Closed,
    /// A pending surface is waiting for pairing.
    Pending,
    /// No live local surface is connected.
    Disconnected,
    /// The node or app is reachable but unhealthy.
    Degraded,
    /// The agent is trying to reconnect.
    Reconnecting,
    /// The node or surface credential was revoked.
    Revoked,
    /// Another agent took over the connection.
    Superseded,
    /// The local agent is stopped.
    Stopped,
    /// The state cannot be determined honestly.
    Unknown,
}

impl LocalSessionState {
    /// Returns the stable snake_case label.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Idle => "idle",
            Self::Running => "running",
            Self::Blocked => "blocked",
            Self::Switching => "switching",
            Self::Closed => "closed",
            Self::Pending => "pending",
            Self::Disconnected => "disconnected",
            Self::Degraded => "degraded",
            Self::Reconnecting => "reconnecting",
            Self::Revoked => "revoked",
            Self::Superseded => "superseded",
            Self::Stopped => "stopped",
            Self::Unknown => "unknown",
        }
    }

    fn from_session_state(state: &str) -> Self {
        match state {
            "idle" => Self::Idle,
            "running" => Self::Running,
            "blocked" => Self::Blocked,
            "switching" => Self::Switching,
            "closed" => Self::Closed,
            _ => Self::Unknown,
        }
    }
}

/// The safe Mac agent status view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalAgentView {
    /// Connection state.
    pub state: String,
    /// Node name, when known.
    pub node: Option<String>,
    /// Negotiated protocol.
    pub protocol: Option<String>,
    /// Last heartbeat.
    pub last_heartbeat: Option<Timestamp>,
    /// Redacted last error.
    pub last_error: Option<String>,
    /// Local cache entries.
    pub cache_entries: u64,
    /// Local cache bytes.
    pub cache_bytes: u64,
    /// Oldest local cache entry.
    pub cache_oldest_at: Option<Timestamp>,
    /// Local surface rows.
    pub surfaces: Vec<AgentSurfaceView>,
}

impl LocalAgentView {
    /// Projects one Mac status document into safe display fields.
    #[must_use]
    pub fn from_document(document: &AgentStatusDocument) -> Self {
        Self {
            state: document.connection.as_str().to_owned(),
            node: document
                .node
                .as_deref()
                .map(|node| safe_token(node, 64))
                .filter(|node| !node.is_empty()),
            protocol: document.protocol.as_deref().map(safe_protocol),
            last_heartbeat: document.last_heartbeat,
            last_error: document.last_error.as_deref().map(safe_text),
            cache_entries: document.cache_entries,
            cache_bytes: document.cache_bytes,
            cache_oldest_at: document.cache_oldest_at,
            surfaces: document
                .surfaces
                .iter()
                .map(AgentSurfaceView::from)
                .collect(),
        }
    }

    /// Returns true only for a connected agent with the frozen protocol.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.state == AgentConnectionState::Connected.as_str()
            && self.protocol.as_deref() == Some(AGENT_PROTOCOL)
    }
}

/// One local surface stored by the Mac agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSurfaceView {
    /// Surface id.
    pub surface: String,
    /// Display prefix of the token; never the token.
    pub prefix: Option<String>,
    /// Token expiry.
    pub expires_at: Option<Timestamp>,
    /// Local state.
    pub state: String,
}

impl From<&AgentSurfaceStatus> for AgentSurfaceView {
    fn from(status: &AgentSurfaceStatus) -> Self {
        Self {
            surface: status.surface.clone(),
            prefix: status.prefix.clone(),
            expires_at: status.expires_at,
            state: status.state.as_str().to_owned(),
        }
    }
}

/// A safe local path-map summary. The absolute Mac root is not present; the shared
/// [`crate::wire::PathRef`] is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentPathView {
    /// Project name.
    pub project: String,
    /// Shared-state path reference (`project:<name>`).
    pub path_ref: String,
    /// Whether a separate worktree root is configured.
    pub worktree_root_configured: bool,
    /// Allowed relative paths.
    pub allow: Vec<String>,
    /// Whether the map passed the local shape checks.
    pub valid: bool,
    /// Path-free reason when invalid.
    pub invalid_reason: Option<String>,
}

impl AgentPathView {
    /// Projects one Mac-local path map without its absolute roots.
    #[must_use]
    pub fn from_map(map: &AgentPathMap) -> Self {
        let validation = map.validate();
        Self {
            project: map.project.to_string(),
            path_ref: format!("project:{}", map.project),
            worktree_root_configured: map.worktree_root.is_some(),
            allow: map.allow.clone(),
            valid: validation.is_ok(),
            invalid_reason: validation.err().map(|error| error.message),
        }
    }
}

/// One home surface with its honest node/agent/lease/session state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalSurfaceView {
    /// Surface id.
    pub surface_id: String,
    /// Node that runs the surface.
    pub node: String,
    /// Agent protocol, when known.
    pub protocol: Option<String>,
    /// Mac agent status.
    pub agent_status: String,
    /// Human-visible lease holder, when a session lease is live.
    pub lease_holder: Option<String>,
    /// Local surface/session state.
    pub session_state: String,
    /// Surface kind.
    pub kind: String,
    /// Surface lifecycle state.
    pub state: String,
    /// Token expiry.
    pub expires_at: Option<Timestamp>,
    /// Whether the stored token is expired.
    pub expired: bool,
    /// Whether a rotation frame is still queued.
    pub rotation_pending: bool,
    /// True only when this surface may write the session.
    pub can_write: bool,
}

impl LocalSurfaceView {
    /// Combines one home surface row with the Mac status and the fleet session card.
    #[must_use]
    pub fn from_parts(
        surface: &SurfaceEntry,
        agent: &AgentStatusDocument,
        node: Option<&LocalNodeState>,
        session: Option<&SessionCardView>,
        now: Timestamp,
    ) -> Self {
        let protocol = node
            .and_then(|node| node.protocol.clone())
            .or_else(|| agent.protocol.clone());
        let lease_holder = session.and_then(|session| {
            session
                .active_surface
                .as_ref()
                .map(|surface| surface.holder.clone())
        });
        let session_state = derive_session_state(surface, agent, node, session, now);
        let lease_is_ours = session
            .and_then(|session| session.active_surface.as_ref())
            .is_some_and(|holder| holder.surface_id == surface.id);
        let node_is_online =
            node.is_some_and(|node| node.presence == NodePresence::Online.as_str());
        let can_write = surface.is_writable(now)
            && agent.connection == AgentConnectionState::Connected
            && protocol.as_deref() == Some(AGENT_PROTOCOL)
            && lease_is_ours
            && node_is_online;
        Self {
            surface_id: surface.id.clone(),
            node: surface.node.clone(),
            protocol,
            agent_status: agent.connection.as_str().to_owned(),
            lease_holder,
            session_state: session_state.as_str().to_owned(),
            kind: surface.kind.as_str().to_owned(),
            state: surface.state.as_str().to_owned(),
            expires_at: surface.expires_at,
            expired: surface.is_expired(now),
            rotation_pending: surface.pending,
            can_write,
        }
    }
}

/// One local orchestrator with the node, protocol, agent and lease state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalOrchestratorView {
    /// Orchestrator name.
    pub name: String,
    /// Session id.
    pub session_id: String,
    /// Project, when bound.
    pub project: Option<String>,
    /// App name.
    pub app: String,
    /// Orchestrator lifecycle state.
    pub state: String,
    /// Node that runs the app, when VOWP reports one.
    pub node: Option<String>,
    /// Agent protocol, when known.
    pub protocol: Option<String>,
    /// Mac agent status.
    pub agent_status: String,
    /// Human-visible lease holder, when a session lease is live.
    pub lease_holder: Option<String>,
    /// Local surface/session state.
    pub session_state: String,
    /// Join instructions.
    pub join: JoinView,
}

impl LocalOrchestratorView {
    /// Combines one orchestrator card with the Mac status and a session card.
    #[must_use]
    pub fn from_parts(
        card: &OrchestratorView,
        agent: &AgentStatusDocument,
        node: Option<&LocalNodeState>,
        session: Option<&SessionCardView>,
    ) -> Self {
        let protocol = node
            .and_then(|node| node.protocol.clone())
            .or_else(|| agent.protocol.clone());
        let lease_holder = session.and_then(|session| {
            session
                .active_surface
                .as_ref()
                .map(|surface| surface.holder.clone())
        });
        let session_state = derive_orchestrator_state(card, agent, node, session);
        Self {
            name: card.name.clone(),
            session_id: card.session_id.clone(),
            project: card.project.clone(),
            app: card.app.clone(),
            state: card.state.clone(),
            node: card.node.clone(),
            protocol,
            agent_status: agent.connection.as_str().to_owned(),
            lease_holder,
            session_state: session_state.as_str().to_owned(),
            join: card.join.clone(),
        }
    }
}

fn derive_session_state(
    surface: &SurfaceEntry,
    agent: &AgentStatusDocument,
    node: Option<&LocalNodeState>,
    session: Option<&SessionCardView>,
    now: Timestamp,
) -> LocalSessionState {
    match agent.connection {
        AgentConnectionState::Revoked => return LocalSessionState::Revoked,
        AgentConnectionState::Superseded => return LocalSessionState::Superseded,
        AgentConnectionState::Stopped => return LocalSessionState::Stopped,
        AgentConnectionState::Reconnecting | AgentConnectionState::Connecting => {
            return LocalSessionState::Reconnecting;
        }
        AgentConnectionState::Unknown(_)
        | AgentConnectionState::NotInstalled
        | AgentConnectionState::NotEnrolled
        | AgentConnectionState::Incompatible => return LocalSessionState::Disconnected,
        AgentConnectionState::Connected => {}
    }
    if surface.is_revoked() {
        return LocalSessionState::Revoked;
    }
    if let Some(node) = node {
        match node.presence.as_str() {
            "revoked" => return LocalSessionState::Revoked,
            "degraded" => return LocalSessionState::Degraded,
            "offline" => return LocalSessionState::Disconnected,
            _ => {}
        }
    }
    if surface.pending {
        return LocalSessionState::Pending;
    }
    if surface.is_expired(now) {
        return LocalSessionState::Disconnected;
    }
    match session {
        Some(session) => LocalSessionState::from_session_state(&session.state),
        None => LocalSessionState::Unknown,
    }
}

fn derive_orchestrator_state(
    card: &OrchestratorView,
    agent: &AgentStatusDocument,
    node: Option<&LocalNodeState>,
    session: Option<&SessionCardView>,
) -> LocalSessionState {
    match card.state.as_str() {
        "node_offline" => return LocalSessionState::Disconnected,
        "stopped" => return LocalSessionState::Stopped,
        "degraded" => return LocalSessionState::Degraded,
        "failed" => return LocalSessionState::Disconnected,
        _ => {}
    }
    match agent.connection {
        AgentConnectionState::Revoked => return LocalSessionState::Revoked,
        AgentConnectionState::Superseded => return LocalSessionState::Superseded,
        AgentConnectionState::Stopped => return LocalSessionState::Stopped,
        AgentConnectionState::Reconnecting | AgentConnectionState::Connecting => {
            return LocalSessionState::Reconnecting;
        }
        AgentConnectionState::Unknown(_)
        | AgentConnectionState::NotInstalled
        | AgentConnectionState::NotEnrolled
        | AgentConnectionState::Incompatible => return LocalSessionState::Disconnected,
        AgentConnectionState::Connected => {}
    }
    if node.is_some_and(|node| node.presence == NodePresence::Degraded.as_str()) {
        return LocalSessionState::Degraded;
    }
    if let Some(session) = session {
        return LocalSessionState::from_session_state(&session.state);
    }
    if card.state == "running" {
        return LocalSessionState::Running;
    }
    LocalSessionState::Unknown
}

fn safe_token(input: &str, limit: usize) -> String {
    input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .take(limit)
        .collect()
}

fn safe_protocol(input: &str) -> String {
    input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '/'))
        .take(64)
        .collect()
}

fn safe_text(input: &str) -> String {
    let redacted = crate::error::redact(input);
    redacted
        .split_whitespace()
        .map(|word| {
            if word.starts_with('/')
                || word.starts_with("~/")
                || word.contains("known_hosts")
                || word.contains("id_rsa")
                || word.contains("id_ed25519")
                || word.contains(".vowp-agent")
            {
                "[path]"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect()
}
