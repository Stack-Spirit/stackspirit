//! Local StackSpirit Engine types: choice, state, status and model access.

pub mod catalog;
pub mod chat;
pub mod config;
pub mod external;
pub mod gate;
pub mod keychain;
pub mod local;
pub mod process;
pub mod runtime;
pub mod supervisor;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::time::Timestamp;

pub use catalog::{CatalogEntry, CatalogVerification, ModelCatalog, bundled_catalog};
pub use chat::{
    ChatCloseReason, ChatEngine, ChatEvent, ChatEventApplier, ChatEventBus, ChatLogPage,
    ChatLogQuery, ChatMessage, ChatModelView, ChatSendReceipt, ChatSessionId, ChatSessionLocation,
    ChatSessionState, ChatSessionView, ChatStartInput, ChatTransport, LocalChatProvider,
    StreamSupport,
};
pub use config::{EnginePreferences, EnginePreferencesStore, JsonEnginePreferences};
pub use external::{ExternalHarness, HarnessProbe, PathHarnessProbe, scan_external_harnesses};
pub use gate::{
    ASIF_MILESTONES, AsifGate, GateCache, GateFeatureFlags, GateProbeOptions, GateSnapshot,
    HostGateCache, LocalAgentState, LocalAgentStatus, LocalOrchestratorGate, PHASE3_MILESTONES,
    local_orchestrator_enabled, phase3_milestones, probe_asif, probe_local_orchestrator,
};
#[cfg(feature = "keychain")]
pub use keychain::OsKeychain;
pub use keychain::{
    FakeKeychain, Keychain, KeychainError, KeychainErrorClass, ProviderKeyState, ProviderKeyStatus,
    SecretStore, SecretString, verify_provider_key,
};
pub use local::{
    LocalNodeState, LocalOrchestratorAction, LocalOrchestratorGuard, LocalOrchestratorPrerequisite,
    LocalOrchestratorState, LocalPathMapStatus, local_orchestrator_action_enabled, refusal,
};
pub use process::{
    EngineBinaryLocator, EngineLogBuffer, PrivateEngineTree, ProcessEngineRuntime,
    current_target_triple, loopback_host, reserve_loopback_port, serve_args, sidecar_file_name,
};
pub use runtime::{
    EngineHealth, EngineRuntime, EngineRuntimeInfo, FakeEngineRuntime, KeyVerification,
    RuntimeModel,
};
pub use supervisor::{EngineSupervisor, EngineSupervisorConfig};

/// Which engine StackSpirit should use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EngineChoice {
    /// The bundled sidecar presented as the StackSpirit Engine.
    #[default]
    Bundled,
    /// An existing user installation, selected explicitly.
    External {
        /// Absolute path selected by the user.
        path: PathBuf,
        /// Which harness it is.
        kind: ExternalHarnessKind,
    },
}

/// An external harness StackSpirit can detect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExternalHarnessKind {
    /// An existing `opencode` installation.
    #[serde(rename = "opencode")]
    Opencode,
    /// An existing Claude Code installation.
    #[serde(rename = "claude-code")]
    ClaudeCode,
}

impl ExternalHarnessKind {
    /// Returns the wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Opencode => "opencode",
            Self::ClaudeCode => "claude-code",
        }
    }
}

/// The engine supervisor state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum EngineState {
    /// The user has not finished onboarding.
    NotConfigured,
    /// The engine is stopped.
    Stopped,
    /// The engine is starting and not yet healthy.
    Starting,
    /// The engine is healthy on a loopback port.
    Ready,
    /// The engine is running but unhealthy; the last error is safe to show.
    Degraded {
        /// Safe detail.
        detail: String,
    },
    /// The engine could not be started.
    Failed {
        /// Safe detail.
        detail: String,
    },
}

/// Local engine status. No field ever carries a provider key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineStatus {
    /// Selected engine.
    pub choice: EngineChoice,
    /// Supervisor state.
    pub state: EngineState,
    /// Engine version, when known.
    pub version: Option<String>,
    /// Local binary path; never sent to the VPS.
    pub binary_path: Option<PathBuf>,
    /// Loopback port, when running.
    pub port: Option<u16>,
    /// Selected model.
    pub selected_model: Option<ModelChoice>,
    /// True only after runtime verification of at least one included model.
    pub included_models_available: bool,
    /// Start time.
    pub started_at: Option<Timestamp>,
    /// Last safe error.
    pub last_error: Option<String>,
    /// True after the user completes the local first-run flow.
    #[serde(default)]
    pub onboarding_complete: bool,
    /// Bounded, redacted engine log tail for the View logs action.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub logs: Vec<String>,
    /// Providers the running engine reports as supported. Ids and labels only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub providers: Vec<EngineProvider>,
}

impl Default for EngineStatus {
    fn default() -> Self {
        Self {
            choice: EngineChoice::Bundled,
            state: EngineState::NotConfigured,
            version: None,
            binary_path: None,
            port: None,
            selected_model: None,
            included_models_available: false,
            started_at: None,
            last_error: None,
            onboarding_complete: false,
            logs: Vec::new(),
            providers: Vec::new(),
        }
    }
}

/// One provider the bundled engine reports as supported for BYOK.
///
/// Only the provider id and a display label cross the local engine boundary. No key,
/// environment variable value or provider configuration is included.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineProvider {
    /// Engine-native provider id.
    pub id: String,
    /// Display label reported by the engine, or the id when it has no name.
    pub label: String,
}

/// Whether a model is included or needs a user key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelAccess {
    /// Included with StackSpirit Engine; no key required.
    Included,
    /// Requires a provider key in the local OS keychain.
    Byok,
}

/// One model choice shown in the engine view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelChoice {
    /// Engine-native model id.
    pub id: String,
    /// Friendly label.
    pub label: String,
    /// Provider name.
    pub provider: String,
    /// Included or BYOK.
    pub access: ModelAccess,
    /// Context window, when known.
    pub context_tokens: Option<u64>,
    /// Extra notes.
    pub notes: String,
    /// When the runtime last verified it.
    pub verified_at: Option<Timestamp>,
}

/// The local engine bundle: a supervisor plus its build-time catalog.
///
/// D1 ships the skeleton and state machine; D4 supplies the real sidecar runtime and
/// catalog generation. This type deliberately holds no provider key.
#[derive(Debug)]
pub struct StackSpiritEngine {
    /// Lifecycle and health owner.
    pub supervisor: EngineSupervisor,
    /// Build-time included-model catalog.
    pub catalog: ModelCatalog,
}

impl StackSpiritEngine {
    /// Builds the bundle from a supervisor and catalog.
    #[must_use]
    pub fn new(supervisor: EngineSupervisor, catalog: ModelCatalog) -> Self {
        Self {
            supervisor,
            catalog,
        }
    }

    /// Returns the current engine status.
    #[must_use]
    pub fn status(&self) -> EngineStatus {
        self.supervisor.status()
    }

    /// Starts the engine.
    pub async fn start(&mut self) -> EngineStatus {
        self.supervisor.start().await
    }

    /// Restarts the engine with capped backoff.
    pub async fn restart(&mut self) -> EngineStatus {
        self.supervisor.restart().await
    }

    /// Stops the engine.
    pub async fn stop(&mut self) -> EngineStatus {
        self.supervisor.stop().await
    }

    /// Verifies the catalog against the engine's runtime models.
    #[must_use]
    pub fn verify_catalog(
        &self,
        runtime_models: &[RuntimeModel],
        verified_at: Timestamp,
    ) -> CatalogVerification {
        self.catalog.verify_runtime(runtime_models, verified_at)
    }
}
