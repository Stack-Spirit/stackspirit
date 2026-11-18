//! Process-wide app state. All remote work is delegated to `spirit-core`; this module
//! only owns paths, the active client and the one host connection.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use spirit_core::SpiritCore;
use spirit_core::connection::ConnectionManager;
use spirit_core::engine::{
    EngineChoice, EnginePreferences, EnginePreferencesStore, EngineRuntime, EngineState,
    EngineStatus, EngineSupervisor, EngineSupervisorConfig, GateFeatureFlags, GateSnapshot,
    HostGateCache, JsonEnginePreferences, Keychain, ModelCatalog, ModelChoice, OsKeychain,
    ProcessEngineRuntime, bundled_catalog,
};
use spirit_core::error::SpiritError;
use spirit_core::host::{HostProfile, HostStore, JsonHostStore};
use spirit_core::ssh::{CommandRunner, SshOptions, SystemCommandRunner};
use spirit_core::time::Timestamp;
use spirit_core::vowp::{CommandLocalAgent, LocalAgentClient, SshVowpClient, VowpClient};
use tokio::sync::{Mutex as AsyncMutex, broadcast};

use crate::error::CommandResult;

const KEYCHAIN_SERVICE: &str = "com.stackspirit.desktop";

/// True only in the D5 build, which ships the ASIF export/import actions.
pub(crate) const ASIF_ACTIONS_ENABLED: bool = cfg!(feature = "d5");

/// True in the D6 build, which ships the local-orchestrator actions.
///
/// This is the build capability only. The published flag is still derived from the
/// three-part enable rule in `spirit-core`: the flag is on exactly when the
/// `LocalOrchestratorGate` is `Ready` **and** the local agent is `connected`.
pub(crate) const LOCAL_ORCHESTRATOR_ACTIONS_ENABLED: bool = true;

/// The feature flags this build advertises. D5 flips the ASIF switch; the D6 build
/// flips the local-orchestrator switch only for a `Ready` gate and a connected agent.
pub(crate) fn build_gate_features() -> GateFeatureFlags {
    GateFeatureFlags {
        asif_actions: ASIF_ACTIONS_ENABLED,
        local_orchestrator_actions: LOCAL_ORCHESTRATOR_ACTIONS_ENABLED,
    }
}

/// Applies the build feature flags to a gate snapshot from any command path.
///
/// The D4 probe deliberately reports conservative defaults. The D5 host overlays this
/// build's flags at the command boundary so the renderer sees the real build capability
/// without weakening the probe: a waiting, incompatible, unreachable, or partial gate
/// still disables its missing action.
pub(crate) fn with_build_gate_features(mut snapshot: GateSnapshot) -> GateSnapshot {
    snapshot.features = build_gate_features();
    snapshot
}

/// The gate snapshot shown before any probe. It carries the build flags but no
/// capability claim.
pub(crate) fn default_gate_snapshot() -> GateSnapshot {
    with_build_gate_features(GateSnapshot::default())
}

/// Everything a Tauri command may touch.
pub struct AppState {
    core: AsyncMutex<SpiritCore>,
    hosts: Arc<dyn HostStore>,
    known_hosts: PathBuf,
    /// App-private directory for keys StackSpirit creates and owns.
    app_keys_dir: PathBuf,
    runner: Arc<dyn CommandRunner>,
    client: Mutex<Option<Arc<dyn VowpClient>>>,
    active_host: Mutex<Option<HostProfile>>,
    engine_runtime: Arc<dyn EngineRuntime>,
    engine_catalog: ModelCatalog,
    engine_preferences: Arc<dyn EnginePreferencesStore>,
    engine_keychain: Arc<dyn Keychain>,
    local_agent: Arc<dyn LocalAgentClient>,
    gate_cache: HostGateCache,
    connection: ConnectionManager,
}

impl AppState {
    /// Builds the state under the OS app-data directory and returns the event receiver
    /// that must be installed before the first command runs.
    pub fn new(
        app_data_dir: PathBuf,
    ) -> CommandResult<(
        Self,
        broadcast::Receiver<spirit_core::connection::SpiritEvent>,
    )> {
        std::fs::create_dir_all(&app_data_dir).map_err(|error| {
            SpiritError::io("creating the StackSpirit app-data directory", &error)
        })?;

        let host_store = Arc::new(JsonHostStore::new(app_data_dir.join("hosts.json")));
        let hosts: Arc<dyn HostStore> = host_store;
        let known_hosts = app_data_dir.join("known_hosts");
        let app_keys_dir = app_data_dir.join("ssh-keys");

        let keychain: Arc<dyn Keychain> = Arc::new(
            OsKeychain::new(KEYCHAIN_SERVICE).with_index(app_data_dir.join("keychain-index.json")),
        );

        // The Mac-side agent is always local: it runs `vowp agent ...` with the local
        // command runner and never over SSH.
        let local_agent: Arc<dyn LocalAgentClient> =
            Arc::new(CommandLocalAgent::new(Arc::new(SystemCommandRunner)));

        // The bundled choice resolves only the sidecar next to the application binary or
        // in its resource directory. It never consults PATH. Stored BYOK keys are read
        // from the same keychain and passed to the child through its ephemeral env only.
        let runtime: Arc<dyn EngineRuntime> =
            Arc::new(ProcessEngineRuntime::for_current_exe().with_key_source(keychain.clone()));
        let engine = EngineSupervisor::new(runtime.clone(), EngineSupervisorConfig::default());
        let preferences = JsonEnginePreferences::new(app_data_dir.join("engine.json"));
        let saved = preferences.load().unwrap_or_default();
        let mut core = SpiritCore::desktop(hosts.clone(), engine, KEYCHAIN_SERVICE);
        core.select_engine(saved.choice.clone());
        core.set_gates(default_gate_snapshot());
        let events = core.subscribe();
        let connection = core.connection().clone();

        Ok((
            Self {
                core: AsyncMutex::new(core),
                hosts,
                known_hosts,
                app_keys_dir,
                runner: Arc::new(SystemCommandRunner),
                client: Mutex::new(None),
                active_host: Mutex::new(None),
                engine_runtime: runtime,
                engine_catalog: bundled_catalog(),
                engine_preferences: Arc::new(preferences),
                engine_keychain: keychain,
                local_agent,
                gate_cache: HostGateCache::new(),
                connection,
            },
            events,
        ))
    }

    pub(crate) async fn core(&self) -> tokio::sync::MutexGuard<'_, SpiritCore> {
        self.core.lock().await
    }

    pub(crate) fn hosts(&self) -> Arc<dyn HostStore> {
        self.hosts.clone()
    }

    pub(crate) fn known_hosts(&self) -> &Path {
        &self.known_hosts
    }

    /// App-private directory for the ed25519 key StackSpirit creates for onboarding.
    /// The renderer never receives this path.
    pub(crate) fn app_keys_dir(&self) -> &Path {
        &self.app_keys_dir
    }

    pub(crate) fn runner(&self) -> Arc<dyn CommandRunner> {
        self.runner.clone()
    }

    pub(crate) fn ssh_options(&self, identity_file: Option<PathBuf>) -> SshOptions {
        SshOptions::new(&self.known_hosts).with_identity_file(identity_file)
    }

    pub(crate) async fn connection_manager(&self) -> ConnectionManager {
        self.core.lock().await.connection().clone()
    }

    pub(crate) fn engine_runtime(&self) -> Arc<dyn EngineRuntime> {
        self.engine_runtime.clone()
    }

    pub(crate) fn engine_keychain(&self) -> &dyn Keychain {
        self.engine_keychain.as_ref()
    }

    /// Returns the Mac-side local agent client. It never uses SSH.
    pub(crate) fn local_agent(&self) -> Arc<dyn LocalAgentClient> {
        self.local_agent.clone()
    }

    /// Forgets node and path-map evidence that belonged to the previous connection.
    ///
    /// A new host can have a node with the same name; the D6 guard must never
    /// authorize an action from a card or path map read through another connection.
    pub(crate) async fn clear_local_orchestrator_state(&self) {
        let core = self.core().await;
        core.clear_local_node_card();
        core.set_local_agent_paths(&[]);
    }

    pub(crate) fn gate_cache(&self) -> &HostGateCache {
        &self.gate_cache
    }

    pub(crate) fn load_engine_preferences(&self) -> EnginePreferences {
        self.engine_preferences.load().unwrap_or_default()
    }

    pub(crate) fn save_engine_preferences(
        &self,
        preferences: &EnginePreferences,
    ) -> CommandResult<()> {
        self.engine_preferences
            .save(preferences)
            .map_err(|error| error.with_hint("The engine selection was not saved locally."))
    }

    /// Starts the bundled engine on first run and restores the saved model when the
    /// runtime still reports it. An external choice is never started by this app.
    pub(crate) async fn ensure_engine_setup(&self) -> EngineStatus {
        {
            let mut core = self.core().await;
            let status = core.engine_status();
            if matches!(status.choice, EngineChoice::Bundled) {
                if matches!(status.state, EngineState::NotConfigured) {
                    core.start_engine().await;
                } else if matches!(status.state, EngineState::Ready)
                    && let Some(port) = status.port
                {
                    let info = spirit_core::engine::EngineRuntimeInfo {
                        pid: None,
                        port,
                        version: status.version.clone(),
                        binary_path: status.binary_path.clone(),
                    };
                    if self.engine_runtime.probe_health(&info).await.is_err() {
                        core.restart_engine().await;
                    }
                }
            }
        }
        self.apply_saved_model().await;
        let status = self.core().await.engine_status();
        self.engine_status_with_preferences(status)
    }

    /// Re-applies the saved model id only when the runtime verifies it. A currently
    /// selected model is re-verified too; a model the engine no longer reports is left
    /// unselected, and no replacement is selected automatically.
    pub(crate) async fn apply_saved_model(&self) {
        let status = self.core().await.engine_status();
        if !matches!(status.state, EngineState::Ready) {
            return;
        }
        let models = self.verified_engine_models(&status).await;
        let current_selected_is_live = status
            .selected_model
            .as_ref()
            .is_some_and(|selected| models.iter().any(|model| model.id == selected.id));
        if current_selected_is_live {
            return;
        }
        let preferences = self.load_engine_preferences();
        let Some(model_id) = preferences.selected_model_id else {
            return;
        };
        if let Some(model) = models.into_iter().find(|model| model.id == model_id) {
            self.core().await.select_model(model);
        }
    }

    /// Adds the persisted local first-run flag to an engine status reply.
    pub(crate) fn engine_status_with_preferences(&self, mut status: EngineStatus) -> EngineStatus {
        status.onboarding_complete = self.load_engine_preferences().onboarding_complete;
        status
    }

    /// Returns the build-time catalog entries the running engine actually reports.
    pub(crate) async fn verified_engine_models(&self, status: &EngineStatus) -> Vec<ModelChoice> {
        if !matches!(status.state, EngineState::Ready) {
            return Vec::new();
        }
        let Some(port) = status.port else {
            return Vec::new();
        };
        let runtime_models = self.engine_runtime.models(port).await.unwrap_or_default();
        self.engine_catalog
            .verify_runtime(&runtime_models, Timestamp::now())
            .models
    }

    pub(crate) fn set_client(&self, client: Option<Arc<dyn VowpClient>>) {
        // Every connect, failed connect and disconnect changes the target connection.
        // Cached gate evidence must never survive across that boundary: clear the
        // in-memory cache and publish the build's default snapshot so no view keeps a
        // Ready. The ASIF action flag stays on in the D5 build; the gate is Unknown.
        self.gate_cache.clear();
        self.connection.publish_gates(default_gate_snapshot());
        *self
            .client
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = client;
    }

    pub(crate) fn client(&self) -> CommandResult<Arc<dyn VowpClient>> {
        self.client
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| {
                SpiritError::new(
                    spirit_core::error::SpiritErrorCode::Conflict,
                    "connect to a host before using fleet commands",
                )
            })
    }

    pub(crate) fn set_active_host(&self, host: Option<HostProfile>) {
        *self
            .active_host
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = host;
    }

    pub(crate) fn active_host(&self) -> Option<HostProfile> {
        self.active_host
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Builds a live client for a profile. The SSH runner never touches the network in
    /// tests because host commands are only invoked from the desktop host.
    pub(crate) fn client_for(&self, host: &HostProfile) -> Arc<dyn VowpClient> {
        Arc::new(SshVowpClient::new(
            self.runner.clone(),
            host.ssh.clone(),
            self.ssh_options(host.identity_file.clone()),
            host.vowp_home.clone(),
        ))
    }
}
