//! Integration tests for `vowp setup plan|home|control` and `scripts/install.sh`.
//!
//! Every setup test runs the real binary in a temporary local home and control home. `ssh`
//! is a fake script on `PATH` that executes the remote argv in a temporary remote home with
//! fake `vowp`, `sudo`, `systemctl`, `loginctl` and user tools. Nothing here touches the real
//! `~/.vowp`, the default tmux server, the network (the installer test uses loopback only)
//! or a real harness.

use std::collections::BTreeMap;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Once};
use std::thread;

use serde_json::Value;
use tempfile::TempDir;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A local machine, a fake remote home and the fake tool directory.
struct Machine {
    local: TempDir,
    control: TempDir,
    remote: TempDir,
    bin: TempDir,
    remote_bin: TempDir,
    log: PathBuf,
    tools_log: PathBuf,
}

impl Machine {
    fn new() -> Self {
        let local = tempfile::tempdir().expect("temp local home");
        let control = tempfile::tempdir().expect("temp control home");
        let remote = tempfile::tempdir().expect("temp remote home");
        let bin = tempfile::tempdir().expect("temp fake bin");
        let remote_bin = tempfile::tempdir().expect("temp remote bin");
        std::fs::create_dir_all(local.path().join("tmux")).expect("private tmux dir");
        let log = bin.path().join("ssh.log");
        let tools_log = bin.path().join("tools.log");
        Self {
            local,
            control,
            remote,
            bin,
            remote_bin,
            log,
            tools_log,
        }
    }

    /// Writes a fake `ssh` that runs the remote command in the temporary remote home.
    fn write_ssh(&self) {
        let script = r#"#!/bin/sh
printf '%s\n' "$*" >> "$FAKE_SSH_LOG"
last=""
for arg in "$@"; do last="$arg"; done
HOME="$FAKE_REMOTE_HOME"
VOWP_HOME="$FAKE_REMOTE_HOME/.vowp"
VOWP_VAULT_HOME="$FAKE_REMOTE_HOME/vowp-vault"
VOWP_VAULT_HELPER="$FAKE_REMOTE_HOME/vowp-vault/helper"
PATH="$FAKE_REMOTE_BIN:$PATH"
export HOME VOWP_HOME VOWP_VAULT_HOME VOWP_VAULT_HELPER PATH
exec sh -c "$last"
"#;
        write_exec(&self.bin.path().join("ssh"), script);
    }

    /// Installs the fake remote tools. `sudo_mode` is `passwordless` or `password`.
    fn write_tools(&self, _sudo_mode: &str) {
        let log = &self.tools_log;
        let state = self.bin.path().join("systemctl-enabled");
        let user_state = self.bin.path().join("user-created");
        let useradd_log = self.bin.path().join("useradd.log");
        let sudo_log = self.bin.path().join("sudo.log");
        let sudo = r#"#!/bin/sh
printf '%s\n' "$*" >> SUDO_LOG
if [ "${1:-}" = "-n" ]; then
  if [ "${FAKE_SUDO_MODE:-passwordless}" = "passwordless" ]; then exit 0; else exit 1; fi
fi
if [ "${1:-}" = "-u" ]; then shift 2; fi
if [ "${1:-}" = "-H" ]; then shift; fi
if [ "${1:-}" = "env" ]; then
  shift
  while [ "$#" -gt 0 ]; do
    case "$1" in
      *=*) shift ;;
      *) break ;;
    esac
  done
fi
exec "$@"
"#
        .replace("SUDO_LOG", &shell_path(&sudo_log));
        write_exec(&self.remote_bin.path().join("sudo"), &sudo);
        let systemctl = r#"#!/bin/sh
printf 'systemctl %s\n' "$*" >> TOOLS_LOG
case "${1:-} ${2:-}" in
  '--user is-enabled') [ -f SYSTEMCTL_STATE ] && exit 0 || exit 1 ;;
  '--user enable') touch SYSTEMCTL_STATE; exit 0 ;;
  *) exit 0 ;;
esac
"#
        .replace("TOOLS_LOG", &shell_path(log))
        .replace("SYSTEMCTL_STATE", &shell_path(&state));
        write_exec(&self.remote_bin.path().join("systemctl"), &systemctl);
        write_exec(
            &self.remote_bin.path().join("loginctl"),
            &format!(
                "#!/bin/sh\nprintf 'loginctl %s\\n' \"$*\" >> {log}\nexit 0\n",
                log = shell_path(log)
            ),
        );
        let useradd = r#"#!/bin/sh
printf '%s\n' "$*" >> USERADD_LOG
touch USER_STATE
exit 0
"#
        .replace("USERADD_LOG", &shell_path(&useradd_log))
        .replace("USER_STATE", &shell_path(&user_state));
        write_exec(&self.remote_bin.path().join("useradd"), &useradd);
        let id = r#"#!/bin/sh
if [ "${1:-}" = "-u" ] && [ "${2:-}" = "vowp" ]; then
  [ -f USER_STATE ] && { printf '1001\n'; exit 0; } || exit 1
fi
exec /usr/bin/id "$@"
"#
        .replace("USER_STATE", &shell_path(&user_state));
        write_exec(&self.remote_bin.path().join("id"), &id);
        let getent = r#"#!/bin/sh
if [ "${1:-}" = "passwd" ] && [ "${2:-}" = "vowp" ] && [ -f USER_STATE ]; then
  printf 'vowp:x:1001:1001::%s:/bin/bash\n' "$FAKE_REMOTE_HOME"
  exit 0
fi
exit 2
"#
        .replace("USER_STATE", &shell_path(&user_state));
        write_exec(&self.remote_bin.path().join("getent"), &getent);
        write_exec(
            &self.remote_bin.path().join("hostname"),
            "#!/bin/sh\nprintf 'fresh-vps\\n'\n",
        );
    }

    /// Creates the vault layout the fake `vowp-vault` helper probes.
    fn write_vault_ready(&self) {
        let vault = self.remote.path().join("vowp-vault");
        std::fs::create_dir_all(&vault).expect("vault dir");
        write_exec(&vault.join("helper"), "#!/bin/sh\nexit 0\n");
    }

    /// Installs a fake `vowp` wrapper in the remote PATH and at the install prefix.
    fn write_remote_vowp(&self) {
        let real = vowp_binary();
        let body = format!(
            "#!/bin/sh\n\
             if [ \"${{1:-}}\" = \"--version\" ]; then printf 'vowp {VERSION}\\n'; exit 0; fi\n\
             exec {real} \"$@\"\n",
            real = shell_path(Path::new(&real))
        );
        write_exec(&self.remote_bin.path().join("vowp"), &body);
        let prefix = self.remote.path().join(".local/bin");
        std::fs::create_dir_all(&prefix).expect("install prefix");
        write_exec(&prefix.join("vowp"), &body);
    }

    fn command(&self) -> Command {
        let mut command = Command::new(vowp_binary());
        command
            .current_dir(self.local.path())
            .env("HOME", self.local.path())
            .env("VOWP_CONTROL_HOME", self.control.path())
            .env_remove("VOWP_HOME")
            .env("TMUX_TMPDIR", self.local.path().join("tmux"))
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env_remove("NO_COLOR")
            .env("FAKE_SSH_LOG", &self.log)
            .env("FAKE_REMOTE_HOME", self.remote.path())
            .env("FAKE_REMOTE_BIN", self.remote_bin.path())
            .env("FAKE_SUDO_MODE", "passwordless")
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.bin.path().display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            );
        command
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        self.command().args(args).output().expect("run vowp")
    }

    fn run_with_sudo(&self, mode: &str, args: &[&str]) -> std::process::Output {
        self.command()
            .env("FAKE_SUDO_MODE", mode)
            .args(args)
            .output()
            .expect("run vowp")
    }

    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    fn tools_log(&self) -> String {
        std::fs::read_to_string(&self.tools_log).unwrap_or_default()
    }

    fn useradd_log(&self) -> String {
        std::fs::read_to_string(self.bin.path().join("useradd.log")).unwrap_or_default()
    }

    /// Returns every path and byte under the fake remote tree.
    fn remote_snapshot(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut files = BTreeMap::new();
        collect(self.remote.path(), &mut files);
        files
    }
}

/// Returns the `vowp` binary Cargo built for this package.
///
/// Integration tests get `CARGO_BIN_EXE_vowp`; the file is also included as a library
/// unit-test module (where Cargo does not set it), so derive the sibling binary there.
fn vowp_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("CARGO_BIN_EXE_vowp") {
        return PathBuf::from(path);
    }
    let mut candidate = None;
    if let Ok(current) = std::env::current_exe() {
        if let Some(debug) = current.parent().and_then(Path::parent) {
            candidate = Some(debug.join("vowp"));
        }
    }
    let candidate = candidate.unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(std::env::var_os("CARGO_TARGET_DIR").unwrap_or_else(|| "target".into()))
            .join("debug/vowp")
    });
    static BUILD: Once = Once::new();
    BUILD.call_once(|| {
        // `cargo test --lib` does not rebuild the binary the test runs. Build it once so
        // the tests always exercise the code compiled from this checkout.
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let _ = Command::new(cargo)
            .args(["build", "--bin", "vowp"])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .status();
    });
    candidate
}

/// Writes an executable script.
fn write_exec(path: &Path, body: &str) {
    std::fs::write(path, body).expect("write fake tool");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod fake tool");
}

/// Reads a directory tree into a map, ignoring the fake tool directory.
fn collect(root: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, files);
        } else if let Ok(bytes) = std::fs::read(&path) {
            files.insert(path.strip_prefix(root).unwrap_or(&path).to_path_buf(), bytes);
        }
    }
}

/// Quotes a path for a POSIX shell script.
fn shell_path(path: &Path) -> String {
    let text = path.display().to_string().replace('\'', "'\\''");
    format!("'{text}'")
}

/// Parses one JSON document from stdout.
fn json(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is not JSON ({error}): {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// Returns a phase by id.
fn phase<'a>(plan: &'a Value, id: &str) -> &'a Value {
    plan["phases"]
        .as_array()
        .expect("phases array")
        .iter()
        .find(|phase| phase["id"] == id)
        .unwrap_or_else(|| panic!("missing phase {id}: {plan}"))
}

#[test]
fn setup_plan_lists_every_phase_command_and_file() {
    let machine = Machine::new();
    machine.write_ssh();
    machine.write_tools("passwordless");
    machine.write_remote_vowp();

    let output = machine.run(&[
        "--json",
        "setup",
        "plan",
        "--target",
        "fake.example",
        "--repository",
        "/srv/app",
        "--harness",
        "opencode",
        "--secret",
        "DEEPSEEK_API_KEY",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let plan = json(&output);
    assert_eq!(plan["target"], "fake.example");
    assert_eq!(plan["version"], VERSION);
    let ids: Vec<&str> = plan["phases"]
        .as_array()
        .expect("phases")
        .iter()
        .map(|phase| phase["id"].as_str().expect("phase id"))
        .collect();
    assert_eq!(
        ids,
        [
            "preflight",
            "user",
            "install",
            "home",
            "service",
            "vault",
            "projects",
            "tunnels"
        ]
    );
    for id in &ids {
        let phase = phase(&plan, id);
        assert!(phase["commands"].is_array(), "{id} commands");
        assert!(phase["files"].is_array(), "{id} files");
        assert!(phase["requires"].is_array(), "{id} requires");
        assert_eq!(phase["idempotent"], true, "{id} must be idempotent");
    }
    let install = phase(&plan, "install");
    let install_text = install["commands"].to_string();
    assert!(install_text.contains("--download"), "{install_text}");
    assert!(install_text.contains(VERSION), "{install_text}");
    let home = phase(&plan, "home");
    assert!(
        home["files"]
            .as_array()
            .expect("home files")
            .iter()
            .any(|file| file.as_str().unwrap_or_default().ends_with(".vowp/vowp.db")),
        "{home}"
    );
    let projects = phase(&plan, "projects");
    assert!(projects["commands"].to_string().contains("/srv/app"), "{projects}");
    assert!(
        plan["handoffs"]
            .as_array()
            .expect("handoffs")
            .iter()
            .any(|handoff| handoff["kind"] == "secret"
                && handoff["prompt"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("DEEPSEEK_API_KEY")),
        "{plan}"
    );
    assert!(
        !output.stdout.windows(6).any(|window| window == b"sk-"),
        "the plan must never contain a value"
    );
}

#[test]
fn setup_plan_changes_nothing() {
    let machine = Machine::new();
    machine.write_ssh();
    machine.write_tools("passwordless");
    machine.write_remote_vowp();
    let before = machine.remote_snapshot();

    let output = machine.run(&["setup", "plan", "--target", "fake.example"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    for title in [
        "preflight",
        "user",
        "install",
        "home",
        "service",
        "vault",
        "projects",
        "tunnels",
    ] {
        assert!(stdout.contains(title), "text plan misses {title}: {stdout}");
    }
    assert_eq!(machine.remote_snapshot(), before, "the plan must not change the home");
    let log = machine.log();
    assert!(!log.contains("useradd"), "plan must not create a user: {log}");
    assert!(!log.contains("curl -fsSL"), "plan must not install: {log}");
    assert!(!log.contains("project add"), "plan must not register a project: {log}");
    assert!(
        !machine.control.path().join("control.json").exists(),
        "the plan must not configure the control machine"
    );
}

#[test]
fn setup_plan_json_is_stable() {
    let machine = Machine::new();
    machine.write_ssh();
    machine.write_tools("passwordless");
    machine.write_remote_vowp();

    let first = machine.run(&["--json", "setup", "plan", "--target", "fake.example"]);
    let second = machine.run(&["--json", "setup", "plan", "--target", "fake.example"]);
    assert!(first.status.success(), "{}", String::from_utf8_lossy(&first.stderr));
    assert!(second.status.success(), "{}", String::from_utf8_lossy(&second.stderr));
    assert_eq!(json(&first), json(&second), "the plan must be deterministic");
}

#[test]
fn setup_home_is_idempotent() {
    let machine = Machine::new();
    machine.write_ssh();
    machine.write_tools("password");
    machine.write_remote_vowp();

    let first = machine.run_with_sudo(
        "password",
        &[
            "--json",
            "setup",
            "home",
            "--target",
            "fake.example",
            "--apply",
            "--yes",
        ],
    );
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let outcome = json(&first);
    assert_eq!(outcome["status"], "ok", "{outcome}");
    assert!(
        machine.remote.path().join(".vowp/vowp.db").is_file(),
        "the global database must exist"
    );
    assert!(
        machine
            .remote
            .path()
            .join(".config/systemd/user/vowp.service")
            .is_file(),
        "the user unit must exist"
    );
    let changed = outcome["changed"].as_array().expect("changed").len();
    assert!(changed > 0, "the first run must report changes: {outcome}");

    let second = machine.run_with_sudo(
        "password",
        &[
            "--json",
            "setup",
            "home",
            "--target",
            "fake.example",
            "--apply",
            "--yes",
        ],
    );
    assert!(
        second.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    let outcome = json(&second);
    assert_eq!(outcome["status"], "ok", "{outcome}");
    assert_eq!(
        outcome["changed"].as_array().expect("changed"),
        &Vec::<Value>::new(),
        "a second run must not redo finished work: {outcome}"
    );
}

#[test]
fn setup_home_creates_the_vowp_user_when_sudo_is_passwordless() {
    let machine = Machine::new();
    machine.write_ssh();
    machine.write_tools("passwordless");
    machine.write_remote_vowp();
    machine.write_vault_ready();

    let output = machine.run_with_sudo(
        "passwordless",
        &[
            "--json",
            "setup",
            "home",
            "--target",
            "fake.example",
            "--apply",
            "--yes",
        ],
    );
    assert!(
        output.status.success(),
        "status: {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let outcome = json(&output);
    assert_eq!(outcome["status"], "ok", "{outcome}");
    assert!(
        !machine.useradd_log().is_empty(),
        "the sudo path must create the vowp user"
    );
    let log = machine.log();
    assert!(
        log.contains("'sudo' '-u' 'vowp'"),
        "commands must switch to vowp: {log}"
    );
    assert!(
        machine.remote.path().join(".vowp/vowp.db").is_file(),
        "the home database must exist"
    );
    assert!(
        machine
            .remote
            .path()
            .join(".config/systemd/user/vowp.service")
            .is_file(),
        "the user unit must exist"
    );
    let checks = outcome["checks"].as_array().expect("checks");
    assert!(
        checks
            .iter()
            .any(|check| check["name"] == "user.mode"
                && check["detail"].as_str().unwrap_or_default().contains("created")),
        "{outcome}"
    );
}

#[test]
fn setup_home_no_sudo_path() {
    let machine = Machine::new();
    machine.write_ssh();
    machine.write_tools("password");
    machine.write_remote_vowp();

    let output = machine.run_with_sudo(
        "password",
        &[
            "--json",
            "setup",
            "home",
            "--target",
            "fake.example",
            "--apply",
            "--yes",
        ],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let outcome = json(&output);
    let checks = outcome["checks"].as_array().expect("checks");
    let user = checks
        .iter()
        .find(|check| check["name"] == "user.mode")
        .expect("user.mode check");
    assert!(
        user["detail"].as_str().unwrap_or_default().contains("no-sudo mode"),
        "{user}"
    );
    let vault = checks
        .iter()
        .find(|check| check["name"] == "vault.mode")
        .expect("vault.mode check");
    assert!(
        vault["detail"].as_str().unwrap_or_default().contains("basic mode"),
        "{vault}"
    );
    assert!(
        machine.useradd_log().is_empty(),
        "the no-sudo path must not create a user: {}",
        machine.useradd_log()
    );
    assert!(
        !machine.tools_log().contains("sudo -u"),
        "the no-sudo path must not switch users: {}",
        machine.tools_log()
    );
}

#[test]
fn setup_home_stops_at_handoff_with_exit_5() {
    let machine = Machine::new();
    machine.write_ssh();
    machine.write_tools("password");
    machine.write_remote_vowp();
    let before = machine.remote_snapshot();

    let output = machine.run_with_sudo(
        "password",
        &["--json", "setup", "home", "--target", "fake.example", "--apply"],
    );
    assert_eq!(
        output.status.code(),
        Some(5),
        "missing --yes must stop with exit 5; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let outcome = json(&output);
    assert_eq!(outcome["status"], "handoff");
    assert_eq!(outcome["handoff"]["kind"], "approval");
    assert_eq!(outcome["changed"].as_array().expect("changed").len(), 0);
    assert_eq!(
        machine.remote_snapshot(),
        before,
        "a missing --yes must not change the home"
    );

    let output = machine.run_with_sudo(
        "password",
        &[
            "--json",
            "setup",
            "home",
            "--target",
            "fake.example",
            "--apply",
            "--yes",
            "--secret",
            "DEEPSEEK_API_KEY",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(5),
        "a missing secret must stop with exit 5; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let outcome = json(&output);
    assert_eq!(outcome["status"], "handoff");
    assert_eq!(outcome["phase"], "vault", "{outcome}");
    assert_eq!(outcome["handoff"]["kind"], "secret");
    assert!(
        outcome["handoff"]["prompt"]
            .as_str()
            .unwrap_or_default()
            .contains("DEEPSEEK_API_KEY"),
        "{outcome}"
    );
    assert!(
        !outcome.to_string().contains("sk-"),
        "a hand-off must never contain a value"
    );
}

#[test]
fn setup_home_registers_projects_and_is_idempotent() {
    let machine = Machine::new();
    machine.write_ssh();
    machine.write_tools("password");
    machine.write_remote_vowp();
    let repo = machine.remote.path().join("projects/app");
    std::fs::create_dir_all(&repo).expect("repo dir");
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .arg(&repo)
            .status()
            .expect("git init")
            .success()
    );
    let mut config = vowp_protocol::VowpConfig::with_defaults_for_tests();
    config.project.name = vowp_protocol::ProjectName::parse("app").expect("project name");
    config.project.path = Some(repo.display().to_string());
    config.project.push = false;
    std::fs::write(
        repo.join("vowp.toml"),
        toml::to_string_pretty(&config).expect("config toml"),
    )
    .expect("vowp.toml");

    let args = [
        "--json",
        "setup",
        "home",
        "--target",
        "fake.example",
        "--repository",
        repo.to_str().expect("repo path"),
        "--apply",
        "--yes",
    ];
    let first = machine.run_with_sudo("password", &args);
    assert!(
        first.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );
    let outcome = json(&first);
    assert_eq!(outcome["status"], "ok", "{outcome}");
    assert!(
        machine.remote.path().join(".vowp/projects/app/project.db").is_file(),
        "the project database must exist"
    );
    assert!(
        outcome["changed"]
            .as_array()
            .expect("changed")
            .iter()
            .any(|path| path.as_str().unwrap_or_default() == repo.to_str().unwrap()),
        "{outcome}"
    );

    let second = machine.run_with_sudo("password", &args);
    assert!(
        second.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );
    let outcome = json(&second);
    assert_eq!(
        outcome["changed"].as_array().expect("changed"),
        &Vec::<Value>::new(),
        "a project re-add must not report a change: {outcome}"
    );
}

#[test]
fn setup_home_stops_when_a_repository_has_no_vowp_toml() {
    let machine = Machine::new();
    machine.write_ssh();
    machine.write_tools("password");
    machine.write_remote_vowp();
    let repo = machine.remote.path().join("projects/plain");
    std::fs::create_dir_all(&repo).expect("repo dir");

    let output = machine.run_with_sudo(
        "password",
        &[
            "--json",
            "setup",
            "home",
            "--target",
            "fake.example",
            "--repository",
            repo.to_str().expect("repo path"),
            "--apply",
            "--yes",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(5),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let outcome = json(&output);
    assert_eq!(outcome["status"], "handoff");
    assert_eq!(outcome["phase"], "projects", "{outcome}");
    assert_eq!(outcome["handoff"]["kind"], "owner-answer");
    assert!(
        outcome["handoff"]["prompt"]
            .as_str()
            .unwrap_or_default()
            .contains(repo.to_str().unwrap()),
        "{outcome}"
    );
}

#[test]
fn setup_control_writes_control_home() {
    let machine = Machine::new();
    machine.write_ssh();
    machine.write_tools("password");
    machine.write_remote_vowp();

    let home = machine.run_with_sudo(
        "password",
        &[
            "--json",
            "setup",
            "home",
            "--target",
            "fake.example",
            "--apply",
            "--yes",
        ],
    );
    assert!(
        home.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&home.stderr)
    );

    let output = machine.run(&["--json", "setup", "control", "--apply"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let outcome = json(&output);
    assert_eq!(outcome["status"], "ok", "{outcome}");
    let control = machine.control.path();
    assert!(control.join("control.json").is_file());
    assert!(control.join("spool").is_dir());
    assert!(control.join("surfaces.json").is_file());
    assert!(control.join("tunnels.json").is_file());
    assert!(control.join("nodes.json").is_file());
    assert!(control.join("setup.json").is_file());
    let config: Value =
        serde_json::from_str(&std::fs::read_to_string(control.join("control.json")).expect("control.json"))
            .expect("control JSON");
    assert_eq!(config["role"], "control");
    assert_eq!(config["home"], "fresh-vps");
    assert_eq!(config["ssh_target"], "fake.example");
    assert_eq!(
        config["remote_home"],
        format!("{}/.vowp", machine.remote.path().display())
    );
    assert!(
        !control.join("supervisor.lock").exists(),
        "a control machine must not install a supervisor"
    );

    let second = machine.run(&["--json", "setup", "control", "--apply"]);
    assert!(
        second.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    let outcome = json(&second);
    assert_eq!(
        outcome["changed"].as_array().expect("changed"),
        &Vec::<Value>::new(),
        "a second control run must not rewrite the same files: {outcome}"
    );
}

#[test]
fn embedded_service_unit_matches_the_release_template() {
    let embedded = include_str!("../src/setup/vowp.service");
    let release = include_str!("../../../scripts/vowp.service");
    assert_eq!(embedded, release, "the embedded unit must match scripts/vowp.service");
}

// --- scripts/install.sh ----------------------------------------------------

/// The release target name for the test host.
fn release_target() -> String {
    let os = Command::new("uname").arg("-s").output().expect("uname -s").stdout;
    let arch = Command::new("uname").arg("-m").output().expect("uname -m").stdout;
    let os = String::from_utf8_lossy(&os).trim().to_owned();
    let arch = String::from_utf8_lossy(&arch).trim().to_owned();
    match (os.as_str(), arch.as_str()) {
        ("Linux", "x86_64" | "amd64") => "x86_64-unknown-linux-gnu".to_owned(),
        ("Linux", "aarch64" | "arm64") => "aarch64-unknown-linux-gnu".to_owned(),
        ("Darwin", "x86_64" | "amd64") => "x86_64-apple-darwin".to_owned(),
        ("Darwin", "arm64" | "aarch64") => "aarch64-apple-darwin".to_owned(),
        other => panic!("unsupported test host {other:?}"),
    }
}

/// Returns the SHA-256 of `bytes` using the host tool the installer also uses.
fn sha256(bytes: &[u8]) -> String {
    let dir = tempfile::tempdir().expect("temp sha");
    let path = dir.path().join("bytes");
    std::fs::write(&path, bytes).expect("write sha input");
    let output = Command::new("sha256sum")
        .arg(&path)
        .output()
        .or_else(|_| {
            Command::new("shasum")
                .args(["-a", "256", path.to_str().unwrap()])
                .output()
        })
        .expect("sha256sum or shasum");
    assert!(output.status.success(), "sha256 tool failed");
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .expect("sha256")
        .to_owned()
}

/// Serves bytes over loopback for the installer test and stops after `requests` hits.
fn serve(files: BTreeMap<String, Vec<u8>>, requests: usize) -> String {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
    let port = listener.local_addr().expect("addr").port();
    let files = Arc::new(files);
    thread::spawn(move || {
        for _ in 0..requests {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            handle_request(&mut stream, &files);
        }
    });
    format!("http://localhost:{port}")
}

/// Answers one minimal HTTP GET.
fn handle_request(stream: &mut TcpStream, files: &BTreeMap<String, Vec<u8>>) {
    use std::io::{Read, Write};
    let mut request = [0_u8; 4096];
    let Ok(read) = stream.read(&mut request) else {
        return;
    };
    let text = String::from_utf8_lossy(&request[..read]);
    let path = text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
    let path = path.split('?').next().unwrap_or(path);
    match files.get(path) {
        Some(body) => {
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(body);
        }
        None => {
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        }
    }
}

#[test]
fn install_sh_download_still_works() {
    let target = release_target();
    let version = "0.2.0";
    let archive_dir = format!("vowp-{version}-{target}");
    let work = tempfile::tempdir().expect("temp archive");
    let payload = work.path().join(&archive_dir);
    std::fs::create_dir_all(&payload).expect("payload dir");
    write_exec(
        &payload.join("vowp"),
        &format!("#!/bin/sh\nprintf 'vowp {version}\\n'\n"),
    );
    let archive_name = format!("{archive_dir}.tar.gz");
    let archive_path = work.path().join(&archive_name);
    let status = Command::new("tar")
        .args([
            "-czf",
            archive_path.to_str().unwrap(),
            "-C",
            work.path().to_str().unwrap(),
            &archive_dir,
        ])
        .status()
        .expect("run tar");
    assert!(status.success(), "tar failed");
    let archive = std::fs::read(&archive_path).expect("read archive");
    let checksum = sha256(&archive);
    let base = format!("/v{version}");
    let mut files = BTreeMap::new();
    files.insert(format!("{base}/{archive_name}"), archive);
    files.insert(
        format!("{base}/{archive_name}.sha256"),
        format!("{checksum}  {archive_name}\n").into_bytes(),
    );
    let url = serve(files, 2);

    let install_home = tempfile::tempdir().expect("temp install home");
    let prefix = install_home.path().join("bin");
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/install.sh");
    let output = Command::new("sh")
        .arg(&script)
        .args([
            "--download",
            "--version",
            version,
            "--base-url",
            &url,
            "--prefix",
            prefix.to_str().unwrap(),
            "--no-service",
            "--no-cron",
            "--no-watchdog",
        ])
        .env("HOME", install_home.path())
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .output()
        .expect("run install.sh");
    assert!(
        output.status.success(),
        "install.sh failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let installed = prefix.join("vowp");
    assert!(installed.is_file(), "installed binary missing: {}", installed.display());
    let version_output = Command::new(&installed)
        .arg("--version")
        .output()
        .expect("run installed binary");
    assert_eq!(
        String::from_utf8_lossy(&version_output.stdout).trim(),
        format!("vowp {version}")
    );
    assert!(
        !prefix.join("vowp-watchdog.sh").exists(),
        "--no-watchdog must not install the watchdog"
    );
}
