//! Agent configuration and private file helpers.
//!
//! The state directory is `~/.vowp-agent` by default. `VOWP_AGENT_HOME` overrides it for tests
//! and portable installs. `agent.json` and `status.json` are created 0600; the directory is
//! created 0700. Paired surface tokens live below `surfaces/<id>/token`, also 0600, in a
//! 0700 directory.

use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use vowp_protocol::{NodeName, SurfaceId};

use crate::error::AgentError;

/// Default directory name below `$HOME`.
pub const AGENT_DIR_NAME: &str = ".vowp-agent";
/// Enrollment config file name.
pub const CONFIG_FILE: &str = "agent.json";
/// Last reported status file name.
pub const STATUS_FILE: &str = "status.json";
/// Directory holding one subdirectory per paired surface.
pub const SURFACES_DIR: &str = "surfaces";
/// Secret surface token file name inside a surface directory.
pub const SURFACE_TOKEN_FILE: &str = "token";
/// RFC 3339 surface-token expiry file name inside a surface directory.
pub const SURFACE_EXPIRY_FILE: &str = "expires_at";

/// The persisted enrollment and transport settings of this agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Home identity returned by `Enrolled.home_id`.
    pub home: String,
    /// Node name assigned by the home.
    pub node: NodeName,
    /// Local state directory (`~/.vowp-agent`).
    pub state_dir: PathBuf,
    /// How to reach the home.
    pub transport: TransportConfig,
}

/// The transport section of [`AgentConfig`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TransportConfig {
    /// The Phase 0 SSH stdio gateway.
    Ssh {
        /// ssh destination of the home.
        target: String,
    },
}

impl AgentConfig {
    /// Builds an SSH-backed config.
    pub fn new(home: impl Into<String>, node: NodeName, state_dir: PathBuf, target: impl Into<String>) -> Self {
        Self {
            home: home.into(),
            node,
            state_dir,
            transport: TransportConfig::Ssh { target: target.into() },
        }
    }

    /// Returns the default state directory (`$VOWP_AGENT_HOME`, else `$HOME/.vowp-agent`).
    pub fn default_state_dir() -> Result<PathBuf, AgentError> {
        if let Some(dir) = std::env::var_os("VOWP_AGENT_HOME") {
            return Ok(PathBuf::from(dir));
        }
        let home = std::env::var_os("HOME")
            .ok_or_else(|| AgentError::Config("HOME is not set; pass VOWP_AGENT_HOME".to_owned()))?;
        Ok(PathBuf::from(home).join(AGENT_DIR_NAME))
    }

    /// Returns `state_dir/agent.json`.
    pub fn file_path(state_dir: &Path) -> PathBuf {
        state_dir.join(CONFIG_FILE)
    }

    /// Returns `state_dir/surfaces`.
    pub fn surfaces_dir(state_dir: &Path) -> PathBuf {
        state_dir.join(SURFACES_DIR)
    }

    /// Returns `state_dir/surfaces/<id>`.
    pub fn surface_dir(state_dir: &Path, surface: &SurfaceId) -> PathBuf {
        Self::surfaces_dir(state_dir).join(&surface.0)
    }

    /// Returns `state_dir/surfaces/<id>/token`.
    pub fn surface_token_path(state_dir: &Path, surface: &SurfaceId) -> PathBuf {
        Self::surface_dir(state_dir, surface).join(SURFACE_TOKEN_FILE)
    }

    /// Returns `state_dir/surfaces/<id>/expires_at`.
    pub fn surface_expiry_path(state_dir: &Path, surface: &SurfaceId) -> PathBuf {
        Self::surface_dir(state_dir, surface).join(SURFACE_EXPIRY_FILE)
    }

    /// Loads `agent.json` from `state_dir`.
    pub fn load(state_dir: &Path) -> Result<Self, AgentError> {
        let path = Self::file_path(state_dir);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(AgentError::NotEnrolled(format!("{} is missing", path.display())));
            }
            Err(error) => return Err(AgentError::Io(error)),
        };
        let config: Self = serde_json::from_str(&text)?;
        config.validate()?;
        Ok(config)
    }

    /// Writes `agent.json` atomically enough for one local process, mode 0600.
    pub fn write(&self) -> Result<(), AgentError> {
        self.validate()?;
        let bytes = serde_json::to_vec_pretty(self)?;
        write_private(&Self::file_path(&self.state_dir), &bytes)
    }

    /// Returns the configured SSH target.
    pub fn ssh_target(&self) -> Result<&str, AgentError> {
        match &self.transport {
            TransportConfig::Ssh { target } => {
                if target.trim().is_empty() {
                    Err(AgentError::Config("ssh target must not be empty".to_owned()))
                } else {
                    Ok(target)
                }
            }
        }
    }

    fn validate(&self) -> Result<(), AgentError> {
        if self.home.trim().is_empty() {
            return Err(AgentError::Config("home name must not be empty".to_owned()));
        }
        self.ssh_target()?;
        Ok(())
    }
}

/// Creates `path` and every missing parent, then makes `path` mode 0700.
pub(crate) fn ensure_private_dir(path: &Path) -> Result<(), AgentError> {
    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

/// Writes a private file (0600) below a 0700 directory.
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<(), AgentError> {
    write_file(path, bytes, 0o600)
}

/// Writes a public file (0644).
pub(crate) fn write_public(path: &Path, bytes: &[u8]) -> Result<(), AgentError> {
    write_file(path, bytes, 0o644)
}

fn write_file(path: &Path, bytes: &[u8], mode: u32) -> Result<(), AgentError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(mode)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}
