//! App-private CLI paths and the injected core boundary.
//!
//! The CLI keeps its own store under `$XDG_CONFIG_HOME/stackspirit` (or
//! `~/.config/stackspirit`). It never reads the desktop app's data directory, a
//! provider key, a `.env` file, or `~/.vowp`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use spirit_core::error::{SpiritError, SpiritErrorCode};
use spirit_core::host::{HostProfile, HostStore, JsonHostStore};
use spirit_core::ssh::{CommandRunner, SshOptions, SystemCommandRunner};
use spirit_core::vowp::{SshVowpClient, VowpClient};

use crate::error::CliError;

const APP_DIR: &str = "stackspirit";

/// The CLI's app-private config directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliPaths {
    root: PathBuf,
}

impl CliPaths {
    /// Resolves the directory from `XDG_CONFIG_HOME` or `HOME`.
    pub fn from_env() -> Result<Self, CliError> {
        let xdg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
        let home = std::env::var_os("HOME").map(PathBuf::from);
        Self::resolve(xdg.as_deref(), home.as_deref())
    }

    /// Resolves the directory from explicit values. A relative `XDG_CONFIG_HOME` is
    /// ignored, matching the XDG base-directory specification.
    pub fn resolve(xdg_config_home: Option<&Path>, home: Option<&Path>) -> Result<Self, CliError> {
        let root = if let Some(xdg) = xdg_config_home.filter(|path| path.is_absolute()) {
            xdg.join(APP_DIR)
        } else if let Some(home) = home {
            home.join(".config").join(APP_DIR)
        } else {
            return Err(CliError::new(
                SpiritError::new(
                    SpiritErrorCode::Io,
                    "cannot locate the StackSpirit config directory",
                )
                .with_hint("Set XDG_CONFIG_HOME or HOME to an absolute directory."),
            ));
        };
        Ok(Self::for_root(root))
    }

    /// Builds paths rooted at an explicit directory. Tests pass a temporary directory.
    #[must_use]
    pub fn for_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Returns the root directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the versioned JSON host store path.
    #[must_use]
    pub fn hosts_file(&self) -> PathBuf {
        self.root.join("hosts.json")
    }

    /// Returns the app-private `known_hosts` path.
    #[must_use]
    pub fn known_hosts_file(&self) -> PathBuf {
        self.root.join("known_hosts")
    }

    /// Returns the read-only local engine preferences path.
    #[must_use]
    pub fn engine_file(&self) -> PathBuf {
        self.root.join("engine.json")
    }

    /// Creates the root directory with Unix mode 0700.
    ///
    /// Read-only commands never call this; writes create it lazily.
    pub fn ensure_root(&self) -> Result<(), CliError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = std::fs::DirBuilder::new();
            builder.recursive(true).mode(0o700);
            builder.create(&self.root).map_err(|error| {
                SpiritError::io("creating the StackSpirit config directory", &error)
            })?;
        }
        #[cfg(not(unix))]
        {
            std::fs::create_dir_all(&self.root).map_err(|error| {
                SpiritError::io("creating the StackSpirit config directory", &error)
            })?;
        }

        let metadata = std::fs::metadata(&self.root)
            .map_err(|error| SpiritError::io("reading the StackSpirit config directory", &error))?;
        if !metadata.is_dir() {
            return Err(CliError::new(SpiritError::new(
                SpiritErrorCode::Io,
                "the StackSpirit config path is not a directory",
            )));
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = metadata.permissions();
            permissions.set_mode(0o700);
            std::fs::set_permissions(&self.root, permissions).map_err(|error| {
                SpiritError::io("securing the StackSpirit config directory", &error)
            })?;
        }
        Ok(())
    }
}

/// Everything a command may touch: paths plus the injected command runner.
///
/// Production uses [`SystemCommandRunner`]. Tests inject [`spirit_core::ssh::FakeCommandRunner`]
/// so no test opens a socket or reads a real keychain.
#[derive(Clone)]
pub struct CliEnv {
    paths: CliPaths,
    runner: Arc<dyn CommandRunner>,
    local_runner: Arc<dyn CommandRunner>,
}

impl CliEnv {
    /// Builds the production environment.
    #[must_use]
    pub fn system(paths: CliPaths) -> Self {
        Self {
            paths,
            runner: Arc::new(SystemCommandRunner),
            local_runner: Arc::new(SystemCommandRunner),
        }
    }

    /// Replaces the remote command runner.
    #[must_use]
    pub fn with_runner(mut self, runner: Arc<dyn CommandRunner>) -> Self {
        self.runner = runner;
        self
    }

    /// Replaces the local runner used by the local-agent gate probe.
    #[must_use]
    pub fn with_local_runner(mut self, runner: Arc<dyn CommandRunner>) -> Self {
        self.local_runner = runner;
        self
    }

    /// Returns the config paths.
    #[must_use]
    pub fn paths(&self) -> &CliPaths {
        &self.paths
    }

    /// Returns the remote runner.
    #[must_use]
    pub fn runner(&self) -> Arc<dyn CommandRunner> {
        self.runner.clone()
    }

    /// Returns the local runner.
    #[must_use]
    pub fn local_runner(&self) -> Arc<dyn CommandRunner> {
        self.local_runner.clone()
    }

    /// Builds the JSON host store over the CLI config directory.
    #[must_use]
    pub fn host_store(&self) -> Arc<dyn HostStore> {
        Arc::new(JsonHostStore::new(self.paths.hosts_file()))
    }

    /// Builds SSH options with the app-private `known_hosts` and an optional identity
    /// file path (the file is never read).
    #[must_use]
    pub fn ssh_options(&self, identity_file: Option<PathBuf>) -> SshOptions {
        SshOptions::new(self.paths.known_hosts_file()).with_identity_file(identity_file)
    }

    /// Builds a live client for one saved profile.
    #[must_use]
    pub fn client_for(&self, host: &HostProfile) -> Arc<dyn VowpClient> {
        Arc::new(SshVowpClient::new(
            self.runner.clone(),
            host.ssh.clone(),
            self.ssh_options(host.identity_file.clone()),
            host.vowp_home.clone(),
        ))
    }
}
