//! Node role discovery: home, control machine or uninitialised. C0 creates the types; C2/D2 fill it.
//!
//! Spec: `docs/design/v0.2/home-and-surfaces.md` §8.1–§8.2 and §13.6.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use vowp_protocol::{Snapshot, Timestamp};
use vowp_store::NodePaths;

use crate::CliError;
use crate::cli::Global;

/// Which side of a home/control pair this machine is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeRole {
    /// The machine that holds the database and runs the supervisor.
    Home,
    /// A machine that reaches a home over SSH.
    Control,
    /// No role has been configured yet.
    Uninitialised,
}

/// The `control.json` document of a control machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlConfig {
    /// Always `control`.
    pub role: String,
    /// Node name of the home.
    pub home: String,
    /// SSH destination of the home.
    pub ssh_target: String,
    /// Absolute home path on the remote machine.
    pub remote_home: String,
    /// When the control machine was configured.
    pub created_at: Timestamp,
}

/// The control-machine directory and its config.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlHome {
    /// `~/.vowp-control` (or `$VOWP_CONTROL_HOME`).
    pub root: PathBuf,
    /// Parsed `control.json`.
    pub config: ControlConfig,
}

/// A home reached over SSH.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteHome {
    /// Home node name.
    pub name: String,
    /// SSH destination.
    pub ssh_target: String,
    /// Absolute home path on the remote machine.
    pub remote_home: String,
}

/// A snapshot read from the control cache.
#[derive(Debug, Clone, PartialEq)]
pub struct CachedSnapshot {
    /// The snapshot.
    pub snapshot: Snapshot,
    /// When it was fetched.
    pub fetched_at: Timestamp,
    /// `remote` or `cache`.
    pub source: String,
}

/// The resolved role of this machine and its node paths.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeCtx {
    role: NodeRole,
    paths: NodePaths,
    control: Option<ControlHome>,
    target: Option<RemoteHome>,
}

impl NodeCtx {
    /// Resolves the role from flags, `VOWP_CONTROL_HOME`, `VOWP_HOME` and the filesystem.
    pub fn discover(global: &Global) -> Result<NodeCtx, CliError> {
        if let Some(home) = &global.home {
            return Ok(Self {
                role: NodeRole::Home,
                paths: NodePaths::new(home),
                control: None,
                target: None,
            });
        }
        if let Some(control) = read_control_home()? {
            let target = RemoteHome {
                name: control.config.home.clone(),
                ssh_target: control.config.ssh_target.clone(),
                remote_home: control.config.remote_home.clone(),
            };
            return Ok(Self {
                role: NodeRole::Control,
                paths: NodePaths::new(&control.root),
                control: Some(control),
                target: Some(target),
            });
        }
        let paths = crate::ctx::node_paths(global)?;
        let role = if paths.db_path().is_file() {
            NodeRole::Home
        } else {
            NodeRole::Uninitialised
        };
        Ok(Self {
            role,
            paths,
            control: None,
            target: None,
        })
    }

    /// Returns the node paths, requiring the home role.
    pub fn require_home(&self) -> Result<&NodePaths, CliError> {
        if self.role == NodeRole::Home {
            Ok(&self.paths)
        } else {
            Err(CliError::Guard(
                "this command needs the home; run it on the machine that holds the node".to_owned(),
            ))
        }
    }

    /// Returns the control configuration, requiring the control role.
    pub fn require_control(&self) -> Result<&ControlHome, CliError> {
        self.control
            .as_ref()
            .ok_or_else(|| CliError::Guard("this machine is not a control machine".to_owned()))
    }

    /// Returns the remote home, when this is a control machine.
    pub fn remote(&self) -> Option<&RemoteHome> {
        self.target.as_ref()
    }

    /// Writes the control snapshot cache.
    pub fn cache_snapshot(&self, _snapshot: &Snapshot) -> Result<(), CliError> {
        Err(CliError::NotImplemented("not implemented in this build"))
    }

    /// Reads the control snapshot cache.
    pub fn cached_snapshot(&self) -> Result<Option<CachedSnapshot>, CliError> {
        Ok(None)
    }

    /// Routes one command to the home and returns its JSON stdout.
    pub fn route_json(&self, _args: &[String]) -> Result<String, CliError> {
        Err(CliError::NotImplemented("not implemented in this build"))
    }
}

/// Returns the control home, when one is configured.
fn read_control_home() -> Result<Option<ControlHome>, CliError> {
    let root = match std::env::var_os("VOWP_CONTROL_HOME").filter(|value| !value.is_empty()) {
        Some(root) => PathBuf::from(root),
        None => {
            let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) else {
                return Ok(None);
            };
            PathBuf::from(home).join(".vowp-control")
        }
    };
    let config_path = root.join("control.json");
    if !config_path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&config_path)?;
    let config: ControlConfig = serde_json::from_str(&text)
        .map_err(|error| CliError::Invalid(format!("{}: {error}", config_path.display())))?;
    Ok(Some(ControlHome { root, config }))
}

/// Returns true when `path` is a usable control directory.
pub fn is_control_dir(path: &Path) -> bool {
    path.join("control.json").is_file()
}
