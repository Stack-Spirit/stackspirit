//! The engine runtime boundary and a deterministic fake for tests.

use std::sync::Mutex;

use async_trait::async_trait;

use crate::error::{SpiritError, SpiritErrorCode};

/// What a successful engine spawn returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineRuntimeInfo {
    /// Child process id, when known.
    pub pid: Option<u32>,
    /// Loopback port. Never a public interface.
    pub port: u16,
    /// Engine version, when known.
    pub version: Option<String>,
}

/// The process boundary implemented by the bundled sidecar adapter in D4.
#[async_trait]
pub trait EngineRuntime: Send + Sync {
    /// Starts the engine on a loopback port and returns its info.
    async fn spawn(&self, port: u16) -> Result<EngineRuntimeInfo, SpiritError>;
    /// Probes the engine health endpoint.
    async fn probe_health(&self, info: &EngineRuntimeInfo) -> Result<(), SpiritError>;
    /// Stops the engine, ensuring no orphan process remains.
    async fn shutdown(&self, info: &EngineRuntimeInfo) -> Result<(), SpiritError>;
}

#[derive(Debug, Default)]
struct FakeRuntimeState {
    spawn_failures: usize,
    health_failures: usize,
    spawns: usize,
    shutdowns: usize,
    port: u16,
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
        })
    }

    async fn probe_health(&self, info: &EngineRuntimeInfo) -> Result<(), SpiritError> {
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
        Ok(())
    }

    async fn shutdown(&self, _info: &EngineRuntimeInfo) -> Result<(), SpiritError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.shutdowns = state.shutdowns.saturating_add(1);
        Ok(())
    }
}
