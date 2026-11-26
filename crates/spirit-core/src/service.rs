//! The headless `SpiritCore` facade used by the Tauri host and, later, the CLI.
//!
//! This module has no Tauri, React or platform-GUI dependency.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::connection::{ConnectionManager, ConnectionSnapshot, PollPolicy, SpiritEvent};
use crate::engine::gate::GateSnapshot;
use crate::engine::local::{
    LocalNodeState, LocalOrchestratorAction, LocalOrchestratorGuard, LocalOrchestratorState,
};
use crate::engine::{
    EngineChoice, EngineStatus, EngineSupervisor, Keychain, KeychainError, ModelChoice,
    SecretString,
};
use crate::error::{SpiritError, SpiritErrorCode};
use crate::host::{HostId, HostKeyPin, HostProfile, HostProfilePatch, HostStore, NewHostProfile};
use crate::time::{Clock, SystemClock, Timestamp};
use crate::view::local::{AgentPathView, LocalAgentView, LocalOrchestratorView, LocalSurfaceView};
use crate::view::{FleetView, OrchestratorView};
use crate::vowp::{
    AgentPairRequest, AgentPairing, AsifExport, AsifExportOptions, AsifImportOptions,
    LocalAgentClient, OrchestratorMoveTarget, ProjectAddOutcome, ProjectAddRequest, SurfacePairing,
    VowpClient,
};
use crate::wire::{
    AgentPathAddRequest, AgentPathMap, AsifImportResponse, NodeCard, NodeName, OrchestratorApp,
    OrchestratorCard, OrchestratorNewRequest, ProjectName, SurfaceEntry, SurfacePairRequest,
    SurfaceRevokeResult, SurfaceRotateRequest, SurfaceRotateResult, ensure_surface_id,
};

/// A provider-key state returned to the UI. It never carries the key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyStatus {
    /// Provider name.
    pub provider: String,
    /// `missing`, `saved`, `invalid`, or `unverified`.
    pub state: String,
    /// When the state was last checked.
    pub last_checked_at: Option<Timestamp>,
}

/// The application facade: hosts, connection, engine and gates in one headless object.
pub struct SpiritCore {
    hosts: Arc<dyn HostStore>,
    connection: ConnectionManager,
    engine: EngineSupervisor,
    keychain: Arc<dyn Keychain>,
    clock: Arc<dyn Clock>,
    gates: Mutex<GateSnapshot>,
    local_orchestrator: Mutex<LocalOrchestratorState>,
}

impl std::fmt::Debug for SpiritCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpiritCore")
            .field("connection", &self.connection.status())
            .field("engine", &self.engine.status())
            .finish_non_exhaustive()
    }
}

impl SpiritCore {
    /// Builds the facade from a host store and engine supervisor.
    #[must_use]
    pub fn new(hosts: Arc<dyn HostStore>, engine: EngineSupervisor) -> Self {
        Self {
            hosts,
            connection: ConnectionManager::new(PollPolicy::default()),
            engine,
            keychain: Arc::new(crate::engine::FakeKeychain::new()),
            clock: Arc::new(SystemClock),
            gates: Mutex::new(GateSnapshot::default()),
            local_orchestrator: Mutex::new(LocalOrchestratorState::default()),
        }
    }

    /// Replaces the keychain implementation.
    #[must_use]
    pub fn with_keychain(mut self, keychain: Arc<dyn Keychain>) -> Self {
        self.keychain = keychain;
        self
    }

    /// Builds a desktop facade backed by the real OS keychain.
    #[cfg(feature = "keychain")]
    #[must_use]
    pub fn desktop(hosts: Arc<dyn HostStore>, engine: EngineSupervisor, service: &str) -> Self {
        Self::new(hosts, engine)
            .with_keychain(Arc::new(crate::engine::keychain::OsKeychain::new(service)))
    }

    /// Replaces the clock (tests).
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Returns the connection manager.
    #[must_use]
    pub fn connection(&self) -> &ConnectionManager {
        &self.connection
    }

    /// Returns the host store.
    #[must_use]
    pub fn host_store(&self) -> Arc<dyn HostStore> {
        self.hosts.clone()
    }

    /// Lists saved hosts.
    pub fn list_hosts(&self) -> Result<Vec<HostProfile>, SpiritError> {
        self.hosts.list()
    }

    /// Adds a host profile with its confirmed pin.
    pub fn add_host(&self, new: NewHostProfile) -> Result<HostProfile, SpiritError> {
        self.hosts.add(new)
    }

    /// Updates a host.
    pub fn update_host(
        &self,
        id: &HostId,
        patch: HostProfilePatch,
    ) -> Result<HostProfile, SpiritError> {
        self.hosts.update(id, patch)
    }

    /// Removes a host.
    pub fn remove_host(&self, id: &HostId) -> Result<(), SpiritError> {
        self.hosts.remove(id)
    }

    /// Re-pins a host key after explicit verification.
    pub fn repin_host(&self, id: &HostId, pin: HostKeyPin) -> Result<HostProfile, SpiritError> {
        self.hosts.repin(id, pin)
    }

    /// Connects one host through the supplied client.
    pub async fn connect(
        &self,
        host_id: HostId,
        client: &dyn VowpClient,
    ) -> Result<ConnectionSnapshot, SpiritError> {
        let snapshot = self.connection.connect(client, host_id.clone()).await?;
        let _ = self.hosts.mark_connected(&host_id, self.clock.now());
        Ok(snapshot)
    }

    /// Disconnects and cancels polls.
    pub fn disconnect(&self) -> ConnectionSnapshot {
        self.connection.disconnect()
    }

    /// Returns the current connection status.
    #[must_use]
    pub fn connection_status(&self) -> ConnectionSnapshot {
        self.connection.status()
    }

    /// Returns the last good fleet view.
    #[must_use]
    pub fn fleet_view(&self) -> Option<FleetView> {
        self.connection.fleet_view()
    }

    /// Returns the first active project in the current snapshot.
    ///
    /// ASIF export and import are project-scoped; when the caller does not name a
    /// project, this is the one the facade uses.
    #[must_use]
    pub fn active_project(&self) -> Option<String> {
        let view = self.fleet_view()?;
        view.projects
            .into_iter()
            .find(|project| project.state == "active")
            .map(|project| project.name)
    }

    /// Returns true only when a mutation is safe to send.
    #[must_use]
    pub fn mutations_allowed(&self) -> bool {
        self.connection.mutations_allowed()
    }

    /// Polls one fleet snapshot.
    pub async fn refresh_fleet(
        &self,
        client: &dyn VowpClient,
    ) -> Result<Option<FleetView>, SpiritError> {
        self.connection.poll_once(client).await
    }

    /// Refreshes capacity detail.
    pub async fn refresh_capacity(
        &self,
        client: &dyn VowpClient,
    ) -> Result<Option<FleetView>, SpiritError> {
        self.connection.refresh_capacity(client).await
    }

    /// Exports one session as a capped `.asif.zip`.
    ///
    /// `project` is the active project by default. The result contains the safe summary
    /// and capped package bytes; it never contains a remote path or a remote file name.
    /// Callers check the ASIF gate and `features.asif_actions` before enabling the UI
    /// action; this method does not run a command when the client itself refuses.
    pub async fn export_asif(
        &self,
        client: &dyn VowpClient,
        project: Option<&str>,
        session: &str,
        options: AsifExportOptions,
    ) -> Result<AsifExport, SpiritError> {
        let project = self.resolve_asif_project(project)?;
        client.asif_export(&project, session, options).await
    }

    /// Imports one local package as a new idle session.
    ///
    /// The current snapshot must not be stale: an import is a write. The returned
    /// response is checked to be idle, and this facade never starts a turn, writes a
    /// capsule, creates a lease or dispatches a job after an import.
    pub async fn import_asif(
        &self,
        client: &dyn VowpClient,
        project: Option<&str>,
        package: &[u8],
        options: AsifImportOptions,
    ) -> Result<AsifImportResponse, SpiritError> {
        if !self.mutations_allowed() {
            return Err(SpiritError::new(
                SpiritErrorCode::Conflict,
                "the fleet view is stale, so an ASIF import would be unsafe",
            )
            .with_hint("Refresh the fleet view, then retry the import."));
        }
        let project = self.resolve_asif_project(project)?;
        let response = client.asif_import(&project, package, options).await?;
        response.ensure_idle()?;
        Ok(response)
    }

    /// Resolves an explicit project or the first active project in the snapshot.
    fn resolve_asif_project(&self, project: Option<&str>) -> Result<String, SpiritError> {
        if self.fleet_view().is_none() {
            return Err(
                SpiritError::new(SpiritErrorCode::NotFound, "no host is connected")
                    .with_hint("Connect a host before exporting or importing an ASIF package."),
            );
        }
        if let Some(project) = project {
            ProjectName::parse(project)?;
            return Ok(project.to_owned());
        }
        self.active_project().ok_or_else(|| {
            SpiritError::new(
                SpiritErrorCode::NotFound,
                "there is no active project in the current snapshot",
            )
            .with_hint("Refresh the fleet view or select a project explicitly.")
        })
    }

    /// Subscribes to state events.
    #[must_use]
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<SpiritEvent> {
        self.connection.subscribe()
    }

    /// Returns engine status.
    #[must_use]
    pub fn engine_status(&self) -> EngineStatus {
        self.engine.status()
    }

    /// Starts the engine.
    pub async fn start_engine(&mut self) -> EngineStatus {
        let status = self.engine.start().await;
        self.connection.publish_engine(status.clone());
        status
    }

    /// Restarts the engine within its budget.
    pub async fn restart_engine(&mut self) -> EngineStatus {
        let status = self.engine.restart().await;
        self.connection.publish_engine(status.clone());
        status
    }

    /// Stops the engine.
    pub async fn stop_engine(&mut self) -> EngineStatus {
        let status = self.engine.stop().await;
        self.connection.publish_engine(status.clone());
        status
    }

    /// Selects a model locally.
    pub fn select_model(&mut self, model: ModelChoice) {
        self.engine.select_model(model);
        self.connection.publish_engine(self.engine.status());
    }

    /// Selects the engine implementation.
    pub fn select_engine(&mut self, choice: EngineChoice) {
        self.engine.set_choice(choice);
        self.connection.publish_engine(self.engine.status());
    }

    /// Stores a provider key and refreshes the live engine's models and providers.
    ///
    /// This is the production connect path: the key goes only into the core's
    /// keychain, a live bundled sidecar restarts with the new ephemeral auth document,
    /// and the refreshed catalog is published. The value is never returned.
    pub async fn connect_provider(
        &mut self,
        provider: &str,
        secret: &SecretString,
    ) -> Result<KeyStatus, KeychainError> {
        let status = self
            .engine
            .connect_provider(self.keychain.as_ref(), provider, secret)
            .await?;
        self.connection.publish_engine(self.engine.status());
        Ok(KeyStatus {
            provider: status.provider,
            state: "saved".to_owned(),
            last_checked_at: status.last_checked_at,
        })
    }

    /// Removes a provider key and refreshes the live engine's models and providers.
    pub async fn disconnect_provider(
        &mut self,
        provider: &str,
    ) -> Result<KeyStatus, KeychainError> {
        let status = self
            .engine
            .disconnect_provider(self.keychain.as_ref(), provider)
            .await?;
        self.connection.publish_engine(self.engine.status());
        Ok(KeyStatus {
            provider: status.provider,
            state: "missing".to_owned(),
            last_checked_at: status.last_checked_at,
        })
    }

    /// Stores a provider key in the local keychain. The value is never returned.
    pub fn store_key(
        &self,
        provider: &str,
        secret: &SecretString,
    ) -> Result<KeyStatus, KeychainError> {
        self.keychain.set(provider, secret)?;
        Ok(KeyStatus {
            provider: provider.to_owned(),
            state: "saved".to_owned(),
            last_checked_at: Some(self.clock.now()),
        })
    }

    /// Deletes a provider key.
    pub fn delete_key(&self, provider: &str) -> Result<KeyStatus, KeychainError> {
        self.keychain.delete(provider)?;
        Ok(KeyStatus {
            provider: provider.to_owned(),
            state: "missing".to_owned(),
            last_checked_at: Some(self.clock.now()),
        })
    }

    /// Lists provider names and key states only.
    pub fn key_status(&self) -> Result<Vec<KeyStatus>, KeychainError> {
        Ok(self
            .keychain
            .list()?
            .into_iter()
            .map(|provider| KeyStatus {
                provider,
                state: "saved".to_owned(),
                last_checked_at: None,
            })
            .collect())
    }

    /// Returns the current gate snapshot.
    #[must_use]
    pub fn gates(&self) -> GateSnapshot {
        self.gates
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Stores and publishes a gate snapshot.
    ///
    /// The D6 feature flag is derived here from the snapshot's own gate and local-agent
    /// state, so a caller cannot publish `Ready` with a stale disabled flag or a Waiting
    /// gate with a stale enabled one.
    pub fn set_gates(&self, mut snapshot: GateSnapshot) {
        snapshot.enforce_local_orchestrator_flag();
        *self
            .gates
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = snapshot.clone();
        self.connection.publish_gates(snapshot);
    }

    /// Records the latest `vowp node show --json` card for the D6 guard.
    ///
    /// The card is redacted by VOWP and carries no path or secret. A later node read
    /// replaces it immediately; a stale card must never enable a revoked node.
    pub fn set_local_node_card(&self, card: &NodeCard) {
        self.local_orchestrator
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .node = Some(LocalNodeState::from_card(card));
    }

    /// Forgets the local node card, for example after a disconnect.
    pub fn clear_local_node_card(&self) {
        self.local_orchestrator
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .node = None;
    }

    /// Re-reads one node card and records it for the D6 guard.
    ///
    /// `project` is required on a multi-project home, matching the gate probe's
    /// `--project`-qualified `node show` read.
    pub async fn refresh_local_node(
        &self,
        client: &dyn VowpClient,
        project: Option<&str>,
        node: &str,
    ) -> Result<NodeCard, SpiritError> {
        let card = client.node_card(project, node).await?;
        self.set_local_node_card(&card);
        Ok(card)
    }

    /// Records the Mac path map. Absolute roots stay in [`AgentPathMap`]; only the
    /// path-free validity is kept for the guard.
    pub fn set_local_agent_paths(&self, maps: &[AgentPathMap]) {
        self.local_orchestrator
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .set_path_maps(maps);
    }

    /// Returns a copy of the D6 local state.
    #[must_use]
    pub fn local_orchestrator_state(&self) -> LocalOrchestratorState {
        self.local_orchestrator
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Builds the backend guard for one D6 action.
    ///
    /// Pass the project an orchestrator will bind; surface actions pass `None`. The
    /// guard re-reads the current gate snapshot and local state each time, so a stale
    /// renderer cannot authorize a write.
    #[must_use]
    pub fn local_orchestrator_guard(&self, project: Option<&str>) -> LocalOrchestratorGuard {
        let state = self.local_orchestrator_state();
        LocalOrchestratorGuard::from_snapshot(&self.gates(), &state).for_project(&state, project)
    }

    /// Re-checks one D6 action without executing it.
    pub fn ensure_local_orchestrator_action(
        &self,
        action: LocalOrchestratorAction,
        project: Option<&str>,
        node: Option<&str>,
    ) -> Result<(), SpiritError> {
        self.local_orchestrator_guard(project).ensure(action, node)
    }

    /// Creates a pending home surface with one-time pairing code.
    ///
    /// The code is returned in [`SurfacePairing`] only; it is never serialized or
    /// logged. The backend re-checks the D6 guard before the command runs.
    pub async fn surface_pair(
        &self,
        client: &dyn VowpClient,
        request: SurfacePairRequest,
    ) -> Result<SurfacePairing, SpiritError> {
        request.validate()?;
        self.require_connected_host()?;
        self.local_orchestrator_guard(None)
            .ensure(LocalOrchestratorAction::SurfacePair, Some(&request.node))?;
        client.surface_pair(request).await
    }

    /// Lists home surfaces. This read stays available while the gate is `Waiting`.
    pub async fn surface_list(
        &self,
        client: &dyn VowpClient,
        node: Option<&str>,
    ) -> Result<Vec<SurfaceEntry>, SpiritError> {
        self.require_connected_host()?;
        client.surface_list(node).await
    }

    /// Rotates one surface token. The old token is only valid for VOWP's overlap.
    pub async fn surface_rotate(
        &self,
        client: &dyn VowpClient,
        request: SurfaceRotateRequest,
    ) -> Result<SurfaceRotateResult, SpiritError> {
        request.validate()?;
        self.require_connected_host()?;
        self.local_orchestrator_guard(None).ensure(
            LocalOrchestratorAction::SurfaceRotate,
            request.node.as_deref(),
        )?;
        client.surface_rotate(request).await
    }

    /// Revokes one surface and every token it holds.
    ///
    /// Revocation is explicit cleanup, so it remains available when a node was revoked
    /// or the gate is `Waiting`. StackSpirit never retries with the old credential.
    pub async fn surface_revoke(
        &self,
        client: &dyn VowpClient,
        surface: &str,
    ) -> Result<SurfaceRevokeResult, SpiritError> {
        ensure_surface_id(surface)?;
        self.require_connected_host()?;
        self.local_orchestrator_guard(None)
            .ensure(LocalOrchestratorAction::SurfaceRevoke, None)?;
        client.surface_revoke(surface).await
    }

    /// Reads the Mac agent status through the local command runner.
    pub async fn agent_status(
        &self,
        local: &dyn LocalAgentClient,
    ) -> Result<LocalAgentView, SpiritError> {
        let document = local.status().await?;
        Ok(LocalAgentView::from_document(&document))
    }

    /// Redeems a home surface pairing code on the Mac.
    pub async fn agent_pair(
        &self,
        local: &dyn LocalAgentClient,
        request: AgentPairRequest,
    ) -> Result<AgentPairing, SpiritError> {
        request.validate()?;
        self.local_orchestrator_guard(None)
            .ensure(LocalOrchestratorAction::AgentPair, None)?;
        local.pair(&request).await
    }

    /// Reads and records the Mac path map; only safe [`AgentPathView`] values return.
    pub async fn agent_paths(
        &self,
        local: &dyn LocalAgentClient,
    ) -> Result<Vec<AgentPathView>, SpiritError> {
        let maps = local.paths().await?;
        self.set_local_agent_paths(&maps);
        Ok(maps.iter().map(AgentPathView::from_map).collect())
    }

    /// Adds or updates one Mac path-map row.
    pub async fn agent_path_add(
        &self,
        local: &dyn LocalAgentClient,
        request: AgentPathAddRequest,
    ) -> Result<AgentPathView, SpiritError> {
        request.validate()?;
        let map = local.add_path(&request).await?;
        self.local_orchestrator
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .set_path_map(&map);
        Ok(AgentPathView::from_map(&map))
    }

    /// Removes one Mac path-map row.
    pub async fn agent_path_remove(
        &self,
        local: &dyn LocalAgentClient,
        project: &str,
    ) -> Result<(), SpiritError> {
        let project = ProjectName::parse(project)?;
        local.remove_path(&project).await?;
        self.local_orchestrator
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove_path_map(project.as_str());
        Ok(())
    }

    /// Creates a local app host through `vowp orchestrator new --node ...`.
    ///
    /// The guard re-checks the gate, connected agent, protocol, node status and the
    /// project path map before VOWP materializes anything.
    pub async fn orchestrator_new(
        &self,
        client: &dyn VowpClient,
        request: OrchestratorNewRequest,
    ) -> Result<OrchestratorView, SpiritError> {
        request.validate()?;
        self.require_connected_host()?;
        self.local_orchestrator_guard(request.project.as_deref())
            .ensure(
                LocalOrchestratorAction::OrchestratorNew,
                Some(request.node.as_str()),
            )?;
        let card = client.orchestrator_new(request).await?;
        Ok(OrchestratorView::from_card(&card))
    }

    /// Lists orchestrators. This read stays available while the gate is `Waiting`.
    pub async fn orchestrator_list(
        &self,
        client: &dyn VowpClient,
    ) -> Result<Vec<OrchestratorView>, SpiritError> {
        self.require_connected_host()?;
        let cards = client.orchestrator_list().await?;
        Ok(cards.iter().map(OrchestratorView::from_card).collect())
    }

    /// Stops a local app host. This explicit cleanup remains available while the gate is
    /// `Waiting` or the node is offline.
    pub async fn orchestrator_stop(
        &self,
        client: &dyn VowpClient,
        name: &str,
    ) -> Result<OrchestratorView, SpiritError> {
        validate_orchestrator_name(name)?;
        self.require_connected_host()?;
        self.local_orchestrator_guard(None)
            .ensure(LocalOrchestratorAction::OrchestratorStop, None)?;
        let card = client.orchestrator_stop(name).await?;
        Ok(OrchestratorView::from_card(&card))
    }

    /// Resumes a local app host after re-checking every D6 prerequisite.
    pub async fn orchestrator_resume(
        &self,
        client: &dyn VowpClient,
        name: &str,
        project: Option<&str>,
        node: Option<&str>,
    ) -> Result<OrchestratorView, SpiritError> {
        validate_orchestrator_name(name)?;
        self.require_connected_host()?;
        self.local_orchestrator_guard(project)
            .ensure(LocalOrchestratorAction::OrchestratorResume, node)?;
        let card = client.orchestrator_resume(name).await?;
        Ok(OrchestratorView::from_card(&card))
    }

    /// Moves an orchestrator to a node or back home after re-checking the D6 gate and the
    /// connected agent. The returned view is only what VOWP printed.
    pub async fn orchestrator_move(
        &self,
        client: &dyn VowpClient,
        name: &str,
        target: OrchestratorMoveTarget,
    ) -> Result<OrchestratorView, SpiritError> {
        validate_orchestrator_name(name)?;
        let target_node = match &target {
            OrchestratorMoveTarget::Home => None,
            OrchestratorMoveTarget::Node { node } => Some(NodeName::parse(node)?),
        };
        self.require_connected_host()?;
        self.local_orchestrator_guard(None).ensure(
            LocalOrchestratorAction::OrchestratorMove,
            target_node.as_ref().map(NodeName::as_str),
        )?;
        let card = client.orchestrator_move(name, target).await?;
        Ok(OrchestratorView::from_card(&card))
    }

    /// Resumes a home-hosted orchestrator (no node). The D6 local-agent guard does not
    /// apply: the app host runs on the connected home, so no Mac agent gate is needed.
    pub async fn orchestrator_resume_home(
        &self,
        client: &dyn VowpClient,
        name: &str,
    ) -> Result<OrchestratorView, SpiritError> {
        validate_orchestrator_name(name)?;
        self.require_connected_host()?;
        let card = client.orchestrator_resume(name).await?;
        Ok(OrchestratorView::from_card(&card))
    }

    /// Adds a project (and optionally a home-hosted orchestrator) in one step.
    ///
    /// The D6 local-orchestrator guard does not apply: `project add` and
    /// `--orchestrator` run on the home. Moving the created orchestrator to the local
    /// node stays a separate explicit `orchestrator_move`.
    pub async fn project_add(
        &self,
        client: &dyn VowpClient,
        request: ProjectAddRequest,
    ) -> Result<ProjectAddOutcome, SpiritError> {
        request.validate()?;
        self.require_connected_host()?;
        client.project_add(request).await
    }

    /// Builds the honest local surface cards: node, protocol, agent status, lease
    /// holder and session state, with revoked/expired surfaces marked non-writable.
    pub async fn local_surface_views(
        &self,
        client: &dyn VowpClient,
        local: &dyn LocalAgentClient,
    ) -> Result<Vec<LocalSurfaceView>, SpiritError> {
        let surfaces = self.surface_list(client, None).await?;
        let agent = local.status().await?;
        let state = self.local_orchestrator_state();
        let fleet = self.fleet_view();
        let now = self.clock.now();
        Ok(surfaces
            .iter()
            .map(|surface| {
                let session = fleet.as_ref().and_then(|fleet| {
                    fleet.sessions.iter().find(|session| {
                        session
                            .active_surface
                            .as_ref()
                            .is_some_and(|holder| holder.surface_id == surface.id)
                    })
                });
                LocalSurfaceView::from_parts(surface, &agent, state.node.as_ref(), session, now)
            })
            .collect())
    }

    /// Builds the honest local orchestrator cards with node, protocol, agent status,
    /// lease holder and session state.
    pub async fn local_orchestrator_views(
        &self,
        client: &dyn VowpClient,
        local: &dyn LocalAgentClient,
    ) -> Result<Vec<LocalOrchestratorView>, SpiritError> {
        let cards = self.orchestrator_list(client).await?;
        let agent = local.status().await?;
        let state = self.local_orchestrator_state();
        let fleet = self.fleet_view();
        Ok(cards
            .iter()
            .map(|card| {
                let session = fleet.as_ref().and_then(|fleet| {
                    fleet
                        .sessions
                        .iter()
                        .find(|session| session.id == card.session_id)
                });
                LocalOrchestratorView::from_parts(card, &agent, state.node.as_ref(), session)
            })
            .collect())
    }

    fn require_connected_host(&self) -> Result<(), SpiritError> {
        if self.connection.status().state == "connected" {
            Ok(())
        } else {
            Err(
                SpiritError::new(SpiritErrorCode::NotFound, "no host is connected")
                    .with_hint("Connect a host before using local orchestrator actions."),
            )
        }
    }

    /// Builds the Finder-style file service for one host over verified SSH.
    #[must_use]
    pub fn remote_files(
        &self,
        runner: Arc<dyn crate::ssh::CommandRunner>,
        target: crate::ssh::SshTarget,
        options: crate::ssh::SshOptions,
    ) -> crate::vowp::RemoteFiles {
        crate::vowp::RemoteFiles::new(crate::ssh::SshRemote::new(runner, target, options))
    }

    /// Scans host keys with the injected runner.
    pub async fn scan_host_keys(
        &self,
        runner: Arc<dyn crate::ssh::CommandRunner>,
        target: &crate::ssh::SshTarget,
    ) -> Result<crate::ssh::HostKeyScan, SpiritError> {
        crate::ssh::scan_host_keys(runner.as_ref(), target, std::time::Duration::from_secs(8)).await
    }

    /// Probes a host with the injected runner.
    pub async fn probe_host(
        &self,
        runner: Arc<dyn crate::ssh::CommandRunner>,
        input: &crate::host::HostProbeInput,
    ) -> crate::host::HostProbe {
        crate::host::probe_host(runner, input).await
    }
}

/// Picks the orchestrator that serves `project` from the registered cards.
///
/// Any card whose `project == <project>` (or that is named `<project>`) matches. A real
/// (non-`chat`) orchestrator, such as `opencode`, always outranks a `chat`-only stub like
/// `local-chat`. Within the same app class the preference is (a) a running orchestrator,
/// (b) the canonical card named `<project>`, (c) any other stopped or failed project-bound
/// card, such as an older `local-<project>`. `None` means no orchestrator exists, so the
/// caller creates the canonical one.
#[must_use]
pub fn select_project_orchestrator<'a>(
    cards: &'a [OrchestratorCard],
    project: &str,
) -> Option<&'a OrchestratorCard> {
    cards
        .iter()
        .filter(|card| {
            card.name == project || card.project.as_ref().is_some_and(|p| p.as_str() == project)
        })
        .min_by_key(|card| {
            (
                card.app == OrchestratorApp::Chat,
                !matches!(card.state.as_str(), "running" | "starting"),
                card.name != project,
            )
        })
}

pub(crate) fn validate_orchestrator_name(name: &str) -> Result<(), SpiritError> {
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

/// Alias used by the D6 brief for the headless service facade.
pub type SpiritService = SpiritCore;
