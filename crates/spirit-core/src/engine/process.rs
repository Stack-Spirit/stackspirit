//! Bundled sidecar launch, loopback health, bounded redacted logs and clean shutdown.
//!
//! The runtime starts only the binary resolved by [`EngineBinaryLocator`] from the
//! Tauri resource/binary directories. It never resolves the bundled engine from `PATH`.
//! All HTTP traffic is loopback-only. Stored BYOK keys reach the child only through its
//! ephemeral environment (`OPENCODE_AUTH_CONTENT`), never through the engine's auth file
//! and never through a request URL or log line.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::{Value, json};
use zeroize::Zeroizing;

use crate::engine::runtime::{
    EngineHealth, EngineRuntime, EngineRuntimeInfo, KeyVerification, RuntimeModel,
};
use crate::engine::{EngineProvider, Keychain, SecretString};
use crate::error::{SpiritError, SpiritErrorCode, redact};

// The catalog parser lives in a sibling file that this module declares, so the engine
// provider document can be grouped without changing the `engine` module root.
#[path = "providers.rs"]
pub mod providers;

/// The only host a bundled engine may listen on.
pub const LOOPBACK_HOST: &str = "127.0.0.1";

/// How many engine log lines are retained.
pub const MAX_LOG_LINES: usize = 200;

// The pinned 1.19.5 `/provider` document is ~8 MiB even without a key (it carries the
// full supported-provider catalog). The cap still bounds local loopback responses, but
// must be large enough for that one document. Smaller documents use `/config/providers`.
const MAX_HTTP_BODY: usize = 16 * 1024 * 1024;

/// Returns the loopback host used for every bundled engine launch.
#[must_use]
pub const fn loopback_host() -> &'static str {
    LOOPBACK_HOST
}

/// Returns this build's Tauri target triple when it is a supported desktop target.
#[must_use]
pub fn current_target_triple() -> Option<&'static str> {
    let arch = std::env::consts::ARCH;
    match std::env::consts::OS {
        "linux" if cfg!(target_env = "musl") => match arch {
            "x86_64" => Some("x86_64-unknown-linux-musl"),
            "aarch64" => Some("aarch64-unknown-linux-musl"),
            _ => None,
        },
        "linux" => match arch {
            "x86_64" => Some("x86_64-unknown-linux-gnu"),
            "aarch64" => Some("aarch64-unknown-linux-gnu"),
            _ => None,
        },
        "macos" => match arch {
            "x86_64" => Some("x86_64-apple-darwin"),
            "aarch64" => Some("aarch64-apple-darwin"),
            _ => None,
        },
        "windows" => match arch {
            "x86_64" => Some("x86_64-pc-windows-msvc"),
            "aarch64" => Some("aarch64-pc-windows-msvc"),
            _ => None,
        },
        _ => None,
    }
}

/// Tauri's sidecar file name for a target triple.
#[must_use]
pub fn sidecar_file_name(triple: &str) -> String {
    let executable = if triple.contains("windows") {
        "opencode.exe"
    } else {
        "opencode"
    };
    format!("{executable}-{triple}")
}

/// Resolves the bundled sidecar from Tauri's binary/resource locations.
///
/// The locator takes an explicit list of directories. It never consults `PATH`, the
/// current directory, or a shell profile.
#[derive(Debug, Clone, Default)]
pub struct EngineBinaryLocator {
    directories: Vec<PathBuf>,
}

impl EngineBinaryLocator {
    /// Builds a locator over explicit directories, de-duplicating them.
    #[must_use]
    pub fn new(directories: Vec<PathBuf>) -> Self {
        let mut unique = Vec::new();
        for directory in directories {
            if !unique.contains(&directory) {
                unique.push(directory);
            }
        }
        Self {
            directories: unique,
        }
    }

    /// Builds a locator for the installed Tauri application.
    ///
    /// Tauri places `externalBin` sidecars next to the main executable. On macOS the
    /// resource directory is `../Resources`; Linux bundles may also use
    /// `../lib/stackspirit`. Development builds use the same executable-relative name.
    #[must_use]
    pub fn for_current_exe() -> Self {
        let mut directories = Vec::new();
        if let Ok(executable) = std::env::current_exe()
            && let Some(directory) = executable.parent()
        {
            directories.push(directory.to_path_buf());
            directories.push(directory.join("binaries"));
            directories.push(directory.join("../Resources"));
            directories.push(directory.join("../Resources/binaries"));
            directories.push(directory.join("../lib/stackspirit"));
        }
        Self::new(directories)
    }

    /// Returns the searched directories in order.
    #[must_use]
    pub fn directories(&self) -> &[PathBuf] {
        &self.directories
    }

    /// Returns the first executable sidecar in the configured directories.
    #[must_use]
    pub fn locate(&self) -> Option<PathBuf> {
        let triple = current_target_triple();
        let mut names = Vec::new();
        if let Some(triple) = triple {
            names.push(sidecar_file_name(triple));
        } else if cfg!(windows) {
            names.push("opencode.exe".to_owned());
        } else {
            names.push("opencode".to_owned());
        }
        names.push(if cfg!(windows) {
            "opencode.exe".to_owned()
        } else {
            "opencode".to_owned()
        });
        for directory in &self.directories {
            for name in &names {
                let candidate = directory.join(name);
                if is_executable(&candidate) {
                    return Some(candidate);
                }
            }
        }
        None
    }
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Reserves a free port bound only to loopback.
///
/// The listener is closed immediately before the child starts. The window is tiny and
/// the child is given the explicit port, so it can never bind a public interface.
pub fn reserve_loopback_port() -> Result<u16, SpiritError> {
    let listener = TcpListener::bind((LOOPBACK_HOST, 0))
        .map_err(|error| SpiritError::io("reserving a loopback port for the engine", &error))?;
    let port = listener
        .local_addr()
        .map_err(|error| SpiritError::io("reading the reserved engine port", &error))?
        .port();
    drop(listener);
    Ok(port)
}

/// Returns the exact arguments used to start the bundled engine.
#[must_use]
pub fn serve_args(port: u16) -> Vec<String> {
    vec![
        "serve".to_owned(),
        "--hostname".to_owned(),
        LOOPBACK_HOST.to_owned(),
        "--port".to_owned(),
        port.to_string(),
    ]
}

/// A bounded, redacted log ring.
#[derive(Debug)]
pub struct EngineLogBuffer {
    lines: VecDeque<String>,
    limit: usize,
}

impl Default for EngineLogBuffer {
    fn default() -> Self {
        Self::new(MAX_LOG_LINES)
    }
}

impl EngineLogBuffer {
    /// Builds a buffer capped at `limit` lines.
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self {
            lines: VecDeque::with_capacity(limit.max(1)),
            limit: limit.max(1),
        }
    }

    /// Pushes one engine line after redaction and length bounding.
    pub fn push(&mut self, line: &str) {
        let redacted = redact(line);
        let bounded: String = redacted.chars().take(1_000).collect();
        if self.lines.len() == self.limit {
            self.lines.pop_front();
        }
        self.lines.push_back(bounded);
    }

    /// Returns a stable snapshot of the retained lines.
    #[must_use]
    pub fn snapshot(&self) -> Vec<String> {
        self.lines.iter().cloned().collect()
    }

    /// Returns the number of retained lines.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Returns true when no line has been retained.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// The app-private tree from `docs/ORCHESTRATOR-CHAT.md` §2.1.3.
///
/// All directories are created `0700`; the setup `AGENTS.md` and `opencode.json` (both in `engine/config/opencode`) are `0600`. The renderer
/// never receives these paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateEngineTree {
    /// `engine/data`, exported as `XDG_DATA_HOME`.
    pub data: PathBuf,
    /// `engine/state`, exported as `XDG_STATE_HOME`.
    pub state: PathBuf,
    /// `engine/config`, exported as `XDG_CONFIG_HOME`.
    pub config: PathBuf,
    /// The setup workspace and the child's current directory.
    pub setup_workspace: PathBuf,
    /// The reserved global workspace.
    pub global_workspace: PathBuf,
    /// The parent of per-project workspaces.
    pub project_workspace_root: PathBuf,
}

impl PrivateEngineTree {
    /// Creates the private tree under `app_data` and returns its resolved paths.
    pub fn prepare(app_data: &Path) -> Result<Self, SpiritError> {
        let engine = app_data.join("engine");
        let workspaces = engine.join("workspaces");
        let tree = Self {
            data: engine.join("data"),
            state: engine.join("state"),
            config: engine.join("config"),
            setup_workspace: workspaces.join("setup"),
            global_workspace: workspaces.join("global"),
            project_workspace_root: workspaces.join("project"),
        };
        for directory in [
            &engine,
            &tree.data,
            &tree.state,
            &tree.config,
            &workspaces,
            &tree.setup_workspace,
            &tree.global_workspace,
            &tree.project_workspace_root,
        ] {
            create_private_directory(directory)?;
        }
        let opencode_dir = tree.opencode_config_dir();
        create_private_directory(&opencode_dir)?;
        write_setup_agents(&opencode_dir)?;
        write_setup_opencode_config(&opencode_dir)?;
        Ok(tree)
    }

    /// The directory holding the setup engine's own `opencode.json`.
    #[must_use]
    pub fn opencode_config_dir(&self) -> PathBuf {
        self.config.join("opencode")
    }

    /// The environment that pins the child to this tree, as `(name, value)` pairs.
    ///
    /// `OPENCODE_CONFIG` and `OPENCODE_CONFIG_DIR` point only at the file written by
    /// `prepare`, and project-level config discovery is disabled, so an unrelated
    /// orchestrator `opencode.json` is never loaded.
    #[must_use]
    pub fn isolated_env(&self) -> Vec<(&'static str, PathBuf)> {
        let dir = self.opencode_config_dir();
        vec![
            ("XDG_DATA_HOME", self.data.clone()),
            ("XDG_STATE_HOME", self.state.clone()),
            ("XDG_CONFIG_HOME", self.config.clone()),
            ("OPENCODE_CONFIG", dir.join("opencode.json")),
            ("OPENCODE_CONFIG_DIR", dir),
            ("OPENCODE_DISABLE_PROJECT_CONFIG", PathBuf::from("1")),
        ]
    }

    /// Variables that could inject outside configuration and must be removed.
    pub const INHERITED_CONFIG_ENV: [&'static str; 2] =
        ["OPENCODE_CONFIG_CONTENT", "OPENCODE_PERMISSION"];
}

/// The setup workspace prompt. It contains no secret and no machine path.
const SETUP_AGENTS_MD: &str = "\
# StackSpirit setup assistant

You are the StackSpirit setup assistant. You run locally on the user's Mac with OpenCode \
free models, or the model the user chose (their own key). You can only chat. You have no \
shell, no file access and no web access, so never try to run commands or look at the \
user's computer. Keep replies direct, short and practical, and ask one short question at \
a time.

## What StackSpirit can do for the user

- Connect a brand-new server with a one-time password: click `+ Connect a new server` in \
the composer bar. It opens the app-owned Secure server details card, verifies the SSH host \
fingerprint, creates an app-owned ed25519 key in the Keychain, installs it, and can turn \
off password login.
- Add a server that already has SSH key auth: click `Settings -> Add server` (or `Add \
server` on Welcome) and enter Host, User, Port, Identity file and VOWP home.
- Add another machine or scale workers: once two or more servers are saved, open the \
Server page or the sidebar Server `...` menu and click `Form a cluster`. A cluster has \
1 Home and 1 or more Worker/Backup servers for pooled worker capacity, scheduled backups \
and backup-to-Home promotion.

## Rules

- Never ask the user to paste a password, API key, token or private key into chat. Send \
them to the secure card or the Settings forms above.
- Do not claim a server is connected until StackSpirit confirms a verified key-only login.
- If you are unsure what a button does, say so instead of guessing.
";

/// The strict permission document for the bundled setup engine. Every host-touching tool
/// is denied; nothing is `ask`, because the setup chat has no permission prompt UI.
const SETUP_OPENCODE_JSON: &str = r#"{
  "$schema": "https://opencode.ai/config.json",
  "autoupdate": false,
  "share": "disabled",
  "permission": {
    "bash": "deny",
    "edit": "deny",
    "write": "deny",
    "read": "deny",
    "glob": "deny",
    "grep": "deny",
    "list": "deny",
    "external_directory": "deny",
    "webfetch": "deny",
    "websearch": "deny",
    "task": "deny",
    "skill": "deny",
    "lsp": "deny"
  }
}
"#;

fn create_private_directory(path: &Path) -> Result<(), SpiritError> {
    std::fs::create_dir_all(path).map_err(|_| {
        SpiritError::new(
            SpiritErrorCode::Engine,
            "could not create the private StackSpirit Engine state tree",
        )
        .with_hint("Check that the StackSpirit app-data directory is writable.")
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(|_| {
            SpiritError::new(
                SpiritErrorCode::Engine,
                "could not secure the private StackSpirit Engine state tree",
            )
            .with_hint("Check the app-data directory permissions.")
        })?;
    }
    Ok(())
}

/// The pinned engine ignores the workspace `AGENTS.md` once project config is disabled and loads
/// only `$OPENCODE_CONFIG_DIR/AGENTS.md`, so the setup prompt lives beside `opencode.json`.
fn write_setup_agents(config_dir: &Path) -> Result<(), SpiritError> {
    write_private_file_atomic(
        &config_dir.join("AGENTS.md"),
        SETUP_AGENTS_MD.as_bytes(),
        "the private setup workspace instructions",
    )
}

fn write_setup_opencode_config(config_dir: &Path) -> Result<(), SpiritError> {
    write_private_file_atomic(
        &config_dir.join("opencode.json"),
        SETUP_OPENCODE_JSON.as_bytes(),
        "the private setup engine permissions",
    )
}

/// Writes `bytes` to a sibling temp file with mode 0600, then renames it into place.
fn write_private_file_atomic(path: &Path, bytes: &[u8], what: &str) -> Result<(), SpiritError> {
    let fail = |action: &str| {
        SpiritError::new(
            SpiritErrorCode::Engine,
            format!("could not {action} {what}"),
        )
        .with_hint("Check that the StackSpirit app-data directory is writable.")
    };
    let staged = path.with_extension("tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&staged).map_err(|_| fail("write"))?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    if written.is_err() {
        let _ = std::fs::remove_file(&staged);
        return Err(fail("write"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o600))
            .map_err(|_| fail("secure"))?;
    }
    std::fs::rename(&staged, path).map_err(|_| {
        let _ = std::fs::remove_file(&staged);
        fail("replace")
    })
}

#[derive(Debug)]
struct ProcessState {
    child: Option<Child>,
}

/// The bundled sidecar runtime backed by a real child process.
pub struct ProcessEngineRuntime {
    locator: EngineBinaryLocator,
    explicit_binary: Option<PathBuf>,
    key_source: Option<Arc<dyn Keychain>>,
    private_state: Option<PathBuf>,
    state: Mutex<ProcessState>,
    logs: Arc<Mutex<EngineLogBuffer>>,
}

impl std::fmt::Debug for ProcessEngineRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessEngineRuntime")
            .field("binary", &self.explicit_binary)
            .field("directories", &self.locator.directories())
            .field("private_state", &self.private_state.is_some())
            .finish_non_exhaustive()
    }
}

impl ProcessEngineRuntime {
    /// Builds a runtime that resolves the sidecar from `locator`.
    #[must_use]
    pub fn new(locator: EngineBinaryLocator) -> Self {
        Self {
            locator,
            explicit_binary: None,
            key_source: None,
            private_state: None,
            state: Mutex::new(ProcessState { child: None }),
            logs: Arc::new(Mutex::new(EngineLogBuffer::default())),
        }
    }

    /// Builds a runtime around an exact binary. Used by tests and repair flows.
    #[must_use]
    pub fn with_binary(binary: PathBuf) -> Self {
        Self {
            locator: EngineBinaryLocator::default(),
            explicit_binary: Some(binary),
            key_source: None,
            private_state: None,
            state: Mutex::new(ProcessState { child: None }),
            logs: Arc::new(Mutex::new(EngineLogBuffer::default())),
        }
    }

    /// Starts the bundled child with an app-private XDG tree and setup workspace.
    ///
    /// This is the §2.1.3 isolation: the child never reads or writes the owner's
    /// OpenCode sessions, configuration or state. The desktop passes its app-data
    /// directory here; tests pass a temporary directory.
    #[must_use]
    pub fn with_private_state(mut self, app_data: PathBuf) -> Self {
        self.private_state = Some(app_data);
        self
    }

    /// Supplies the local keychain used to make stored BYOK keys available to the
    /// child process. Keys are read at spawn time; no key is written to disk.
    #[must_use]
    pub fn with_key_source(mut self, keychain: Arc<dyn Keychain>) -> Self {
        self.key_source = Some(keychain);
        self
    }

    /// Builds a runtime for the installed application.
    #[must_use]
    pub fn for_current_exe() -> Self {
        Self::new(EngineBinaryLocator::for_current_exe())
    }

    fn resolve_binary(&self) -> Result<PathBuf, SpiritError> {
        if let Some(binary) = &self.explicit_binary {
            if is_executable(binary) {
                return Ok(binary.clone());
            }
            return Err(unavailable_error());
        }
        self.locator.locate().ok_or_else(unavailable_error)
    }

    fn http_get(&self, port: u16, path: &str) -> Result<HttpResponse, SpiritError> {
        http_get(port, path)
    }

    fn spawn_process(&self, port: u16) -> Result<EngineRuntimeInfo, SpiritError> {
        let auth = self.key_source.as_deref().and_then(keychain_auth_content);
        self.spawn_process_with_auth(port, auth)
    }

    fn spawn_process_with_auth(
        &self,
        port: u16,
        auth_content: Option<Zeroizing<String>>,
    ) -> Result<EngineRuntimeInfo, SpiritError> {
        let (child, info) = self.spawn_child(port, auth_content, None)?;
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .child = Some(child);
        Ok(info)
    }

    fn spawn_child(
        &self,
        port: u16,
        auth_content: Option<Zeroizing<String>>,
        probe_root: Option<&Path>,
    ) -> Result<(Child, EngineRuntimeInfo), SpiritError> {
        let binary = self.resolve_binary()?;
        let port = if port == 0 {
            reserve_loopback_port()?
        } else {
            port
        };
        let mut command = Command::new(&binary);
        command
            .args(serve_args(port))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // The pinned 1.19.5 build honors OPENCODE_SERVER_PASSWORD/USERNAME as HTTP Basic
        // auth for every route. This launch has no server auth and the client sends none,
        // so never inherit a credential from the desktop process environment.
        Command::env_remove(&mut command, "OPENCODE_SERVER_PASSWORD");
        Command::env_remove(&mut command, "OPENCODE_SERVER_USERNAME");
        if let Some(root) = probe_root {
            // A verification probe never shares the live engine's SQLite database,
            // config or workspace: everything lives in a throwaway directory.
            for name in PrivateEngineTree::INHERITED_CONFIG_ENV {
                Command::env_remove(&mut command, name);
            }
            let config = root.join("config");
            let config_dir = config.join("opencode");
            for directory in [
                root.join("home"),
                root.join("data"),
                root.join("state"),
                config.clone(),
                config_dir.clone(),
                root.join("workspace"),
            ] {
                std::fs::create_dir_all(&directory).map_err(|error| {
                    SpiritError::new(
                        SpiritErrorCode::Engine,
                        format!(
                            "verification sandbox failed: {}",
                            redact(&error.to_string())
                        ),
                    )
                })?;
            }
            Command::env(&mut command, "HOME", root.join("home"));
            Command::env(&mut command, "XDG_DATA_HOME", root.join("data"));
            Command::env(&mut command, "XDG_STATE_HOME", root.join("state"));
            Command::env(&mut command, "XDG_CONFIG_HOME", &config);
            Command::env(
                &mut command,
                "OPENCODE_CONFIG",
                config_dir.join("opencode.json"),
            );
            Command::env(&mut command, "OPENCODE_CONFIG_DIR", &config_dir);
            Command::env(&mut command, "OPENCODE_DISABLE_PROJECT_CONFIG", "1");
            command.current_dir(root.join("workspace"));
        } else if let Some(app_data) = &self.private_state {
            let tree = PrivateEngineTree::prepare(app_data)?;
            for name in PrivateEngineTree::INHERITED_CONFIG_ENV {
                Command::env_remove(&mut command, name);
            }
            for (name, value) in tree.isolated_env() {
                Command::env(&mut command, name, value);
            }
            command.current_dir(&tree.setup_workspace);
        }
        if let Some(auth_content) = auth_content {
            // The engine reads this as an in-memory auth document. It is an environment
            // value for the child only; StackSpirit never writes it to a file or a log.
            Command::env(&mut command, "OPENCODE_AUTH_CONTENT", auth_content.as_str());
        }
        let mut child = command.spawn().map_err(|error| {
            let detail = redact(&error.to_string());
            SpiritError::new(
                SpiritErrorCode::Engine,
                format!("StackSpirit Engine could not start: {detail}"),
            )
            .with_hint("Repair the installation and retry.")
        })?;
        let pid = child.id();
        if let Some(stdout) = child.stdout.take() {
            spawn_log_reader(stdout, "stdout", self.logs.clone());
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_log_reader(stderr, "stderr", self.logs.clone());
        }
        Ok((
            child,
            EngineRuntimeInfo {
                pid: Some(pid),
                port,
                version: None,
                binary_path: Some(binary),
            },
        ))
    }

    fn shutdown_blocking(&self) -> Result<(), SpiritError> {
        let child = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .child
            .take();
        if let Some(child) = child {
            terminate_child(child);
        }
        Ok(())
    }

    fn health(&self, port: u16) -> Result<EngineHealth, SpiritError> {
        let response = self.http_get(port, "/global/health")?;
        if !(200..300).contains(&response.status) {
            return Err(SpiritError::new(
                SpiritErrorCode::Engine,
                format!("engine health returned HTTP {}", response.status),
            ));
        }
        let document: Value = serde_json::from_slice(&response.body).map_err(|error| {
            SpiritError::new(
                SpiritErrorCode::Engine,
                format!("engine health is not valid JSON: {error}"),
            )
        })?;
        if document.get("healthy").and_then(Value::as_bool) != Some(true) {
            return Err(SpiritError::new(
                SpiritErrorCode::Engine,
                "engine health reported unhealthy",
            ));
        }
        Ok(EngineHealth {
            version: document
                .get("version")
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
    }

    fn fetch_models(&self, port: u16) -> Result<Vec<RuntimeModel>, SpiritError> {
        let mut last_error = None;
        for path in ["/config/providers", "/provider"] {
            let timeout = provider_document_timeout(path);
            match http_request(port, "GET", path, None, timeout) {
                Ok(response) if (200..300).contains(&response.status) => {
                    return Ok(parse_models(&response.body));
                }
                Ok(response) => {
                    last_error = Some(SpiritError::new(
                        SpiritErrorCode::Engine,
                        format!("engine model list returned HTTP {}", response.status),
                    ));
                }
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| {
            SpiritError::new(SpiritErrorCode::Engine, "engine model list is unavailable")
        }))
    }

    fn fetch_providers(&self, port: u16) -> Result<Vec<EngineProvider>, SpiritError> {
        let mut last_error = None;
        for path in ["/provider", "/config/providers"] {
            let timeout = provider_document_timeout(path);
            match http_request(port, "GET", path, None, timeout) {
                Ok(response) if (200..300).contains(&response.status) => {
                    let providers = parse_providers(&response.body);
                    if !providers.is_empty() {
                        return Ok(providers);
                    }
                    last_error = Some(SpiritError::new(
                        SpiritErrorCode::Engine,
                        "engine provider list reported no providers",
                    ));
                }
                Ok(response) => {
                    last_error = Some(SpiritError::new(
                        SpiritErrorCode::Engine,
                        format!("engine provider list returned HTTP {}", response.status),
                    ));
                }
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| {
            SpiritError::new(
                SpiritErrorCode::Engine,
                "engine provider list is unavailable",
            )
        }))
    }

    fn fetch_provider_document(&self, port: u16) -> Option<Vec<u8>> {
        for path in ["/config/providers", "/provider"] {
            let timeout = provider_document_timeout(path);
            if let Ok(response) = http_request(port, "GET", path, None, timeout)
                && (200..300).contains(&response.status)
            {
                return Some(response.body);
            }
        }
        None
    }

    fn verify_against_engine(&self, port: u16, provider: &str) -> KeyVerification {
        let Some(document) = self.fetch_provider_document(port) else {
            return KeyVerification::Unverified;
        };
        let candidates = select_verification_models(&document, provider);
        verify_candidates(&candidates, |model_id| {
            self.probe_model(port, provider, model_id)
        })
    }

    fn probe_model(&self, port: u16, provider: &str, model_id: &str) -> ProbeOutcome {
        let response = match http_request(
            port,
            "POST",
            "/session",
            Some(b"{}"),
            Duration::from_secs(3),
        ) {
            Ok(response) if (200..300).contains(&response.status) => response,
            _ => return ProbeOutcome::Final(KeyVerification::Unverified),
        };
        let Ok(session) = serde_json::from_slice::<Value>(&response.body) else {
            return ProbeOutcome::Final(KeyVerification::Unverified);
        };
        let Some(session_id) = session.get("id").and_then(Value::as_str) else {
            return ProbeOutcome::Final(KeyVerification::Unverified);
        };
        let model_id = model_id
            .strip_prefix(&format!("{provider}/"))
            .unwrap_or(model_id);
        let body = json!({
            "model": {
                "providerID": provider,
                "modelID": model_id,
            },
            "parts": [{
                "type": "text",
                "text": "Reply with OK.",
            }],
        })
        .to_string();
        let message_path = format!("/session/{session_id}/message");
        let result = match http_request(
            port,
            "POST",
            &message_path,
            Some(body.as_bytes()),
            Duration::from_secs(20),
        ) {
            // The engine often returns HTTP 200 for a failed provider call and puts the
            // provider error in the response body. Never report Verified from the status
            // line alone.
            Ok(response) if (200..300).contains(&response.status) => {
                classify_probe_response(&response.body)
            }
            Ok(response) if matches!(response.status, 401 | 403) => {
                ProbeOutcome::Final(KeyVerification::Invalid)
            }
            Ok(response) if matches!(response.status, 400 | 404) => ProbeOutcome::TryNext,
            Ok(_) | Err(_) => ProbeOutcome::Final(KeyVerification::Unverified),
        };
        let delete_path = format!("/session/{session_id}");
        let _ = http_request(port, "DELETE", &delete_path, None, Duration::from_secs(3));
        result
    }

    async fn verify_with_secret(&self, provider: &str, secret: &SecretString) -> KeyVerification {
        let Some(auth_content) = secret_auth_content(provider, secret) else {
            return KeyVerification::Invalid;
        };
        // The sandbox outlives the child and is removed when this function returns.
        let Ok(sandbox) = tempfile::tempdir() else {
            return KeyVerification::Unverified;
        };
        let (child, info) = match self.spawn_child(0, Some(auth_content), Some(sandbox.path())) {
            Ok(spawned) => spawned,
            Err(_) => return KeyVerification::Unverified,
        };
        let mut healthy = false;
        // A second sidecar may take as long as the supervised cold start, especially
        // while the main engine is running. Use the same 10 s budget before giving up.
        for _ in 0..100 {
            if self.probe_health(&info).await.is_ok() {
                healthy = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let result = if healthy {
            self.verify_against_engine(info.port, provider)
        } else {
            KeyVerification::Unverified
        };
        terminate_child(child);
        result
    }
}

fn secret_auth_content(provider: &str, secret: &SecretString) -> Option<Zeroizing<String>> {
    if provider.trim().is_empty() || secret.is_empty() {
        return None;
    }
    Some(Zeroizing::new(
        json!({
            provider: {
                "type": "api",
                "key": secret.expose(),
            }
        })
        .to_string(),
    ))
}

fn keychain_auth_content(keychain: &dyn Keychain) -> Option<Zeroizing<String>> {
    let providers = keychain.list().ok()?;
    let mut entries = serde_json::Map::new();
    for provider in providers {
        let Ok(Some(secret)) = keychain.get(&provider) else {
            continue;
        };
        if secret.is_empty() {
            continue;
        }
        entries.insert(
            provider,
            json!({
                "type": "api",
                "key": secret.expose(),
            }),
        );
    }
    if entries.is_empty() {
        None
    } else {
        Some(Zeroizing::new(Value::Object(entries).to_string()))
    }
}

impl Drop for ProcessEngineRuntime {
    fn drop(&mut self) {
        let _ = self.shutdown_blocking();
    }
}

#[async_trait]
impl EngineRuntime for ProcessEngineRuntime {
    async fn spawn(&self, port: u16) -> Result<EngineRuntimeInfo, SpiritError> {
        self.spawn_process(port)
    }

    async fn probe_health(&self, info: &EngineRuntimeInfo) -> Result<EngineHealth, SpiritError> {
        self.health(info.port)
    }

    async fn shutdown(&self, _info: &EngineRuntimeInfo) -> Result<(), SpiritError> {
        self.shutdown_blocking()
    }

    async fn models(&self, port: u16) -> Result<Vec<RuntimeModel>, SpiritError> {
        self.fetch_models(port)
    }

    async fn providers(&self, port: u16) -> Result<Vec<EngineProvider>, SpiritError> {
        self.fetch_providers(port)
    }

    async fn verify_key(
        &self,
        _port: u16,
        provider: &str,
        secret: &SecretString,
    ) -> KeyVerification {
        // A provider inventory read without a key cannot prove access, and a large or
        // temporarily busy `/provider` document must not short-circuit verification.
        // Exercise the supplied key on an isolated loopback engine instead: the child
        // receives only this key through its ephemeral auth environment, makes one
        // minimal model call, and is terminated. The supervised engine's port is not
        // used; verification never writes the key into that engine's auth store.
        self.verify_with_secret(provider, secret).await
    }

    fn logs(&self) -> Vec<String> {
        self.logs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot()
    }

    fn kill_now(&self) {
        let _ = self.shutdown_blocking();
    }
}

fn unavailable_error() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Engine,
        "StackSpirit Engine is unavailable — repair the installation",
    )
    .with_hint("Reinstall StackSpirit to restore the bundled engine. StackSpirit does not use PATH for the bundled choice.")
}

fn spawn_log_reader<R: Read + Send + 'static>(
    reader: R,
    label: &'static str,
    logs: Arc<Mutex<EngineLogBuffer>>,
) {
    std::thread::spawn(move || {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(reader);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    let line = if label == "stderr" {
                        format!("stderr: {line}")
                    } else {
                        line
                    };
                    logs.lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(&line);
                }
                Err(_) => break,
            }
        }
    });
}

fn terminate_child(mut child: Child) {
    let _ = child.kill();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[derive(Debug)]
struct HttpResponse {
    status: u16,
    body: Vec<u8>,
}

fn http_get(port: u16, path: &str) -> Result<HttpResponse, SpiritError> {
    http_request(port, "GET", path, None, Duration::from_secs(3))
}

fn provider_document_timeout(path: &str) -> Duration {
    if path == "/provider" {
        // The full supported-provider document is several MiB and can take longer to
        // materialize on a cold engine. The smaller configured list stays on 3 s.
        Duration::from_secs(10)
    } else {
        Duration::from_secs(3)
    }
}

fn http_request(
    port: u16,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
    read_timeout: Duration,
) -> Result<HttpResponse, SpiritError> {
    if !path.starts_with('/') {
        return Err(SpiritError::new(
            SpiritErrorCode::Internal,
            "engine HTTP path must start with '/'",
        ));
    }
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(700))
        .map_err(|error| SpiritError::io("connecting to the local engine", &error))?;
    stream
        .set_read_timeout(Some(read_timeout))
        .map_err(|error| SpiritError::io("setting the engine read timeout", &error))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| SpiritError::io("setting the engine write timeout", &error))?;
    let body = body.unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {LOOPBACK_HOST}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| SpiritError::io("writing the engine request", &error))?;
    if !body.is_empty() {
        stream
            .write_all(body)
            .map_err(|error| SpiritError::io("writing the engine request body", &error))?;
    }
    let mut raw = Vec::new();
    let mut chunk = [0_u8; 8 * 1024];
    loop {
        let read = stream
            .read(&mut chunk)
            .map_err(|error| SpiritError::io("reading the engine response", &error))?;
        if read == 0 {
            break;
        }
        if raw.len() + read > MAX_HTTP_BODY {
            return Err(SpiritError::new(
                SpiritErrorCode::Engine,
                "engine response exceeded the local size cap",
            ));
        }
        raw.extend_from_slice(&chunk[..read]);
    }
    parse_http_response(&raw)
}

fn parse_http_response(raw: &[u8]) -> Result<HttpResponse, SpiritError> {
    let boundary = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| {
            SpiritError::new(
                SpiritErrorCode::Engine,
                "engine response has no HTTP headers",
            )
        })?;
    let headers = String::from_utf8_lossy(&raw[..boundary]);
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| {
            SpiritError::new(
                SpiritErrorCode::Engine,
                "engine response has no HTTP status",
            )
        })?;
    let body = &raw[boundary + 4..];
    let chunked = headers.lines().skip(1).any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.starts_with("transfer-encoding:") && lower.contains("chunked")
    });
    let body = if chunked {
        decode_chunked(body)?
    } else {
        body.to_vec()
    };
    Ok(HttpResponse { status, body })
}

fn decode_chunked(input: &[u8]) -> Result<Vec<u8>, SpiritError> {
    let mut output = Vec::new();
    let mut cursor = 0;
    loop {
        let line_end = input[cursor..]
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or_else(|| {
                SpiritError::new(SpiritErrorCode::Engine, "malformed chunked engine response")
            })?;
        let header = std::str::from_utf8(&input[cursor..cursor + line_end]).map_err(|_| {
            SpiritError::new(SpiritErrorCode::Engine, "malformed chunked engine response")
        })?;
        let size = usize::from_str_radix(header.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| {
                SpiritError::new(SpiritErrorCode::Engine, "malformed chunked engine response")
            })?;
        cursor += line_end + 2;
        if size == 0 {
            break;
        }
        if cursor + size > input.len() {
            return Err(SpiritError::new(
                SpiritErrorCode::Engine,
                "truncated chunked engine response",
            ));
        }
        output.extend_from_slice(&input[cursor..cursor + size]);
        cursor += size;
        if input.get(cursor..cursor + 2) == Some(b"\r\n") {
            cursor += 2;
        }
    }
    Ok(output)
}

/// The result of probing one candidate model during key verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbeOutcome {
    /// The probe reached a conclusion about the key.
    Final(KeyVerification),
    /// The failure was specific to the model; another candidate may succeed.
    TryNext,
}

/// Tries candidates in order. `Verified` and `Invalid` return immediately; a model-specific
/// failure moves to the next candidate before settling on `Unverified`.
fn verify_candidates(
    candidates: &[String],
    mut probe: impl FnMut(&str) -> ProbeOutcome,
) -> KeyVerification {
    for model_id in candidates {
        match probe(model_id) {
            ProbeOutcome::Final(KeyVerification::Unverified) | ProbeOutcome::TryNext => {}
            ProbeOutcome::Final(decided) => return decided,
        }
    }
    KeyVerification::Unverified
}

fn classify_probe_response(body: &[u8]) -> ProbeOutcome {
    match classify_message_response(body) {
        KeyVerification::Unverified => {
            let Ok(document) = serde_json::from_slice::<Value>(body) else {
                return ProbeOutcome::Final(KeyVerification::Unverified);
            };
            let Some(error) = find_error(&document) else {
                return ProbeOutcome::Final(KeyVerification::Unverified);
            };
            let model_specific = matches!(error_status(error), Some(400 | 404))
                || error.to_string().contains("ProviderModelNotFoundError")
                || error.as_object().is_some_and(serde_json::Map::is_empty);
            if model_specific {
                ProbeOutcome::TryNext
            } else {
                ProbeOutcome::Final(KeyVerification::Unverified)
            }
        }
        decided => ProbeOutcome::Final(decided),
    }
}

const VERIFICATION_CANDIDATES: usize = 5;

/// Picks up to five cheap, active, text-capable candidate models for a key probe.
///
/// Reads a `/config/providers` or `/provider` document. Deprecated and non-text models are
/// dropped; non-reasoning models rank before reasoning ones, then lowest input cost, then id.
/// The provider's `default` model is appended as a fallback when it is text-capable.
#[must_use]
pub fn select_verification_models(body: &[u8], provider: &str) -> Vec<String> {
    let Ok(document) = serde_json::from_slice::<Value>(body) else {
        return Vec::new();
    };
    let mut entries: Vec<(String, Value)> = Vec::new();
    for key in ["providers", "all"] {
        let Some(providers) = document.get(key).and_then(Value::as_array) else {
            continue;
        };
        for item in providers {
            if item.get("id").and_then(Value::as_str) != Some(provider) {
                continue;
            }
            match item.get("models") {
                Some(Value::Object(map)) => {
                    entries.extend(map.iter().map(|(id, model)| (id.clone(), model.clone())));
                }
                Some(Value::Array(array)) => {
                    for model in array {
                        if let Some(id) = model.get("id").and_then(Value::as_str) {
                            entries.push((id.to_owned(), model.clone()));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let text_capable = |model: &Value| {
        model.get("status").and_then(Value::as_str) != Some("deprecated")
            && ["input", "output"].iter().all(|side| {
                model
                    .pointer(&format!("/capabilities/{side}/text"))
                    .and_then(Value::as_bool)
                    != Some(false)
            })
    };
    // Free-tier models (`:free`) are rate-limited and often gated by account privacy
    // settings, so a valid key can fail on them. Rank them after paid models.
    let mut ranked: Vec<(bool, bool, f64, String)> = entries
        .iter()
        .filter(|(_, model)| text_capable(model))
        .map(|(id, model)| {
            let reasoning = model
                .pointer("/capabilities/reasoning")
                .and_then(Value::as_bool)
                == Some(true);
            let cost = model
                .pointer("/cost/input")
                .and_then(Value::as_f64)
                .unwrap_or(f64::MAX);
            (id.ends_with(":free"), reasoning, cost, id.clone())
        })
        .collect();
    ranked.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.cmp(&b.1))
            .then(a.2.total_cmp(&b.2))
            .then_with(|| a.3.cmp(&b.3))
    });
    let mut chosen: Vec<String> = ranked
        .into_iter()
        .map(|(_, _, _, id)| id)
        .take(VERIFICATION_CANDIDATES)
        .collect();
    if chosen.len() < VERIFICATION_CANDIDATES
        && let Some(default) = document
            .pointer(&format!("/default/{provider}"))
            .and_then(Value::as_str)
    {
        let default = default
            .strip_prefix(&format!("{provider}/"))
            .unwrap_or(default);
        let usable = entries
            .iter()
            .find(|(id, _)| id == default)
            .is_none_or(|(_, model)| text_capable(model));
        if usable && !chosen.iter().any(|id| id == default) {
            chosen.push(default.to_owned());
        }
    }
    chosen
}

/// Parses the engine's supported-provider document into a display-ordered catalog.
///
/// `/provider` returns `{ "all": [ ... ], "connected": [ ... ] }`; `/config/providers`
/// returns `{ "providers": [ ... ] }`. Both shapes are accepted. Connected and popular
/// providers come first. Only ids and display labels cross the bridge; provider
/// configuration, environment names, model details and keys stay in the core.
#[must_use]
pub fn parse_providers(body: &[u8]) -> Vec<EngineProvider> {
    providers::parse_provider_catalog(body).engine_providers()
}

/// Classifies an engine message response body that may carry a provider error under
/// HTTP 200. The pinned engine returns `info.error.statusCode: 401` for a rejected key.
#[must_use]
pub fn classify_message_response(body: &[u8]) -> KeyVerification {
    let Ok(document) = serde_json::from_slice::<Value>(body) else {
        return KeyVerification::Unverified;
    };
    let Some(error) = find_error(&document) else {
        return KeyVerification::Verified;
    };
    if error.is_null() {
        return KeyVerification::Verified;
    }
    match error_status(error) {
        Some(401 | 403) => KeyVerification::Invalid,
        // A quota, rate limit or provider outage does not prove the key is wrong.
        Some(_) | None => KeyVerification::Unverified,
    }
}

fn find_error(value: &Value) -> Option<&Value> {
    match value {
        Value::Object(map) => {
            if let Some(error) = map.get("error") {
                return Some(error);
            }
            map.values().find_map(find_error)
        }
        Value::Array(items) => items.iter().find_map(find_error),
        _ => None,
    }
}

fn error_status(value: &Value) -> Option<u16> {
    match value {
        Value::Object(map) => {
            for key in ["statusCode", "status_code", "status", "code"] {
                if let Some(code) = map.get(key).and_then(Value::as_u64)
                    && (100..=599).contains(&code)
                {
                    return u16::try_from(code).ok();
                }
            }
            map.values().find_map(error_status)
        }
        Value::Array(items) => items.iter().find_map(error_status),
        _ => None,
    }
}

/// Parses `opencode` provider/model documents without depending on optional fields.
///
/// When a `/provider` document carries a `connected` provider id array, only models from
/// connected providers are returned from `all` so disconnected catalog entries (such as
/// `opencode-go/*`) are never offered as usable runtime models.
#[must_use]
pub fn parse_models(body: &[u8]) -> Vec<RuntimeModel> {
    let Ok(document) = serde_json::from_slice::<Value>(body) else {
        return Vec::new();
    };
    let connected: Option<std::collections::HashSet<&str>> = document
        .get("connected")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect());
    let mut models = Vec::new();
    for key in ["all", "providers"] {
        let Some(providers) = document.get(key).and_then(Value::as_array) else {
            continue;
        };
        for provider in providers {
            let Some(provider_id) = provider.get("id").and_then(Value::as_str) else {
                continue;
            };
            if key == "all"
                && let Some(connected) = connected.as_ref()
                && !connected.contains(provider_id)
            {
                continue;
            }
            let Some(entries) = provider.get("models") else {
                continue;
            };
            if let Some(map) = entries.as_object() {
                for (model_id, model) in map {
                    models.push(runtime_model(provider_id, model_id, model));
                }
            } else if let Some(array) = entries.as_array() {
                for model in array {
                    if let Some(model_id) = model.get("id").and_then(Value::as_str) {
                        models.push(runtime_model(provider_id, model_id, model));
                    }
                }
            }
        }
    }
    models.sort_by(|left, right| left.id.cmp(&right.id));
    models.dedup_by(|left, right| left.id == right.id);
    models
}

fn runtime_model(provider_id: &str, model_id: &str, model: &Value) -> RuntimeModel {
    let id = if model_id.starts_with(&format!("{provider_id}/")) {
        model_id.to_owned()
    } else {
        format!("{provider_id}/{model_id}")
    };
    RuntimeModel {
        id,
        provider: provider_id.to_owned(),
        label: model
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(model_id)
            .to_owned(),
        context_tokens: model
            .get("limit")
            .and_then(|limit| limit.get("context"))
            .and_then(Value::as_u64),
        free: matches!(provider_id, "opencode" | "opencode-go") && input_cost(model) == Some(0.0),
    }
}

fn input_cost(model: &Value) -> Option<f64> {
    model
        .get("cost")
        .and_then(|cost| {
            cost.get("input")
                .or_else(|| cost.get("0").and_then(|tier| tier.get("input")))
        })
        .and_then(Value::as_f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_locator_prefers_the_resource_dir_and_never_path() {
        let resource = tempfile::tempdir().unwrap();
        let path_dir = tempfile::tempdir().unwrap();
        let triple = current_target_triple().unwrap_or("x86_64-unknown-linux-gnu");
        let binaries = resource.path().join("binaries");
        std::fs::create_dir_all(&binaries).unwrap();
        let bundled = binaries.join(sidecar_file_name(triple));
        std::fs::write(&bundled, b"#!/bin/sh\n").unwrap();
        let decoy = path_dir.path().join("opencode");
        std::fs::write(&decoy, b"#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bundled, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::set_permissions(&decoy, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let locator = EngineBinaryLocator::new(vec![resource.path().join("missing"), binaries]);
        assert_eq!(locator.locate(), Some(bundled));
        assert!(
            !locator
                .directories()
                .iter()
                .any(|dir| dir == path_dir.path())
        );
        assert_eq!(loopback_host(), "127.0.0.1");
    }

    #[test]
    fn engine_serve_args_are_loopback_only() {
        let args = serve_args(43117);
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--hostname", "127.0.0.1"])
        );
        assert!(args.windows(2).any(|pair| pair == ["--port", "43117"]));
        assert!(!args.iter().any(|arg| arg == "0.0.0.0"));
        let port = reserve_loopback_port().unwrap();
        assert!(port > 0);
    }

    #[test]
    fn engine_logs_are_bounded_and_redacted() {
        let mut buffer = EngineLogBuffer::new(3);
        let secret = ["sk", "abcdefgh12345678"].join("-");
        buffer.push(&format!("provider token {secret}"));
        buffer.push("one");
        buffer.push("two");
        buffer.push("three");
        buffer.push("four");
        assert_eq!(buffer.len(), 3);
        let snapshot = buffer.snapshot();
        assert!(!snapshot.join("\n").contains(&secret));
        assert!(!snapshot.iter().any(|line| line.contains("abcdefgh")));
        assert_eq!(snapshot.last().map(String::as_str), Some("four"));
    }

    #[test]
    fn engine_model_parser_marks_only_free_opencode_models_included() {
        let body = br#"{
          "all": [
            {"id": "opencode", "models": {
              "big-pickle": {"name": "Big Pickle", "cost": {"input": 0}, "limit": {"context": 200000}},
              "paid": {"name": "Paid", "cost": {"input": 1.5}}
            }},
            {"id": "openai", "models": {"free-looking": {"name": "Not included", "cost": {"input": 0}}}}
          ]
        }"#;
        let models = parse_models(body);
        assert_eq!(models.len(), 3);
        let pickle = models
            .iter()
            .find(|model| model.id == "opencode/big-pickle")
            .unwrap();
        assert!(pickle.free);
        assert_eq!(pickle.context_tokens, Some(200_000));
        let openai = models
            .iter()
            .find(|model| model.id == "openai/free-looking")
            .unwrap();
        assert!(!openai.free);
    }

    #[test]
    fn engine_provider_parser_reads_the_full_supported_catalog() {
        let body = br#"{
          "all": [
            {"id": "opencode", "name": "OpenCode"},
            {"id": "groq", "name": "Groq"},
            {"id": "mistral", "name": "Mistral"},
            {"id": "no-name"}
          ],
          "default": {}
        }"#;
        let providers = parse_providers(body);
        assert_eq!(providers.len(), 4);
        assert!(
            providers
                .iter()
                .any(|provider| provider.id == "groq" && provider.label == "Groq")
        );
        assert!(
            providers
                .iter()
                .any(|provider| provider.id == "mistral" && provider.label == "Mistral")
        );
        assert!(
            providers
                .iter()
                .any(|provider| provider.id == "no-name" && provider.label == "no-name")
        );
    }

    #[test]
    fn engine_provider_parser_accepts_the_config_providers_shape() {
        let body = br#"{"providers":[{"id":"deepseek","name":"DeepSeek"},{"id":"opencode","name":"OpenCode"}]}"#;
        let providers = parse_providers(body);
        let ids: Vec<&str> = providers
            .iter()
            .map(|provider| provider.id.as_str())
            .collect();
        assert_eq!(ids, ["opencode", "deepseek"]);
        assert_eq!(providers[0].label, "OpenCode");
    }

    #[test]
    fn engine_message_body_auth_error_is_invalid_even_with_http_200() {
        let invalid = br#"{"info":{"error":{"statusCode":401,"message":"Invalid API key"}}}"#;
        assert_eq!(classify_message_response(invalid), KeyVerification::Invalid);
        let forbidden = br#"{"error":{"status":403}}"#;
        assert_eq!(
            classify_message_response(forbidden),
            KeyVerification::Invalid
        );
    }

    #[test]
    fn engine_message_body_success_and_quota_errors_are_not_auth_success() {
        let success = br#"{"info":{"role":"assistant"},"parts":[{"type":"text","text":"OK"}]}"#;
        assert_eq!(
            classify_message_response(success),
            KeyVerification::Verified
        );
        let null_error = br#"{"info":{"error":null},"parts":[]}"#;
        assert_eq!(
            classify_message_response(null_error),
            KeyVerification::Verified
        );
        let quota = br#"{"info":{"error":{"statusCode":429}}}"#;
        assert_eq!(
            classify_message_response(quota),
            KeyVerification::Unverified
        );
        let not_json = b"<html>bad gateway</html>";
        assert_eq!(
            classify_message_response(not_json),
            KeyVerification::Unverified
        );
    }

    #[test]
    fn engine_http_parser_reads_chunked_responses() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        let response = parse_http_response(raw).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"hello");
    }

    fn python_available() -> bool {
        Command::new("python3")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    /// Process tests create and execute temporary scripts; running them one at a time
    /// avoids Linux ETXTBSY races between test threads.
    static PROCESS_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    /// A unique executable name prevents a reused temp directory from colliding with a
    /// fixture named `opencode` written by another test.
    static FAKE_ENGINE_COUNTER: std::sync::atomic::AtomicUsize =
        std::sync::atomic::AtomicUsize::new(0);

    #[cfg(unix)]
    fn write_fake_engine(directory: &Path) -> PathBuf {
        let sequence = FAKE_ENGINE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = directory.join(format!("opencode-fake-{sequence}"));
        let script = r#"#!/usr/bin/env python3
import http.server, json, os, sys
port = int(sys.argv[sys.argv.index("--port") + 1])
try:
    auth = json.loads(os.getenv("OPENCODE_AUTH_CONTENT", "{}"))
except Exception:
    auth = {}
class Handler(http.server.BaseHTTPRequestHandler):
    def send_json(self, payload, status=200):
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def providers(self):
        all_providers = [{"id": "opencode", "models": {"big-pickle": {"name": "Big Pickle", "cost": {"input": 0}}}}]
        if "deepseek" in auth:
            all_providers.append({"id": "deepseek", "models": {"deepseek-flash": {"name": "DeepSeek Flash", "cost": {"input": 1}}}})
        if "openrouter" in auth:
            all_providers.append({"id": "openrouter", "models": {
                "free/model-a:free": {"name": "Free A", "cost": {"input": 0}},
                "free/model-b:free": {"name": "Free B", "cost": {"input": 0}},
                "free/model-c:free": {"name": "Free C", "cost": {"input": 0}},
                "paid/model-a": {"name": "Paid A", "cost": {"input": 0.1}},
                "paid/model-b": {"name": "Paid B", "cost": {"input": 0.2}}
            }})
        return all_providers
    def do_GET(self):
        if self.path == "/global/health":
            self.send_json({"healthy": True, "version": "1.19.5"})
        elif self.path == "/provider":
            # The real /provider document can exceed a small body cap. The adapter must
            # fall back to /config/providers instead of failing key verification.
            self.send_error(500)
        elif self.path == "/config/providers":
            self.send_json({"providers": self.providers(), "default": {}, "connected": list(auth)})
        else:
            self.send_error(404)
    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0") or 0)
        raw = self.rfile.read(length) if length else b"{}"
        try:
            payload = json.loads(raw or b"{}")
        except Exception:
            payload = {}
        if self.path == "/session":
            self.send_json({"id": "ses_test"})
        elif self.path.endswith("/message"):
            model = payload.get("model") or {}
            provider_id = model.get("providerID")
            model_id = model.get("modelID") or ""
            if provider_id == "openrouter":
                openrouter_key = (auth.get("openrouter") or {}).get("key")
                if openrouter_key != "valid-test-key":
                    self.send_json({"info": {"error": {"statusCode": 401, "message": "Invalid API key"}}})
                elif model_id.endswith(":free"):
                    # Free-tier calls are rate-limited or privacy-gated, so a key probe
                    # must never settle Unverified while a paid model could succeed.
                    self.send_json({"info": {"error": {"statusCode": 404, "message": "No endpoints found matching your data policy"}}})
                else:
                    self.send_json({"info": {"role": "assistant"}, "parts": [{"type": "text", "text": "OK"}]})
            else:
                key = (auth.get("deepseek") or {}).get("key")
                if key == "valid-test-key":
                    self.send_json({"info": {"role": "assistant"}, "parts": [{"type": "text", "text": "OK"}]})
                else:
                    # The pinned engine returns HTTP 200 and carries the provider error in
                    # the body. The adapter must inspect the body, not just the status line.
                    self.send_json({"info": {"error": {"statusCode": 401, "message": "Invalid API key"}}})
        else:
            self.send_error(404)
    def do_DELETE(self):
        self.send_json({"ok": True})
    def log_message(self, *args):
        pass
http.server.HTTPServer(("127.0.0.1", port), Handler).serve_forever()
"#;
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(script.as_bytes()).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn process_alive(pid: u32) -> bool {
        Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn engine_process_runtime_health_models_and_shutdown_have_no_orphan() {
        if !python_available() {
            return;
        }
        let _guard = PROCESS_TEST_LOCK.lock().await;
        let directory = tempfile::tempdir().unwrap();
        let binary = write_fake_engine(directory.path());
        let runtime = ProcessEngineRuntime::with_binary(binary);
        let info = runtime.spawn(0).await.unwrap();
        let pid = info.pid.unwrap();
        let mut health = None;
        for _ in 0..80 {
            if let Ok(result) = runtime.probe_health(&info).await {
                health = Some(result);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let health = health.expect("fake engine should become healthy");
        assert_eq!(health.version.as_deref(), Some("1.19.5"));
        let models = runtime.models(info.port).await.unwrap();
        assert_eq!(models.len(), 1);
        assert!(models[0].free);
        runtime.shutdown(&info).await.unwrap();
        assert!(!process_alive(pid), "shutdown must reap the engine process");
    }

    #[test]
    fn parse_models_filters_disconnected_providers_when_connected_list_is_present() {
        let body = br#"{
            "connected": ["opencode"],
            "all": [
                {
                    "id": "opencode-go",
                    "models": {
                        "beam-free": { "name": "Beam Free", "cost": { "input": 0, "output": 0 } }
                    }
                },
                {
                    "id": "opencode",
                    "models": {
                        "beam-free": { "name": "Beam Free", "cost": { "input": 0, "output": 0 } }
                    }
                }
            ]
        }"#;
        let models = parse_models(body);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].provider, "opencode");
        assert_eq!(models[0].id, "opencode/beam-free");
    }

    const OPENROUTER_FIXTURE: &[u8] = br#"{
        "default": { "openrouter": "zeta/default-text" },
        "providers": [
            { "id": "other", "models": { "x": { "cost": { "input": 0.0 } } } },
            { "id": "openrouter", "models": {
                "aion-labs/aion-2.0": { "capabilities": { "reasoning": true, "input": { "text": true }, "output": { "text": true } }, "cost": { "input": 0.1 } },
                "old/deprecated-cheap": { "status": "deprecated", "cost": { "input": 0.0 } },
                "img/image-only": { "capabilities": { "reasoning": false, "input": { "text": false }, "output": { "text": false } }, "cost": { "input": 0.0 } },
                "cheap/text-b": { "capabilities": { "reasoning": false, "input": { "text": true }, "output": { "text": true } }, "cost": { "input": 0.05 } },
                "cheap/text-a": { "capabilities": { "reasoning": false, "input": { "text": true }, "output": { "text": true } }, "cost": { "input": 0.05 } },
                "mid/text": { "capabilities": { "reasoning": false }, "cost": { "input": 0.5 } },
                "free/model:free": { "capabilities": { "reasoning": false }, "cost": { "input": 0.0 } }
            } }
        ]
    }"#;

    #[test]
    fn select_verification_models_prefers_cheap_active_text_non_reasoning() {
        let chosen = select_verification_models(OPENROUTER_FIXTURE, "openrouter");
        // Paid non-reasoning first, then paid reasoning, with the rate-limited free tier last.
        assert_eq!(
            chosen,
            vec![
                "cheap/text-a",
                "cheap/text-b",
                "mid/text",
                "aion-labs/aion-2.0",
                "free/model:free"
            ]
        );
        assert!(select_verification_models(OPENROUTER_FIXTURE, "missing").is_empty());
    }

    #[test]
    fn select_verification_models_falls_back_to_a_text_capable_default() {
        let body = br#"{
            "default": { "p": "p/only" },
            "all": [ { "id": "p", "models": [
                { "id": "only", "cost": { "input": 1 } },
                { "id": "bad", "status": "deprecated" }
            ] } ]
        }"#;
        assert_eq!(select_verification_models(body, "p"), vec!["only"]);
        let body = br#"{
            "default": { "p": "img" },
            "providers": [ { "id": "p", "models": { "img": { "capabilities": { "input": { "text": false } } } } } ]
        }"#;
        assert!(select_verification_models(body, "p").is_empty());
    }

    #[test]
    fn verify_candidates_moves_past_model_specific_failures() {
        let candidates: Vec<String> = ["a", "b", "c"].map(str::to_owned).to_vec();
        let mut seen = Vec::new();
        let result = verify_candidates(&candidates, |model| {
            seen.push(model.to_owned());
            match model {
                "a" => ProbeOutcome::TryNext,
                "b" => ProbeOutcome::Final(KeyVerification::Unverified),
                _ => ProbeOutcome::Final(KeyVerification::Verified),
            }
        });
        assert_eq!(result, KeyVerification::Verified);
        assert_eq!(seen, vec!["a", "b", "c"]);
        let invalid = verify_candidates(&candidates, |model| {
            if model == "a" {
                ProbeOutcome::Final(KeyVerification::Invalid)
            } else {
                panic!("must stop at the first decisive result")
            }
        });
        assert_eq!(invalid, KeyVerification::Invalid);
        assert_eq!(
            verify_candidates(&candidates, |_| ProbeOutcome::TryNext),
            KeyVerification::Unverified
        );
        assert_eq!(
            verify_candidates(&[], |_| unreachable!()),
            KeyVerification::Unverified
        );
    }

    #[test]
    fn classify_probe_response_flags_model_specific_errors() {
        let not_found = br#"{"info":{"error":{"name":"ProviderModelNotFoundError","data":{}}}}"#;
        assert_eq!(classify_probe_response(not_found), ProbeOutcome::TryNext);
        let bad_request = br#"{"info":{"error":{"statusCode":404}}}"#;
        assert_eq!(classify_probe_response(bad_request), ProbeOutcome::TryNext);
        let empty = br#"{"info":{"error":{}}}"#;
        assert_eq!(classify_probe_response(empty), ProbeOutcome::TryNext);
        let rate = br#"{"info":{"error":{"statusCode":429}}}"#;
        assert_eq!(
            classify_probe_response(rate),
            ProbeOutcome::Final(KeyVerification::Unverified)
        );
        let rejected = br#"{"info":{"error":{"statusCode":401}}}"#;
        assert_eq!(
            classify_probe_response(rejected),
            ProbeOutcome::Final(KeyVerification::Invalid)
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn engine_process_runtime_uses_and_verifies_a_stored_byok_key() {
        if !python_available() {
            return;
        }
        let _guard = PROCESS_TEST_LOCK.lock().await;
        let directory = tempfile::tempdir().unwrap();
        let binary = write_fake_engine(directory.path());
        let keychain = Arc::new(crate::engine::FakeKeychain::new());
        keychain
            .set("deepseek", &SecretString::new("valid-test-key"))
            .unwrap();
        let runtime = ProcessEngineRuntime::with_binary(binary).with_key_source(keychain);
        let info = runtime.spawn(0).await.unwrap();
        let mut healthy = false;
        for _ in 0..80 {
            if runtime.probe_health(&info).await.is_ok() {
                healthy = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(healthy, "fake engine should become healthy");
        let models = runtime.models(info.port).await.unwrap();
        assert!(
            models
                .iter()
                .any(|model| model.id == "deepseek/deepseek-flash"),
            "the stored key must reach the sidecar so its provider is usable"
        );

        let verified = runtime
            .verify_key(info.port, "deepseek", &SecretString::new("valid-test-key"))
            .await;
        assert_eq!(verified, KeyVerification::Verified);
        let rejected = runtime
            .verify_key(
                info.port,
                "deepseek",
                &SecretString::new("invalid-test-key"),
            )
            .await;
        assert_eq!(rejected, KeyVerification::Invalid);
        runtime.shutdown(&info).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn openrouter_key_verifies_on_a_paid_model_when_free_models_fail() {
        if !python_available() {
            return;
        }
        let _guard = PROCESS_TEST_LOCK.lock().await;
        let directory = tempfile::tempdir().unwrap();
        let binary = write_fake_engine(directory.path());
        let runtime = ProcessEngineRuntime::with_binary(binary);
        let info = runtime.spawn(0).await.unwrap();
        let verified = runtime
            .verify_key(
                info.port,
                "openrouter",
                &SecretString::new("valid-test-key"),
            )
            .await;
        assert_eq!(verified, KeyVerification::Verified);
        let rejected = runtime
            .verify_key(info.port, "openrouter", &SecretString::new("bad-key"))
            .await;
        assert_eq!(rejected, KeyVerification::Invalid);
        runtime.shutdown(&info).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn engine_process_runtime_drop_kills_a_live_child() {
        if !python_available() {
            return;
        }
        let _guard = PROCESS_TEST_LOCK.lock().await;
        let directory = tempfile::tempdir().unwrap();
        let binary = write_fake_engine(directory.path());
        let runtime = ProcessEngineRuntime::with_binary(binary);
        let info = runtime.spawn(0).await.unwrap();
        let pid = info.pid.unwrap();
        drop(runtime);
        assert!(!process_alive(pid), "app exit must leave no engine orphan");
    }
}
