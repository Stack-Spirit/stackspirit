//! Process-wide app state. All remote work is delegated to `spirit-core`; this module
//! only owns paths, the active client, the one host connection, the shared engine
//! handles and the one chat service.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use spirit_core::SpiritCore;
use spirit_core::connection::ConnectionManager;
use spirit_core::engine::chat::{ChatEngine, ChatModelTier, ChatModelView, LocalChatProvider};
use spirit_core::engine::{
    EngineChoice, EnginePreferences, EnginePreferencesStore, EngineRuntime, EngineState,
    EngineStatus, EngineSupervisor, EngineSupervisorConfig, GateFeatureFlags, GateSnapshot,
    HostGateCache, JsonEnginePreferences, Keychain, ModelAccess, ModelCatalog, ModelChoice,
    OsKeychain, PrivateEngineTree, ProcessEngineRuntime, bundled_catalog,
};
use spirit_core::error::{SpiritError, SpiritErrorCode};
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

/// The engine boundary shared by the existing engine commands and the chat adapter.
///
/// Both paths hold clones of the same handles, so chat resolves the one supervisor,
/// runtime, catalog and preferences store and never starts a second engine.
#[derive(Clone)]
struct EngineHost {
    core: Arc<AsyncMutex<SpiritCore>>,
    runtime: Arc<dyn EngineRuntime>,
    catalog: ModelCatalog,
    preferences: Arc<dyn EnginePreferencesStore>,
    /// Provider names with a stored key. Never a key value.
    keychain: Arc<dyn Keychain>,
}

impl EngineHost {
    fn load_preferences(&self) -> EnginePreferences {
        self.preferences.load().unwrap_or_default()
    }

    fn save_preferences(&self, preferences: &EnginePreferences) -> CommandResult<()> {
        self.preferences
            .save(preferences)
            .map_err(|error| error.with_hint("The engine selection was not saved locally."))
    }

    /// Starts the bundled engine on first run and restores the saved model when the
    /// runtime still reports it. An external choice is never started by this app.
    async fn ensure_setup(&self) -> EngineStatus {
        {
            let mut core = self.core.lock().await;
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
                    if self.runtime.probe_health(&info).await.is_err() {
                        core.restart_engine().await;
                    }
                }
            }
        }
        self.apply_saved_model().await;
        let status = self.core.lock().await.engine_status();
        self.status_with_preferences(status)
    }

    /// Reuses [`Self::ensure_setup`], then starts a stopped or failed bundled engine
    /// once, matching the chat table in `docs/ORCHESTRATOR-CHAT.md` §2.1.4.
    async fn ensure_chat_ready(&self) -> EngineStatus {
        let status = self.ensure_setup().await;
        if matches!(status.choice, EngineChoice::Bundled)
            && matches!(
                status.state,
                EngineState::Stopped | EngineState::Degraded { .. } | EngineState::Failed { .. }
            )
        {
            let status = self.core.lock().await.restart_engine().await;
            self.apply_saved_model().await;
            return self.status_with_preferences(status);
        }
        status
    }

    /// Re-applies the saved model id only when the runtime verifies it as usable. A
    /// currently selected model is re-verified too; a model the engine no longer
    /// reports is left unselected, and no replacement is selected here.
    async fn apply_saved_model(&self) {
        let status = self.core.lock().await.engine_status();
        if !matches!(status.state, EngineState::Ready) {
            return;
        }
        let models = self.usable_chat_models(&status).await;
        let current_selected_is_live = status
            .selected_model
            .as_ref()
            .is_some_and(|selected| models.iter().any(|(view, _)| view.id == selected.id));
        if current_selected_is_live {
            return;
        }
        let preferences = self.load_preferences();
        let Some(model_id) = preferences.selected_model_id else {
            return;
        };
        if let Some((_, choice)) = models.into_iter().find(|(view, _)| view.id == model_id) {
            self.core.lock().await.select_model(choice);
        }
    }

    /// Adds the persisted local first-run flag to an engine status reply.
    fn status_with_preferences(&self, mut status: EngineStatus) -> EngineStatus {
        status.onboarding_complete = self.load_preferences().onboarding_complete;
        status
    }

    /// Returns the build-time catalog entries the running engine actually reports.
    async fn verified_models(&self, status: &EngineStatus) -> Vec<ModelChoice> {
        if !matches!(status.state, EngineState::Ready) {
            return Vec::new();
        }
        let Some(port) = status.port else {
            return Vec::new();
        };
        let runtime_models = self.runtime.models(port).await.unwrap_or_default();
        self.catalog
            .verify_runtime(&runtime_models, Timestamp::now())
            .models
    }

    /// Returns the models chat can use right now: verified free runtime models plus
    /// runtime models whose provider has a stored BYOK key. Order is the runtime's.
    async fn usable_chat_models(&self, status: &EngineStatus) -> Vec<(ChatModelView, ModelChoice)> {
        if !matches!(status.state, EngineState::Ready) {
            return Vec::new();
        }
        let Some(port) = status.port else {
            return Vec::new();
        };
        let runtime_models = self.runtime.models(port).await.unwrap_or_default();
        let configured = self.keychain.list().unwrap_or_default();
        let verified_at = Timestamp::now();
        let mut usable = Vec::new();
        for runtime in runtime_models {
            let tier = if runtime.free {
                ChatModelTier::Included
            } else if configured
                .iter()
                .any(|provider| provider == &runtime.provider)
            {
                ChatModelTier::Byok
            } else {
                continue;
            };
            let catalog = self
                .catalog
                .entries
                .iter()
                .find(|entry| entry.id == runtime.id);
            let label = catalog
                .map(|entry| entry.label.clone())
                .filter(|label| !label.is_empty())
                .unwrap_or_else(|| runtime.label.clone());
            let label = if label.is_empty() {
                runtime.id.clone()
            } else {
                label
            };
            let context_tokens = runtime
                .context_tokens
                .or_else(|| catalog.and_then(|entry| entry.context_tokens));
            let access = if tier == ChatModelTier::Included {
                ModelAccess::Included
            } else {
                ModelAccess::Byok
            };
            let view = ChatModelView {
                id: runtime.id.clone(),
                label,
                provider: runtime.provider.clone(),
                tier,
                context_tokens,
            };
            let choice = ModelChoice {
                id: runtime.id,
                label: view.label.clone(),
                provider: runtime.provider,
                access,
                context_tokens,
                notes: catalog.map(|entry| entry.notes.clone()).unwrap_or_default(),
                verified_at: Some(verified_at),
            };
            usable.push((view, choice));
        }
        usable
    }

    /// Selects a model for the setup assistant and optionally persists the choice.
    async fn select_chat_model(
        &self,
        model_id: &str,
        persist: bool,
    ) -> Result<ChatModelView, SpiritError> {
        let status = self.core.lock().await.engine_status();
        if matches!(status.choice, EngineChoice::External { .. }) {
            return Err(SpiritError::new(
                SpiritErrorCode::Conflict,
                "an external harness manages its own models",
            )
            .with_hint("Switch to the bundled engine before choosing a model."));
        }
        if !matches!(status.state, EngineState::Ready) {
            return Err(SpiritError::new(
                SpiritErrorCode::Conflict,
                "start the StackSpirit Engine before choosing a model",
            )
            .with_hint("Restart the engine and retry."));
        }
        let selected = self
            .usable_chat_models(&status)
            .await
            .into_iter()
            .find(|(view, _)| view.id == model_id)
            .ok_or_else(|| {
                SpiritError::new(
                    SpiritErrorCode::NotFound,
                    "that model is not available in this engine build",
                )
                .with_hint("Choose a model from the list or add a provider key.")
            })?;
        let (view, choice) = selected;
        self.core.lock().await.select_model(choice);
        if persist {
            let mut preferences = self.load_preferences();
            preferences.selected_model_id = Some(view.id.clone());
            preferences.onboarding_complete = true;
            self.save_preferences(&preferences)?;
        }
        Ok(view)
    }
}

/// The last observed model state, used when the core lock is briefly busy.
#[derive(Default)]
struct ChatModelCache {
    selected_model: Option<ChatModelView>,
    available_models: Vec<ChatModelView>,
}

fn chat_model_view(model: &ModelChoice) -> ChatModelView {
    ChatModelView {
        id: model.id.clone(),
        label: model.label.clone(),
        provider: model.provider.clone(),
        tier: if model.access == ModelAccess::Included {
            ChatModelTier::Included
        } else {
            ChatModelTier::Byok
        },
        context_tokens: model.context_tokens,
    }
}

/// The desktop [`ChatEngine`] adapter over the shared [`EngineHost`].
///
/// Chat asks this adapter for the bundled engine; the adapter reuses the existing
/// `ensure_engine_setup` semantics and never spawns a process itself. The status and
/// selected model are core-only and are never serialized into a chat DTO.
struct DesktopChatEngine {
    engine: EngineHost,
    cache: Mutex<ChatModelCache>,
}

impl DesktopChatEngine {
    fn new(engine: EngineHost) -> Self {
        Self {
            engine,
            cache: Mutex::new(ChatModelCache::default()),
        }
    }

    fn remember(&self, selected: Option<ChatModelView>, available: Vec<ChatModelView>) {
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.selected_model = selected;
        cache.available_models = available;
    }

    fn cached(&self) -> std::sync::MutexGuard<'_, ChatModelCache> {
        self.cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl ChatEngine for DesktopChatEngine {
    fn ensure_ready<'life0, 'async_trait>(
        &'life0 self,
    ) -> Pin<Box<dyn Future<Output = Result<u16, SpiritError>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let status = self.engine.ensure_chat_ready().await;
            if matches!(status.choice, EngineChoice::External { .. }) {
                self.remember(None, Vec::new());
                return Err(SpiritError::new(
                    SpiritErrorCode::Engine,
                    "engine_external: the setup assistant needs the bundled StackSpirit Engine",
                )
                .with_hint("Switch to the bundled engine in Settings → Advanced."));
            }
            if matches!(status.state, EngineState::Ready)
                && let Some(port) = status.port
            {
                // An unsaved first run defaults to the first verified free runtime model.
                // The choice is in-memory: only an explicit chat.setModel persists it.
                let usable = self.engine.usable_chat_models(&status).await;
                let mut selected = status.selected_model.as_ref().map(chat_model_view);
                if selected.is_none()
                    && let Some((view, choice)) = usable
                        .iter()
                        .find(|(view, _)| view.tier == ChatModelTier::Included)
                        .or_else(|| {
                            usable
                                .iter()
                                .find(|(view, _)| view.tier == ChatModelTier::Byok)
                        })
                        .cloned()
                {
                    self.engine.core.lock().await.select_model(choice);
                    selected = Some(view);
                }
                let available = usable.into_iter().map(|(view, _)| view).collect();
                self.remember(selected, available);
                return Ok(port);
            }
            self.remember(None, Vec::new());
            Err(SpiritError::new(
                SpiritErrorCode::Engine,
                "engine_not_ready: the StackSpirit Engine is not ready",
            )
            .retryable()
            .with_hint("Restart StackSpirit Engine in Settings → Advanced and retry."))
        })
    }

    fn selected_model(&self) -> Option<ChatModelView> {
        match self.engine.core.try_lock() {
            Ok(core) => core
                .engine_status()
                .selected_model
                .as_ref()
                .map(chat_model_view),
            Err(_) => self.cached().selected_model.clone(),
        }
    }

    fn available_models(&self) -> Vec<ChatModelView> {
        self.cached().available_models.clone()
    }

    fn select_model<'life0, 'life1, 'async_trait>(
        &'life0 self,
        model_id: &'life1 str,
        persist: bool,
    ) -> Pin<Box<dyn Future<Output = Result<ChatModelView, SpiritError>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let view = self.engine.select_chat_model(model_id, persist).await?;
            {
                let mut cache = self
                    .cache
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                cache.selected_model = Some(view.clone());
                if !cache
                    .available_models
                    .iter()
                    .any(|model| model.id == view.id)
                {
                    cache.available_models.push(view.clone());
                }
            }
            Ok(view)
        })
    }

    fn status(&self) -> EngineStatus {
        // A busy core lock falls back to the default bundled status; the following
        // `ensure_ready` re-reads the real choice before any engine action.
        match self.engine.core.try_lock() {
            Ok(core) => core.engine_status(),
            Err(_) => EngineStatus::default(),
        }
    }
}

/// Everything a Tauri command may touch.
pub struct AppState {
    core: Arc<AsyncMutex<SpiritCore>>,
    hosts: Arc<dyn HostStore>,
    known_hosts: PathBuf,
    /// App-private directory for keys StackSpirit creates and owns.
    app_keys_dir: PathBuf,
    runner: Arc<dyn CommandRunner>,
    client: Mutex<Option<Arc<dyn VowpClient>>>,
    active_host: Mutex<Option<HostProfile>>,
    engine: EngineHost,
    engine_keychain: Arc<dyn Keychain>,
    chat: Arc<LocalChatProvider>,
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
        // The private XDG tree keeps chat and models away from the owner's own OpenCode
        // state (`docs/ORCHESTRATOR-CHAT.md` §2.1.3).
        let runtime: Arc<dyn EngineRuntime> = Arc::new(
            ProcessEngineRuntime::for_current_exe()
                .with_key_source(keychain.clone())
                .with_private_state(app_data_dir.clone()),
        );
        let supervisor = EngineSupervisor::new(runtime.clone(), EngineSupervisorConfig::default());
        let preferences: Arc<dyn EnginePreferencesStore> =
            Arc::new(JsonEnginePreferences::new(app_data_dir.join("engine.json")));
        let saved = preferences.load().unwrap_or_default();
        let mut core = SpiritCore::desktop(hosts.clone(), supervisor, KEYCHAIN_SERVICE)
            .with_keychain(keychain.clone());
        core.select_engine(saved.choice.clone());
        core.set_gates(default_gate_snapshot());
        let events = core.subscribe();
        let connection = core.connection().clone();
        let core = Arc::new(AsyncMutex::new(core));

        let engine = EngineHost {
            core: core.clone(),
            runtime,
            catalog: bundled_catalog(),
            preferences,
            keychain: keychain.clone(),
        };

        // The chat provider shares the engine host and uses the setup workspace the
        // private tree creates; it never spawns an engine and never receives a key.
        let setup_workspace = PrivateEngineTree::prepare(&app_data_dir)?.setup_workspace;
        let chat = Arc::new(LocalChatProvider::new(
            Arc::new(DesktopChatEngine::new(engine.clone())),
            setup_workspace,
        ));

        Ok((
            Self {
                core,
                hosts,
                known_hosts,
                app_keys_dir,
                runner: Arc::new(SystemCommandRunner),
                client: Mutex::new(None),
                active_host: Mutex::new(None),
                engine,
                engine_keychain: keychain,
                chat,
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

    /// The one chat service. Command adapters call it; they never own session logic.
    pub(crate) fn chat(&self) -> Arc<LocalChatProvider> {
        self.chat.clone()
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
        self.engine.runtime.clone()
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
        self.engine.load_preferences()
    }

    pub(crate) fn save_engine_preferences(
        &self,
        preferences: &EnginePreferences,
    ) -> CommandResult<()> {
        self.engine.save_preferences(preferences)
    }

    /// Starts the bundled engine on first run and restores the saved model when the
    /// runtime still reports it. An external choice is never started by this app.
    pub(crate) async fn ensure_engine_setup(&self) -> EngineStatus {
        self.engine.ensure_setup().await
    }

    /// Re-applies the saved model id only when the runtime verifies it.
    pub(crate) async fn apply_saved_model(&self) {
        self.engine.apply_saved_model().await;
    }

    /// Adds the persisted local first-run flag to an engine status reply.
    pub(crate) fn engine_status_with_preferences(&self, status: EngineStatus) -> EngineStatus {
        self.engine.status_with_preferences(status)
    }

    /// Returns the build-time catalog entries the running engine actually reports.
    pub(crate) async fn verified_engine_models(&self, status: &EngineStatus) -> Vec<ModelChoice> {
        self.engine.verified_models(status).await
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
