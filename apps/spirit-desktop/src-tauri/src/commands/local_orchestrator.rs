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

use std::path::Path;

use serde::{Deserialize, Serialize};
use spirit_core::engine::GateSnapshot;
use spirit_core::engine::chat::KnownOrchestratorSession;
use spirit_core::engine::gate::LocalOrchestratorGate;
use spirit_core::error::{SpiritError, SpiritErrorCode};
use spirit_core::time::Timestamp;
use spirit_core::view::{
    AgentPathView, LocalAgentView, LocalOrchestratorView, LocalSurfaceView, OrchestratorView,
};
use spirit_core::vowp::local::{
    agent_socket_path, agent_state_dir, bind_orchestrator_surface, find_paired_surface,
};
use spirit_core::vowp::{
    AgentPairRequest, AgentPairing, LocalAgentClient, OrchestratorMoveTarget, SurfacePairingView,
    VowpClient,
};
use spirit_core::wire::{
    AgentPathAddRequest, NodeCard, OrchestratorApp, OrchestratorCard, OrchestratorNewRequest,
    SurfaceEntry, SurfaceKind, SurfacePairRequest, SurfaceRevokeResult, SurfaceRotateRequest,
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
async fn current_local_gates(state: &AppState) -> GateSnapshot {
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

/// Moves an orchestrator to a node or back home after re-checking every D6 prerequisite.
#[tauri::command]
pub async fn local_orchestrator_move(
    state: State<'_, AppState>,
    name: String,
    target: OrchestratorMoveTarget,
) -> CommandResult<OrchestratorView> {
    let gates = set_current_gates(state.inner()).await;
    let client = state.client()?;
    let core = state.core().await;
    core.set_gates(gates);
    core.orchestrator_move(client.as_ref(), &name, target).await
}

/// Ensures the real VOWP universal orchestrator session for `project` on the connected VPS
/// and wires it to this Mac: the home record, a paired `opencode` surface, the local agent's
/// `orchestrators/<project>/config.json` binding, and a running app host.
///
/// Returns `Ok(None)` when no host or enrolled local node is connected, so the caller falls
/// back to the bundled engine without VOWP context. The returned session is passed straight
/// into `LocalChatProvider::provision_orchestrator_with`.
pub(crate) async fn ensure_project_orchestrator(
    state: &AppState,
    project: &str,
) -> Result<Option<KnownOrchestratorSession>, SpiritError> {
    let gates = current_local_gates(state).await;
    let LocalOrchestratorGate::Ready {
        node: Some(node), ..
    } = &gates.local_orchestrator
    else {
        return Ok(None);
    };
    let Ok(client) = state.client() else {
        return Ok(None);
    };
    let Some(state_dir) = agent_state_dir() else {
        return Ok(None);
    };
    let local = state.local_agent();
    ensure_on_client(
        client.as_ref(),
        local.as_ref(),
        &state_dir,
        &node.to_string(),
        project,
    )
    .await
    .map(Some)
}

/// Finds the home orchestrator card for `project`.
async fn find_card(
    client: &dyn VowpClient,
    project: &str,
) -> Result<Option<OrchestratorCard>, SpiritError> {
    let cards = client.orchestrator_list().await?;
    Ok(spirit_core::service::select_project_orchestrator(&cards, project).cloned())
}

/// Makes sure the project's orchestrator exists, owns a paired and bound surface, runs on
/// `node` and is live.
///
/// `vowp orchestrator new --node` persists the home record (with its fresh `session_id`)
/// before `vowp-agent` tries to start the app, and the agent refuses with `surface_missing`
/// or `surface_ambiguous` while no surface is paired for the new session. That refusal is
/// therefore recovered by re-reading the list; pairing, binding and `resume` follow.
async fn ensure_on_client(
    client: &dyn VowpClient,
    local: &dyn LocalAgentClient,
    state_dir: &Path,
    node: &str,
    project: &str,
) -> Result<KnownOrchestratorSession, SpiritError> {
    let mut card = match find_card(client, project).await? {
        Some(card) => card,
        None => {
            let request = OrchestratorNewRequest {
                node: node.to_owned(),
                app: OrchestratorApp::Opencode,
                project: Some(project.to_owned()),
                name: Some(project.to_owned()),
                force: false,
            };
            match client.orchestrator_new(request).await {
                Ok(card) => card,
                // The record exists even though the start was refused; recover it.
                Err(error) => find_card(client, project).await?.ok_or(error)?,
            }
        }
    };
    if card.state == "node_offline" {
        return Err(SpiritError::new(
            SpiritErrorCode::Engine,
            format!("the orchestrator node for {project} is offline"),
        ));
    }
    let session = card.session_id.to_string();
    let surface = ensure_paired_surface(client, local, state_dir, node, &session).await?;
    bind_orchestrator_surface(state_dir, &card.name, &session, &surface)?;
    if card.node.as_ref().map(ToString::to_string).as_deref() != Some(node) {
        card = client
            .orchestrator_move(
                &card.name,
                OrchestratorMoveTarget::Node {
                    node: node.to_owned(),
                },
            )
            .await?;
    }
    if !matches!(card.state.as_str(), "running" | "starting") {
        client.orchestrator_resume(&card.name).await?;
    }
    Ok(KnownOrchestratorSession {
        session_id: session,
        surface_id: Some(surface),
        agent_socket_path: Some(agent_socket_path(state_dir)),
    })
}

/// Returns the active `opencode` surface bound to `session` on this Mac, pairing one when
/// none exists: home `surface pair`, then the Mac's `agent pair` with the one-time code on
/// stdin. The code is never logged, stored or returned.
async fn ensure_paired_surface(
    client: &dyn VowpClient,
    local: &dyn LocalAgentClient,
    state_dir: &Path,
    node: &str,
    session: &str,
) -> Result<String, SpiritError> {
    if let Some(surface) = find_paired_surface(state_dir, session) {
        let listed = client.surface_list(Some(node)).await?;
        if listed.iter().any(|entry| {
            entry.id == surface && entry.kind == SurfaceKind::Opencode && !entry.is_revoked()
        }) {
            return Ok(surface);
        }
    }
    let pairing = client
        .surface_pair(SurfacePairRequest {
            session: session.to_owned(),
            node: node.to_owned(),
            kind: SurfaceKind::Opencode,
            app: Some("opencode".to_owned()),
            replace: None,
        })
        .await?;
    let request = AgentPairRequest::new(
        pairing.code().clone(),
        pairing.surface.clone(),
        pairing.session.clone(),
        Some("opencode".to_owned()),
    );
    local.pair(&request).await?;
    Ok(pairing.surface)
}

#[cfg(test)]
mod ensure_tests {
    use super::*;
    use spirit_core::fixtures::{Scenario, capacity_report, fleet_status};
    use spirit_core::vowp::{MockLocalAgentClient, MockVowpClient};
    use spirit_core::wire::{AgentPairOutput, OrchestratorCard, SurfacePairOutput};

    const SESSION: &str = "01JCF5WQ8ZK3M2V7N4P6R8T0AE";
    const SURFACE: &str = "01JCF5WQ8ZK3M2V7N4P6R8T0AF";
    const CODE: &str = "one-time-code-never-logged";

    /// A private scratch agent-state directory removed on drop (the desktop crate has no
    /// `tempfile` dev-dependency).
    struct TempState(std::path::PathBuf);

    impl TempState {
        fn new() -> Self {
            static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let dir = std::env::temp_dir().join(format!(
                "spirit-ensure-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempState {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn mock(cards: Vec<OrchestratorCard>) -> MockVowpClient {
        MockVowpClient::new(
            fleet_status(Scenario::Idle).expect("status"),
            capacity_report(Scenario::Idle).expect("capacity"),
        )
        .with_orchestrators(cards)
        .with_surface_pairing(pairing_output())
    }

    fn pairing_output() -> SurfacePairOutput {
        serde_json::from_value(serde_json::json!({
            "surface": SURFACE, "node": "mac", "session": SESSION, "kind": "opencode",
            "app": "opencode", "code": CODE,
            "expires_at": "2026-12-01T00:00:00.000Z",
            "command": "vowp agent pair --code-file -",
        }))
        .unwrap()
    }

    fn agent() -> MockLocalAgentClient {
        MockLocalAgentClient::connected().with_pair(
            serde_json::from_value::<AgentPairOutput>(serde_json::json!({
                "surface_id": SURFACE,
                "scopes": [],
                "expires_at": "2026-12-01T00:00:00.000Z",
            }))
            .unwrap(),
        )
    }

    fn card(state: &str, node: Option<&str>) -> OrchestratorCard {
        serde_json::from_value(serde_json::json!({
            "name": "stackspirit",
            "session_id": SESSION,
            "project": "stackspirit",
            "app": "opencode",
            "state": state,
            "node": node,
            "join": {"desktop": "d", "phone": "p", "chat": "c"}
        }))
        .unwrap()
    }

    fn mark_paired(state_dir: &Path) {
        let dir = state_dir.join("surfaces").join(SURFACE);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("session"), format!("{SESSION}\n")).unwrap();
    }

    fn listed_surface() -> spirit_core::wire::SurfaceEntry {
        serde_json::from_value(serde_json::json!({
            "id": SURFACE, "node": "mac", "kind": "opencode", "state": "attached",
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn creates_pairs_binds_and_resumes_a_missing_orchestrator() {
        let state = TempState::new();
        let client = mock(Vec::new()).with_orchestrator_new_result(card("failed", Some("mac")));
        let local = agent();
        let known = ensure_on_client(&client, &local, state.path(), "mac", "stackspirit")
            .await
            .unwrap();
        let calls = client.orchestrator_new_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].node, "mac");
        assert_eq!(calls[0].project.as_deref(), Some("stackspirit"));
        assert!(!calls[0].force);

        let pairs = client.surface_pair_calls();
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].session, SESSION);
        assert_eq!(pairs[0].kind, SurfaceKind::Opencode);
        let agent_pairs = local.pair_calls();
        assert_eq!(agent_pairs.len(), 1);
        assert_eq!(agent_pairs[0].surface, SURFACE);
        assert_eq!(agent_pairs[0].session, SESSION);
        assert_eq!(agent_pairs[0].app.as_deref(), Some("opencode"));
        // The code only travels in the request object (stdin on the real client).
        assert!(!format!("{agent_pairs:?}").contains(CODE));

        let config = state.path().join("orchestrators/stackspirit/config.json");
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
        assert_eq!(value["surface"], SURFACE);
        assert_eq!(value["session"], SESSION);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&config).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        assert_eq!(known.session_id, SESSION);
        assert_eq!(known.surface_id.as_deref(), Some(SURFACE));
        assert_eq!(
            known.agent_socket_path.as_deref(),
            Some(state.path().join("agent.sock").as_path())
        );
        assert!(known.mcp_command().is_some());
        assert_eq!(client.orchestrator_resume_calls(), vec!["stackspirit"]);
    }

    #[tokio::test]
    async fn recovers_the_record_after_a_surface_missing_refusal() {
        for code in ["surface_missing", "surface_ambiguous"] {
            let state = TempState::new();
            let client = mock(Vec::new())
                .with_orchestrator_new_result(card("failed", Some("mac")))
                .with_orchestrator_new_refusal(SpiritError::new(
                    SpiritErrorCode::Engine,
                    format!("{code}: pair a surface for this orchestrator"),
                ));
            let known = ensure_on_client(&client, &agent(), state.path(), "mac", "stackspirit")
                .await
                .unwrap();
            assert_eq!(known.session_id, SESSION, "{code}");
            assert_eq!(client.orchestrator_new_calls().len(), 1);
            assert_eq!(client.surface_pair_calls().len(), 1);
            assert_eq!(client.orchestrator_resume_calls(), vec!["stackspirit"]);
        }
    }

    #[tokio::test]
    async fn a_refusal_without_a_persisted_record_is_reported() {
        let state = TempState::new();
        let client = mock(Vec::new()).with_orchestrator_new_refusal(SpiritError::new(
            SpiritErrorCode::Engine,
            "vowp orchestrator new failed",
        ));
        let err = ensure_on_client(&client, &agent(), state.path(), "mac", "stackspirit")
            .await
            .unwrap_err();
        assert!(err.message.contains("orchestrator new failed"), "{err:?}");
    }

    #[tokio::test]
    async fn reuses_an_active_paired_surface_without_pairing_again() {
        let state = TempState::new();
        mark_paired(state.path());
        let client = mock(vec![card("running", Some("mac"))]).with_surfaces(vec![listed_surface()]);
        let local = agent();
        let known = ensure_on_client(&client, &local, state.path(), "mac", "stackspirit")
            .await
            .unwrap();
        assert!(client.orchestrator_new_calls().is_empty());
        assert!(client.surface_pair_calls().is_empty());
        assert!(local.pair_calls().is_empty());
        assert!(client.orchestrator_resume_calls().is_empty());
        assert!(client.orchestrator_move_calls().is_empty());
        assert_eq!(known.surface_id.as_deref(), Some(SURFACE));
        // The binding is rewritten so a second paired surface (`global`) never makes the
        // agent report `surface_ambiguous`.
        assert!(
            state
                .path()
                .join("orchestrators/stackspirit/config.json")
                .exists()
        );
    }

    #[tokio::test]
    async fn pairs_again_when_home_no_longer_lists_the_surface() {
        let state = TempState::new();
        mark_paired(state.path());
        let client = mock(vec![card("running", Some("mac"))]);
        let local = agent();
        ensure_on_client(&client, &local, state.path(), "mac", "stackspirit")
            .await
            .unwrap();
        assert_eq!(client.surface_pair_calls().len(), 1);
        assert_eq!(local.pair_calls().len(), 1);
    }

    #[tokio::test]
    async fn resumes_a_stopped_or_failed_orchestrator() {
        for state_name in ["stopped", "failed", "degraded"] {
            let state = TempState::new();
            mark_paired(state.path());
            let client =
                mock(vec![card(state_name, Some("mac"))]).with_surfaces(vec![listed_surface()]);
            ensure_on_client(&client, &agent(), state.path(), "mac", "stackspirit")
                .await
                .unwrap();
            assert_eq!(client.orchestrator_resume_calls(), vec!["stackspirit"]);
            assert!(client.orchestrator_new_calls().is_empty());
        }
    }

    #[tokio::test]
    async fn moves_an_orchestrator_on_another_node() {
        let state = TempState::new();
        mark_paired(state.path());
        let client =
            mock(vec![card("running", Some("other"))]).with_surfaces(vec![listed_surface()]);
        ensure_on_client(&client, &agent(), state.path(), "mac", "stackspirit")
            .await
            .unwrap();
        let moves = client.orchestrator_move_calls();
        assert_eq!(moves.len(), 1);
        assert!(matches!(&moves[0].1, OrchestratorMoveTarget::Node { node } if node == "mac"));
    }

    #[tokio::test]
    async fn resuming_a_non_canonical_card_binds_the_surface_by_its_own_name() {
        let state = TempState::new();
        mark_paired(state.path());
        let mut stopped = card("stopped", Some("mac"));
        stopped.name = "local-stackspirit".to_owned();
        let client = mock(vec![stopped]).with_surfaces(vec![listed_surface()]);
        ensure_on_client(&client, &agent(), state.path(), "mac", "stackspirit")
            .await
            .unwrap();
        assert!(client.orchestrator_new_calls().is_empty());
        assert_eq!(
            client.orchestrator_resume_calls(),
            vec!["local-stackspirit"]
        );
        assert!(
            state
                .path()
                .join("orchestrators/local-stackspirit/config.json")
                .exists()
        );
        assert!(!state.path().join("orchestrators/stackspirit").exists());
    }

    #[tokio::test]
    async fn reports_an_offline_node() {
        let state = TempState::new();
        let client = mock(vec![card("node_offline", Some("mac"))]);
        assert!(
            ensure_on_client(&client, &agent(), state.path(), "mac", "stackspirit")
                .await
                .is_err()
        );
    }
}
