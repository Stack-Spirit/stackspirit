//! Engine supervision: start, health, restart with a bounded budget, and stop.

use std::sync::Arc;
use std::time::Duration;

use crate::engine::runtime::{EngineRuntime, EngineRuntimeInfo};
use crate::engine::{EngineChoice, EngineState, EngineStatus, ModelChoice};
use crate::error::SpiritError;
use crate::time::{Clock, SystemClock, Timestamp};

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
    pub async fn start(&mut self) -> EngineStatus {
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
                Ok(()) => {
                    self.consecutive_failures = 0;
                    self.status.state = EngineState::Ready;
                    self.status.version = info.version.clone();
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
            Ok(()) => {
                self.consecutive_failures = 0;
                self.status.state = EngineState::Ready;
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
        self.status.clone()
    }

    async fn stop_runtime(&mut self) {
        if let Some(info) = self.handle.take() {
            let _ = self.runtime.shutdown(&info).await;
        }
    }

    /// Returns the last successful start time.
    #[must_use]
    pub fn started_at(&self) -> Option<Timestamp> {
        self.status.started_at
    }
}
