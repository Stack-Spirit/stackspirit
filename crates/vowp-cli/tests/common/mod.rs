//! Shared hermetic fixtures for the `vowp-cli` integration tests.
//!
//! Every test gets its own temporary node home and project directory. Commands run
//! against a private tmux directory derived from that home; the helper never reads the
//! real home directory, starts no harness, and never touches the network.
//!
//! Cleanup has two layers. A `Drop` guard kills its private server during a normal
//! panic-safe unwind, but a test process killed by a timeout, the OOM killer or an abort
//! never runs `Drop`; the server then outlives its temporary home and every
//! `vowp __window` pane keeps polling. [`sweep_stale_test_servers_once`] runs at test
//! start, before the first private server exists, and kills only the leftovers. It never
//! touches a server whose private directory still exists, the default socket, or the
//! `vowp-home`, `vowp-night` and `vowp-orchestrator` sessions.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;
use std::time::{Duration, Instant};

use tempfile::TempDir;
use vowp_cli::{BufferIo, Global};
use vowp_protocol::{Job, JobId, JobKind, NewJob, Node, NodeName, Project, Role, VowpConfig};
use vowp_store::{NodePaths, Store};
use vowp_tmux::Tmux;

/// One temporary node home and project.
pub struct TestEnv {
    /// Node home (`--home`).
    pub home: TempDir,
    /// Project checkout containing `vowp.toml`.
    pub project: TempDir,
    /// The config written into the project.
    pub config: VowpConfig,
}

impl TestEnv {
    /// Creates the temp home, project config and registered node/project rows.
    pub fn new() -> Self {
        sweep_stale_test_servers_once();
        let home = tempfile::tempdir().expect("temp home");
        let project = tempfile::tempdir().expect("temp project");
        let config = VowpConfig::with_defaults_for_tests();
        let text = toml::to_string_pretty(&config).expect("config serialises");
        std::fs::write(project.path().join("vowp.toml"), text).expect("write vowp.toml");
        std::fs::create_dir_all(project.path().join(".vowp/jobs")).expect("jobs dir");

        let paths = NodePaths::new(home.path());
        let store = Store::open(&paths).expect("store opens");
        let now = store.clock().now();
        store
            .upsert_node(&Node {
                name: NodeName::parse("local").expect("local slug"),
                ssh_target: None,
                is_local: true,
                capabilities: serde_json::json!({}),
                last_seen_at: Some(now),
                created_at: now,
            })
            .expect("node row");
        store
            .upsert_project(&Project {
                name: config.project.name.clone(),
                path: project.path().display().to_string(),
                remote_url: None,
                default_branch: "main".to_owned(),
                config: serde_json::to_value(&config).expect("config json"),
                created_at: now,
                updated_at: now,
            })
            .expect("project row");
        let tmux = Tmux::for_home_with_socket(home.path(), None);
        let dir = tmux.socket_dir().expect("private tmux dir").to_path_buf();
        assert_private_tmux(&tmux, &dir);
        assert_eq!(dir.file_name(), Some(std::ffi::OsStr::new("tmux")));
        Self { home, project, config }
    }

    /// Returns the node home path.
    pub fn home_path(&self) -> &Path {
        self.home.path()
    }

    /// Returns the project path.
    pub fn project_path(&self) -> &Path {
        self.project.path()
    }

    /// Returns the private tmux directory for this environment.
    pub fn tmux_dir(&self) -> PathBuf {
        std::fs::canonicalize(self.home.path())
            .unwrap_or_else(|_| self.home.path().to_path_buf())
            .join("tmux")
    }

    /// Returns the scoped private tmux manager for this environment.
    pub fn tmux(&self) -> Tmux {
        let tmux = Tmux::for_home_with_socket(self.home.path(), None);
        assert_private_tmux(&tmux, &self.tmux_dir());
        tmux
    }

    /// Builds a child command with the isolated home and tmux environment.
    pub fn command(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        command
            .env("HOME", self.home.path())
            .env("TMUX_TMPDIR", self.tmux_dir())
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env_remove("VOWP_TMUX_SOCKET");
        command
    }

    /// Builds the global flags this environment needs.
    pub fn global(&self, json: bool) -> Global {
        Global {
            json,
            home: Some(self.home.path().to_path_buf()),
            project: Some(self.project.path().display().to_string()),
            ..Global::default()
        }
    }

    /// Runs one command in-process and returns `(code, stdout, stderr)`.
    pub fn run(&self, args: &[&str]) -> (i32, String, String) {
        let mut full = vec![
            "vowp".to_owned(),
            "--home".to_owned(),
            self.home.path().display().to_string(),
            "--project".to_owned(),
            self.project.path().display().to_string(),
        ];
        full.extend(args.iter().map(|value| (*value).to_owned()));
        let mut io = BufferIo::new("");
        let code = vowp_cli::run(full, &mut io);
        (code, io.stdout(), io.stderr())
    }

    /// Runs one command with `--json` and parses its stdout document.
    pub fn run_json(&self, args: &[&str]) -> (i32, serde_json::Value, String) {
        let mut full = vec!["--json"];
        full.extend(args.iter().copied());
        let (code, stdout, stderr) = self.run(&full);
        let value = serde_json::from_str(&stdout)
            .unwrap_or_else(|error| panic!("stdout is not JSON ({error}): {stdout:?}; stderr: {stderr:?}"));
        (code, value, stderr)
    }

    /// Opens a fresh store connection to the temporary home.
    pub fn store(&self) -> Store {
        Store::open(&NodePaths::new(self.home.path())).expect("store opens")
    }

    /// Inserts a pending job directly.
    pub fn insert_job(&self, id: &str, title: &str, kind: JobKind, needs: &[&str]) -> Job {
        let store = self.store();
        let new = NewJob {
            id: JobId::parse(id).expect("job id"),
            title: title.to_owned(),
            kind,
            role: Role::Worker,
            profile: None,
            needs: needs.iter().map(|need| JobId::parse(need).expect("need id")).collect(),
            owns: Vec::new(),
            reads: Vec::new(),
            gates: None,
            priority: None,
            loop_: None,
            brief_text: None,
        };
        store
            .insert_job(&self.config.project.name, &new, "test")
            .expect("insert job")
    }

    /// Returns the path of one job's brief.
    pub fn brief_path(&self, id: &str) -> PathBuf {
        self.project.path().join(".vowp/jobs").join(id).join("brief.md")
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        kill_private_server_for_home(self.home.path());
    }
}

/// Kills the private tmux server owned by `home`, when one is running.
///
/// Fixtures that spawn a real supervisor (which may create a job window even with
/// `--no-tmux`) call this from their own `Drop`; a normal unwind is the only chance
/// to stop the server before the temporary home disappears.
pub fn kill_private_server_for_home(home: &Path) {
    let tmux = Tmux::for_home_with_socket(home, None);
    let Some(socket) = tmux.socket_name().map(str::to_owned) else {
        return;
    };
    let Some(dir) = tmux.socket_dir().map(Path::to_path_buf) else {
        return;
    };
    if let Ok(guard) = Tmux::in_private_dir(socket, dir) {
        let _ = guard.kill_server_for_tests();
    }
}

/// Fails loudly if a test manager could resolve to the user's default tmux server.
pub fn assert_private_tmux(tmux: &Tmux, dir: &Path) {
    let socket = tmux.socket_name().expect("private tmux socket");
    assert_ne!(socket, "default", "refusing to test against the default tmux server");
    assert_eq!(tmux.socket_dir(), Some(dir), "unexpected TMUX_TMPDIR");
    let path = tmux.socket_path().expect("private tmux socket path");
    assert_eq!(path, dir.join(socket), "tmux socket escaped the private directory");
    assert!(!path.ends_with("default"), "private tmux path ends in default");
}

/// One private tmux server found by the test-start sweep.
struct PrivateTestServer {
    pid: u32,
    socket: String,
    session: String,
    tmpdir: PathBuf,
}

static SWEEP_ONCE: Once = Once::new();

/// Runs [`sweep_stale_test_servers`] at most once in this test binary.
///
/// Call this before creating a private tmux server outside [`TestEnv`]. A test
/// process killed before its `Drop` runs leaks its server; the sweep is the only
/// cleanup that still happens on the next run.
pub fn sweep_stale_test_servers_once() {
    SWEEP_ONCE.call_once(sweep_stale_test_servers);
}

/// Kills leftover private test tmux servers whose temporary home was deleted.
///
/// The sweep reads `/proc/<pid>/cmdline` and only considers a process whose argv
/// starts with `tmux -L vowp-…` and whose `-s` session is a test session
/// (`vowp-test` or `vowp-test-…`). It reads that server's `TMUX_TMPDIR` from
/// `/proc/<pid>/environ`; a server whose directory or parent home still exists is
/// left alone. The default socket and the `vowp-home`, `vowp-night` and
/// `vowp-orchestrator` sessions are never candidates. Stale candidates get
/// `SIGTERM`, then `SIGKILL` if they do not exit; the process command line is
/// re-checked before each signal so a recycled pid is not touched.
pub fn sweep_stale_test_servers() {
    for server in private_test_servers() {
        if stale_private_path(&server.tmpdir) {
            kill_private_test_server(&server);
        }
    }
}

/// Enumerates the running `tmux -L vowp-… -s vowp-test…` servers.
fn private_test_servers() -> Vec<PrivateTestServer> {
    let mut servers = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return servers;
    };
    for entry in entries.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|name| name.parse::<u32>().ok()) else {
            continue;
        };
        let Some(argv) = process_argv(pid) else {
            continue;
        };
        let Some(socket) = private_socket(&argv) else {
            continue;
        };
        let Some(session) = session_argument(&argv) else {
            continue;
        };
        if !is_test_session(&session) || is_reserved_session(&session) {
            continue;
        }
        let Some(tmpdir) = process_tmpdir(pid) else {
            continue;
        };
        servers.push(PrivateTestServer {
            pid,
            socket: socket.to_owned(),
            session,
            tmpdir,
        });
    }
    servers
}

/// Returns the NUL-separated argv of `pid`.
fn process_argv(pid: u32) -> Option<Vec<String>> {
    let bytes = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let argv: Vec<String> = bytes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect();
    (!argv.is_empty()).then_some(argv)
}

/// Returns the private socket from a `tmux -L <socket> …` command line.
fn private_socket(argv: &[String]) -> Option<&str> {
    let program = Path::new(argv.first()?).file_name()?.to_str()?;
    if program != "tmux" || argv.get(1)? != "-L" {
        return None;
    }
    let socket = argv.get(2)?.as_str();
    if socket == "default" || !socket.starts_with("vowp-") {
        return None;
    }
    Some(socket)
}

/// Returns the value of a tmux `-s` session argument before the command separator.
fn session_argument(argv: &[String]) -> Option<String> {
    let mut index = 0;
    while index < argv.len() {
        let argument = &argv[index];
        if argument == "--" {
            return None;
        }
        if argument == "-s" {
            return argv.get(index + 1).cloned();
        }
        if let Some(value) = argument.strip_prefix("-s=") {
            return (!value.is_empty()).then(|| value.to_owned());
        }
        index += 1;
    }
    None
}

/// Returns the server's `TMUX_TMPDIR` from `/proc/<pid>/environ`.
fn process_tmpdir(pid: u32) -> Option<PathBuf> {
    let environ = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
    for entry in environ.split(|byte| *byte == 0) {
        let Some(value) = entry.strip_prefix(b"TMUX_TMPDIR=") else {
            continue;
        };
        if value.is_empty() {
            return None;
        }
        return Some(PathBuf::from(String::from_utf8_lossy(value).into_owned()));
    }
    None
}

/// Whether a private `vowp-test` session name is in scope for the sweep.
fn is_test_session(session: &str) -> bool {
    session == "vowp-test" || session.starts_with("vowp-test-")
}

/// Sessions that belong to the real units and must never be swept.
fn is_reserved_session(session: &str) -> bool {
    matches!(session, "vowp-home" | "vowp-night" | "vowp-orchestrator")
}

/// Whether the server's private directory, or the home containing it, is gone.
fn stale_private_path(dir: &Path) -> bool {
    if !dir.is_absolute() {
        return false;
    }
    if !dir.exists() {
        return true;
    }
    match dir.parent() {
        Some(home) => !home.exists(),
        None => false,
    }
}

/// Kills one stale private server after re-checking that the pid is still the same server.
fn kill_private_test_server(server: &PrivateTestServer) {
    if !still_matches(server) {
        return;
    }
    let _ = Command::new("kill")
        .arg("-TERM")
        .arg(server.pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if !Path::new(&format!("/proc/{}", server.pid)).exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if still_matches(server) {
        let _ = Command::new("kill")
            .arg("-KILL")
            .arg(server.pid.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

/// Re-reads the process state so a recycled pid is never signalled.
fn still_matches(server: &PrivateTestServer) -> bool {
    let Some(argv) = process_argv(server.pid) else {
        return false;
    };
    private_socket(&argv) == Some(server.socket.as_str())
        && session_argument(&argv).as_deref() == Some(server.session.as_str())
        && process_tmpdir(server.pid).as_deref() == Some(server.tmpdir.as_path())
}
