//! Home, agent and control directory compatibility: resolution, `home migrate`, `doctor` and the
//! dual environment export. Hermetic: every test uses a temporary `$HOME` and a private tmux
//! directory; nothing reads the real home, starts tmux or touches the network.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use vowp_store::{NodePaths, Store};

const BIN: &str = env!("CARGO_BIN_EXE_stackspirit");

/// Fake tools doctor invokes; every one answers `--version` and nothing touches a real harness.
const FAKE_TOOLS: &[(&str, &str)] = &[
    ("git", "#!/bin/sh\necho 'git version 9.9.9'\nexit 0\n"),
    ("tmux", "#!/bin/sh\necho 'tmux 9.9'\nexit 0\n"),
    (
        "df",
        "#!/bin/sh\necho 'Filesystem 1024-blocks Used Available Capacity Mounted on'\necho '/dev/fake 9999999 1 8000000 1% /'\nexit 0\n",
    ),
    ("claude", "#!/bin/sh\necho '2.1.0 (Claude Code)'\nexit 0\n"),
    ("opencode", "#!/bin/sh\necho 'opencode 2.0.0'\nexit 0\n"),
];

/// Returns a process-wide directory of fake tools used as the only `PATH` entry.
fn fake_path() -> &'static Path {
    use std::os::unix::fs::PermissionsExt;
    static DIR: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let dir = tempfile::tempdir().expect("fake bin");
        for (name, body) in FAKE_TOOLS {
            let path = dir.path().join(name);
            std::fs::write(&path, body).expect("fake script");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        }
        // Liveness probes shell out to `kill -0`; the wrapper keeps `PATH` free of real harnesses.
        let kill = dir.path().join("kill");
        std::fs::write(&kill, "#!/bin/sh\nexec /usr/bin/kill \"$@\"\n").expect("kill wrapper");
        std::fs::set_permissions(&kill, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        dir
    })
    .path()
}

/// Runs `stackspirit` with `HOME=home`, no home variables and a private tmux directory.
fn run(home: &Path, env: &[(&str, &str)], args: &[&str]) -> Output {
    let tmux = home.join(".tmux-private");
    std::fs::create_dir_all(&tmux).expect("private tmux dir");
    let mut command = Command::new(BIN);
    command
        .args(args)
        .current_dir(home)
        .env_remove("VOWP_HOME")
        .env_remove("STACKSPIRIT_HOME")
        .env_remove("VOWP_AGENT_HOME")
        .env_remove("STACKSPIRIT_AGENT_HOME")
        .env_remove("VOWP_CONTROL_HOME")
        .env_remove("STACKSPIRIT_CONTROL_HOME")
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .env("HOME", home)
        .env("PATH", fake_path())
        .env("TMUX_TMPDIR", &tmux);
    for (key, value) in env {
        command.env(key, value);
    }
    command.output().expect("run stackspirit")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Creates a real home (a database) at `path`.
fn make_home(path: &Path) {
    std::fs::create_dir_all(path).expect("home dir");
    Store::open(&NodePaths::new(path)).expect("store opens");
    assert!(path.join("vowp.db").is_file());
}

fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

/// Returns the `doctor --json` check named `name`.
fn doctor_check(home: &Path, env: &[(&str, &str)], name: &str) -> Option<serde_json::Value> {
    let output = run(home, env, &["--json", "doctor"]);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "doctor json: {error}\n{}\n{}",
            text(&output.stdout),
            text(&output.stderr)
        )
    });
    report["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .find(|check| check["name"] == name)
        .cloned()
}

/// Recursive snapshot (kind, size, mtime, link target) of everything under `dir` except the
/// private tmux directory, so a dry run that creates or touches any nested file is caught.
fn snapshot(dir: &Path) -> std::collections::BTreeMap<String, String> {
    fn walk(root: &Path, dir: &Path, out: &mut std::collections::BTreeMap<String, String>) {
        for entry in std::fs::read_dir(dir).expect("read dir") {
            let entry = entry.expect("entry");
            let path = entry.path();
            if path.file_name().is_some_and(|name| name == ".tmux-private") {
                continue;
            }
            let meta = std::fs::symlink_metadata(&path).expect("metadata");
            let mtime = meta.modified().expect("mtime");
            let link = std::fs::read_link(&path)
                .map(|t| t.display().to_string())
                .unwrap_or_default();
            let key = path.strip_prefix(root).expect("prefix").display().to_string();
            out.insert(key, format!("{:?}|{}|{mtime:?}|{link}", meta.file_type(), meta.len()));
            if meta.is_dir() {
                walk(root, &path, out);
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

#[test]
fn legacy_only_home_is_selected_and_only_warns() {
    let dir = tempfile::tempdir().expect("tempdir");
    make_home(&dir.path().join(".vowp"));
    let check = doctor_check(dir.path(), &[], "home-path").expect("home-path check");
    assert_eq!(check["status"], "warn", "{check}");
    let detail = check["detail"].as_str().expect("detail");
    assert!(
        detail.contains(&dir.path().join(".vowp").display().to_string()),
        "{detail}"
    );
    assert!(
        detail.contains("home migrate --link --dry-run"),
        "migration hint: {detail}"
    );
    assert!(
        !dir.path().join(".stackspirit").exists(),
        "doctor must not create anything"
    );
    // The legacy name alone must not make doctor fail.
    let home_check = doctor_check(dir.path(), &[], "home").expect("home check");
    assert_eq!(home_check["status"], "ok");
}

#[test]
fn two_distinct_homes_conflict_and_name_both_paths_and_the_hint() {
    let dir = tempfile::tempdir().expect("tempdir");
    make_home(&dir.path().join(".vowp"));
    make_home(&dir.path().join(".stackspirit"));
    let output = run(dir.path(), &[], &["status"]);
    assert!(!output.status.success());
    let err = text(&output.stderr);
    assert!(
        err.contains(".vowp") && err.contains(".stackspirit") && err.contains("--home"),
        "{err}"
    );

    let check = doctor_check(dir.path(), &[], "home").expect("doctor reports the conflict");
    assert_eq!(check["status"], "fail");
    assert!(check["detail"].as_str().expect("detail").contains("--home"));

    // --home resolves the ambiguity.
    let chosen = dir.path().join(".stackspirit");
    let output = run(
        dir.path(),
        &[],
        &["--home", chosen.to_str().expect("utf8"), "--json", "doctor"],
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    assert!(
        report["checks"]
            .as_array()
            .expect("checks")
            .iter()
            .any(|c| c["name"] == "db" && c["status"] == "ok")
    );
}

#[test]
fn new_name_symlink_to_old_home_is_one_home() {
    let dir = tempfile::tempdir().expect("tempdir");
    let old = dir.path().join(".vowp");
    make_home(&old);
    std::os::unix::fs::symlink(&old, dir.path().join(".stackspirit")).expect("link");
    let check = doctor_check(dir.path(), &[], "home-path").expect("home-path check");
    assert_eq!(check["status"], "ok", "no conflict, no legacy warning: {check}");
    assert!(
        check["detail"]
            .as_str()
            .expect("detail")
            .contains(&dir.path().join(".stackspirit").display().to_string())
    );
}

#[test]
fn explicit_environment_precedence_is_probed_in_child_processes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (a, b) = (dir.path().join("a"), dir.path().join("b"));
    make_home(&a);
    make_home(&b);
    let (a_text, b_text) = (a.to_str().expect("utf8"), b.to_str().expect("utf8"));
    let both = doctor_check(
        dir.path(),
        &[("STACKSPIRIT_HOME", a_text), ("VOWP_HOME", b_text)],
        "home-path",
    )
    .expect("home-path");
    assert!(
        both["detail"].as_str().expect("detail").contains(a_text),
        "new name wins: {both}"
    );
    let warning = doctor_check(
        dir.path(),
        &[("STACKSPIRIT_HOME", a_text), ("VOWP_HOME", b_text)],
        "home-env",
    )
    .expect("one warning for differing variables");
    assert_eq!(warning["status"], "warn");
    let legacy = doctor_check(dir.path(), &[("VOWP_HOME", b_text)], "home-path").expect("home-path");
    assert!(legacy["detail"].as_str().expect("detail").contains(b_text));
}

#[test]
fn missing_explicit_home_is_an_error_and_never_falls_back() {
    let dir = tempfile::tempdir().expect("tempdir");
    make_home(&dir.path().join(".vowp"));
    let missing = dir.path().join("does-not-exist");
    let output = run(
        dir.path(),
        &[("STACKSPIRIT_HOME", missing.to_str().expect("utf8"))],
        &["status"],
    );
    assert!(!output.status.success());
    let err = text(&output.stderr);
    assert!(
        err.contains("STACKSPIRIT_HOME") && err.contains("does-not-exist"),
        "{err}"
    );
    assert!(!missing.exists(), "reading must not create the path");
}

#[test]
fn link_dry_run_writes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    make_home(&dir.path().join(".vowp"));
    std::fs::create_dir_all(dir.path().join(".vowp-agent")).expect("agent");
    std::fs::write(dir.path().join(".vowp-agent/agent.json"), "{}").expect("agent.json");
    std::fs::create_dir_all(dir.path().join(".vowp/projects/proj")).expect("project dir");
    Store::open(&NodePaths::new(dir.path().join(".vowp"))).expect("reopen");
    let before = snapshot(dir.path());
    let output = run(dir.path(), &[], &["home", "migrate", "--link", "--dry-run"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let out = text(&output.stdout);
    assert!(
        out.contains(".stackspirit") && out.contains(".stackspirit-agent"),
        "{out}"
    );
    assert!(out.contains("nothing was written"), "{out}");
    assert_eq!(
        snapshot(dir.path()),
        before,
        "dry run must not write anywhere under the home"
    );
}

#[test]
fn link_without_yes_or_dry_run_refuses() {
    let dir = tempfile::tempdir().expect("tempdir");
    make_home(&dir.path().join(".vowp"));
    let output = run(dir.path(), &[], &["home", "migrate", "--link"]);
    assert!(!output.status.success());
    assert!(!dir.path().join(".stackspirit").exists());
}

#[test]
fn link_yes_creates_symlinks_without_moving_data_and_is_idempotent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let old = dir.path().join(".vowp");
    make_home(&old);
    std::fs::create_dir_all(dir.path().join(".vowp-control")).expect("control");
    // The control machine is also reported by role, so it needs a parseable control.json.
    let control = vowp_cli::node_ctx::ControlConfig {
        role: "control".to_owned(),
        home: "main".to_owned(),
        ssh_target: "user@example.invalid".to_owned(),
        remote_home: "/home/user/.vowp".to_owned(),
        created_at: vowp_protocol::Timestamp::now(),
    };
    std::fs::write(
        dir.path().join(".vowp-control/control.json"),
        serde_json::to_string(&control).expect("control json"),
    )
    .expect("control.json");
    let db_before = std::fs::metadata(old.join("vowp.db")).expect("db").len();
    let output = run(dir.path(), &[], &["home", "migrate", "--link", "--yes"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let new = dir.path().join(".stackspirit");
    assert!(is_symlink(&new));
    assert_eq!(
        std::fs::canonicalize(&new).expect("canonical"),
        std::fs::canonicalize(&old).expect("canonical")
    );
    assert!(is_symlink(&dir.path().join(".stackspirit-control")));
    assert!(
        !dir.path().join(".stackspirit-agent").exists(),
        "no agent on this machine"
    );
    assert!(!is_symlink(&old), "the legacy directory is still the real one");
    assert_eq!(std::fs::metadata(old.join("vowp.db")).expect("db").len(), db_before);

    let again = run(dir.path(), &[], &["home", "migrate", "--link", "--yes"]);
    assert!(again.status.success(), "{}", text(&again.stderr));
    assert!(text(&again.stdout).contains("already linked"));
}

#[test]
fn link_refuses_when_the_new_path_is_a_distinct_real_directory() {
    let dir = tempfile::tempdir().expect("tempdir");
    make_home(&dir.path().join(".vowp"));
    make_home(&dir.path().join(".stackspirit"));
    let output = run(dir.path(), &[], &["home", "migrate", "--link", "--yes"]);
    assert!(!output.status.success());
    assert!(!is_symlink(&dir.path().join(".stackspirit")));
}

#[test]
fn link_on_a_fresh_new_home_links_the_old_name_to_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    make_home(&dir.path().join(".stackspirit"));
    let output = run(dir.path(), &[], &["home", "migrate", "--link", "--yes"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(is_symlink(&dir.path().join(".vowp")));
}

/// Starts a long-lived child and returns it so its pid is a live process.
fn live_process() -> std::process::Child {
    Command::new("sleep")
        .arg("60")
        .stdin(Stdio::null())
        .spawn()
        .expect("spawn sleep")
}

#[test]
fn move_refuses_while_the_supervisor_lock_is_live() {
    let dir = tempfile::tempdir().expect("tempdir");
    let old = dir.path().join(".vowp");
    make_home(&old);
    let mut sleeper = live_process();
    std::fs::write(old.join("supervisor.lock"), format!("{}\n", sleeper.id())).expect("lock");
    let target = dir.path().join(".stackspirit");
    let output = run(
        dir.path(),
        &[],
        &[
            "home",
            "migrate",
            "--move",
            "--to",
            target.to_str().expect("utf8"),
            "--yes",
        ],
    );
    let _ = sleeper.kill();
    let _ = sleeper.wait();
    assert!(!output.status.success());
    let err = text(&output.stderr);
    assert!(err.contains("supervisor.lock"), "{err}");
    assert!(!target.exists(), "a refused move leaves no destination");
    assert!(old.join("vowp.db").is_file());
    assert!(!is_symlink(&old));
}

#[test]
fn move_refuses_while_an_orchestrator_host_is_running() {
    let dir = tempfile::tempdir().expect("tempdir");
    let old = dir.path().join(".vowp");
    make_home(&old);
    let mut sleeper = live_process();
    let host = old.join("orchestrators").join("main");
    std::fs::create_dir_all(&host).expect("host dir");
    std::fs::write(
        host.join(vowp_core::surface::orchestrator::KEEPER_PID_FILE),
        format!("{}\n", sleeper.id()),
    )
    .expect("pid");
    let target = dir.path().join(".stackspirit");
    let output = run(
        dir.path(),
        &[],
        &[
            "home",
            "migrate",
            "--move",
            "--to",
            target.to_str().expect("utf8"),
            "--yes",
        ],
    );
    let _ = sleeper.kill();
    let _ = sleeper.wait();
    assert!(!output.status.success());
    assert!(text(&output.stderr).contains("orchestrator host"));
    assert!(!target.exists());
}

#[test]
fn move_dry_run_writes_nothing_and_prints_socket_names() {
    let dir = tempfile::tempdir().expect("tempdir");
    make_home(&dir.path().join(".vowp"));
    let before = snapshot(dir.path());
    let target = dir.path().join(".stackspirit");
    let output = run(
        dir.path(),
        &[],
        &[
            "home",
            "migrate",
            "--move",
            "--to",
            target.to_str().expect("utf8"),
            "--dry-run",
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    let out = text(&output.stdout);
    assert!(
        out.contains("VOWP_TMUX_SOCKET=vowp-") && out.contains("STACKSPIRIT_TMUX_SOCKET=vowp-"),
        "{out}"
    );
    assert_eq!(
        snapshot(dir.path()),
        before,
        "dry run must not write anywhere under the home"
    );
}

#[test]
fn move_honours_the_global_home_flag_over_dollar_home() {
    let dir = tempfile::tempdir().expect("tempdir");
    let selected = dir.path().join("selected");
    make_home(&selected);
    let elsewhere = dir.path().join("elsewhere-home");
    std::fs::create_dir_all(&elsewhere).expect("elsewhere");
    let target = dir.path().join("moved");
    let output = run(
        &elsewhere,
        &[],
        &[
            "--home",
            selected.to_str().expect("utf8"),
            "home",
            "migrate",
            "--move",
            "--to",
            target.to_str().expect("utf8"),
            "--dry-run",
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    let canonical = std::fs::canonicalize(&selected).expect("canonical");
    assert!(
        text(&output.stdout).contains(&canonical.display().to_string()),
        "{}",
        text(&output.stdout)
    );
}

#[test]
fn home_move_refuses_while_the_agent_socket_is_live() {
    let dir = tempfile::tempdir().expect("tempdir");
    make_home(&dir.path().join(".vowp"));
    std::fs::create_dir_all(dir.path().join(".vowp-agent")).expect("agent dir");
    let _listener = std::os::unix::net::UnixListener::bind(
        dir.path()
            .join(".vowp-agent")
            .join(vowp_agent::socket::AGENT_SOCKET_FILE),
    )
    .expect("bind agent socket");
    let target = dir.path().join(".stackspirit");
    let output = run(
        dir.path(),
        &[],
        &[
            "home",
            "migrate",
            "--move",
            "--to",
            target.to_str().expect("utf8"),
            "--dry-run",
        ],
    );
    assert_eq!(output.status.code(), Some(4), "{}", text(&output.stderr));
    assert!(
        text(&output.stderr).contains("agent socket"),
        "{}",
        text(&output.stderr)
    );
}

#[test]
fn partial_legacy_agent_dir_is_present_for_link_move_and_doctor() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join(".vowp-agent")).expect("agent dir");
    std::fs::write(dir.path().join(".vowp-agent/node.key"), "k").expect("node.key");
    let link = run(dir.path(), &[], &["home", "migrate", "--link", "--dry-run"]);
    assert!(link.status.success(), "{}", text(&link.stderr));
    assert!(text(&link.stdout).contains("would link"), "{}", text(&link.stdout));
    let target = dir.path().join("agent-moved");
    let moved = run(
        dir.path(),
        &[],
        &[
            "home",
            "migrate",
            "--move",
            "--kind",
            "agent",
            "--to",
            target.to_str().expect("utf8"),
            "--dry-run",
        ],
    );
    assert!(moved.status.success(), "{}", text(&moved.stderr));
    let check = doctor_check(dir.path(), &[], "agent-state").expect("agent-state check");
    assert_eq!(check["status"], "warn", "{check}");
}

#[test]
fn move_copies_verifies_and_leaves_the_legacy_path_as_a_symlink() {
    let dir = tempfile::tempdir().expect("tempdir");
    let old = dir.path().join(".vowp");
    make_home(&old);
    std::fs::create_dir_all(old.join("sessions/s1")).expect("session dir");
    std::fs::write(old.join("sessions/s1/log.jsonl"), "{}\n").expect("log");
    let db_bytes = std::fs::read(old.join("vowp.db")).expect("db");
    let new = dir.path().join(".stackspirit");
    let output = run(
        dir.path(),
        &[],
        &[
            "home",
            "migrate",
            "--move",
            "--to",
            new.to_str().expect("utf8"),
            "--yes",
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    let out = text(&output.stdout);
    assert!(out.contains("VOWP_TMUX_SOCKET=vowp-"), "{out}");
    assert!(!is_symlink(&new) && new.join("vowp.db").is_file());
    assert_eq!(
        std::fs::read(new.join("vowp.db")).expect("db"),
        db_bytes,
        "database is byte-identical"
    );
    assert_eq!(
        std::fs::read_to_string(new.join("sessions/s1/log.jsonl")).expect("log"),
        "{}\n"
    );
    assert!(is_symlink(&old), "legacy path is a symlink to the new home");
    assert_eq!(
        std::fs::canonicalize(&old).expect("canonical"),
        std::fs::canonicalize(&new).expect("canonical")
    );
    let backup = PathBuf::from(format!("{}.premove", old.display()));
    assert!(
        backup.join("vowp.db").is_file(),
        "original kept for the owner to delete"
    );
    // Both names still resolve to one home.
    let check = doctor_check(dir.path(), &[], "home-path").expect("home-path");
    assert_eq!(check["status"], "ok", "{check}");
}

#[test]
fn doctor_reports_agent_and_control_state_with_legacy_warning() {
    let dir = tempfile::tempdir().expect("tempdir");
    make_home(&dir.path().join(".vowp"));
    std::fs::create_dir_all(dir.path().join(".vowp-agent")).expect("agent");
    std::fs::write(dir.path().join(".vowp-agent/agent.json"), "{}").expect("agent.json");
    let check = doctor_check(dir.path(), &[], "agent-state").expect("agent-state");
    assert_eq!(check["status"], "warn");
    assert!(check["detail"].as_str().expect("detail").contains("agent.json"));
    assert!(
        doctor_check(dir.path(), &[], "control-state").is_none(),
        "no control state here"
    );
}

#[test]
fn exported_child_environments_carry_both_names_with_one_value() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("state");
    for (export, new_name, old_name) in [
        (
            vowp_cli::ctx::export_home_env as fn(&mut Command, &Path),
            "STACKSPIRIT_HOME",
            "VOWP_HOME",
        ),
        (
            vowp_cli::ctx::export_agent_env,
            "STACKSPIRIT_AGENT_HOME",
            "VOWP_AGENT_HOME",
        ),
        (
            vowp_cli::ctx::export_control_env,
            "STACKSPIRIT_CONTROL_HOME",
            "VOWP_CONTROL_HOME",
        ),
    ] {
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(format!("printf '%s|%s' \"${new_name}\" \"${old_name}\""))
            .env_remove(new_name)
            .env_remove(old_name);
        export(&mut command, &target);
        let output = command.output().expect("probe child");
        let expected = format!("{0}|{0}", target.display());
        assert_eq!(text(&output.stdout), expected, "{new_name}/{old_name}");
    }
}

#[test]
fn control_status_reports_a_two_control_directory_conflict() {
    let dir = tempfile::tempdir().expect("tempdir");
    make_home(&dir.path().join(".stackspirit"));
    for name in [".vowp-control", ".stackspirit-control"] {
        let control = dir.path().join(name);
        std::fs::create_dir_all(&control).expect("control dir");
        std::fs::write(control.join("control.json"), "{}").expect("control.json");
    }
    let output = run(dir.path(), &[], &["control", "status"]);
    assert!(!output.status.success());
    let err = text(&output.stderr);
    assert!(
        err.contains(".vowp-control")
            && err.contains(".stackspirit-control")
            && err.contains("STACKSPIRIT_CONTROL_HOME"),
        "{err}"
    );
    let doctor = run(dir.path(), &[], &["--json", "doctor"]);
    assert!(!doctor.status.success(), "doctor must not pass with a control conflict");
    let report: serde_json::Value = serde_json::from_slice(&doctor.stdout).expect("doctor json");
    assert_eq!(report["error"]["kind"], "conflict", "{report}");
    assert!(
        report["error"]["message"]
            .as_str()
            .expect("message")
            .contains(".stackspirit-control"),
        "{report}"
    );
}

#[test]
fn doctor_and_link_report_a_broken_new_name_symlink() {
    let dir = tempfile::tempdir().expect("tempdir");
    make_home(&dir.path().join(".vowp"));
    std::os::unix::fs::symlink("/nonexistent-stackspirit-target", dir.path().join(".stackspirit")).expect("symlink");
    let check = doctor_check(dir.path(), &[], "state-link").expect("state-link check");
    assert_eq!(check["status"], "warn", "{check}");
    let detail = check["detail"].as_str().expect("detail");
    assert!(
        detail.contains(".stackspirit") && detail.contains("broken symlink"),
        "{detail}"
    );
    let output = run(dir.path(), &[], &["home", "migrate", "--link", "--yes"]);
    assert!(!output.status.success());
    assert!(
        text(&output.stderr).contains("broken symlink"),
        "{}",
        text(&output.stderr)
    );
}

#[test]
fn link_honours_the_home_environment_variables() {
    let root = tempfile::tempdir().expect("tempdir");
    let home = root.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    let selected = root.path().join("h2").join(".vowp");
    make_home(&selected);
    for var in ["STACKSPIRIT_HOME", "VOWP_HOME"] {
        let output = run(
            &home,
            &[(var, selected.to_str().expect("utf8"))],
            &["--json", "home", "migrate", "--link", "--dry-run"],
        );
        assert!(output.status.success(), "{var}: {}", text(&output.stderr));
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
        let step = report["links"]
            .as_array()
            .expect("links")
            .iter()
            .find(|step| step["kind"] == "home")
            .expect("home step");
        assert_eq!(step["action"], "create", "{var}: {step}");
        assert!(
            step["link"].as_str().expect("link").ends_with("h2/.stackspirit"),
            "{var}: {step}"
        );
    }
    assert!(
        !root.path().join("h2").join(".stackspirit").exists(),
        "dry run writes nothing"
    );
}

#[test]
fn daemon_reports_a_home_conflict_with_both_paths_and_the_hint() {
    let dir = tempfile::tempdir().expect("tempdir");
    make_home(&dir.path().join(".vowp"));
    make_home(&dir.path().join(".stackspirit"));
    let daemon = Path::new(BIN).with_file_name("vowp-daemon");
    if !daemon.is_file() {
        // `cargo test -p vowp-cli` alone does not build the daemon binary; the workspace run does.
        eprintln!("skipping: {} is not built", daemon.display());
        return;
    }
    let output = Command::new(&daemon)
        .current_dir(dir.path())
        .env_remove("VOWP_HOME")
        .env_remove("STACKSPIRIT_HOME")
        .env("HOME", dir.path())
        .env("PATH", fake_path())
        .output()
        .expect("run vowp-daemon");
    assert!(!output.status.success());
    let err = text(&output.stderr);
    assert!(
        err.contains(".vowp") && err.contains(".stackspirit") && err.contains("--home"),
        "{err}"
    );
}
