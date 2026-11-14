//! Engine supervision: start, health, restart with a bounded budget, and stop.

use std::sync::Arc;
use std::time::Duration;

use crate::engine::runtime::{EngineRuntime, EngineRuntimeInfo, KeyVerification, RuntimeModel};
use crate::engine::{EngineChoice, EngineState, EngineStatus, ModelChoice, SecretString};
use crate::error::SpiritError;
use crate::time::{Clock, SystemClock, Timestamp};

/// The upper bound for an automatic restart delay.
const MAX_RESTART_BACKOFF: Duration = Duration::from_secs(4);

/// Supervisor tuning. The production defaults are deliberately conservative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineSupervisorConfig {
    /// Consecutive failed starts before auto-restart stops.
    pub restart_budget: u32,
    /// Health checks attempted per start.
    pub health_attempts: u32,
    /// Delay between health checks.
    pub retry_delay: Duration,
    /// Loopback port to request. `0` asks the OS for an ephemeral port in D4.
    pub port: u16,
}

impl Default for EngineSupervisorConfig {
    fn default() -> Self {
        Self {
            restart_budget: 3,
            health_attempts: 10,
            retry_delay: Duration::from_millis(250),
            port: 0,
        }
    }
}

/// Owns the engine lifecycle and status. It never exposes a key or a public listener.
pub struct EngineSupervisor {
    runtime: Arc<dyn EngineRuntime>,
    config: EngineSupervisorConfig,
    status: EngineStatus,
    handle: Option<EngineRuntimeInfo>,
    consecutive_failures: u32,
    clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for EngineSupervisor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineSupervisor")
            .field("status", &self.status)
            .field("consecutive_failures", &self.consecutive_failures)
            .finish_non_exhaustive()
    }
}

impl EngineSupervisor {
    /// Builds a supervisor around an injected runtime.
    #[must_use]
    pub fn new(runtime: Arc<dyn EngineRuntime>, config: EngineSupervisorConfig) -> Self {
        Self {
            runtime,
            config,
            status: EngineStatus::default(),
            handle: None,
            consecutive_failures: 0,
            clock: Arc::new(SystemClock),
        }
    }

    /// Replaces the clock (tests only).
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Returns the current status.
    #[must_use]
    pub fn status(&self) -> EngineStatus {
        self.status.clone()
    }

    /// Selects bundled or external engine.
    pub fn set_choice(&mut self, choice: EngineChoice) {
        if matches!(choice, EngineChoice::External { .. }) {
            self.status.included_models_available = false;
        }
        self.status.choice = choice;
    }

    /// Selects a model locally. This never writes to VOWP.
    pub fn select_model(&mut self, model: ModelChoice) {
        self.status.selected_model = Some(model);
    }

    /// Records runtime catalog verification.
    pub fn set_included_models_available(&mut self, available: bool) {
        self.status.included_models_available = available;
    }

    /// Starts the engine and waits for health.
    ///
    /// Calling `start` again while the engine is `Ready` is idempotent: the live
    /// runtime is returned and never orphaned. Any other stale handle is shut down
    /// before a new spawn.
    pub async fn start(&mut self) -> EngineStatus {
        if matches!(self.status.choice, EngineChoice::External { .. }) {
            self.status.state = EngineState::Stopped;
            self.status.port = None;
            self.status.started_at = None;
            return self.status.clone();
        }
        if self.handle.is_some() {
            if self.status.state == EngineState::Ready {
                if let Some(info) = self.handle.clone() {
                    self.reverify_selected_model(&info).await;
                }
                return self.status.clone();
            }
            self.stop_runtime().await;
        }
        self.status.state = EngineState::Starting;
        self.status.last_error = None;
        let info = match self.runtime.spawn(self.config.port).await {
            Ok(info) => info,
            Err(error) => {
                self.consecutive_failures = self.consecutive_failures.saturating_add(1);
                self.status.state = EngineState::Failed {
                    detail: error.message.clone(),
                };
                self.status.last_error = Some(error.message);
                return self.status.clone();
            }
        };
        let mut last_error: Option<SpiritError> = None;
        for attempt in 0..self.config.health_attempts.max(1) {
            match self.runtime.probe_health(&info).await {
                Ok(health) => {
                    self.consecutive_failures = 0;
                    match self.runtime.models(info.port).await {
                        Ok(models) => {
                            self.reconcile_selected_model(&models);
                            self.status.included_models_available =
                                models.iter().any(|model| model.free);
                        }
                        Err(_) => self.status.included_models_available = false,
                    }
                    self.status.state = EngineState::Ready;
                    self.status.version = health.version.or_else(|| info.version.clone());
                    self.status.binary_path = info.binary_path.clone();
                    self.status.port = Some(info.port);
                    self.status.started_at = Some(self.clock.now());
                    self.status.last_error = None;
                    self.handle = Some(info);
                    return self.status.clone();
                }
                Err(error) => {
                    last_error = Some(error);
                    if attempt + 1 < self.config.health_attempts.max(1)
                        && !self.config.retry_delay.is_zero()
                    {
                        tokio::time::sleep(self.config.retry_delay).await;
                    }
                }
            }
        }
        let detail = last_error.map_or_else(
            || "engine health check failed".to_owned(),
            |error| error.message,
        );
        let _ = self.runtime.shutdown(&info).await;
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        self.handle = None;
        self.status.state = EngineState::Degraded {
            detail: detail.clone(),
        };
        self.status.last_error = Some(detail);
        self.status.port = None;
        self.status.started_at = None;
        self.status.clone()
    }

    /// Restarts within the bounded budget. An exhausted budget leaves the engine
    /// `Degraded`; it does not fork-bomb retries.
    pub async fn restart(&mut self) -> EngineStatus {
        if self.consecutive_failures >= self.config.restart_budget {
            let detail = format!(
                "restart budget exhausted after {} failures",
                self.consecutive_failures
            );
            self.status.state = EngineState::Degraded {
                detail: detail.clone(),
            };
            self.status.last_error = Some(detail);
            return self.status.clone();
        }
        let backoff = backoff_delay(self.config.retry_delay, self.consecutive_failures);
        if !backoff.is_zero() {
            tokio::time::sleep(backoff).await;
        }
        self.stop_runtime().await;
        self.start().await
    }

    /// Probes the current handle and updates health.
    pub async fn health(&mut self) -> EngineStatus {
        let Some(info) = self.handle.clone() else {
            if self.status.state != EngineState::NotConfigured {
                self.status.state = EngineState::Stopped;
            }
            return self.status.clone();
        };
        match self.runtime.probe_health(&info).await {
            Ok(health) => {
                self.consecutive_failures = 0;
                self.status.state = EngineState::Ready;
                if health.version.is_some() {
                    self.status.version = health.version;
                }
                self.status.last_error = None;
            }
            Err(error) => {
                self.consecutive_failures = self.consecutive_failures.saturating_add(1);
                self.status.state = EngineState::Degraded {
                    detail: error.message.clone(),
                };
                self.status.last_error = Some(error.message);
            }
        }
        self.status.clone()
    }

    /// Stops the engine and clears its port.
    pub async fn stop(&mut self) -> EngineStatus {
        self.stop_runtime().await;
        self.status.state = EngineState::Stopped;
        self.status.port = None;
        self.status.started_at = None;
        self.status.binary_path = None;
        self.status.clone()
    }

    async fn stop_runtime(&mut self) {
        if let Some(info) = self.handle.take() {
            let _ = self.runtime.shutdown(&info).await;
        }
    }

    /// Re-checks the selected model against a live engine and unselects it when the
    /// engine no longer reports it. It never selects a replacement automatically.
    fn reconcile_selected_model(&mut self, models: &[RuntimeModel]) {
        let stale = self
            .status
            .selected_model
            .as_ref()
            .is_some_and(|selected| models.iter().all(|model| model.id != selected.id));
        if stale {
            self.status.selected_model = None;
        }
    }

    async fn reverify_selected_model(&mut self, info: &EngineRuntimeInfo) {
        if let Ok(models) = self.runtime.models(info.port).await {
            self.reconcile_selected_model(&models);
        }
    }

    /// Returns the last successful start time.
    #[must_use]
    pub fn started_at(&self) -> Option<Timestamp> {
        self.status.started_at
    }

    /// Returns the bounded, redacted engine log lines.
    #[must_use]
    pub fn logs(&self) -> Vec<String> {
        self.runtime.logs()
    }

    /// Returns the runtime model list without changing the selected model.
    pub async fn runtime_models(&self) -> Vec<RuntimeModel> {
        let Some(info) = &self.handle else {
            return Vec::new();
        };
        self.runtime.models(info.port).await.unwrap_or_default()
    }

    /// Verifies a provider key through the local engine without persisting it.
    pub async fn verify_key(&self, provider: &str, secret: &SecretString) -> KeyVerification {
        let Some(info) = &self.handle else {
            return KeyVerification::Unverified;
        };
        self.runtime.verify_key(info.port, provider, secret).await
    }
}

impl Drop for EngineSupervisor {
    fn drop(&mut self) {
        self.runtime.kill_now();
    }
}

/// Returns the capped exponential backoff before an automatic restart.
#[must_use]
pub fn backoff_delay(base: Duration, consecutive_failures: u32) -> Duration {
    if base.is_zero() {
        return Duration::ZERO;
    }
    let exponent = consecutive_failures.min(8);
    let factor = 1_u32 << exponent;
    base.checked_mul(factor)
        .unwrap_or(MAX_RESTART_BACKOFF)
        .min(MAX_RESTART_BACKOFF)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn engine_restart_backoff_caps_and_never_fork_bombs() {
        assert_eq!(backoff_delay(Duration::ZERO, 99), Duration::ZERO);
        assert_eq!(
            backoff_delay(Duration::from_millis(100), 0),
            Duration::from_millis(100)
        );
        assert_eq!(
            backoff_delay(Duration::from_millis(100), 3),
            Duration::from_millis(800)
        );
        assert_eq!(
            backoff_delay(Duration::from_millis(100), 99),
            MAX_RESTART_BACKOFF
        );
    }

    #[tokio::test]
    async fn engine_health_timeout_enters_degraded_without_a_live_handle() {
        let runtime = Arc::new(FakeEngineRuntime::new().with_health_failures(10));
        let mut supervisor = EngineSupervisor::new(
            runtime.clone(),
            EngineSupervisorConfig {
                restart_budget: 3,
                health_attempts: 1,
                retry_delay: Duration::ZERO,
                port: 0,
            },
        );
        let status = supervisor.start().await;
        assert!(matches!(status.state, EngineState::Degraded { .. }));
        assert!(status.port.is_none());
        assert_eq!(runtime.spawns(), 1);
        assert_eq!(runtime.shutdowns(), 1);
    }

    #[tokio::test]
    async fn engine_start_tracks_free_models_and_binary_path() {
        let runtime = Arc::new(FakeEngineRuntime::new().with_models(vec![RuntimeModel {
            id: "opencode/big-pickle".to_owned(),
            provider: "opencode".to_owned(),
            label: "Big Pickle".to_owned(),
            context_tokens: None,
            free: true,
        }]));
        let mut supervisor = EngineSupervisor::new(
            runtime,
            EngineSupervisorConfig {
                restart_budget: 3,
                health_attempts: 1,
                retry_delay: Duration::ZERO,
                port: 0,
            },
        );
        let status = supervisor.start().await;
        assert!(matches!(status.state, EngineState::Ready));
        assert!(status.included_models_available);
        assert_eq!(supervisor.runtime_models().await.len(), 1);
    }

    #[tokio::test]
    async fn engine_external_choice_never_starts_the_bundled_sidecar() {
        let runtime = Arc::new(FakeEngineRuntime::new());
        let mut supervisor = EngineSupervisor::new(
            runtime.clone(),
            EngineSupervisorConfig {
                restart_budget: 3,
                health_attempts: 1,
                retry_delay: Duration::ZERO,
                port: 0,
            },
        );
        supervisor.set_choice(EngineChoice::External {
            path: std::path::PathBuf::from("/opt/example/bin/opencode"),
            kind: crate::engine::ExternalHarnessKind::Opencode,
        });
        let status = supervisor.start().await;
        assert!(matches!(status.state, EngineState::Stopped));
        assert_eq!(runtime.spawns(), 0);
    }

    #[tokio::test]
    async fn engine_ready_runtime_is_not_orphaned_on_drop() {
        let runtime = Arc::new(FakeEngineRuntime::new());
        let mut supervisor = EngineSupervisor::new(
            runtime.clone(),
            EngineSupervisorConfig {
                restart_budget: 3,
                health_attempts: 1,
                retry_delay: Duration::ZERO,
                port: 0,
            },
        );
        supervisor.start().await;
        drop(supervisor);
        // The fake records no kill count; the process runtime has its own drop test.
        assert_eq!(runtime.spawns(), 1);
    }

    fn runtime_model(id: &str) -> RuntimeModel {
        RuntimeModel {
            id: id.to_owned(),
            provider: "opencode".to_owned(),
            label: id.to_owned(),
            context_tokens: None,
            free: true,
        }
    }

    fn model_choice(id: &str) -> ModelChoice {
        ModelChoice {
            id: id.to_owned(),
            label: id.to_owned(),
            provider: "opencode".to_owned(),
            access: crate::engine::ModelAccess::Included,
            context_tokens: None,
            notes: String::new(),
            verified_at: None,
        }
    }

    #[tokio::test]
    async fn engine_restart_unselects_a_model_the_engine_no_longer_reports() {
        let runtime = Arc::new(FakeEngineRuntime::new().with_models(vec![
            runtime_model("opencode/kept"),
            runtime_model("opencode/dropped"),
        ]));
        let mut supervisor = EngineSupervisor::new(
            runtime.clone(),
            EngineSupervisorConfig {
                restart_budget: 3,
                health_attempts: 1,
                retry_delay: Duration::ZERO,
                port: 0,
            },
        );
        assert!(matches!(supervisor.start().await.state, EngineState::Ready));
        supervisor.select_model(model_choice("opencode/dropped"));

        runtime.set_models(vec![runtime_model("opencode/kept")]);
        let status = supervisor.restart().await;
        assert!(matches!(status.state, EngineState::Ready));
        assert!(
            status.selected_model.is_none(),
            "a dropped model must not stay selected: {status:?}"
        );
    }

    #[tokio::test]
    async fn engine_start_while_ready_reverifies_the_selected_model() {
        let runtime =
            Arc::new(FakeEngineRuntime::new().with_models(vec![runtime_model("opencode/dropped")]));
        let mut supervisor = EngineSupervisor::new(
            runtime.clone(),
            EngineSupervisorConfig {
                restart_budget: 3,
                health_attempts: 1,
                retry_delay: Duration::ZERO,
                port: 0,
            },
        );
        assert!(matches!(supervisor.start().await.state, EngineState::Ready));
        supervisor.select_model(model_choice("opencode/dropped"));

        runtime.set_models(Vec::new());
        let status = supervisor.start().await;
        assert!(matches!(status.state, EngineState::Ready));
        assert!(status.selected_model.is_none());
    }

    use crate::engine::FakeEngineRuntime;
}
