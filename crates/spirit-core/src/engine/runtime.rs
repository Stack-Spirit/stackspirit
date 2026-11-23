//! The engine runtime boundary and a deterministic fake for tests.

use std::sync::Mutex;

use async_trait::async_trait;

use crate::engine::{EngineProvider, SecretString};
use crate::error::{SpiritError, SpiritErrorCode};

/// What a successful engine spawn returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineRuntimeInfo {
    /// Child process id, when known.
    pub pid: Option<u32>,
    /// Loopback port. Never a public interface.
    pub port: u16,
    /// Engine version, when known before the health probe.
    pub version: Option<String>,
    /// Local binary path. Never sent to the VPS.
    pub binary_path: Option<std::path::PathBuf>,
}

/// A health probe result.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EngineHealth {
    /// Version reported by the engine health endpoint.
    pub version: Option<String>,
}

/// One model the running engine reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeModel {
    /// Engine-native id, `provider/model`.
    pub id: String,
    /// Provider id.
    pub provider: String,
    /// Display name reported by the engine, when any.
    pub label: String,
    /// Context window, when reported.
    pub context_tokens: Option<u64>,
    /// True only when the engine reports that the model needs no user key.
    pub free: bool,
    /// Reasoning-effort variants the engine declares for this model (e.g. `low`, `high`).
    pub variants: Vec<String>,
}

/// The result of asking the local engine to check a stored provider key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyVerification {
    /// The engine proved the key works.
    Verified,
    /// The engine rejected the key.
    Invalid,
    /// The local engine could not prove or disprove the key.
    Unverified,
}

/// The process boundary implemented by the bundled sidecar adapter.
#[async_trait]
pub trait EngineRuntime: Send + Sync {
    /// Starts the engine on a loopback port and returns its info.
    async fn spawn(&self, port: u16) -> Result<EngineRuntimeInfo, SpiritError>;
    /// Probes the engine health endpoint.
    async fn probe_health(&self, info: &EngineRuntimeInfo) -> Result<EngineHealth, SpiritError>;
    /// Stops the engine, ensuring no orphan process remains.
    async fn shutdown(&self, info: &EngineRuntimeInfo) -> Result<(), SpiritError>;
    /// Lists the models the running engine reports.
    async fn models(&self, _port: u16) -> Result<Vec<RuntimeModel>, SpiritError> {
        Ok(Vec::new())
    }
    /// Lists the providers the running engine reports as supported.
    async fn providers(&self, _port: u16) -> Result<Vec<EngineProvider>, SpiritError> {
        Ok(Vec::new())
    }
    /// Verifies a provider key through a local engine call.
    async fn verify_key(
        &self,
        _port: u16,
        _provider: &str,
        _secret: &SecretString,
    ) -> KeyVerification {
        KeyVerification::Unverified
    }
    /// Returns the bounded, redacted engine log lines.
    fn logs(&self) -> Vec<String> {
        Vec::new()
    }
    /// Kills a live child synchronously. Called from `Drop` so app exit leaves no orphan.
    fn kill_now(&self) {}
}

#[derive(Debug, Default)]
struct FakeRuntimeState {
    spawn_failures: usize,
    health_failures: usize,
    spawns: usize,
    shutdowns: usize,
    port: u16,
    models: Vec<RuntimeModel>,
    providers: Vec<EngineProvider>,
    key_verification: Option<KeyVerification>,
}

/// A scriptable runtime. It starts no process and opens no socket.
#[derive(Debug, Default)]
pub struct FakeEngineRuntime {
    state: Mutex<FakeRuntimeState>,
}

impl FakeEngineRuntime {
    /// Builds a fake that spawns and reports healthy.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes the next `n` health probes fail.
    #[must_use]
    pub fn with_health_failures(self, n: usize) -> Self {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .health_failures = n;
        self
    }

    /// Makes the next `n` spawns fail.
    #[must_use]
    pub fn with_spawn_failures(self, n: usize) -> Self {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .spawn_failures = n;
        self
    }

    /// Makes every model request return these models.
    #[must_use]
    pub fn with_models(self, models: Vec<RuntimeModel>) -> Self {
        self.set_models(models);
        self
    }

    /// Replaces the model list reported by the fake engine.
    pub fn set_models(&self, models: Vec<RuntimeModel>) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .models = models;
    }

    /// Makes every provider request return these providers.
    #[must_use]
    pub fn with_providers(self, providers: Vec<EngineProvider>) -> Self {
        self.set_providers(providers);
        self
    }

    /// Replaces the provider list reported by the fake engine.
    pub fn set_providers(&self, providers: Vec<EngineProvider>) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .providers = providers;
    }

    /// Makes the next key verification return `result`; the default is `Unverified`.
    #[must_use]
    pub fn with_key_verification(self, result: KeyVerification) -> Self {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .key_verification = Some(result);
        self
    }

    /// Returns how many times `spawn` ran.
    #[must_use]
    pub fn spawns(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .spawns
    }

    /// Returns how many times `shutdown` ran.
    #[must_use]
    pub fn shutdowns(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .shutdowns
    }

    /// Returns the last requested port.
    #[must_use]
    pub fn last_port(&self) -> u16 {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .port
    }
}

#[async_trait]
impl EngineRuntime for FakeEngineRuntime {
    async fn spawn(&self, port: u16) -> Result<EngineRuntimeInfo, SpiritError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.spawns = state.spawns.saturating_add(1);
        state.port = port;
        if state.spawn_failures > 0 {
            state.spawn_failures -= 1;
            return Err(SpiritError::new(
                SpiritErrorCode::Engine,
                "fake engine could not start",
            ));
        }
        Ok(EngineRuntimeInfo {
            pid: Some(4_242),
            port,
            version: Some("fake-0.1".to_owned()),
            binary_path: None,
        })
    }

    async fn probe_health(&self, info: &EngineRuntimeInfo) -> Result<EngineHealth, SpiritError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.health_failures > 0 {
            state.health_failures -= 1;
            return Err(SpiritError::new(
                SpiritErrorCode::Engine,
                format!("fake engine on port {} is not healthy", info.port),
            ));
        }
        Ok(EngineHealth {
            version: Some("fake-0.1".to_owned()),
        })
    }

    async fn shutdown(&self, _info: &EngineRuntimeInfo) -> Result<(), SpiritError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.shutdowns = state.shutdowns.saturating_add(1);
        Ok(())
    }

    async fn models(&self, _port: u16) -> Result<Vec<RuntimeModel>, SpiritError> {
        Ok(self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .models
            .clone())
    }

    async fn providers(&self, _port: u16) -> Result<Vec<EngineProvider>, SpiritError> {
        Ok(self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .providers
            .clone())
    }

    async fn verify_key(
        &self,
        _port: u16,
        _provider: &str,
        _secret: &SecretString,
    ) -> KeyVerification {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .key_verification
            .unwrap_or(KeyVerification::Unverified)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn engine_fake_runtime_reports_scripted_models_and_key_state() {
        let runtime = FakeEngineRuntime::new()
            .with_models(vec![RuntimeModel {
                id: "opencode/big-pickle".to_owned(),
                provider: "opencode".to_owned(),
                label: "Big Pickle".to_owned(),
                context_tokens: None,
                free: true,
                variants: Vec::new(),
            }])
            .with_key_verification(KeyVerification::Invalid);
        let info = runtime.spawn(0).await.unwrap();
        assert_eq!(runtime.models(info.port).await.unwrap().len(), 1);
        let secret = SecretString::new("not-a-real-key");
        assert_eq!(
            runtime.verify_key(info.port, "deepseek", &secret).await,
            KeyVerification::Invalid
        );
        assert_eq!(runtime.spawns(), 1);
    }
}
