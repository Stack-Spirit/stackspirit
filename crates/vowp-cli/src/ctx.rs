//! The per-command context: node paths, parsed config, dependencies and rendering flags.
//!
//! Spec: `docs/design/crates/vowp-cli.md` §1–§2. [`Ctx::build`] never creates the node home:
//! commands other than `init` fail with `not found` and the hint to run `vowp init`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use vowp_core::deps::SystemRng;
use vowp_core::{AdapterRegistry, Deps, SystemGit, SystemRunner};
use vowp_protocol::{Clock, NodeFile, NullLog, ProjectName, SystemClock, VowpConfig};
use vowp_store::{NodePaths, Store};
use vowp_tmux::{Tmux, WindowManager};

use crate::cli::Global;
use crate::{CliError, IoStreams};

/// The keys a `[loops.<name>]` table may contain, matching `vowp_protocol::config`.
const LOOP_KEYS: [&str; 11] = [
    "interval_secs",
    "state",
    "max_continues",
    "stall_minutes",
    "silent_minutes",
    "max_hang_restarts",
    "same_action_max",
    "probe_interval_minutes",
    "planner_min_gap_minutes",
    "idle_gap_minutes",
    "checkpoint_minutes",
];

/// Everything a command handler needs.
pub struct Ctx {
    paths: NodePaths,
    deps: Deps,
    project: ProjectName,
    project_root: PathBuf,
    json: bool,
    color: bool,
}

impl Ctx {
    /// Builds the context from the global flags without creating the node home.
    ///
    /// Errors: `NotFound` when the node database or project config is missing,
    /// `InvalidInput` when no project can be selected.
    pub fn build(global: &Global, io: &dyn IoStreams) -> Result<Ctx, CliError> {
        let paths = node_paths(global)?;
        if !paths.db_path().is_file() {
            return Err(CliError::NotFound(
                "vowp home is not initialised; run `vowp init`".to_owned(),
            ));
        }
        let project_root = resolve_project_root(global, &paths)?;
        let config_path = project_root.join("vowp.toml");
        if !config_path.is_file() {
            return Err(CliError::NotFound(format!(
                "project config {}; run `vowp init`",
                config_path.display()
            )));
        }
        let config = Arc::new(load_project_config(&config_path, &paths)?);
        let project = config.project.name.clone();
        let node_name = config.node.name.clone();

        let cmd: Arc<dyn vowp_core::CommandRunner> = Arc::new(SystemRunner);
        let git = Arc::new(SystemGit::new(Arc::clone(&cmd)));
        let windows: Arc<dyn WindowManager> = Arc::new(Tmux::for_home(paths.root()));
        let adapters = Arc::new(AdapterRegistry::from_config(&config)?);
        let deps = Deps {
            paths: paths.clone(),
            config,
            node: node_name,
            clock: Arc::new(SystemClock),
            log: Arc::new(NullLog),
            adapters,
            windows,
            git,
            cmd,
            rng: Arc::new(SystemRng::new()),
        };

        let color = !global.no_color && io.is_tty() && std::env::var_os("NO_COLOR").is_none();
        Ok(Ctx {
            paths,
            deps,
            project,
            project_root,
            json: global.json,
            color,
        })
    }

    /// Returns the node paths.
    pub fn paths(&self) -> &NodePaths {
        &self.paths
    }

    /// Returns the injected dependencies.
    pub fn deps(&self) -> &Deps {
        &self.deps
    }

    /// Returns the parsed project config.
    pub fn config(&self) -> &VowpConfig {
        &self.deps.config
    }

    /// Returns the project name.
    pub fn project(&self) -> &ProjectName {
        &self.project
    }

    /// Returns the directory containing `vowp.toml`.
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// Returns the clock used by the supervisor.
    pub fn clock(&self) -> &dyn Clock {
        self.deps.clock.as_ref()
    }

    /// Opens a fresh store connection.
    pub fn store(&self) -> Result<Store, CliError> {
        Ok(self.deps.open_store()?)
    }

    /// Returns whether `--json` was passed.
    pub fn json(&self) -> bool {
        self.json
    }

    /// Returns whether human output may use ANSI colour.
    pub fn color(&self) -> bool {
        self.color
    }

    /// Returns the configured UTC offset in minutes.
    pub fn tz_offset_minutes(&self) -> i32 {
        self.deps.config.node.tz_offset_minutes
    }
}

/// Returns the node home from `--home`, `$VOWP_HOME`, or `~/.vowp`.
pub fn node_paths(global: &Global) -> Result<NodePaths, CliError> {
    match &global.home {
        Some(home) => Ok(NodePaths::new(home)),
        None => Ok(NodePaths::from_env()?),
    }
}

/// Resolves the project root from `--project`, the cwd, or the only registered project.
fn resolve_project_root(global: &Global, paths: &NodePaths) -> Result<PathBuf, CliError> {
    if let Some(value) = &global.project {
        let candidate = PathBuf::from(value);
        if candidate.is_dir() {
            if candidate.join("vowp.toml").is_file() {
                return Ok(candidate);
            }
            return Err(CliError::NotFound(format!(
                "no vowp.toml in {}; run `vowp init`",
                candidate.display()
            )));
        }
        let name = ProjectName::parse(value)?;
        let store = Store::open(paths)?;
        let project = store
            .project(&name)?
            .ok_or_else(|| CliError::NotFound(format!("project {name}; run `vowp init`")))?;
        let root = PathBuf::from(project.path);
        if !root.join("vowp.toml").is_file() {
            return Err(CliError::NotFound(format!(
                "no vowp.toml in {}; run `vowp init`",
                root.display()
            )));
        }
        return Ok(root);
    }

    let cwd = std::env::current_dir()?;
    if let Some(root) = find_project_root(&cwd) {
        return Ok(root);
    }

    let store = Store::open(paths)?;
    let projects = store.list_projects()?;
    match projects.as_slice() {
        [project] => {
            let root = PathBuf::from(&project.path);
            if !root.join("vowp.toml").is_file() {
                return Err(CliError::NotFound(format!(
                    "no vowp.toml in {}; run `vowp init`",
                    root.display()
                )));
            }
            Ok(root)
        }
        [] => Err(CliError::Invalid("no project: run `vowp init`".to_owned())),
        _ => Err(CliError::Invalid(
            "several projects are registered; pass --project <name|path>".to_owned(),
        )),
    }
}

/// Walks from `start` to the filesystem root looking for `vowp.toml`.
pub fn find_project_root(start: &Path) -> Option<PathBuf> {
    for directory in start.ancestors() {
        if directory.join("vowp.toml").is_file() {
            return Some(directory.to_path_buf());
        }
    }
    None
}

/// Walks from `start` to the filesystem root looking for `.git`.
pub fn find_git_root(start: &Path) -> Option<PathBuf> {
    for directory in start.ancestors() {
        if directory.join(".git").exists() {
            return Some(directory.to_path_buf());
        }
    }
    None
}

/// Returns the live supervisor pid from `<home>/supervisor.lock`, if one holds the lock.
///
/// The lock file contains the pid written by `vowp up`; a stale file (no such process) is
/// treated as "not running". Linux reports process existence through `/proc/<pid>`.
pub fn supervisor_pid(paths: &NodePaths) -> Option<u32> {
    let text = std::fs::read_to_string(paths.root().join("supervisor.lock")).ok()?;
    let pid: u32 = text.lines().next()?.trim().parse().ok()?;
    if pid == 0 {
        return None;
    }
    if Path::new("/proc").is_dir() {
        Path::new(&format!("/proc/{pid}")).exists().then_some(pid)
    } else {
        Some(pid)
    }
}

/// Reads and validates `vowp.toml`, merging `~/.vowp/config.toml` defaults first.
///
/// `VowpConfig::from_toml` validates before callers can merge node defaults, so the CLI does
/// the documented order itself: parse, merge, validate. Unknown keys inside `[loops.*]`
/// are rejected here because the flattened `LoopBounds` cannot use `deny_unknown_fields`.
pub fn load_project_config(path: &Path, paths: &NodePaths) -> Result<VowpConfig, CliError> {
    let text = std::fs::read_to_string(path)?;
    reject_unknown_loop_keys(&text)?;
    let mut config: VowpConfig =
        toml::from_str(&text).map_err(|error| vowp_protocol::ProtocolError::Config(error.to_string()))?;

    let node_file_path = paths.root().join("config.toml");
    if node_file_path.is_file() {
        let node_text = std::fs::read_to_string(&node_file_path)?;
        let node_file = NodeFile::from_toml(&node_text)?;
        config = config.with_node_defaults(&node_file);
    }
    config.validate()?;
    Ok(config)
}

/// Rejects unknown keys inside any `[loops.<name>]` table.
fn reject_unknown_loop_keys(text: &str) -> Result<(), CliError> {
    let raw: toml::Value =
        toml::from_str(text).map_err(|error| vowp_protocol::ProtocolError::Config(error.to_string()))?;
    let Some(loops) = raw.get("loops").and_then(toml::Value::as_table) else {
        return Ok(());
    };
    let allowed: BTreeSet<&str> = LOOP_KEYS.into_iter().collect();
    for (name, value) in loops {
        let Some(entry) = value.as_table() else {
            continue;
        };
        for key in entry.keys() {
            if !allowed.contains(key.as_str()) {
                return Err(CliError::Protocol(vowp_protocol::ProtocolError::Config(format!(
                    "unknown key loops.{name}.{key}"
                ))));
            }
        }
    }
    Ok(())
}
