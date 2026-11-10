//! `vowp session` integration tests.
//!
//! Every test uses a temporary node home and project, a fake opencode script and never
//! touches the real home, network or a real harness binary.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use vowp_protocol::{
    Actor, AssistantMessage, Cost, EventBody, EventSource, EventType, HarnessKind, RemoteEvent, RemoteLine, StopReason,
    Timestamp, TurnGrant, UserMessage,
};
use vowp_store::session_log::{NewEvent, SessionLog, index_row};

/// Writes a fake opencode binary that probes successfully and returns one valid capsule.
fn fake_opencode(env: &common::TestEnv) -> std::path::PathBuf {
    let dir = env.home_path().join("fake-bin");
    std::fs::create_dir_all(&dir).expect("fake bin dir");
    let draft = serde_json::json!({
        "intent": "continue the session",
        "constraints": [],
        "decisions": [],
        "state": {"done": ["started"], "in_progress": [], "blocked": [], "next_steps": ["finish"]},
        "files": [],
        "gotchas": [],
        "open_questions": []
    });
    let events = [
        serde_json::json!({
            "type": "text",
            "timestamp": 1_794_168_421_621_i64,
            "sessionID": "ses_fake",
            "part": {"text": draft.to_string()}
        }),
        serde_json::json!({
            "type": "step_finish",
            "timestamp": 1_794_168_421_622_i64,
            "sessionID": "ses_fake",
            "part": {"cost": 0.0, "tokens": {"input": 1, "output": 1}, "reason": "stop"}
        }),
    ];
    let fixture = dir.join("capsule.jsonl");
    let text = events.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n");
    std::fs::write(&fixture, format!("{text}\n")).expect("fixture");
    let script = dir.join("opencode");
    let body = format!(
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then printf '%s\\n' '2.0.28'; exit 0; fi\ncat '{}'\n",
        fixture.display()
    );
    std::fs::write(&script, body).expect("fake script");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    script
}

/// Rewrites the project config to use the fake opencode binary.
fn configure(env: &common::TestEnv) -> std::path::PathBuf {
    let script = fake_opencode(env);
    let mut config = env.config.clone();
    config.harnesses.insert(
        "opencode".to_owned(),
        vowp_protocol::HarnessConfig {
            binary: Some(script.display().to_string()),
            ..vowp_protocol::HarnessConfig::default()
        },
    );
    std::fs::write(
        env.project_path().join("vowp.toml"),
        toml::to_string_pretty(&config).expect("config"),
    )
    .expect("write config");
    script
}

/// Creates a session and returns its id.
fn create(env: &common::TestEnv) -> String {
    let (code, stdout, stderr) = env.run(&[
        "session",
        "new",
        "--title",
        "session test",
        "--executor",
        "opencode:deepseek/deepseek-flash#max",
    ]);
    assert_eq!(code, 0, "{stderr}");
    stdout.trim().to_owned()
}

#[test]
fn session_export_import_roundtrip_via_cli() {
    let env = common::TestEnv::new();
    configure(&env);
    let id = create(&env);

    let bundle_path = env.project_path().join("session.vowp-bundle.json");
    let (code, _, stderr) = env.run(&[
        "session",
        "export",
        &id,
        "--out",
        bundle_path.to_str().expect("path"),
        "--tail",
        "5",
    ]);
    assert_eq!(code, 0, "{stderr}");
    let text = std::fs::read_to_string(&bundle_path).expect("bundle written");
    let bundle: serde_json::Value = serde_json::from_str(&text).expect("bundle JSON");
    assert_eq!(bundle["format"], "vowp-bundle-v1");
    assert_eq!(bundle["session"]["id"], id);
    assert!(bundle["checksum"].as_str().unwrap_or_default().starts_with("sha256:"));

    let (code, stdout, stderr) = env.run(&["session", "import", bundle_path.to_str().expect("path")]);
    assert_eq!(code, 0, "{stderr}");
    let child = stdout.trim();
    assert_ne!(child, id);
    let store = env.store();
    let origin = store
        .require_session(&vowp_protocol::SessionId::parse(&id).expect("id"))
        .expect("origin");
    let child_row = store
        .require_session(&vowp_protocol::SessionId::parse(child).expect("id"))
        .expect("child");
    assert_eq!(child_row.parent_session_id.as_ref(), Some(&origin.id));
    assert_eq!(child_row.title, "session test (imported)");
}

#[test]
fn session_tail_pretty_golden() {
    let env = common::TestEnv::new();
    let id = create(&env);
    let session_id = vowp_protocol::SessionId::parse(&id).expect("id");
    let paths = vowp_store::NodePaths::new(env.home_path());
    let clock = std::sync::Arc::new(vowp_protocol::SystemClock);
    let log = SessionLog::open(&paths, &session_id, clock).expect("open log");
    let store = env.store();
    let source = EventSource {
        harness: HarnessKind::Opencode,
        model: Some("deepseek/deepseek-flash".to_owned()),
        node: Some(vowp_protocol::NodeName::parse("local").expect("node")),
        native_session_id: None,
    };
    let fixed = Timestamp::parse("2026-11-08T19:30:12.000Z").expect("timestamp");
    for body in [
        EventBody::UserMessage(UserMessage {
            text: "cargo test".to_owned(),
            attachments: Vec::new(),
            note: None,
        }),
        EventBody::AssistantMessage(AssistantMessage {
            text: "all green".to_owned(),
        }),
    ] {
        let appended = log
            .append(NewEvent {
                actor: Actor::User,
                source: source.clone(),
                body,
                raw: None,
                ts: Some(fixed),
            })
            .expect("append");
        store.index_event(&index_row(&appended)).expect("index");
    }
    log.sync().expect("sync");
    drop(log);

    let (code, stdout, stderr) = env.run(&["session", "tail", &id, "--pretty", "--from", "1"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        stdout,
        "19:30:12 #2 user.message  user: cargo test\n19:30:12 #3 assistant.message  assistant: all green\n"
    );
}

#[test]
fn session_switch_with_fake_adapters() {
    let env = common::TestEnv::new();
    configure(&env);
    let id = create(&env);

    let (code, stdout, stderr) = env.run(&[
        "session",
        "switch",
        &id,
        "--to",
        "opencode:deepseek/deepseek-chat",
        "--reason",
        "user",
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("switched session"), "{stdout}");

    let store = env.store();
    let session = store
        .require_session(&vowp_protocol::SessionId::parse(&id).expect("id"))
        .expect("session");
    assert_eq!(
        session.executor.as_ref().map(ToString::to_string).as_deref(),
        Some("opencode:deepseek/deepseek-chat")
    );
    let switches = store
        .list_switches(&vowp_protocol::SessionId::parse(&id).expect("id"))
        .expect("switches");
    assert_eq!(switches.len(), 1);
    assert_eq!(switches[0].state, vowp_protocol::SwitchState::Completed);
    let session_id = vowp_protocol::SessionId::parse(&id).expect("id");
    let events = SessionLog::open_read(&vowp_store::NodePaths::new(env.home_path()), &session_id)
        .expect("read log")
        .read_all()
        .expect("events");
    assert!(
        events.iter().any(|event| matches!(
            event.body(),
            Ok(EventBody::Switch(payload)) if payload.capsule_id.is_some()
        )),
        "the switch event carries no capsule"
    );
    assert!(
        van_capsule_exists(env.home_path(), &id),
        "the switch capsule file was not written"
    );
}

/// Returns whether the session has at least one capsule file.
fn van_capsule_exists(home: &Path, id: &str) -> bool {
    let dir = home.join("sessions").join(id).join("capsules");
    std::fs::read_dir(dir)
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false)
}

/// One remote node home plus the fake ssh/vowp PATH used to reach it.
struct RemoteNode {
    home: tempfile::TempDir,
    bin: tempfile::TempDir,
}

/// Creates a remote node home, registers the shared project and installs a fake `ssh`
/// that executes the quoted remote command locally with `VOWP_HOME` pointing at it.
fn remote_node(env: &common::TestEnv) -> RemoteNode {
    let home = tempfile::tempdir().expect("remote home");
    let paths = vowp_store::NodePaths::new(home.path());
    let store = vowp_store::Store::open(&paths).expect("remote store");
    let now = store.clock().now();
    store
        .upsert_node(&vowp_protocol::Node {
            name: env.config.node.name.clone(),
            ssh_target: None,
            is_local: true,
            capabilities: serde_json::json!({}),
            last_seen_at: Some(now),
            created_at: now,
        })
        .expect("remote node");
    store
        .upsert_project(&vowp_protocol::Project {
            name: env.config.project.name.clone(),
            path: env.project_path().display().to_string(),
            remote_url: None,
            default_branch: "main".to_owned(),
            config: serde_json::to_value(&env.config).expect("config json"),
            created_at: now,
            updated_at: now,
        })
        .expect("remote project");
    drop(store);

    let bin = tempfile::tempdir().expect("fake bin");
    let ssh = bin.path().join("ssh");
    std::fs::write(&ssh, "#!/bin/sh\nfor last; do :; done\nexec /bin/sh -c \"$last\"\n").expect("fake ssh");
    std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o755)).expect("chmod ssh");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_vowp"), bin.path().join("vowp")).expect("vowp link");

    let local = env.store();
    local
        .upsert_node(&vowp_protocol::Node {
            name: vowp_protocol::NodeName::parse("vps").expect("node"),
            ssh_target: Some("vps.example".to_owned()),
            is_local: false,
            capabilities: serde_json::json!({}),
            last_seen_at: None,
            created_at: now,
        })
        .expect("local vps row");
    RemoteNode { home, bin }
}

/// Runs the real binary with the local home/project and a remote `VOWP_HOME`.
fn run_cli(env: &common::TestEnv, remote: &RemoteNode, args: &[&str]) -> std::process::Output {
    let path = format!("{}:/usr/bin:/bin", remote.bin.path().display());
    env.command(env!("CARGO_BIN_EXE_vowp"))
        .env("PATH", path)
        .env("VOWP_HOME", remote.home.path())
        .arg("--home")
        .arg(env.home_path())
        .arg("--project")
        .arg(env.project_path())
        .args(args)
        .output()
        .expect("run vowp")
}

/// Returns the text of one stream.
fn output_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Creates a session directly in a home without going through `--node`.
fn new_session_in(env: &common::TestEnv, home: &std::path::Path, title: &str) -> String {
    let output = env
        .command(env!("CARGO_BIN_EXE_vowp"))
        .env("VOWP_HOME", home)
        .arg("--home")
        .arg(home)
        .arg("--project")
        .arg(env.project_path())
        .args([
            "session",
            "new",
            "--title",
            title,
            "--executor",
            "opencode:deepseek/deepseek-flash#max",
        ])
        .output()
        .expect("new session");
    assert!(
        output.status.success(),
        "session new failed: {}",
        output_text(&output.stderr)
    );
    output_text(&output.stdout).trim().to_owned()
}

#[test]
fn session_cross_node_export_import_switch_via_fake_ssh() {
    let env = common::TestEnv::new();
    configure(&env);
    let remote = remote_node(&env);

    // §7.5 export --node: the bundle comes from the remote and is written locally.
    let remote_id = new_session_in(&env, remote.home.path(), "remote session");
    let output = run_cli(
        &env,
        &remote,
        &["session", "new", "--title", "created over ssh", "--node", "vps"],
    );
    assert!(
        output.status.success(),
        "cross-node new failed: {}",
        output_text(&output.stderr)
    );
    let created_id = output_text(&output.stdout).trim().to_owned();
    let remote_store_for_new =
        vowp_store::Store::open(&vowp_store::NodePaths::new(remote.home.path())).expect("remote store");
    assert!(
        remote_store_for_new
            .session(&vowp_protocol::SessionId::parse(&created_id).expect("id"))
            .expect("lookup")
            .is_some(),
        "session new --node did not create the session remotely"
    );
    drop(remote_store_for_new);

    let exported = env.project_path().join("remote-export.vowp-bundle.json");
    let output = run_cli(
        &env,
        &remote,
        &[
            "session",
            "export",
            &remote_id,
            "--node",
            "vps",
            "--out",
            exported.to_str().expect("path"),
        ],
    );
    assert!(
        output.status.success(),
        "cross-node export failed: {}",
        output_text(&output.stderr)
    );
    let bundle: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&exported).expect("bundle")).expect("json");
    assert_eq!(bundle["session"]["id"], remote_id);

    // §7.5 import --node: the local bundle is streamed to the remote over stdin.
    let local_id = create(&env);
    let local_bundle = env.project_path().join("local-import.vowp-bundle.json");
    let (code, _, stderr) = env.run(&[
        "session",
        "export",
        &local_id,
        "--out",
        local_bundle.to_str().expect("path"),
    ]);
    assert_eq!(code, 0, "{stderr}");
    let output = run_cli(
        &env,
        &remote,
        &[
            "session",
            "import",
            local_bundle.to_str().expect("path"),
            "--node",
            "vps",
        ],
    );
    assert!(
        output.status.success(),
        "cross-node import failed: {}",
        output_text(&output.stderr)
    );
    let imported_id = output_text(&output.stdout).trim().to_owned();
    let remote_store = vowp_store::Store::open(&vowp_store::NodePaths::new(remote.home.path())).expect("remote store");
    let imported = remote_store
        .require_session(&vowp_protocol::SessionId::parse(&imported_id).expect("id"))
        .expect("remote child");
    assert_eq!(
        imported.parent_session_id.as_ref().map(ToString::to_string).as_deref(),
        Some(local_id.as_str())
    );

    // §7.6 switch --node: export, remote import, remote switch and local lineage.
    let switched_local = create(&env);
    let output = run_cli(
        &env,
        &remote,
        &[
            "session",
            "switch",
            &switched_local,
            "--to",
            "opencode:deepseek/deepseek-chat",
            "--reason",
            "user",
            "--node",
            "vps",
        ],
    );
    assert!(
        output.status.success(),
        "cross-node switch failed: {}",
        output_text(&output.stderr)
    );
    let stdout = output_text(&output.stdout);
    assert!(stdout.contains("remote session"), "{stdout}");

    let local_store = env.store();
    let session_id = vowp_protocol::SessionId::parse(&switched_local).expect("id");
    let switches = local_store.list_switches(&session_id).expect("local switches");
    assert_eq!(switches.len(), 1);
    assert_eq!(switches[0].state, vowp_protocol::SwitchState::Completed);
    assert_eq!(switches[0].to_node.to_string(), "vps");
    let events = SessionLog::open_read(&vowp_store::NodePaths::new(env.home_path()), &session_id)
        .expect("log")
        .read_all()
        .expect("events");
    assert!(events.iter().any(|event| matches!(
        event.body(),
        Ok(EventBody::Switch(payload))
            if payload.to.node.as_ref().map(ToString::to_string).as_deref() == Some("vps")
    )));

    let remote_store = vowp_store::Store::open(&vowp_store::NodePaths::new(remote.home.path())).expect("remote store");
    let child = remote_store
        .list_sessions(&vowp_store::SessionFilter::default())
        .expect("remote sessions")
        .into_iter()
        .find(|session| session.parent_session_id.as_ref() == Some(&session_id))
        .expect("remote child session");
    assert_eq!(
        child.executor.as_ref().map(ToString::to_string).as_deref(),
        Some("opencode:deepseek/deepseek-chat")
    );
    let remote_switches = remote_store.list_switches(&child.id).expect("remote switches");
    assert!(
        remote_switches.iter().any(|switch| {
            switch.state == vowp_protocol::SwitchState::Completed
                && switch.reason == vowp_protocol::SwitchReason::User
                && switch.to_executor.to_string() == "opencode:deepseek/deepseek-chat"
        }),
        "{remote_switches:?}"
    );
}

/// The executor every remote-exec CLI test uses.
const REMOTE_EXECUTOR: &str = "opencode:deepseek/deepseek-flash#max";

/// Writes a `turn-begin` request, runs it in-process and stores the grant at mode 0600.
fn begin_remote_turn(env: &common::TestEnv, id: &str, lease: &str, prompt: &str) -> (TurnGrant, std::path::PathBuf) {
    let request_path = env.project_path().join("turn-request.json");
    let request = serde_json::json!({
        "lease": lease,
        "prompt": prompt,
        "executor": REMOTE_EXECUTOR,
        "purpose": "start",
        "node": "local",
    });
    std::fs::write(&request_path, request.to_string()).expect("request");
    let (code, stdout, stderr) = env.run(&[
        "session",
        "turn-begin",
        id,
        "--executor",
        REMOTE_EXECUTOR,
        "--purpose",
        "start",
        "--node",
        "local",
        "--request",
        request_path.to_str().expect("path"),
    ]);
    assert_eq!(code, 0, "{stderr}");
    let grant: TurnGrant = serde_json::from_str(&stdout).expect("grant JSON");
    let grant_path = env.home_path().join("turn-grant.json");
    std::fs::write(&grant_path, &stdout).expect("grant file");
    std::fs::set_permissions(&grant_path, std::fs::Permissions::from_mode(0o600)).expect("grant mode");
    (grant, grant_path)
}

/// Runs the real `vowp __exec` binary against one grant.
fn run_exec(env: &common::TestEnv, grant: &Path, spool: Option<&Path>) -> std::process::Output {
    let mut command = env.command(env!("CARGO_BIN_EXE_vowp"));
    command
        .arg("--home")
        .arg(env.home_path())
        .arg("--project")
        .arg(env.project_path())
        .arg("__exec")
        .arg("--grant")
        .arg(grant);
    if let Some(spool) = spool {
        command.arg("--spool-dir").arg(spool);
    }
    command.output().expect("run __exec")
}

/// One fake-ssh fixture: fake `ssh`/`vowp` PATH, control home and trace files.
struct SshFixture {
    bin: tempfile::TempDir,
    control: tempfile::TempDir,
    args: std::path::PathBuf,
    runs: std::path::PathBuf,
    hello: std::path::PathBuf,
}

/// Installs a fake `ssh` that runs the quoted remote command locally.
fn ssh_fixture(env: &common::TestEnv, script: &str) -> SshFixture {
    let bin = tempfile::tempdir().expect("fake bin");
    let ssh = bin.path().join("ssh");
    std::fs::write(&ssh, script).expect("fake ssh");
    std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o755)).expect("chmod ssh");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_vowp"), bin.path().join("vowp")).expect("vowp link");
    let control = tempfile::tempdir().expect("control home");
    let pointer = serde_json::json!({
        "role": "control",
        "home": "local",
        "ssh_target": "fake.example",
        "remote_home": env.home_path().display().to_string(),
        "created_at": "2026-11-08T18:00:00Z",
    });
    std::fs::write(control.path().join("control.json"), pointer.to_string()).expect("control");
    SshFixture {
        args: bin.path().join("ssh-args.txt"),
        runs: bin.path().join("ssh-runs.txt"),
        hello: bin.path().join("hello.json"),
        bin,
        control,
    }
}

/// The forwarding fake ssh used by the happy-path tests.
const FORWARDING_SSH: &str = "#!/bin/sh\n\
for last; do :; done\n\
IFS= read -r line\n\
printf '%s\\n' \"$line\" > \"$FAKE_SSH_HELLO\"\n\
printf '%s\\n' \"$@\" >> \"$FAKE_SSH_ARGS\"\n\
{ printf '%s\\n' \"$line\"; cat; } | /bin/sh -c \"$last\"\n";

/// A fake ssh that cuts its first connection: it forwards at most two complete lines
/// (plus 12 bytes of the next line in `mid_line` mode) and drops every ack.
const CUTTING_SSH: &str = "#!/bin/sh\n\
printf '%s\\n' \"$@\" >> \"$FAKE_SSH_ARGS\"\n\
printf 'run\\n' >> \"$FAKE_SSH_RUNS\"\n\
for last; do :; done\n\
if [ \"$(wc -l < \"$FAKE_SSH_RUNS\")\" -eq 1 ]; then\n\
  if [ \"$FAKE_SSH_MODE\" = \"mid_line\" ]; then\n\
    { i=0; while [ $i -lt 2 ]; do IFS= read -r line || break; printf '%s\\n' \"$line\"; i=$((i+1)); done; head -c 12 2>/dev/null; } | /bin/sh -c \"$last\" >/dev/null\n\
  else\n\
    { i=0; while [ $i -lt 2 ]; do IFS= read -r line || break; printf '%s\\n' \"$line\"; i=$((i+1)); done; } | /bin/sh -c \"$last\" >/dev/null\n\
  fi\n\
  exit 0\n\
fi\n\
exec /bin/sh -c \"$last\"\n";

/// Runs `__exec` with the fixture's fake ssh and control home.
fn run_exec_over_ssh(
    env: &common::TestEnv,
    fixture: &SshFixture,
    grant: &Path,
    spool: Option<&Path>,
    extra: &[(&str, &str)],
) -> std::process::Output {
    let mut command = env.command(env!("CARGO_BIN_EXE_vowp"));
    command
        .env("PATH", format!("{}:/usr/bin:/bin", fixture.bin.path().display()))
        .env("VOWP_CONTROL_HOME", fixture.control.path())
        .env("FAKE_SSH_ARGS", &fixture.args)
        .env("FAKE_SSH_RUNS", &fixture.runs)
        .env("FAKE_SSH_HELLO", &fixture.hello)
        .arg("--home")
        .arg(env.home_path())
        .arg("--project")
        .arg(env.project_path())
        .arg("__exec")
        .arg("--grant")
        .arg(grant);
    for (name, value) in extra {
        command.env(name, value);
    }
    if let Some(spool) = spool {
        command.arg("--spool-dir").arg(spool);
    }
    command.output().expect("run __exec over fake ssh")
}

/// Returns every regular file below `root`.
fn files_below(root: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(files_below(&path));
        } else if path.is_file() {
            files.push(path);
        }
    }
    files
}

#[test]
fn remote_turn_begin_and_exec_streams_over_local_ingest() {
    let env = common::TestEnv::new();
    configure(&env);
    let id = create(&env);
    let (grant, grant_path) = begin_remote_turn(&env, &id, "lease-token", "stream this turn");

    let output = run_exec(&env, &grant_path, None);
    assert!(
        output.status.success(),
        "__exec failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(&grant.run_id.to_string()),
        "the status document does not name the run"
    );

    let session_id = vowp_protocol::SessionId::parse(&id).expect("session id");
    let paths = vowp_store::NodePaths::new(env.home_path());
    let events = SessionLog::open_read(&paths, &session_id)
        .expect("log")
        .read_all()
        .expect("events");
    assert!(
        events.iter().any(|event| matches!(
            event.body(),
            Ok(EventBody::AssistantMessage(payload)) if !payload.text.is_empty()
        )),
        "no assistant event reached the home log"
    );
    let store = env.store();
    let run = store.require_run(&grant.run_id).expect("run");
    assert_eq!(run.outcome, Some(vowp_protocol::RunOutcome::Ok));
    assert!(run.ended_at.is_some());
    let session = store.require_session(&session_id).expect("session");
    assert_eq!(session.state, vowp_protocol::SessionState::Idle);
    assert_eq!(
        events.iter().map(|event| event.seq).collect::<Vec<_>>(),
        (1..=events.len() as u64).collect::<Vec<_>>()
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == EventType::TurnEnded)
            .count(),
        1,
        "the streamed turn ended more than once"
    );

    let spool = env.home_path().join("spool").join(&id).join(grant.run_id.to_string());
    let spool_text = std::fs::read_to_string(spool.join("events.jsonl")).expect("spool");
    assert!(spool_text.contains("assistant.message"), "{spool_text}");
    let acked = std::fs::read_to_string(spool.join("acked.json")).expect("acked");
    assert!(acked.contains("\"summary\": true"), "{acked}");
}

#[test]
fn client_crash_resumes_from_spool() {
    let env = common::TestEnv::new();
    configure(&env);
    let id = create(&env);
    let (grant, grant_path) = begin_remote_turn(&env, &id, "lease-token", "resume this turn");
    let spool = env.home_path().join("resume-spool");
    std::fs::create_dir_all(&spool).expect("spool dir");
    let ts = Timestamp::parse("2026-11-08T18:00:00Z").expect("timestamp");
    let event = RemoteLine::Event {
        event: RemoteEvent {
            id: vowp_protocol::EventId::new(),
            ts,
            event_type: EventType::AssistantMessage,
            actor: Actor::Assistant,
            source: EventSource {
                harness: HarnessKind::Opencode,
                model: Some("deepseek/deepseek-flash".to_owned()),
                node: Some(vowp_protocol::NodeName::parse("local").expect("node")),
                native_session_id: None,
            },
            payload: serde_json::json!({"text": "resumed from the spool"}),
            raw: None,
            replayable: true,
        },
    };
    let summary = RemoteLine::Summary {
        exit_code: Some(0),
        outcome: vowp_protocol::RunOutcome::Ok,
        native_session_id: None,
        cost: Cost::unknown(),
        limit: None,
        result_text: Some("resumed".to_owned()),
        stop_reason: StopReason::EndTurn,
        usage: None,
    };
    let text = format!(
        "{}\n{}\n",
        serde_json::to_string(&event).expect("event"),
        serde_json::to_string(&summary).expect("summary")
    );
    std::fs::write(spool.join("events.jsonl"), text).expect("spool events");

    let output = run_exec(&env, &grant_path, Some(&spool));
    assert!(
        output.status.success(),
        "resume failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let session_id = vowp_protocol::SessionId::parse(&id).expect("session id");
    let events = SessionLog::open_read(&vowp_store::NodePaths::new(env.home_path()), &session_id)
        .expect("log")
        .read_all()
        .expect("events");
    assert!(events.iter().any(|event| matches!(
        event.body(),
        Ok(EventBody::AssistantMessage(payload)) if payload.text == "resumed from the spool"
    )));
    let store = env.store();
    let run = store.require_run(&grant.run_id).expect("run");
    assert_eq!(run.outcome, Some(vowp_protocol::RunOutcome::Ok));
}

#[test]
fn lease_token_never_in_argv_or_logs() {
    let env = common::TestEnv::new();
    configure(&env);
    let id = create(&env);
    let token = "lease-token-4f9c2b7d-never-log";
    let (grant, grant_path) = begin_remote_turn(&env, &id, token, "keep the token private");
    let fixture = ssh_fixture(&env, FORWARDING_SSH);
    let mode = std::fs::metadata(&grant_path).expect("grant").permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "the grant file is not mode 0600");

    let output = run_exec_over_ssh(&env, &fixture, &grant_path, None, &[]);
    assert!(
        output.status.success(),
        "__exec failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // This is the real argv ssh received, not a hand-built literal.
    let argv = std::fs::read_to_string(&fixture.args).expect("ssh argv trace");
    assert!(!argv.contains(token), "the lease token was placed on argv: {argv}");
    assert!(argv.contains("session"), "the ingest command was not traced: {argv}");
    assert!(argv.contains("ingest"), "the ingest command was not traced: {argv}");

    for path in files_below(env.home_path()) {
        if path == grant_path {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap_or_default();
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            !text.contains(token),
            "the lease token was written to {}",
            path.display()
        );
    }
    let _ = grant;
}

#[test]
fn remote_exec_streams_over_fake_ssh() {
    let env = common::TestEnv::new();
    configure(&env);
    let id = create(&env);
    let (grant, grant_path) = begin_remote_turn(&env, &id, "lease-token", "stream over ssh");
    let fixture = ssh_fixture(&env, FORWARDING_SSH);
    let output = run_exec_over_ssh(&env, &fixture, &grant_path, None, &[]);
    assert!(
        output.status.success(),
        "__exec over ssh failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let hello: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&fixture.hello).expect("hello trace")).expect("hello JSON");
    assert_eq!(hello["type"], "hello");
    assert_eq!(hello["executor"], grant.executor);
    assert_eq!(hello["node"], "local");
    assert_eq!(hello["run_id"], grant.run_id.to_string());

    let session_id = vowp_protocol::SessionId::parse(&id).expect("session id");
    let events = SessionLog::open_read(&vowp_store::NodePaths::new(env.home_path()), &session_id)
        .expect("log")
        .read_all()
        .expect("events");
    assert!(
        events
            .iter()
            .any(|event| event.event_type == EventType::AssistantMessage)
    );
    let store = env.store();
    let run = store.require_run(&grant.run_id).expect("run");
    assert_eq!(run.outcome, Some(vowp_protocol::RunOutcome::Ok));
}

#[test]
fn remote_exec_start_failure_finalises_the_turn() {
    let env = common::TestEnv::new();
    let script = configure(&env);
    let id = create(&env);
    let (grant, grant_path) = begin_remote_turn(&env, &id, "lease-token", "start nothing");
    std::fs::remove_file(&script).expect("remove the fake harness binary");

    let output = run_exec(&env, &grant_path, None);
    assert!(
        !output.status.success(),
        "__exec reported success after the harness failed to start: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_failed_turn_finalised(&env, &id, &grant);
}

#[test]
fn remote_exec_env_failure_finalises_the_turn() {
    let env = common::TestEnv::new();
    configure(&env);
    let id = create(&env);
    let (grant, grant_path) = begin_remote_turn(&env, &id, "lease-token", "missing env");
    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&grant_path).expect("grant")).expect("grant JSON");
    value["env_names"] = serde_json::json!(["VOWP_EXEC_TEST_MISSING_ENV_9F3C"]);
    std::fs::write(&grant_path, value.to_string()).expect("rewrite grant");
    std::fs::set_permissions(&grant_path, std::fs::Permissions::from_mode(0o600)).expect("grant mode");

    let output = run_exec(&env, &grant_path, None);
    assert!(
        !output.status.success(),
        "__exec reported success after env resolution failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_failed_turn_finalised(&env, &id, &grant);
}

/// Asserts a failed `__exec` sent a summary that closed the run and the session.
fn assert_failed_turn_finalised(env: &common::TestEnv, id: &str, grant: &TurnGrant) {
    let session_id = vowp_protocol::SessionId::parse(id).expect("session id");
    let events = SessionLog::open_read(&vowp_store::NodePaths::new(env.home_path()), &session_id)
        .expect("log")
        .read_all()
        .expect("events");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == EventType::TurnEnded)
            .count(),
        1,
        "a failed start left the turn open or ended it twice"
    );
    assert!(
        events.iter().any(|event| matches!(
            event.body(),
            Ok(EventBody::Error(payload))
                if payload.kind == vowp_protocol::ErrorKind::AdapterFailed && !payload.message.is_empty()
        )),
        "the failure was not recorded as an error event"
    );
    assert!(
        events.iter().any(|event| matches!(
            event.body(),
            Ok(EventBody::SessionExecutor(payload))
                if payload.action == vowp_protocol::AttachAction::Detach
                    && payload.run_id.as_ref() == Some(&grant.run_id)
        )),
        "the failed turn left the executor attached"
    );
    let store = env.store();
    let run = store.require_run(&grant.run_id).expect("run");
    assert_eq!(run.outcome, Some(vowp_protocol::RunOutcome::Error));
    assert!(run.ended_at.is_some(), "the failed run was left open");
    let session = store.require_session(&session_id).expect("session");
    assert_eq!(session.state, vowp_protocol::SessionState::Idle);
    let spool = env.home_path().join("spool").join(id).join(grant.run_id.to_string());
    let acked = std::fs::read_to_string(spool.join("acked.json")).expect("acked");
    assert!(acked.contains("\"summary\": true"), "{acked}");
}

#[test]
fn ssh_drop_spools_and_replays_without_gap_or_duplicate() {
    let env = common::TestEnv::new();
    configure(&env);
    let id = create(&env);
    let (grant, grant_path) = begin_remote_turn(&env, &id, "lease-token", "survive an ack loss");
    let fixture = ssh_fixture(&env, CUTTING_SSH);
    let output = run_exec_over_ssh(&env, &fixture, &grant_path, None, &[("FAKE_SSH_MODE", "ack_lost")]);
    assert!(
        output.status.success(),
        "ack-loss replay failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let runs = std::fs::read_to_string(&fixture.runs).expect("ssh runs");
    assert!(runs.lines().count() >= 2, "the client did not reconnect: {runs}");

    let session_id = vowp_protocol::SessionId::parse(&id).expect("session id");
    let events = SessionLog::open_read(&vowp_store::NodePaths::new(env.home_path()), &session_id)
        .expect("log")
        .read_all()
        .expect("events");
    let mut ids = std::collections::HashSet::new();
    for event in &events {
        assert!(ids.insert(event.id.clone()), "duplicate event id {}", event.id);
    }
    assert_eq!(
        events.iter().map(|event| event.seq).collect::<Vec<_>>(),
        (1..=events.len() as u64).collect::<Vec<_>>()
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == EventType::TurnEnded)
            .count(),
        1
    );
    let store = env.store();
    let run = store.require_run(&grant.run_id).expect("run");
    assert_eq!(run.outcome, Some(vowp_protocol::RunOutcome::Ok));
    assert!(run.ended_at.is_some());
    let spool = env.home_path().join("spool").join(&id).join(grant.run_id.to_string());
    let acked = std::fs::read_to_string(spool.join("acked.json")).expect("acked");
    assert!(acked.contains("\"summary\": true"), "{acked}");
}

#[test]
fn ssh_mid_line_cut_replays_without_gap() {
    let env = common::TestEnv::new();
    configure(&env);
    let id = create(&env);
    let (grant, grant_path) = begin_remote_turn(&env, &id, "lease-token", "survive a mid-line cut");
    let fixture = ssh_fixture(&env, CUTTING_SSH);
    let output = run_exec_over_ssh(&env, &fixture, &grant_path, None, &[("FAKE_SSH_MODE", "mid_line")]);
    assert!(
        output.status.success(),
        "mid-line replay failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let runs = std::fs::read_to_string(&fixture.runs).expect("ssh runs");
    assert!(runs.lines().count() >= 2, "the client did not reconnect: {runs}");

    let session_id = vowp_protocol::SessionId::parse(&id).expect("session id");
    let events = SessionLog::open_read(&vowp_store::NodePaths::new(env.home_path()), &session_id)
        .expect("log")
        .read_all()
        .expect("events");
    let mut ids = std::collections::HashSet::new();
    for event in &events {
        assert!(ids.insert(event.id.clone()), "duplicate event id {}", event.id);
    }
    let store = env.store();
    let run = store.require_run(&grant.run_id).expect("run");
    assert_eq!(run.outcome, Some(vowp_protocol::RunOutcome::Ok));
    let _ = grant;
}

#[test]
fn session_ingest_json_stdout_is_only_acks() {
    let env = common::TestEnv::new();
    configure(&env);
    let id = create(&env);
    let (grant, _grant_path) = begin_remote_turn(&env, &id, "lease-token", "ingest a scripted stream");
    let ts = Timestamp::parse("2026-11-08T18:00:00Z").expect("timestamp");
    let hello = RemoteLine::Hello {
        session_id: grant.session_id.clone(),
        run_id: grant.run_id.clone(),
        executor: grant.executor.clone(),
        node: grant.node.clone(),
        spool_id: "cli-test".to_owned(),
        lease: grant.lease_token.clone(),
    };
    let event = RemoteLine::Event {
        event: RemoteEvent {
            id: vowp_protocol::EventId::new(),
            ts,
            event_type: EventType::AssistantMessage,
            actor: Actor::Assistant,
            source: EventSource {
                harness: HarnessKind::Opencode,
                model: Some("deepseek/deepseek-flash".to_owned()),
                node: Some(grant.node.clone()),
                native_session_id: None,
            },
            payload: serde_json::json!({"text": "scripted ingest"}),
            raw: None,
            replayable: true,
        },
    };
    let summary = RemoteLine::Summary {
        exit_code: Some(0),
        outcome: vowp_protocol::RunOutcome::Ok,
        native_session_id: None,
        cost: Cost::unknown(),
        limit: None,
        result_text: Some("done".to_owned()),
        stop_reason: StopReason::EndTurn,
        usage: None,
    };
    let text = format!(
        "{}\n{}\n{}\n",
        serde_json::to_string(&hello).expect("hello"),
        serde_json::to_string(&event).expect("event"),
        serde_json::to_string(&summary).expect("summary")
    );
    let mut command = env.command(env!("CARGO_BIN_EXE_vowp"));
    command
        .arg("--home")
        .arg(env.home_path())
        .arg("--project")
        .arg(env.project_path())
        .args(["--json", "session", "ingest", &id, "--run", grant.run_id.as_str()])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command.spawn().expect("spawn ingest");
    {
        use std::io::Write as _;
        let mut stdin = child.stdin.take().expect("stdin");
        stdin.write_all(text.as_bytes()).expect("write stream");
    }
    let output = child.wait_with_output().expect("ingest output");
    assert!(
        output.status.success(),
        "session ingest failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut acks = Vec::new();
    for line in stdout.lines() {
        let ack: vowp_protocol::IngestAck =
            serde_json::from_str(line).unwrap_or_else(|error| panic!("non-ack stdout line {line:?}: {error}"));
        acks.push(ack);
    }
    assert!(
        acks.iter()
            .any(|ack| matches!(ack, vowp_protocol::IngestAck::Summary { done: true, .. })),
        "no summary ack: {acks:?}"
    );
}
