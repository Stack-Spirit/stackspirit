//! D6 local-orchestrator command adapters.
//!
//! Every rule lives in `spirit-core`: the backend guard re-checks the current gate,
//! the connected Mac agent, the validated node and the project path map before any
//! VOWP command runs. This module only:
//!
//! - resolves the gate snapshot that belongs to the current host connection,
//! - maps renderer inputs to validated core requests,
//! - runs the home-side command through the connected [`VowpClient`] and the Mac-side
//!   command through the local [`LocalAgentClient`], and
//! - returns safe view models. Absolute Mac paths, surface tokens, token files and
//!   key fingerprints never cross this boundary.
//!
//! The one-time surface pairing code is the only credential returned to the renderer,
//! and only on the direct `local_surface_pair` result so the owner can hand it to the
//! Mac agent. It is never logged, stored in a view, or included in an event.

use serde::{Deserialize, Serialize};
use spirit_core::engine::GateSnapshot;
use spirit_core::engine::gate::LocalOrchestratorGate;
use spirit_core::time::Timestamp;
use spirit_core::view::{
    AgentPathView, LocalAgentView, LocalOrchestratorView, LocalSurfaceView, OrchestratorView,
};
use spirit_core::vowp::{AgentPairRequest, AgentPairing, SurfacePairingView};
use spirit_core::wire::{
    AgentPathAddRequest, NodeCard, OrchestratorApp, OrchestratorNewRequest, SurfaceEntry,
    SurfaceKind, SurfacePairRequest, SurfaceRevokeResult, SurfaceRotateRequest,
    SurfaceRotateResult,
};
use tauri::State;

use crate::error::CommandResult;
use crate::state::{AppState, default_gate_snapshot, with_build_gate_features};

/// One safe local-orchestrator read model for the Engine view.
#[derive(Debug, Clone, Serialize)]
pub struct LocalOrchestratorOverview {
    /// Safe Mac agent status, including the local cache counters.
    pub agent: LocalAgentView,
    /// The active local-agent node card, when the gate validated one.
    pub node: Option<NodeCard>,
    /// Home surfaces projected with node, agent, lease and session state.
    pub surfaces: Vec<LocalSurfaceView>,
    /// Local orchestrators projected with node, agent, lease and session state.
    pub orchestrators: Vec<LocalOrchestratorView>,
    /// Safe Mac path-map rows; absolute roots are deliberately absent.
    pub paths: Vec<AgentPathView>,
    /// Path-free reason when the Mac path map could not be read.
    pub paths_error: Option<String>,
}

/// A home surface pairing plus the one-time code the Mac agent redeems.
#[derive(Serialize)]
pub struct SurfacePairResult {
    /// Safe pairing fields; the code is not part of the view.
    pub pairing: SurfacePairingView,
    /// The one-time pairing code. The renderer must not log or persist it.
    pub code: String,
}

impl std::fmt::Debug for SurfacePairResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SurfacePairResult")
            .field("pairing", &self.pairing)
            .field("code", &"[redacted]")
            .finish()
    }
}

/// A requested home surface pairing as the renderer sends it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SurfacePairInput {
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

impl SurfacePairInput {
    fn into_request(self) -> SurfacePairRequest {
        SurfacePairRequest {
            session: self.session,
            node: self.node,
            kind: self.kind,
            app: self.app,
            replace: self.replace,
        }
    }
}

/// A requested surface token rotation.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SurfaceRotateInput {
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

impl SurfaceRotateInput {
    fn into_request(self) -> SurfaceRotateRequest {
        SurfaceRotateRequest {
            surface: self.surface,
            node: self.node,
            wait: self.wait,
            grace: self.grace,
        }
    }
}

/// A requested `vowp orchestrator new` app host.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct OrchestratorNewInput {
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

impl OrchestratorNewInput {
    fn into_request(self) -> OrchestratorNewRequest {
        OrchestratorNewRequest {
            node: self.node,
            app: self.app,
            project: self.project,
            name: self.name,
            force: self.force,
        }
    }
}

/// A requested Mac-side surface pairing.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentPairInput {
    /// One-time pairing code from `local_surface_pair`.
    pub code: String,
    /// Surface id printed by the home pairing.
    pub surface: String,
    /// Session id printed by the home pairing.
    pub session: String,
    /// Application name.
    #[serde(default)]
    pub app: Option<String>,
}

impl std::fmt::Debug for AgentPairInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentPairInput")
            .field("code", &"[redacted]")
            .field("surface", &self.surface)
            .field("session", &self.session)
            .field("app", &self.app)
            .finish()
    }
}

impl AgentPairInput {
    fn into_request(self) -> AgentPairRequest {
        AgentPairRequest::new(
            spirit_core::engine::SecretString::new(self.code),
            self.surface,
            self.session,
            self.app,
        )
    }
}

/// A requested Mac path-map row.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentPathAddInput {
    /// Absolute local checkout path. It stays on the Mac.
    pub project_root: String,
    /// Project name override.
    pub name: String,
    /// Absolute local worktree root, when separate.
    #[serde(default)]
    pub worktree_root: Option<String>,
    /// Allowed relative paths below the project root.
    #[serde(default)]
    pub allow: Vec<String>,
}

impl std::fmt::Debug for AgentPathAddInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentPathAddInput")
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

impl AgentPathAddInput {
    fn into_request(self) -> AgentPathAddRequest {
        AgentPathAddRequest {
            project_root: self.project_root,
            name: Some(self.name),
            worktree_root: self.worktree_root,
            allow: self.allow,
        }
    }
}

/// Resolves the gate snapshot for the current connection and applies the build flags.
///
/// A missing or expired cache entry is the honest "unknown" default, never a remembered
/// `Ready` from another connection. The D6 action flag is derived from the three-part
/// rule so a stale renderer cannot advertise an enabled action.
pub(crate) async fn current_local_gates(state: &AppState) -> GateSnapshot {
    let connection = state.connection_manager().await.status();
    if connection.state != "connected" {
        return default_gate_snapshot();
    }
    let Some(host_id) = connection.host_id else {
        return default_gate_snapshot();
    };
    let mut snapshot = state
        .gate_cache()
        .get(host_id.as_str(), Timestamp::now())
        .map(with_build_gate_features)
        .unwrap_or_else(default_gate_snapshot);
    snapshot.enforce_local_orchestrator_flag();
    snapshot
}

async fn set_current_gates(state: &AppState) -> GateSnapshot {
    let snapshot = current_local_gates(state).await;
    state.core().await.set_gates(snapshot.clone());
    snapshot
}

/// Keeps a path-map read error short and free of local paths.
fn safe_paths_error(message: &str) -> String {
    let redacted = spirit_core::error::redact(message);
    redacted
        .split_whitespace()
        .map(|word| {
            if word.starts_with('/')
                || word.starts_with("~/")
                || word.contains("known_hosts")
                || word.contains(".vowp-agent")
                || word.contains("id_rsa")
                || word.contains("id_ed25519")
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

/// Returns safe local state plus the honest surface/orchestrator cards.
///
/// The read stays available while the gate is `Waiting` so the owner can see exactly
/// which prerequisite is missing and which local surfaces still exist.
#[tauri::command]
pub async fn local_orchestrator_overview(
    state: State<'_, AppState>,
) -> CommandResult<LocalOrchestratorOverview> {
    let gates = set_current_gates(state.inner()).await;
    let client = state.client()?;
    let local = state.local_agent();
    let core = state.core().await;

    let agent = core.agent_status(local.as_ref()).await?;

    let mut node = None;
    if let LocalOrchestratorGate::Ready {
        node: Some(name), ..
    } = &gates.local_orchestrator
    {
        let project = core.active_project();
        match core
            .refresh_local_node(client.as_ref(), project.as_deref(), name)
            .await
        {
            Ok(card) => node = Some(card),
            Err(_) => core.clear_local_node_card(),
        }
    } else {
        core.clear_local_node_card();
    }

    // Never authorize an orchestration from a path map read through another
    // connection or an older overview.
    core.set_local_agent_paths(&[]);
    let (paths, paths_error) = match core.agent_paths(local.as_ref()).await {
        Ok(paths) => (paths, None),
        Err(error) => (Vec::new(), Some(safe_paths_error(&error.message))),
    };

    let surfaces = core
        .local_surface_views(client.as_ref(), local.as_ref())
        .await?;
    let orchestrators = core
        .local_orchestrator_views(client.as_ref(), local.as_ref())
        .await?;

    Ok(LocalOrchestratorOverview {
        agent,
        node,
        surfaces,
        orchestrators,
        paths,
        paths_error,
    })
}

/// Reads the Mac agent status. This is a read-only action and works while the gate is
/// `Waiting`.
#[tauri::command]
pub async fn local_agent_status(state: State<'_, AppState>) -> CommandResult<LocalAgentView> {
    let local = state.local_agent();
    let core = state.core().await;
    core.agent_status(local.as_ref()).await
}

/// Redeems a home surface pairing code on the Mac.
#[tauri::command]
pub async fn local_agent_pair(
    state: State<'_, AppState>,
    input: AgentPairInput,
) -> CommandResult<AgentPairing> {
    let gates = set_current_gates(state.inner()).await;
    let local = state.local_agent();
    let core = state.core().await;
    core.set_gates(gates);
    core.agent_pair(local.as_ref(), input.into_request()).await
}

/// Reads the Mac path map and records its validity for the action guard.
#[tauri::command]
pub async fn local_agent_paths(state: State<'_, AppState>) -> CommandResult<Vec<AgentPathView>> {
    let local = state.local_agent();
    let core = state.core().await;
    core.agent_paths(local.as_ref()).await
}

/// Adds or updates one Mac path-map row.
#[tauri::command]
pub async fn local_agent_path_add(
    state: State<'_, AppState>,
    input: AgentPathAddInput,
) -> CommandResult<AgentPathView> {
    let local = state.local_agent();
    let core = state.core().await;
    core.agent_path_add(local.as_ref(), input.into_request())
        .await
}

/// Removes one Mac path-map row.
#[tauri::command]
pub async fn local_agent_path_remove(
    state: State<'_, AppState>,
    project: String,
) -> CommandResult<()> {
    let local = state.local_agent();
    let core = state.core().await;
    core.agent_path_remove(local.as_ref(), &project).await
}

/// Creates a pending home surface with a one-time pairing code.
#[tauri::command]
pub async fn local_surface_pair(
    state: State<'_, AppState>,
    input: SurfacePairInput,
) -> CommandResult<SurfacePairResult> {
    let gates = set_current_gates(state.inner()).await;
    let client = state.client()?;
    let core = state.core().await;
    core.set_gates(gates);
    let pairing = core
        .surface_pair(client.as_ref(), input.into_request())
        .await?;
    Ok(SurfacePairResult {
        pairing: pairing.view(),
        code: pairing.code().expose().to_owned(),
    })
}

/// Lists home surfaces. The read stays available while the gate is `Waiting`.
#[tauri::command]
pub async fn local_surface_list(
    state: State<'_, AppState>,
    node: Option<String>,
) -> CommandResult<Vec<SurfaceEntry>> {
    let gates = set_current_gates(state.inner()).await;
    let client = state.client()?;
    let core = state.core().await;
    core.set_gates(gates);
    core.surface_list(client.as_ref(), node.as_deref()).await
}

/// Rotates one surface token. The old token is only valid for VOWP's overlap.
#[tauri::command]
pub async fn local_surface_rotate(
    state: State<'_, AppState>,
    input: SurfaceRotateInput,
) -> CommandResult<SurfaceRotateResult> {
    let gates = set_current_gates(state.inner()).await;
    let client = state.client()?;
    let core = state.core().await;
    core.set_gates(gates);
    core.surface_rotate(client.as_ref(), input.into_request())
        .await
}

/// Revokes one surface and every token it holds. This explicit cleanup remains
/// available while the gate is `Waiting` or the node is revoked.
#[tauri::command]
pub async fn local_surface_revoke(
    state: State<'_, AppState>,
    surface: String,
) -> CommandResult<SurfaceRevokeResult> {
    let gates = set_current_gates(state.inner()).await;
    let client = state.client()?;
    let core = state.core().await;
    core.set_gates(gates);
    core.surface_revoke(client.as_ref(), &surface).await
}

/// Creates a local app host through `vowp orchestrator new --node ...`.
#[tauri::command]
pub async fn local_orchestrator_new(
    state: State<'_, AppState>,
    input: OrchestratorNewInput,
) -> CommandResult<OrchestratorView> {
    let gates = set_current_gates(state.inner()).await;
    let client = state.client()?;
    let core = state.core().await;
    core.set_gates(gates);
    core.orchestrator_new(client.as_ref(), input.into_request())
        .await
}

/// Lists orchestrators. The read stays available while the gate is `Waiting`.
#[tauri::command]
pub async fn local_orchestrator_list(
    state: State<'_, AppState>,
) -> CommandResult<Vec<OrchestratorView>> {
    let gates = set_current_gates(state.inner()).await;
    let client = state.client()?;
    let core = state.core().await;
    core.set_gates(gates);
    core.orchestrator_list(client.as_ref()).await
}

/// Stops a local app host. This explicit cleanup remains available while the gate is
/// `Waiting` or the node is offline.
#[tauri::command]
pub async fn local_orchestrator_stop(
    state: State<'_, AppState>,
    name: String,
) -> CommandResult<OrchestratorView> {
    let gates = set_current_gates(state.inner()).await;
    let client = state.client()?;
    let core = state.core().await;
    core.set_gates(gates);
    core.orchestrator_stop(client.as_ref(), &name).await
}

/// Resumes a local app host after re-checking every D6 prerequisite.
#[tauri::command]
pub async fn local_orchestrator_resume(
    state: State<'_, AppState>,
    name: String,
    project: Option<String>,
    node: Option<String>,
) -> CommandResult<OrchestratorView> {
    let gates = set_current_gates(state.inner()).await;
    let client = state.client()?;
    let core = state.core().await;
    core.set_gates(gates);
    core.orchestrator_resume(client.as_ref(), &name, project.as_deref(), node.as_deref())
        .await
}
