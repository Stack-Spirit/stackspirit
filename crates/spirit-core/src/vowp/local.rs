//! Secret-bearing D6 values that stay on the local side of the boundary.
//!
//! A surface pairing code is a short-lived credential. It is held in
//! [`SecretString`] so it is zeroized, never `Serialize`s and never appears in
//! `Debug`. The Mac agent pair request follows the same rule: the code is sent on
//! stdin with `--code-file -`, never in argv, a file, a log or shared state.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::engine::keychain::SecretString;
use crate::error::SpiritError;
use crate::time::Timestamp;
use crate::wire::{AgentPairOutput, ProjectName, SessionId, SurfacePairOutput, ensure_surface_id};

/// A successful home surface pairing.
///
/// Only the one-time code is secret. The surface id, node, session, expiry and the
/// exact Mac command are safe display fields; [`SurfacePairing::view`] returns those
/// without the code for anything that may be logged or serialized.
#[derive(Clone)]
pub struct SurfacePairing {
    /// Surface id.
    pub surface: String,
    /// Node the surface runs on.
    pub node: String,
    /// Session the surface mirrors.
    pub session: String,
    /// Surface kind.
    pub kind: String,
    /// Application name, when any.
    pub app: Option<String>,
    /// Pairing-code expiry.
    pub expires_at: Timestamp,
    /// Exact Mac-side `vowp agent pair` command; never carries the code.
    pub command: String,
    /// Surface revoked by `--replace`, when one was named.
    pub replaced: Option<String>,
    code: SecretString,
}

impl SurfacePairing {
    /// Wraps one raw `vowp surface pair --json` output.
    #[must_use]
    pub fn from_output(output: SurfacePairOutput) -> Self {
        Self {
            surface: output.surface,
            node: output.node,
            session: output.session,
            kind: output.kind.as_str().to_owned(),
            app: output.app,
            expires_at: output.expires_at,
            command: output.command,
            replaced: output.replaced,
            code: SecretString::new(output.code),
        }
    }

    /// Borrows the one-time code for the one UI copy action. Callers must not log it.
    #[must_use]
    pub fn code(&self) -> &SecretString {
        &self.code
    }

    /// Returns the safe view with no secret.
    #[must_use]
    pub fn view(&self) -> SurfacePairingView {
        SurfacePairingView {
            surface: self.surface.clone(),
            node: self.node.clone(),
            session: self.session.clone(),
            kind: self.kind.clone(),
            app: self.app.clone(),
            expires_at: self.expires_at,
            command: self.command.clone(),
            replaced: self.replaced.clone(),
        }
    }
}

impl fmt::Debug for SurfacePairing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SurfacePairing")
            .field("surface", &self.surface)
            .field("node", &self.node)
            .field("session", &self.session)
            .field("kind", &self.kind)
            .field("app", &self.app)
            .field("expires_at", &self.expires_at)
            .field("command", &self.command)
            .field("replaced", &self.replaced)
            .field("code", &"[redacted]")
            .finish()
    }
}

/// The safe, serializable part of a surface pairing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SurfacePairingView {
    /// Surface id.
    pub surface: String,
    /// Node the surface runs on.
    pub node: String,
    /// Session the surface mirrors.
    pub session: String,
    /// Surface kind.
    pub kind: String,
    /// Application name, when any.
    pub app: Option<String>,
    /// Pairing-code expiry.
    pub expires_at: Timestamp,
    /// Exact Mac-side `vowp agent pair` command; never carries the code.
    pub command: String,
    /// Surface revoked by `--replace`, when one was named.
    pub replaced: Option<String>,
}

/// A requested Mac-side surface pairing.
#[derive(Clone)]
pub struct AgentPairRequest {
    /// One-time pairing code; never logged or serialized.
    pub code: SecretString,
    /// Surface id printed by `vowp surface pair`.
    pub surface: String,
    /// Session id printed by `vowp surface pair`.
    pub session: String,
    /// Application name.
    pub app: Option<String>,
}

impl AgentPairRequest {
    /// Builds a request from the one-time code and the home's pairing result.
    #[must_use]
    pub fn new(
        code: SecretString,
        surface: impl Into<String>,
        session: impl Into<String>,
        app: Option<String>,
    ) -> Self {
        Self {
            code,
            surface: surface.into(),
            session: session.into(),
            app,
        }
    }

    /// Validates every value before it becomes local argv or stdin.
    pub fn validate(&self) -> Result<(), SpiritError> {
        let code = self.code.expose();
        if code.trim().is_empty() || code.len() > 4096 || code.contains('\0') {
            return Err(SpiritError::invalid_input(
                "pairing code",
                "must be a non-empty single line",
            ));
        }
        ensure_surface_id(&self.surface)?;
        SessionId::parse(&self.session)?;
        if let Some(app) = &self.app {
            if app.is_empty()
                || app.len() > 64
                || app.starts_with('-')
                || !app
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
            {
                return Err(SpiritError::invalid_input(
                    "surface app",
                    "use 1-64 letters, digits, '.', '-' or '_'",
                ));
            }
        }
        Ok(())
    }
}

impl fmt::Debug for AgentPairRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentPairRequest")
            .field("code", &"[redacted]")
            .field("surface", &self.surface)
            .field("session", &self.session)
            .field("app", &self.app)
            .finish()
    }
}

/// A paired Mac surface, without the token or its file path.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentPairing {
    /// Surface id.
    pub surface_id: String,
    /// Scopes granted to the token.
    pub scopes: Vec<String>,
    /// Token expiry.
    pub expires_at: Timestamp,
}

impl AgentPairing {
    /// Wraps the safe fields of `vowp agent pair --json`.
    #[must_use]
    pub fn from_output(output: AgentPairOutput) -> Self {
        Self {
            surface_id: output.surface_id,
            scopes: output.scopes,
            expires_at: output.expires_at,
        }
    }
}

/// Validates a project name used in a local path map.
pub fn ensure_project(project: &str) -> Result<ProjectName, SpiritError> {
    ProjectName::parse(project)
}

// --- Mac agent state (project orchestrator binding) -------------------------------------
//
// `vowp-agent` reads these private files to bind an orchestrator to one paired surface. The
// helpers below only touch non-secret metadata: surface ids, session ids and the agent
// socket path. They never read or write a surface token or a pairing code.

/// Name of the agent's unix socket inside its state directory.
pub const AGENT_SOCKET_FILE: &str = "agent.sock";

/// The Mac agent state directory: `$VOWP_AGENT_HOME`, else `$HOME/.vowp-agent`.
#[must_use]
pub fn agent_state_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("VOWP_AGENT_HOME").filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join(".vowp-agent"))
}

/// The agent socket `vowp mcp --agent-socket` connects to.
#[must_use]
pub fn agent_socket_path(state_dir: &Path) -> PathBuf {
    state_dir.join(AGENT_SOCKET_FILE)
}

/// The exact argv for the local MCP proxy as `vowp mcp --help` defines it: `--surface` is
/// required and `--agent-socket` selects the Mac proxy mode. There is no `--agent` flag.
#[must_use]
pub fn vowp_mcp_argv(agent_socket: &Path, surface: &str) -> Vec<String> {
    vec![
        "vowp".to_owned(),
        "mcp".to_owned(),
        "--agent-socket".to_owned(),
        agent_socket.display().to_string(),
        "--surface".to_owned(),
        surface.to_owned(),
    ]
}

/// Finds an active paired surface bound to `session` in the Mac agent state, newest first
/// is not needed: an orchestrator needs exactly one. A `revoked` or `needs_pair` surface
/// is never returned.
#[must_use]
pub fn find_paired_surface(state_dir: &Path, session: &str) -> Option<String> {
    let entries = std::fs::read_dir(state_dir.join("surfaces")).ok()?;
    let mut found: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let id = path.file_name()?.to_str()?.to_owned();
            ensure_surface_id(&id).ok()?;
            let bound = std::fs::read_to_string(path.join("session")).ok()?;
            if bound.trim() != session {
                return None;
            }
            let state = std::fs::read_to_string(path.join("state")).unwrap_or_default();
            (!matches!(state.trim(), "needs_pair" | "revoked")).then_some(id)
        })
        .collect();
    found.sort();
    found.pop()
}

/// Writes `<state_dir>/orchestrators/<project>/config.json` so `vowp-agent` binds the
/// project to its exact paired surface and session. This is what prevents
/// `surface_ambiguous` when `global` is already paired on the same Mac.
///
/// Existing fields (command, args, working directory, ...) are preserved; a missing file
/// becomes the minimal valid `OrchestratorConfig`. The directory is `0700` and the file is
/// written atomically with mode `0600`.
pub fn bind_orchestrator_surface(
    state_dir: &Path,
    project: &str,
    session: &str,
    surface: &str,
) -> Result<(), SpiritError> {
    let project = ProjectName::parse(project)?;
    SessionId::parse(session)?;
    ensure_surface_id(surface)?;
    let fail = |what: &str| {
        SpiritError::new(
            crate::error::SpiritErrorCode::Engine,
            format!("could not {what} the local orchestrator config"),
        )
    };
    let dir = state_dir.join("orchestrators").join(project.to_string());
    ensure_private_dir(&dir).map_err(|_| fail("prepare"))?;
    let path = dir.join("config.json");
    let mut document = match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(value) if value.is_object() => value,
            _ => return Err(fail("read")),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::json!({
            "name": project.to_string(),
            "app": "opencode",
            "project": project.to_string(),
        }),
        Err(_) => return Err(fail("read")),
    };
    document["session"] = serde_json::Value::String(session.to_owned());
    document["surface"] = serde_json::Value::String(surface.to_owned());
    let bytes = serde_json::to_vec_pretty(&document).map_err(|_| fail("encode"))?;
    write_private_atomic(&path, &bytes).map_err(|_| fail("write"))
}

fn ensure_private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Only the leaf and its parent are ours to tighten.
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        if let Some(parent) = dir.parent() {
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
}

fn write_private_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("no parent"))?;
    let mut file = tempfile::Builder::new()
        .prefix(".tmp-")
        .tempfile_in(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod state_tests {
    use super::*;

    const SESSION: &str = "01JCF5WQ8ZK3M2V7N4P6R8T0AE";
    const SURFACE: &str = "01JCF5WQ8ZK3M2V7N4P6R8T0AF";

    #[test]
    fn mcp_argv_matches_the_installed_cli_contract() {
        let argv = vowp_mcp_argv(Path::new("/s/agent.sock"), SURFACE);
        assert_eq!(
            argv,
            [
                "vowp",
                "mcp",
                "--agent-socket",
                "/s/agent.sock",
                "--surface",
                SURFACE
            ]
        );
        assert!(!argv.iter().any(|arg| arg == "--agent"));
    }

    #[test]
    fn binding_writes_a_private_config_and_keeps_existing_fields() {
        let state = tempfile::tempdir().unwrap();
        bind_orchestrator_surface(state.path(), "stackspirit", SESSION, SURFACE).unwrap();
        let path = state.path().join("orchestrators/stackspirit/config.json");
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["surface"], SURFACE);
        assert_eq!(value["session"], SESSION);
        assert_eq!(value["app"], "opencode");
        assert_eq!(value["name"], "stackspirit");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&path), 0o600);
            assert_eq!(mode(path.parent().unwrap()), 0o700);
        }
        std::fs::write(
            &path,
            r#"{"name":"stackspirit","app":"opencode","command":"opencode","args":["serve"]}"#,
        )
        .unwrap();
        bind_orchestrator_surface(state.path(), "stackspirit", SESSION, SURFACE).unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["command"], "opencode");
        assert_eq!(value["surface"], SURFACE);
    }

    #[test]
    fn binding_rejects_bad_names_and_ids() {
        let state = tempfile::tempdir().unwrap();
        assert!(bind_orchestrator_surface(state.path(), "../x", SESSION, SURFACE).is_err());
        assert!(bind_orchestrator_surface(state.path(), "p", "nope", SURFACE).is_err());
        assert!(bind_orchestrator_surface(state.path(), "p", SESSION, "nope").is_err());
    }

    #[test]
    fn finds_only_an_active_surface_for_the_session() {
        let state = tempfile::tempdir().unwrap();
        let dir = state.path().join("surfaces").join(SURFACE);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("session"), format!("{SESSION}\n")).unwrap();
        assert_eq!(
            find_paired_surface(state.path(), SESSION).as_deref(),
            Some(SURFACE)
        );
        assert_eq!(
            find_paired_surface(state.path(), "01JCF5WQ8ZK3M2V7N4P6R8T0AG"),
            None
        );
        std::fs::write(dir.join("state"), "needs_pair").unwrap();
        assert_eq!(find_paired_surface(state.path(), SESSION), None);
    }
}
