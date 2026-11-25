//! The on-disk layout of one VOWP node.
//! Spec: `docs/design/crates/vowp-store.md` §3.1 and `docs/design/sessions.md` §1.
//!
//! [`NodePaths`] is a pure path calculator: construction performs no I/O and never reads
//! `$HOME`. Only [`NodePaths::from_env`] consults the environment, and only for the CLI and
//! supervisor entry points.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use vowp_protocol::{RunId, SessionId};

use crate::error::StoreError;

/// All files and directories beneath one node root (`~/.vowp` by default).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodePaths {
    root: PathBuf,
}

impl NodePaths {
    /// Builds the path set for `root` without touching the filesystem.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Resolves the node root from the process environment and `$HOME`.
    ///
    /// Follows [`resolve_state`] for [`StateKind::Home`]: `STACKSPIRIT_HOME`, then `VOWP_HOME`,
    /// then the real one of `~/.stackspirit` and `~/.vowp`. Pure: nothing is created or moved.
    /// Returns [`StoreError::NoHome`] when no path can be located and
    /// [`StoreError::InvalidInput`] for a home conflict or a missing explicit home.
    pub fn from_env() -> Result<Self, StoreError> {
        let resolved = resolve_state_from_env(StateKind::Home, None)?;
        Ok(Self::new(resolved.path))
    }

    /// Returns the node root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns `<root>/vowp.db`.
    pub fn db_path(&self) -> PathBuf {
        self.root.join("vowp.db")
    }

    /// Returns `<root>/projects`.
    pub fn projects_dir(&self) -> PathBuf {
        self.root.join("projects")
    }

    /// Returns `<root>/projects/<name>`.
    pub fn project_dir(&self, name: &vowp_protocol::ProjectName) -> PathBuf {
        self.projects_dir().join(name.as_str())
    }

    /// Returns `<root>/projects/<name>/project.db`.
    pub fn project_db_path(&self, name: &vowp_protocol::ProjectName) -> PathBuf {
        self.project_dir(name).join("project.db")
    }

    /// Returns `<root>/migration.json`, the restart manifest of the v0.1→v0.2 split.
    pub fn migration_manifest_path(&self) -> PathBuf {
        self.root.join("migration.json")
    }

    /// Returns `<root>/supervisor.lock`.
    pub fn supervisor_lock_path(&self) -> PathBuf {
        self.root.join("supervisor.lock")
    }

    /// Returns `<root>/sessions`.
    pub fn sessions_dir(&self) -> PathBuf {
        self.root.join("sessions")
    }

    /// Returns `<root>/sessions/<id>`.
    pub fn session_dir(&self, id: &SessionId) -> PathBuf {
        self.sessions_dir().join(id.as_str())
    }

    /// Returns `<root>/sessions/<id>/log.jsonl`.
    pub fn log_path(&self, id: &SessionId) -> PathBuf {
        self.session_dir(id).join("log.jsonl")
    }

    /// Returns `<root>/sessions/<id>/.lock`.
    pub fn lock_path(&self, id: &SessionId) -> PathBuf {
        self.session_dir(id).join(".lock")
    }

    /// Returns `<root>/sessions/<id>/blobs`.
    pub fn blobs_dir(&self, id: &SessionId) -> PathBuf {
        self.session_dir(id).join("blobs")
    }

    /// Returns `<root>/sessions/<id>/capsules`.
    pub fn capsules_dir(&self, id: &SessionId) -> PathBuf {
        self.session_dir(id).join("capsules")
    }

    /// Returns `<root>/sessions/<id>/adapters.json`.
    pub fn adapters_json_path(&self, id: &SessionId) -> PathBuf {
        self.session_dir(id).join("adapters.json")
    }

    /// Returns `<root>/sessions/<id>/manifest.json`.
    ///
    /// The file is written only by a full directory pull; `reindex` ignores it. A session
    /// directory without one is an ordinary local session.
    pub fn session_manifest_path(&self, id: &SessionId) -> PathBuf {
        self.session_dir(id).join("manifest.json")
    }

    /// Returns `<root>/logs` (supervisor log and other process output).
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// Returns `<root>/runs` (raw harness output mirrors).
    pub fn runs_dir(&self) -> PathBuf {
        self.root.join("runs")
    }

    /// Returns `<root>/runs/<run-id>.jsonl`.
    pub fn run_raw_path(&self, id: &RunId) -> PathBuf {
        self.runs_dir().join(format!("{id}.jsonl"))
    }

    /// Returns `<root>/backups`.
    pub fn backups_dir(&self) -> PathBuf {
        self.root.join("backups")
    }

    /// Returns `<root>/tmp` (atomic-write staging on the same filesystem).
    pub fn tmp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }

    /// Creates the node root and the directories every node always has.
    ///
    /// Creates `root`, `sessions`, `logs`, `runs`, `backups` and `tmp`; idempotent.
    pub fn ensure(&self) -> Result<(), StoreError> {
        for dir in [
            self.root.clone(),
            self.sessions_dir(),
            self.logs_dir(),
            self.runs_dir(),
            self.backups_dir(),
            self.tmp_dir(),
        ] {
            std::fs::create_dir_all(&dir).map_err(|err| StoreError::io(dir.display(), err))?;
        }
        Ok(())
    }
}

/// Which of the three per-machine state directories is being resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateKind {
    /// The node home (`~/.stackspirit` / `~/.vowp`), keyed by `vowp.db`.
    Home,
    /// The Mac agent state (`~/.stackspirit-agent` / `~/.vowp-agent`), keyed by `agent.json`.
    Agent,
    /// The control machine state (`~/.stackspirit-control` / `~/.vowp-control`), keyed by `control.json`.
    Control,
}

impl StateKind {
    /// Directory name under `$HOME` for the StackSpirit spelling.
    pub fn new_dir_name(self) -> &'static str {
        match self {
            Self::Home => ".stackspirit",
            Self::Agent => ".stackspirit-agent",
            Self::Control => ".stackspirit-control",
        }
    }

    /// Directory name under `$HOME` for the legacy spelling.
    pub fn legacy_dir_name(self) -> &'static str {
        match self {
            Self::Home => ".vowp",
            Self::Agent => ".vowp-agent",
            Self::Control => ".vowp-control",
        }
    }

    /// File whose presence marks a directory as a real, in-use state directory.
    pub fn marker(self) -> &'static str {
        match self {
            Self::Home => "vowp.db",
            Self::Agent => "agent.json",
            Self::Control => "control.json",
        }
    }

    /// The StackSpirit environment variable.
    pub fn new_env(self) -> &'static str {
        match self {
            Self::Home => "STACKSPIRIT_HOME",
            Self::Agent => "STACKSPIRIT_AGENT_HOME",
            Self::Control => "STACKSPIRIT_CONTROL_HOME",
        }
    }

    /// The legacy environment variable.
    pub fn legacy_env(self) -> &'static str {
        match self {
            Self::Home => "VOWP_HOME",
            Self::Agent => "VOWP_AGENT_HOME",
            Self::Control => "VOWP_CONTROL_HOME",
        }
    }
}

/// Where a resolved state directory came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateSource {
    /// The `--home` flag (or an equivalent explicit argument).
    Flag,
    /// An explicit environment variable, by name.
    Env(&'static str),
    /// A real directory found under `$HOME` in the StackSpirit spelling.
    Existing,
    /// A real directory found under `$HOME` in the legacy spelling.
    ExistingLegacy,
    /// Nothing exists yet; the StackSpirit default for a fresh machine.
    Fresh,
}

/// The outcome of resolving one state directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedState {
    /// Which directory kind this is.
    pub kind: StateKind,
    /// The selected path, as spelled (symlinks are not resolved in the result).
    pub path: PathBuf,
    /// Where the choice came from.
    pub source: StateSource,
    /// True when the selected path is spelled with the legacy name.
    pub legacy: bool,
    /// A one-line warning (both environment names set to different paths).
    pub warning: Option<String>,
}

impl ResolvedState {
    /// Fails with [`ResolveError::MissingExplicit`] when an explicit path does not exist.
    pub fn require_existing(&self) -> Result<(), ResolveError> {
        let explicit = matches!(self.source, StateSource::Env(_));
        if explicit && !self.path.exists() {
            let var = match &self.source {
                StateSource::Env(name) => *name,
                _ => self.kind.new_env(),
            };
            return Err(ResolveError::MissingExplicit {
                var,
                path: self.path.clone(),
            });
        }
        Ok(())
    }
}

/// Why a state directory could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResolveError {
    /// No explicit path was given and `$HOME` is unset.
    #[error("home directory not determinable (set {0} or HOME)")]
    NoHome(&'static str),
    /// Two distinct real directories exist under the two names.
    #[error(
        "both {new} and {old} hold {marker}; choose one with --home (or {env}) or remove the stray copy",
        new = new.display(), old = old.display()
    )]
    Conflict {
        /// The StackSpirit-named candidate.
        new: PathBuf,
        /// The legacy-named candidate.
        old: PathBuf,
        /// The marker file both contain.
        marker: &'static str,
        /// The environment variable that selects one.
        env: &'static str,
    },
    /// An explicit environment variable names a path that does not exist.
    #[error("{var} points at {path}, which does not exist", path = path.display())]
    MissingExplicit {
        /// The variable that was set.
        var: &'static str,
        /// The missing path.
        path: PathBuf,
    },
}

impl From<ResolveError> for StoreError {
    fn from(error: ResolveError) -> Self {
        match error {
            ResolveError::NoHome(_) => StoreError::NoHome,
            other => StoreError::InvalidInput(other.to_string()),
        }
    }
}

/// Resolves a state directory against the process environment and `$HOME`.
pub fn resolve_state_from_env(kind: StateKind, flag: Option<&Path>) -> Result<ResolvedState, ResolveError> {
    let home = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    resolve_state(kind, flag, |name| std::env::var_os(name), home.as_deref())
}

/// Resolves one state directory. Pure: only reads the filesystem, never creates, moves or deletes.
///
/// Order: `flag`, then the StackSpirit variable, then the legacy variable, then `$HOME/<new>` and
/// `$HOME/<legacy>`. A candidate is real when it contains [`StateKind::marker`]. Two candidates
/// that canonicalise to the same directory are one state directory (the new spelling is
/// preferred); two distinct real ones are a [`ResolveError::Conflict`]; none selects the new name
/// (except a legacy agent/control directory that exists without its marker yet).
pub fn resolve_state(
    kind: StateKind,
    flag: Option<&Path>,
    get: impl Fn(&str) -> Option<OsString>,
    home: Option<&Path>,
) -> Result<ResolvedState, ResolveError> {
    if let Some(path) = flag {
        return Ok(ResolvedState {
            kind,
            path: path.to_path_buf(),
            source: StateSource::Flag,
            legacy: false,
            warning: None,
        });
    }
    let new_value = get(kind.new_env()).filter(|value| !value.is_empty());
    let old_value = get(kind.legacy_env()).filter(|value| !value.is_empty());
    let explicit = match (&new_value, &old_value) {
        (Some(new), old) => {
            let warning = match old {
                Some(old) if !same_dir(Path::new(new), Path::new(old)) => Some(format!(
                    "{} and {} differ; using {}",
                    kind.new_env(),
                    kind.legacy_env(),
                    kind.new_env()
                )),
                _ => None,
            };
            Some((kind.new_env(), PathBuf::from(new), warning))
        }
        (None, Some(old)) => Some((kind.legacy_env(), PathBuf::from(old), None)),
        (None, None) => None,
    };
    if let Some((var, path, warning)) = explicit {
        return Ok(ResolvedState {
            kind,
            legacy: false,
            path,
            source: StateSource::Env(var),
            warning,
        });
    }
    let home = home.ok_or(ResolveError::NoHome(kind.new_env()))?;
    let new = home.join(kind.new_dir_name());
    let old = home.join(kind.legacy_dir_name());
    let real = |path: &Path| path.join(kind.marker()).is_file();
    let (new_real, old_real) = (real(&new), real(&old));
    let resolved = |path: PathBuf, source: StateSource, legacy: bool| ResolvedState {
        kind,
        path,
        source,
        legacy,
        warning: None,
    };
    match (new_real, old_real) {
        (true, true) => {
            if same_dir(&new, &old) {
                Ok(resolved(new, StateSource::Existing, false))
            } else {
                Err(ResolveError::Conflict {
                    new,
                    old,
                    marker: kind.marker(),
                    env: kind.new_env(),
                })
            }
        }
        (true, false) => Ok(resolved(new, StateSource::Existing, false)),
        (false, true) => Ok(resolved(old, StateSource::ExistingLegacy, true)),
        (false, false) => {
            // Agent and control directories exist before their marker does (a node key, a surface
            // token or a spool is written first), so an existing legacy directory with no new-name
            // sibling is still the machine's state. The home is stricter: no `vowp.db`, no home.
            let partial_legacy = kind != StateKind::Home && old.is_dir() && !exists_any(&new);
            if partial_legacy {
                Ok(resolved(old, StateSource::ExistingLegacy, true))
            } else {
                Ok(resolved(new, StateSource::Fresh, false))
            }
        }
    }
}

/// True when anything, including a dangling symlink, exists at `path`.
fn exists_any(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// True when both paths exist and canonicalise to the same directory.
fn same_dir(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}
