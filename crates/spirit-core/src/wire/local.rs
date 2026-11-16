//! Local-orchestrator (D6) wire types: home surfaces, the Mac agent, path refs,
//! node cards and orchestrator requests.
//!
//! The types mirror the public VOWP v0.4 CLI/JSON surface in `docs/VOWP-CONTRACT.md`
//! §11.3. Absolute Mac paths NEVER cross this boundary: [`AgentPathMap`] is a
//! Mac-local input that is deliberately `Deserialize`-only, and shared state uses
//! [`PathRef`] (`project:`, `worktree:`, `subdir:`) instead.
//!
//! A surface pairing code and an agent surface token are credentials. Their wire
//! outputs are parsed here but only the `vowp` module can wrap them in a secret type;
//! these document types intentionally do not implement `Serialize`.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::SpiritError;
use crate::time::Timestamp;
use crate::wire::enums::{
    AgentConnectionState, AgentSurfaceState, NodeKind, NodePresence, NodeStatus, OrchestratorApp,
    SurfaceKind, SurfaceLifecycleState,
};
use crate::wire::ids::{JobId, NodeName, ProjectName, SessionId};

/// The only agent protocol this build negotiates.
pub const AGENT_PROTOCOL: &str = "vowp-agent/1";

/// A surface id is a 26-character ULID encoded as ASCII alphanumerics.
pub const SURFACE_ID_LEN: usize = 26;

/// The longest accepted `PathRef` wire string, mirroring VOWP's 1024-byte cap.
pub const MAX_PATH_REF_LEN: usize = 1024;

/// A project-relative path that may cross the home/Mac boundary.
///
/// Absolute Mac paths, `..`, backslashes and NUL are rejected by construction. The
/// wire form is one short string: `project:vowp`, `worktree:vowp/ss-1-core` or
/// `subdir:vowp/src`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PathRef(String);

impl PathRef {
    /// Parses and validates a `PathRef` wire string.
    pub fn parse(input: &str) -> Result<Self, SpiritError> {
        if input.len() > MAX_PATH_REF_LEN || input.contains('\0') || input.contains('\\') {
            return Err(SpiritError::invalid_input(
                "path_ref",
                "must be a short POSIX relative reference",
            ));
        }
        let Some((kind, rest)) = input.split_once(':') else {
            return Err(SpiritError::invalid_input(
                "path_ref",
                "expected project:, worktree: or subdir:",
            ));
        };
        match kind {
            "project" => {
                if rest.is_empty() || rest.contains('/') {
                    return Err(SpiritError::invalid_input(
                        "path_ref",
                        "project refs carry one project name",
                    ));
                }
                let project = ProjectName::parse(rest)?;
                Ok(Self(format!("project:{project}")))
            }
            "worktree" => {
                let Some((project, job)) = rest.split_once('/') else {
                    return Err(SpiritError::invalid_input(
                        "path_ref",
                        "worktree refs use project:<name>/<job>",
                    ));
                };
                if project.is_empty() || job.is_empty() || job.contains('/') {
                    return Err(SpiritError::invalid_input(
                        "path_ref",
                        "worktree refs use one project and one job id",
                    ));
                }
                let project = ProjectName::parse(project)?;
                let job = JobId::parse(job)?;
                Ok(Self(format!("worktree:{project}/{job}")))
            }
            "subdir" => {
                let Some((project, rel)) = rest.split_once('/') else {
                    return Err(SpiritError::invalid_input(
                        "path_ref",
                        "subdir refs use project:<name>/<relative path>",
                    ));
                };
                if project.is_empty() {
                    return Err(SpiritError::invalid_input(
                        "path_ref",
                        "subdir refs need a project",
                    ));
                }
                let project = ProjectName::parse(project)?;
                let rel = normalise_rel(rel).ok_or_else(|| {
                    SpiritError::invalid_input(
                        "path_ref",
                        "subdir must be relative and must not escape its project",
                    )
                })?;
                Ok(Self(format!("subdir:{project}/{rel}")))
            }
            _ => Err(SpiritError::invalid_input(
                "path_ref",
                "expected project:, worktree: or subdir:",
            )),
        }
    }

    /// Builds a project-root reference.
    pub fn project(project: impl AsRef<str>) -> Result<Self, SpiritError> {
        Self::parse(&format!("project:{}", project.as_ref()))
    }

    /// Builds a job-worktree reference.
    pub fn worktree(project: impl AsRef<str>, job: impl AsRef<str>) -> Result<Self, SpiritError> {
        Self::parse(&format!("worktree:{}/{}", project.as_ref(), job.as_ref()))
    }

    /// Builds a project-relative subdirectory reference.
    pub fn subdir(project: impl AsRef<str>, rel: impl AsRef<str>) -> Result<Self, SpiritError> {
        Self::parse(&format!("subdir:{}/{}", project.as_ref(), rel.as_ref()))
    }

    /// Returns the wire string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PathRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<PathRef> for String {
    fn from(value: PathRef) -> Self {
        value.0
    }
}

impl TryFrom<String> for PathRef {
    type Error = SpiritError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

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
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

/// Validates one surface id without keeping a second id type.
pub fn ensure_surface_id(input: &str) -> Result<(), SpiritError> {
    if input.len() == SURFACE_ID_LEN && input.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        Ok(())
    } else {
        Err(SpiritError::invalid_input(
            "surface id",
            "expected a 26-character ULID",
        ))
    }
}

/// Validates a duration argument such as `30s`, `10m`, `24h` or `7d`.
pub fn ensure_duration_arg(value: &str, field: &str) -> Result<(), SpiritError> {
    let text = value.trim();
    let digits = text
        .strip_suffix(['s', 'm', 'h', 'd', 'w'])
        .ok_or_else(|| {
            SpiritError::invalid_input(field, "use a duration such as 30m, 10h or 7d")
        })?;
    if digits.is_empty() || digits.len() > 8 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(SpiritError::invalid_input(
            field,
            "use a duration such as 30m, 10h or 7d",
        ));
    }
    Ok(())
}

fn ensure_app_name(app: &str) -> Result<(), SpiritError> {
    if app.is_empty()
        || app.len() > 64
        || app.starts_with('-')
        || !app
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        return Err(SpiritError::invalid_input(
            "surface app",
            "use 1-64 letters, digits, '.', '-' or '_'",
        ));
    }
    Ok(())
}

fn ensure_orchestrator_name(name: &str) -> Result<(), SpiritError> {
    if name.is_empty()
        || name.len() > 64
        || name.starts_with('-')
        || name == "."
        || name == ".."
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        return Err(SpiritError::invalid_input(
            "orchestrator name",
            "use 1-64 letters, digits, '.', '-' or '_'",
        ));
    }
    Ok(())
}

/// One row from `vowp surface ls --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceEntry {
    /// Surface id.
    pub id: String,
    /// Node that runs the surface.
    pub node: String,
    /// Kind of client.
    pub kind: SurfaceKind,
    /// Lifecycle state.
    pub state: SurfaceLifecycleState,
    /// Scopes granted to the surface token.
    #[serde(default)]
    pub scopes: Vec<String>,
    /// Display prefix of the newest active token; never the token.
    #[serde(default)]
    pub prefix: Option<String>,
    /// Expiry of the newest stored token.
    #[serde(default)]
    pub expires_at: Option<Timestamp>,
    /// Latest expiry among older tokens while a rotation overlaps.
    #[serde(default)]
    pub overlap_until: Option<Timestamp>,
    /// Whether VOWP reports the stored token as expired.
    #[serde(default)]
    pub expired: bool,
    /// Whether a rotation frame is still queued for the node.
    #[serde(default)]
    pub pending: bool,
    /// When a queued rotation was written.
    #[serde(default)]
    pub queued_at: Option<Timestamp>,
    /// Delivery deadline of a queued replacement token.
    #[serde(default)]
    pub delivery_deadline: Option<Timestamp>,
}

impl SurfaceEntry {
    /// Returns true when the surface is revoked.
    #[must_use]
    pub fn is_revoked(&self) -> bool {
        self.state == SurfaceLifecycleState::Revoked
    }

    /// Returns true when the surface token is pending or past its expiry.
    #[must_use]
    pub fn is_expired(&self, now: Timestamp) -> bool {
        self.expired || self.expires_at.is_some_and(|expiry| expiry <= now)
    }

    /// Returns true when this surface can present a token for a write.
    #[must_use]
    pub fn is_writable(&self, now: Timestamp) -> bool {
        self.state == SurfaceLifecycleState::Attached && !self.is_expired(now) && !self.pending
    }
}

/// A requested home surface pairing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfacePairRequest {
    /// Session the surface mirrors.
    pub session: String,
    /// Node that runs the surface.
    pub node: String,
    /// Surface kind (`opencode` or `chat`).
    pub kind: SurfaceKind,
    /// Application name carried into the token scopes.
    #[serde(default)]
    pub app: Option<String>,
    /// Existing surface to revoke and replace in one transaction.
    #[serde(default)]
    pub replace: Option<String>,
}

impl SurfacePairRequest {
    /// Validates every value before it becomes remote argv.
    pub fn validate(&self) -> Result<(), SpiritError> {
        SessionId::parse(&self.session)?;
        NodeName::parse(&self.node)?;
        match self.kind {
            SurfaceKind::Opencode | SurfaceKind::Chat => {}
            _ => {
                return Err(SpiritError::invalid_input(
                    "surface kind",
                    "use `opencode` or `chat`",
                ));
            }
        }
        if let Some(app) = &self.app {
            ensure_app_name(app)?;
        }
        if let Some(replace) = &self.replace {
            ensure_surface_id(replace)?;
        }
        Ok(())
    }
}

/// The raw `vowp surface pair --json` output.
///
/// The one-time `code` is a credential. The type is `Deserialize`-only and its `Debug`
/// redacts the code; only [`crate::vowp::SurfacePairing`] may hold it.
#[derive(Clone, PartialEq, Eq, Deserialize)]
pub struct SurfacePairOutput {
    /// Surface id.
    pub surface: String,
    /// Node the surface runs on.
    pub node: String,
    /// Session the surface mirrors.
    pub session: String,
    /// Surface kind.
    pub kind: SurfaceKind,
    /// Application name, when any.
    #[serde(default)]
    pub app: Option<String>,
    /// One-time pairing code, never logged.
    pub code: String,
    /// Pairing-code expiry.
    pub expires_at: Timestamp,
    /// Exact Mac-side `vowp agent pair` command (never contains the code).
    pub command: String,
    /// Surface revoked by `--replace`, when one was named.
    #[serde(default)]
    pub replaced: Option<String>,
}

impl fmt::Debug for SurfacePairOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SurfacePairOutput")
            .field("surface", &self.surface)
            .field("node", &self.node)
            .field("session", &self.session)
            .field("kind", &self.kind)
            .field("app", &self.app)
            .field("code", &"[redacted]")
            .field("expires_at", &self.expires_at)
            .field("command", &self.command)
            .field("replaced", &self.replaced)
            .finish()
    }
}

/// A requested surface token rotation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceRotateRequest {
    /// Surface id.
    pub surface: String,
    /// Node filter; the surface must belong to it when set.
    #[serde(default)]
    pub node: Option<String>,
    /// Poll the outbox for this duration.
    #[serde(default)]
    pub wait: Option<String>,
    /// Shorten the old token to now + grace.
    #[serde(default)]
    pub grace: Option<String>,
}

impl SurfaceRotateRequest {
    /// Validates every value before it becomes remote argv.
    pub fn validate(&self) -> Result<(), SpiritError> {
        ensure_surface_id(&self.surface)?;
        if let Some(node) = &self.node {
            NodeName::parse(node)?;
        }
        if let Some(wait) = &self.wait {
            ensure_duration_arg(wait, "--wait")?;
        }
        if let Some(grace) = &self.grace {
            ensure_duration_arg(grace, "--grace")?;
        }
        Ok(())
    }
}

/// The `vowp surface rotate --json` result. The token itself is never on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceRotateResult {
    /// Surface id.
    pub surface: String,
    /// Node that runs the surface.
    pub node: String,
    /// Display prefix of the replacement token.
    pub prefix: String,
    /// Replacement token expiry.
    pub expires_at: Timestamp,
    /// Old token expiry during the overlap.
    pub old_expires_at: Timestamp,
    /// Whether the replacement frame is still queued for the node.
    #[serde(default)]
    pub pending: bool,
    /// When the replacement frame was queued.
    #[serde(default)]
    pub queued_at: Option<Timestamp>,
}

/// The `vowp surface revoke --json` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceRevokeResult {
    /// Surface id.
    pub surface: String,
    /// Lifecycle state after the write (`revoked`).
    pub status: String,
    /// Number of tokens revoked.
    #[serde(default)]
    pub tokens_revoked: u64,
}

/// One local surface on the Mac, as reported by `vowp agent status --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSurfaceStatus {
    /// Surface id.
    pub surface: String,
    /// Display prefix of the stored token.
    #[serde(default)]
    pub prefix: Option<String>,
    /// Token expiry.
    #[serde(default)]
    pub expires_at: Option<Timestamp>,
    /// Local state (`paired`, `needs_pair` or `revoked`).
    pub state: AgentSurfaceState,
}

/// The Mac-side `vowp agent status --json` document.
///
/// The CLI also prints owner-only `paths`; that field is deliberately ignored so an
/// absolute Mac path can never reach a view or a log. The node key fingerprint is not
/// modelled because the D6 UI never displays it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentStatusDocument {
    /// Connection state.
    #[serde(default)]
    pub connection: AgentConnectionState,
    /// Node name, when known.
    #[serde(default)]
    pub node: Option<String>,
    /// Home identity, when known.
    #[serde(default)]
    pub home: Option<String>,
    /// Negotiated protocol.
    #[serde(default)]
    pub protocol: Option<String>,
    /// Last heartbeat.
    #[serde(default)]
    pub last_heartbeat: Option<Timestamp>,
    /// Last connection, transport or policy error.
    #[serde(default)]
    pub last_error: Option<String>,
    /// Bytes waiting in the local spool.
    #[serde(default)]
    pub pending_spool_bytes: u64,
    /// Earliest local surface-token expiry.
    #[serde(default)]
    pub surface_token_expires_at: Option<Timestamp>,
    /// Local read-cache entries.
    #[serde(default)]
    pub cache_entries: u64,
    /// Local read-cache bytes.
    #[serde(default)]
    pub cache_bytes: u64,
    /// Oldest local cache entry.
    #[serde(default)]
    pub cache_oldest_at: Option<Timestamp>,
    /// Local surface rows.
    #[serde(default)]
    pub surfaces: Vec<AgentSurfaceStatus>,
}

impl Default for AgentStatusDocument {
    fn default() -> Self {
        Self {
            connection: AgentConnectionState::parse("unknown"),
            node: None,
            home: None,
            protocol: None,
            last_heartbeat: None,
            last_error: None,
            pending_spool_bytes: 0,
            surface_token_expires_at: None,
            cache_entries: 0,
            cache_bytes: 0,
            cache_oldest_at: None,
            surfaces: Vec::new(),
        }
    }
}

impl AgentStatusDocument {
    /// Builds a status document for a local state that has no JSON document.
    #[must_use]
    pub fn with_connection(connection: AgentConnectionState) -> Self {
        Self {
            connection,
            ..Self::default()
        }
    }

    /// Returns true when the agent negotiated the required protocol.
    #[must_use]
    pub fn protocol_compatible(&self) -> bool {
        self.protocol.as_deref() == Some(AGENT_PROTOCOL)
    }
}

/// One Mac-local path-map row from `vowp agent paths ls --json`.
///
/// This is a Mac-local value: it is `Deserialize`-only, and its `Debug` output redacts
/// both absolute roots. Shared state must use [`PathRef`] instead.
#[derive(Clone, PartialEq, Eq, Deserialize)]
pub struct AgentPathMap {
    /// Project the map belongs to.
    pub project: ProjectName,
    /// Absolute project root on the Mac; never leaves the Mac.
    pub local_root: String,
    /// Absolute worktree root on the Mac, when separate.
    #[serde(default)]
    pub worktree_root: Option<String>,
    /// Allowed relative paths below the project root.
    #[serde(default)]
    pub allow: Vec<String>,
}

impl fmt::Debug for AgentPathMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentPathMap")
            .field("project", &self.project)
            .field("local_root", &"[local-path]")
            .field(
                "worktree_root",
                &self.worktree_root.as_ref().map(|_| "[local-path]"),
            )
            .field("allow", &self.allow)
            .finish()
    }
}

impl AgentPathMap {
    /// Validates the local shape before it is used for a D6 action.
    ///
    /// The Mac-side VOWP command performs the authoritative filesystem checks. This
    /// guard rejects an absolute/traversing shape that could never be valid and keeps a
    /// generic reason that names no local path.
    pub fn validate(&self) -> Result<(), SpiritError> {
        ProjectName::parse(self.project.as_str())?;
        if !is_absolute_local_path(&self.local_root) {
            return Err(SpiritError::invalid_input(
                "agent path map",
                "the project root must be an absolute local path",
            ));
        }
        if let Some(worktree_root) = &self.worktree_root {
            if !is_absolute_local_path(worktree_root) {
                return Err(SpiritError::invalid_input(
                    "agent path map",
                    "the worktree root must be an absolute local path",
                ));
            }
        }
        for allowed in &self.allow {
            if allowed.is_empty()
                || allowed.starts_with('/')
                || allowed.contains('\\')
                || allowed.split('/').any(|part| part == "..")
            {
                return Err(SpiritError::invalid_input(
                    "agent path map",
                    "allowed paths must be relative and must not escape the project",
                ));
            }
        }
        Ok(())
    }

    /// Returns the safe shared-state reference for this project.
    pub fn path_ref(&self) -> Result<PathRef, SpiritError> {
        PathRef::project(self.project.as_str())
    }
}

fn is_absolute_local_path(path: &str) -> bool {
    path.starts_with('/') && path.len() > 1 && !path.contains('\0')
}

/// The raw `vowp agent pair --json` result. The token itself is never printed and the
/// mode-0600 `token_path` is ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentPairOutput {
    /// Surface id.
    pub surface_id: String,
    /// Scopes granted to the token.
    #[serde(default)]
    pub scopes: Vec<String>,
    /// Token expiry.
    pub expires_at: Timestamp,
}

/// A requested `vowp agent paths add`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentPathAddRequest {
    /// Absolute local checkout path. It stays on the Mac.
    pub project_root: String,
    /// Project name override, when the checkout has no `vowp.toml`.
    #[serde(default)]
    pub name: Option<String>,
    /// Absolute local worktree root, when separate.
    #[serde(default)]
    pub worktree_root: Option<String>,
    /// Allowed relative paths below the project root.
    #[serde(default)]
    pub allow: Vec<String>,
}

impl fmt::Debug for AgentPathAddRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentPathAddRequest")
            .field("project_root", &"[local-path]")
            .field("name", &self.name)
            .field(
                "worktree_root",
                &self.worktree_root.as_ref().map(|_| "[local-path]"),
            )
            .field("allow", &self.allow)
            .finish()
    }
}

impl AgentPathAddRequest {
    /// Validates the request shape and returns the project name it maps.
    pub fn validate(&self) -> Result<ProjectName, SpiritError> {
        if !is_absolute_local_path(&self.project_root) {
            return Err(SpiritError::invalid_input(
                "project root",
                "must be an absolute local path",
            ));
        }
        if let Some(worktree_root) = &self.worktree_root {
            if !is_absolute_local_path(worktree_root) {
                return Err(SpiritError::invalid_input(
                    "worktree root",
                    "must be an absolute local path",
                ));
            }
        }
        for allowed in &self.allow {
            if allowed.is_empty()
                || allowed.starts_with('/')
                || allowed.contains('\\')
                || allowed.split('/').any(|part| part == "..")
            {
                return Err(SpiritError::invalid_input(
                    "allow",
                    "must be a relative path below the project root",
                ));
            }
        }
        match &self.name {
            Some(name) => ProjectName::parse(name),
            None => Err(SpiritError::invalid_input(
                "project name",
                "is required when the Mac checkout has no vowp.toml",
            )),
        }
    }
}

/// A requested `vowp orchestrator new`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrchestratorNewRequest {
    /// Node that runs the app; this is the local-node meaning of `--node`.
    pub node: String,
    /// App to run.
    pub app: OrchestratorApp,
    /// Project to bind, when any.
    #[serde(default)]
    pub project: Option<String>,
    /// Orchestrator name; VOWP defaults it from the project.
    #[serde(default)]
    pub name: Option<String>,
    /// Replace an orchestrator with the same name.
    #[serde(default)]
    pub force: bool,
}

impl OrchestratorNewRequest {
    /// Validates every value before it becomes remote argv.
    pub fn validate(&self) -> Result<(), SpiritError> {
        NodeName::parse(&self.node)?;
        if matches!(self.app, OrchestratorApp::Unknown(_)) {
            return Err(SpiritError::invalid_input(
                "orchestrator app",
                "use `opencode`, `claude-code` or `chat`",
            ));
        }
        if let Some(project) = &self.project {
            ProjectName::parse(project)?;
        }
        if let Some(name) = &self.name {
            ensure_orchestrator_name(name)?;
        }
        Ok(())
    }
}

/// One redacted `vowp node show --json` card. Capacity is not modelled because the D6
/// view does not invent a node capacity panel (`VOWP-CONTRACT.md` §11.3.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    #[serde(default)]
    pub labels: Vec<String>,
    /// Active runs on the node.
    #[serde(default)]
    pub runs: u32,
    /// Projects mapped on the node.
    #[serde(default)]
    pub mapped: Vec<ProjectName>,
    /// Agent binary version, when known.
    #[serde(default)]
    pub agent_version: Option<String>,
    /// Agent protocol, when known.
    #[serde(default)]
    pub protocol: Option<String>,
    /// Redacted last error.
    #[serde(default)]
    pub last_error: Option<String>,
}

impl NodeCard {
    /// Returns true for an active, online local-agent node with the required protocol.
    #[must_use]
    pub fn is_ready_local_agent(&self) -> bool {
        self.kind == NodeKind::Agent
            && self.status == NodeStatus::Active
            && self.presence == NodePresence::Online
            && self.protocol.as_deref() == Some(AGENT_PROTOCOL)
    }
}
