//! Process-wide app state. All remote work is delegated to `spirit-core`; this module
//! only owns paths, the active client and the one host connection.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use spirit_core::SpiritCore;
use spirit_core::connection::ConnectionManager;
use spirit_core::engine::{EngineSupervisor, EngineSupervisorConfig, FakeEngineRuntime};
use spirit_core::error::SpiritError;
use spirit_core::host::{HostProfile, HostStore, JsonHostStore};
use spirit_core::ssh::{CommandRunner, SshOptions, SystemCommandRunner};
use spirit_core::vowp::{SshVowpClient, VowpClient};
use tokio::sync::{Mutex as AsyncMutex, broadcast};

use crate::error::CommandResult;

/// Everything a Tauri command may touch.
pub struct AppState {
    core: AsyncMutex<SpiritCore>,
    hosts: Arc<dyn HostStore>,
    known_hosts: PathBuf,
    runner: Arc<dyn CommandRunner>,
    client: Mutex<Option<Arc<dyn VowpClient>>>,
    active_host: Mutex<Option<HostProfile>>,
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

        // D2 ships the engine boundary and state machine. D4 replaces the fake runtime
        // with the bundled opencode sidecar launcher.
        let engine = EngineSupervisor::new(
            Arc::new(FakeEngineRuntime::new()),
            EngineSupervisorConfig::default(),
        );
        let core = SpiritCore::desktop(hosts.clone(), engine, "com.stackspirit.desktop");
        let events = core.subscribe();

        Ok((
            Self {
                core: AsyncMutex::new(core),
                hosts,
                known_hosts,
                runner: Arc::new(SystemCommandRunner),
                client: Mutex::new(None),
                active_host: Mutex::new(None),
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

    pub(crate) fn runner(&self) -> Arc<dyn CommandRunner> {
        self.runner.clone()
    }

    pub(crate) fn ssh_options(&self, identity_file: Option<PathBuf>) -> SshOptions {
        SshOptions::new(&self.known_hosts).with_identity_file(identity_file)
    }

    pub(crate) async fn connection_manager(&self) -> ConnectionManager {
        self.core.lock().await.connection().clone()
    }

    pub(crate) fn set_client(&self, client: Option<Arc<dyn VowpClient>>) {
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
