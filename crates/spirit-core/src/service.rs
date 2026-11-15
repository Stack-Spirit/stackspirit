//! The headless `SpiritCore` facade used by the Tauri host and, later, the CLI.
//!
//! This module has no Tauri, React or platform-GUI dependency.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::connection::{ConnectionManager, ConnectionSnapshot, PollPolicy, SpiritEvent};
use crate::engine::gate::GateSnapshot;
use crate::engine::{
    EngineChoice, EngineStatus, EngineSupervisor, Keychain, KeychainError, ModelChoice,
    SecretString,
};
use crate::error::{SpiritError, SpiritErrorCode};
use crate::host::{HostId, HostKeyPin, HostProfile, HostProfilePatch, HostStore, NewHostProfile};
use crate::time::{Clock, SystemClock, Timestamp};
use crate::view::FleetView;
use crate::vowp::{AsifExport, AsifExportOptions, AsifImportOptions, VowpClient};
use crate::wire::{AsifImportResponse, ProjectName};

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
    pub fn set_gates(&self, snapshot: GateSnapshot) {
        *self
            .gates
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = snapshot.clone();
        self.connection.publish_gates(snapshot);
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
