//! `vowp agent` CLI tests.
//!
//! Every test uses a temporary `HOME` or in-process parsing; none reads the real
//! `~/.vowp-agent`, opens a store or starts ssh.

use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command as ProcessCommand, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use clap::Parser;
use tempfile::TempDir;
use vowp_agent::{
    AgentClient, AgentClock, AgentConfig, AgentError, AgentStatus, Cache, ConnectionState, FrameRead, FrameStream,
    NodeKey, PushStatus, RunHandler, RunOptions, RunSink, RunSinkError, RunSummary, ServiceError, Transport,
    TransportConfig, WssTlsMode,
};
use vowp_cli::BufferIo;
use vowp_cli::agent_exec::CoreRunHandler;
use vowp_cli::cli::{AgentCommand, Cli, Command};
use vowp_cli::cmd::agent::{enrollment_transport_config, service_error_exit_code};
use vowp_cli::node_ctx::ControlConfig;
use vowp_core::agent::pair::{issue_surface_token, verify_surface_token};
use vowp_protocol::{
    AGENT_PROTOCOL, AgentFrame, AgentPathMap, AgentRole, AssignmentPayload, Executor, JobId, NodeKind, NodeName,
    NodeStatus, PathRef, ProjectName, RemoteEvent, RemoteLine, RemoteRunState, RunId, RunOutcome, RunPurpose,
    RunRejectReason, SecretName, Session, SessionId, SessionRole, SessionState, StopReason, SurfaceId, SurfaceState,
    Timestamp, TurnSummary,
};
use vowp_store::{NodeCredential, NodePath};

mod common;

use common::TestEnv;

fn run_binary(home: &Path, args: &[&str]) -> (i32, String, String) {
    run_binary_with(home, args, &[])
}

/// Runs the real binary with an isolated home and extra environment values.
fn run_binary_with(home: &Path, args: &[&str], envs: &[(&str, String)]) -> (i32, String, String) {
    let mut command = ProcessCommand::new(env!("CARGO_BIN_EXE_vowp"));
    command
        .args(args)
        .env("HOME", home)
        .env("NO_COLOR", "1")
        .env_remove("VOWP_AGENT_HOME")
        .env_remove("VOWP_HOME")
        .env_remove("VOWP_CONTROL_HOME")
        .env_remove("XDG_CONFIG_HOME");
    for (key, value) in envs {
        command.env(key, value);
    }
    let output = command.output().expect("run vowp");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn agent_status_without_enrollment_exits_three() {
    let home = TempDir::new().expect("temp home");
    let (code, stdout, stderr) = run_binary(home.path(), &["agent", "status"]);

    assert_eq!(code, 3, "no enrollment is exit 3; stderr: {stderr}");
    assert!(stdout.is_empty(), "status failure prints no document: {stdout}");
    assert!(
        stderr.contains("vowp agent enroll --target") && stderr.contains("--code-file"),
        "the error must hint at enrollment: {stderr}"
    );
}

#[test]
fn agent_status_prints_json() {
    let home = TempDir::new().expect("temp home");
    let state_dir = home.path().join(".vowp-agent");
    let node = NodeName::parse("mac-studio").expect("node slug");
    let config = AgentConfig::new("home-1", node.clone(), state_dir.clone(), "home.example");
    config.write().expect("agent.json");

    let heartbeat = Timestamp::parse("2026-11-14T10:00:00.000Z").expect("timestamp");
    let status = AgentStatus {
        connection: ConnectionState::Connected,
        node: Some(node.clone()),
        home: Some("home-1".to_owned()),
        protocol: Some(AGENT_PROTOCOL.to_owned()),
        last_heartbeat: Some(heartbeat),
        last_error: None,
        pending_spool_bytes: 42,
        key_fingerprint: Some("sha256:0123456789abcdef".to_owned()),
        surface_token_expires_at: None,
        cache_entries: 0,
        cache_bytes: 0,
        cache_oldest_at: None,
    };
    status.write(&state_dir).expect("status.json");
    std::fs::create_dir_all(state_dir.join("spool")).expect("spool dir");
    std::fs::write(state_dir.join("spool/pending.json"), vec![b'x'; 42]).expect("spool bytes");
    let cached_at = Timestamp::parse("2026-11-14T09:00:00.000Z").expect("timestamp");
    let surface = SurfaceId("01ARZ3NDEKTSV4RRFFQ69G5FAW".to_owned());
    Cache::new(&state_dir)
        .store(
            "fleet_status",
            &surface,
            &serde_json::json!({"project": "vowp"}),
            &serde_json::json!({"content": [{"type": "text", "text": "{}"}], "isError": false}),
            cached_at,
        )
        .expect("cache entry");

    let (code, stdout, stderr) = run_binary(home.path(), &["--json", "agent", "status"]);
    assert_eq!(code, 0, "status reports even when stopped; stderr: {stderr}");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("status JSON");
    assert_eq!(value["connection"], "connected");
    assert_eq!(value["node"], "mac-studio");
    assert_eq!(value["home"], "home-1");
    assert_eq!(value["protocol"], AGENT_PROTOCOL);
    assert_eq!(value["pending_spool_bytes"], 42);
    assert_eq!(value["last_heartbeat"], "2026-11-14T10:00:00.000Z");
    assert_eq!(value["key_fingerprint"], "sha256:0123456789abcdef");
    assert_eq!(value["cache_entries"], 1);
    assert!(value["cache_bytes"].as_u64().expect("cache bytes") > 0);
    assert_eq!(value["cache_oldest_at"], "2026-11-14T09:00:00.000Z");

    let (code, human, stderr) = run_binary(home.path(), &["agent", "status"]);
    assert_eq!(code, 0, "human status failed: {stderr}");
    assert!(human.contains("cache: 1 entries"), "{human}");
    assert!(human.contains("cache oldest: 2026-11-14T09:00:00.000Z"), "{human}");
}

#[test]
fn agent_enroll_help_names_code_file() {
    let mut io = BufferIo::new("");
    let code = vowp_cli::run(
        vec![
            "vowp".to_owned(),
            "agent".to_owned(),
            "enroll".to_owned(),
            "--help".to_owned(),
        ],
        &mut io,
    );
    assert_eq!(code, 0, "help exits 0; stderr: {}", io.stderr());
    let help = io.stdout();
    assert!(help.contains("--code-file"), "help must name --code-file: {help}");
    assert!(help.contains("--target"), "help must name --target: {help}");
    assert!(help.contains("--wss"), "help must name --wss: {help}");
    assert!(
        help.contains("--tls-fingerprint"),
        "help must name --tls-fingerprint: {help}"
    );
    assert!(help.contains("PATH|-"), "help must document stdin: {help}");
}

#[test]
fn agent_enroll_accepts_code_file_stdin() {
    let cli = Cli::try_parse_from([
        "vowp",
        "agent",
        "enroll",
        "--target",
        "home.example",
        "--code-file",
        "-",
    ])
    .expect("parse agent enroll");
    match cli.command {
        Command::Agent {
            command: AgentCommand::Enroll(args),
        } => {
            assert_eq!(args.target.as_deref(), Some("home.example"));
            assert_eq!(args.code_file, "-");
            assert!(args.project.is_empty());
            assert!(args.worktree_root.is_none());
            assert!(args.wss.is_none());
            assert!(args.tls_fingerprint.is_none());
        }
        other => panic!("expected agent enroll, got {other:?}"),
    }
}

/// Creates an enrolled agent config under `home` for the path-map commands.
fn enrolled_agent_config(home: &Path) -> AgentConfig {
    let state_dir = home.join(".vowp-agent");
    let config = AgentConfig::new(
        "home-1",
        NodeName::parse("mac-studio").expect("node slug"),
        state_dir,
        "home.example",
    );
    config.write().expect("agent.json");
    config
}

/// Creates a temporary checkout and the map `vowp agent paths add` should produce.
fn checkout_map(home: &Path, directory: &str, project: &str) -> (PathBuf, AgentPathMap) {
    let root = home.join(directory);
    let worktrees = root.join(".vowp/worktrees");
    std::fs::create_dir_all(&worktrees).expect("worktree root");
    std::fs::write(root.join("vowp.toml"), format!("[project]\nname = \"{project}\"\n")).expect("vowp.toml");
    let root = std::fs::canonicalize(root).expect("canonical checkout");
    let worktrees = std::fs::canonicalize(worktrees).expect("canonical worktrees");
    let map = AgentPathMap {
        project: ProjectName::parse(project).expect("project slug"),
        local_root: root.to_str().expect("utf-8").to_owned(),
        worktree_root: Some(worktrees.to_str().expect("utf-8").to_owned()),
        allow: Vec::new(),
    };
    (root, map)
}

#[test]
fn agent_paths_add_update_and_remove_one_project() {
    let home = TempDir::new().expect("temp home");
    enrolled_agent_config(home.path());
    let (checkout, expected) = checkout_map(home.path(), "checkout", "vowp");
    let checkout_text = checkout.to_str().expect("utf-8 checkout");

    let (code, stdout, stderr) = run_binary(
        home.path(),
        &["agent", "paths", "add", "--project", checkout_text, "--allow", "src"],
    );
    assert_eq!(code, 0, "paths add failed: {stderr}");
    assert!(stdout.contains("vowp"), "{stdout}");
    assert!(
        stdout.contains(&expected.local_root),
        "the owner sees the root: {stdout}"
    );

    let state_dir = home.path().join(".vowp-agent");
    let config = AgentConfig::load(&state_dir).expect("stored config");
    let map = config.path_map(&expected.project).expect("added map").clone();
    assert_eq!(map.local_root, expected.local_root);
    assert_eq!(map.worktree_root, expected.worktree_root);
    assert_eq!(map.allow, vec!["src".to_owned()]);
    let mode = std::fs::metadata(AgentConfig::file_path(&state_dir))
        .expect("agent.json")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);

    let (code, list, stderr) = run_binary(home.path(), &["--json", "agent", "paths", "ls"]);
    assert_eq!(code, 0, "paths ls failed: {stderr}");
    let list: serde_json::Value = serde_json::from_str(&list).expect("paths JSON");
    assert_eq!(list[0]["project"], "vowp");
    assert_eq!(list[0]["local_root"], expected.local_root);

    let other = home.path().join("other-worktrees");
    let other_text = other.to_str().expect("utf-8");
    let (code, _stdout, stderr) = run_binary(
        home.path(),
        &[
            "agent",
            "paths",
            "add",
            "--project",
            checkout_text,
            "--worktree-root",
            other_text,
            "--allow",
            "src",
            "--allow",
            "docs",
        ],
    );
    assert_eq!(code, 0, "paths update failed: {stderr}");
    let config = AgentConfig::load(&state_dir).expect("updated config");
    let map = config.path_map(&expected.project).expect("updated map");
    assert_eq!(
        map.worktree_root.as_deref(),
        Some(
            std::fs::canonicalize(&other)
                .expect("canonical")
                .to_str()
                .expect("utf-8")
        )
    );
    assert_eq!(map.allow, vec!["src".to_owned(), "docs".to_owned()]);

    let (code, stdout, stderr) = run_binary(home.path(), &["agent", "paths", "rm", "vowp"]);
    assert_eq!(code, 0, "paths rm failed: {stderr}");
    assert!(stdout.contains("unmapped project vowp"), "{stdout}");
    let config = AgentConfig::load(&state_dir).expect("config after rm");
    assert!(config.path_maps.is_empty());

    let (code, _stdout, stderr) = run_binary(home.path(), &["agent", "paths", "rm", "vowp"]);
    assert_eq!(code, 3, "an unknown project is not found: {stderr}");
    assert!(stderr.contains("not mapped"), "{stderr}");
}

#[test]
fn agent_status_redacts_absolute_paths_from_the_snapshot_cache() {
    let home = TempDir::new().expect("temp home");
    let state_dir = home.path().join(".vowp-agent");
    let (_checkout, map) = checkout_map(home.path(), "checkout", "vowp");
    let config = AgentConfig::new(
        "home-1",
        NodeName::parse("mac-studio").expect("node slug"),
        state_dir.clone(),
        "home.example",
    )
    .with_path_maps(vec![map.clone()]);
    config.write().expect("agent.json");

    let status = AgentStatus {
        connection: ConnectionState::Connected,
        node: Some(NodeName::parse("mac-studio").expect("node slug")),
        home: Some("home-1".to_owned()),
        protocol: Some(AGENT_PROTOCOL.to_owned()),
        last_heartbeat: Some(Timestamp::parse("2026-11-14T10:00:00Z").expect("timestamp")),
        last_error: None,
        pending_spool_bytes: 0,
        key_fingerprint: Some("sha256:0123456789abcdef".to_owned()),
        surface_token_expires_at: None,
        cache_entries: 0,
        cache_bytes: 0,
        cache_oldest_at: None,
    };
    status.write(&state_dir).expect("status.json");
    let cached_before = std::fs::read_to_string(AgentStatus::file_path(&state_dir)).expect("status cache");

    let (code, stdout, stderr) = run_binary(home.path(), &["--json", "agent", "status"]);
    assert_eq!(code, 0, "status failed: {stderr}");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("status JSON");
    assert_eq!(value["paths"][0]["project"], "vowp");
    assert_eq!(
        value["paths"][0]["local_root"], map.local_root,
        "the owner-only JSON may carry the absolute root"
    );

    let cached_after = std::fs::read_to_string(AgentStatus::file_path(&state_dir)).expect("status cache");
    assert_eq!(
        cached_before, cached_after,
        "status must not rewrite its cache with a path map"
    );
    assert!(
        !cached_after.contains(&map.local_root),
        "the status snapshot cache keeps names only: {cached_after}"
    );

    let (code, human, stderr) = run_binary(home.path(), &["agent", "status"]);
    assert_eq!(code, 0, "human status failed: {stderr}");
    assert!(human.contains("paths:"), "{human}");
    assert!(
        human.contains(&map.local_root),
        "the owner sees the mapped root: {human}"
    );
}

#[test]
fn node_enroll_creates_a_pending_node_and_prints_the_code_once() {
    let env = TestEnv::new();
    let (code, value, stderr) = env.run_json(&[
        "node",
        "enroll",
        "mac-studio",
        "--label",
        "darwin-arm64",
        "--label",
        "gpu",
    ]);
    assert_eq!(code, 0, "node enroll failed: {stderr}");
    let text = value["code"].as_str().expect("code is printed once");
    assert_eq!(text.len(), 52, "the code is a 52-character token");
    assert_eq!(value["node"], "mac-studio");
    let command = value["command"].as_str().expect("enrollment command");
    assert!(command.contains("vowp agent enroll --target"), "{command}");
    assert!(command.contains("--code-file -"), "{command}");
    assert!(
        !stderr.contains(text),
        "the code must never be written to stderr: {stderr}"
    );

    let store = env.store();
    let name = NodeName::parse("mac-studio").expect("node slug");
    let card = store
        .node_card(&name, store.clock().now())
        .expect("card")
        .expect("node exists");
    assert_eq!(card.kind, NodeKind::Agent);
    assert_eq!(card.status, NodeStatus::Pending);
    assert_eq!(card.labels, vec!["darwin-arm64".to_owned(), "gpu".to_owned()]);
    let hash = vowp_core::agent::hash_code(text);
    assert_eq!(hash.len(), 64);
    assert!(
        store
            .consume_enrollment_code(&hash, store.clock().now())
            .expect("code lookup")
            .is_some(),
        "the store keeps the code hash"
    );
    let database = std::fs::read(env.home_path().join("vowp.db")).expect("database bytes");
    assert!(
        !database.windows(text.len()).any(|window| window == text.as_bytes()),
        "the plaintext code must not be stored in the database"
    );
}

#[test]
fn node_enroll_refuses_a_duplicate_node() {
    let env = TestEnv::new();
    let (code, _stdout, stderr) = env.run(&["node", "enroll", "mac-studio"]);
    assert_eq!(code, 0, "first enrollment failed: {stderr}");
    let store = env.store();
    let name = NodeName::parse("mac-studio").expect("node slug");
    store
        .set_node_status(&name, NodeStatus::Active, "", store.clock().now())
        .expect("activate node");

    let (code, _stdout, stderr) = env.run(&["node", "enroll", "mac-studio"]);
    assert_eq!(code, 4, "a duplicate node is a conflict: {stderr}");
    assert!(stderr.contains("already exists"), "{stderr}");
}

#[test]
fn node_revoke_marks_the_node_and_revokes_credentials() {
    let env = TestEnv::new();
    let (code, _stdout, stderr) = env.run(&["node", "enroll", "mac-studio"]);
    assert_eq!(code, 0, "enrollment failed: {stderr}");
    let store = env.store();
    let now = store.clock().now();
    let name = NodeName::parse("mac-studio").expect("node slug");
    store
        .set_node_status(&name, NodeStatus::Active, "", now)
        .expect("activate node");
    store
        .upsert_node_credential(&NodeCredential {
            node: name.clone(),
            key_id: "key-1".to_owned(),
            public_key: "00".repeat(32),
            created_at: now,
            last_used_at: None,
            revoked_at: None,
        })
        .expect("credential");

    let (code, value, stderr) = env.run_json(&["node", "revoke", "mac-studio", "--yes"]);
    assert_eq!(code, 0, "revoke failed: {stderr}");
    assert_eq!(value["node"], "mac-studio");
    assert_eq!(value["status"], "revoked");
    assert_eq!(value["credentials_revoked"], 1);
    let card = store.node_card(&name, now).expect("card").expect("node exists");
    assert_eq!(card.status, NodeStatus::Revoked);
    let credential = store
        .node_credential(&name, "key-1")
        .expect("credential lookup")
        .expect("credential exists");
    assert!(credential.revoked_at.is_some());

    let (code, _stdout, stderr) = env.run(&["node", "revoke", "missing", "--yes"]);
    assert_eq!(code, 3, "an unknown node is not found: {stderr}");
}

#[test]
fn node_show_returns_the_redacted_card() {
    let env = TestEnv::new();
    let store = env.store();
    let now = store.clock().now();
    let name = NodeName::parse("mac-studio").expect("node slug");
    store
        .insert_pending_node(&name, &["darwin-arm64".to_owned()], now)
        .expect("pending node");
    store
        .set_node_status(&name, NodeStatus::Active, "", now)
        .expect("activate node");
    store
        .upsert_node_path(&NodePath {
            node: name.clone(),
            project: ProjectName::parse("vowp").expect("project"),
            local_root: "/Users/owner/private/vowp".to_owned(),
            worktree_root: "/Users/owner/private/vowp/.vowp/worktrees".to_owned(),
            allow: vec!["src/**".to_owned()],
            updated_at: now,
        })
        .expect("path map");
    let capabilities = serde_json::json!({
        "os": "darwin",
        "arch": "arm64",
        "agent_version": "0.3.0",
        "harnesses": {},
        "git_version": "git version 2.47.0",
        "labels": ["darwin-arm64"],
        "secret_names": ["DEPLOY_TOKEN"],
        "projects": ["vowp"],
        "capacity": {
            "cpu_cores": 8,
            "load1": 0.25,
            "mem_total_bytes": 17179869184_u64,
            "mem_free_bytes": 8589934592_u64,
            "disk_free_bytes": 107374182400_u64,
            "busy_slots": 0,
            "max_slots": 4,
            "on_battery": null,
            "thermal_state": "nominal"
        }
    });
    store
        .touch_node(&name, now, Some("0.3.0"), Some(AGENT_PROTOCOL), &capabilities)
        .expect("heartbeat");

    let (code, value, stderr) = env.run_json(&["node", "show", "mac-studio"]);
    assert_eq!(code, 0, "node show failed: {stderr}");
    assert_eq!(value["name"], "mac-studio");
    assert_eq!(value["kind"], "agent");
    assert_eq!(value["status"], "active");
    assert_eq!(value["presence"], "online");
    assert_eq!(value["labels"][0], "darwin-arm64");
    assert_eq!(value["mapped"][0], "vowp");
    assert_eq!(value["agent_version"], "0.3.0");
    assert_eq!(value["protocol"], AGENT_PROTOCOL);
    assert_eq!(value["capacity"]["cpu_cores"], 8);
    assert_eq!(value["capacity"]["max_slots"], 4);
    assert!(value["last_seen_at"].is_string());
    let text = serde_json::to_string(&value).expect("json text");
    assert!(!text.contains("/Users/owner/private"), "path map leaked: {text}");
    assert!(!text.contains("DEPLOY_TOKEN"), "secret name leaked: {text}");
    assert!(!stderr.contains("/Users/owner/private"), "path map leaked: {stderr}");
}

#[test]
fn node_enroll_is_home_only() {
    let home = TempDir::new().expect("temp home");
    let control = TempDir::new().expect("temp control");
    let created_at = Timestamp::parse("2026-11-14T10:00:00Z").expect("timestamp");
    let config = ControlConfig {
        role: "control".to_owned(),
        home: "home-1".to_owned(),
        ssh_target: "home.example".to_owned(),
        remote_home: "/home/dev/.vowp".to_owned(),
        created_at,
    };
    std::fs::write(
        control.path().join("control.json"),
        serde_json::to_string(&config).expect("control JSON"),
    )
    .expect("write control.json");

    let output = ProcessCommand::new(env!("CARGO_BIN_EXE_vowp"))
        .args(["node", "enroll", "mac-studio"])
        .env("HOME", home.path())
        .env("VOWP_CONTROL_HOME", control.path())
        .env_remove("VOWP_HOME")
        .env("NO_COLOR", "1")
        .output()
        .expect("run vowp");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(5),
        "a control machine is refused, not routed: {stderr}"
    );
    assert!(stderr.contains("home"), "{stderr}");
    assert!(
        !home.path().join(".vowp/vowp.db").exists(),
        "the control machine must not create a home database"
    );
}

/// Creates a temporary home with an active `mac-studio` node and one idle session.
fn seed_pairing_env() -> (TestEnv, NodeName, SessionId) {
    let env = TestEnv::new();
    let store = env.store();
    let now = store.clock().now();
    let node = NodeName::parse("mac-studio").expect("node slug");
    store.insert_pending_node(&node, &[], now).expect("pending node");
    store
        .set_node_status(&node, NodeStatus::Active, "", now)
        .expect("active node");
    let session = SessionId::new();
    store
        .upsert_session(&Session {
            id: session.clone(),
            project: Some(env.config.project.name.clone()),
            job_id: None,
            role: Some(SessionRole::User),
            title: "surface pairing".to_owned(),
            parent_session_id: None,
            node: node.clone(),
            cwd: "/tmp/vowp".to_owned(),
            state: SessionState::Idle,
            executor: None,
            last_seq: 0,
            last_capsule_id: None,
            log_bytes: 0,
            created_at: now,
            updated_at: now,
        })
        .expect("session");
    (env, node, session)
}

fn pair_surface(env: &TestEnv, session: &SessionId) -> serde_json::Value {
    let (code, value, stderr) = env.run_json(&[
        "surface",
        "pair",
        "--session",
        session.as_ref(),
        "--node",
        "mac-studio",
        "--kind",
        "opencode",
        "--app",
        "opencode",
    ]);
    assert_eq!(code, 0, "surface pair failed: {stderr}");
    value
}

#[test]
fn agent_pair_help_names_code_surface_and_session() {
    let mut io = BufferIo::new("");
    let code = vowp_cli::run(
        vec![
            "vowp".to_owned(),
            "agent".to_owned(),
            "pair".to_owned(),
            "--help".to_owned(),
        ],
        &mut io,
    );
    assert_eq!(code, 0, "help exits 0; stderr: {}", io.stderr());
    let help = io.stdout();
    assert!(help.contains("--code-file"), "help must name --code-file: {help}");
    assert!(help.contains("--surface"), "help must name --surface: {help}");
    assert!(help.contains("--session"), "help must name --session: {help}");
}

#[test]
fn agent_pair_accepts_code_file_stdin() {
    let cli = Cli::try_parse_from([
        "vowp",
        "agent",
        "pair",
        "--code-file",
        "-",
        "--surface",
        "01ARZ3NDEKTSV4RRFFQ69G5FAW",
        "--session",
        "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "--app",
        "opencode",
    ])
    .expect("parse agent pair");
    match cli.command {
        Command::Agent {
            command: AgentCommand::Pair(args),
        } => {
            assert_eq!(args.code_file, "-");
            assert_eq!(args.surface, "01ARZ3NDEKTSV4RRFFQ69G5FAW");
            assert_eq!(args.session, "01ARZ3NDEKTSV4RRFFQ69G5FAV");
            assert_eq!(args.app.as_deref(), Some("opencode"));
        }
        other => panic!("expected agent pair, got {other:?}"),
    }
}

#[test]
fn agent_pair_without_enrollment_exits_three_and_prints_no_code() {
    let home = TempDir::new().expect("temp home");
    let code = "A".repeat(52);
    let code_file = home.path().join("pair.code");
    std::fs::write(&code_file, &code).expect("code file");
    let code_file = code_file.display().to_string();

    let (status, stdout, stderr) = run_binary(
        home.path(),
        &[
            "agent",
            "pair",
            "--code-file",
            &code_file,
            "--surface",
            "01ARZ3NDEKTSV4RRFFQ69G5FAW",
            "--session",
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "--app",
            "opencode",
        ],
    );
    assert_eq!(status, 3, "no enrollment is exit 3; stderr: {stderr}");
    assert!(stdout.is_empty(), "the failure prints no document: {stdout}");
    assert!(!stderr.contains(&code), "the code must never be printed: {stderr}");
}

#[test]
fn surface_pair_cli_prints_code_once_and_lists_pending() {
    let (env, node, session) = seed_pairing_env();
    let value = pair_surface(&env, &session);

    let code = value["code"].as_str().expect("code is printed once");
    assert_eq!(code.len(), 52, "the pairing code is 52 characters");
    assert_eq!(value["node"], "mac-studio");
    assert_eq!(value["session"], session.as_str());
    assert_eq!(value["kind"], "opencode");
    assert_eq!(value["app"], "opencode");
    let surface_id = value["surface"].as_str().expect("surface id").to_owned();
    assert_eq!(surface_id.len(), 26);
    let command = value["command"].as_str().expect("Mac command");
    assert!(command.contains("vowp agent pair --code-file -"), "{command}");
    assert!(command.contains("--surface"), "{command}");
    assert!(command.contains("--session"), "{command}");
    assert!(command.contains("--app opencode"), "{command}");
    assert!(
        !command.contains(code),
        "the code is not part of the command: {command}"
    );

    let (status, stdout, stderr) = env.run(&[
        "surface",
        "pair",
        "--session",
        session.as_ref(),
        "--node",
        "mac-studio",
        "--kind",
        "opencode",
        "--app",
        "opencode",
    ]);
    assert_eq!(status, 4, "a duplicate pending pairing is a conflict: {stderr}");
    assert!(stdout.is_empty(), "the conflict prints no document: {stdout}");
    assert!(
        !stderr.contains(code),
        "the code must never be written to stderr: {stderr}"
    );

    // A different kind is a separate surface and prints the human code block exactly once.
    let (status, stdout, stderr) = env.run(&[
        "surface",
        "pair",
        "--session",
        session.as_ref(),
        "--node",
        "mac-studio",
        "--kind",
        "chat",
    ]);
    assert_eq!(status, 0, "{stderr}");
    assert_eq!(stdout.matches("code:").count(), 1, "the code is printed once: {stdout}");
    assert!(stdout.contains("vowp agent pair --code-file -"), "{stdout}");

    let store = env.store();
    let id = SurfaceId(surface_id.clone());
    let surface = store.surface_by_id(&id).expect("surface lookup").expect("surface row");
    assert_eq!(surface.node, node);
    assert_eq!(surface.state, SurfaceState::Pending);
    assert_eq!(surface.app.as_deref(), Some("opencode"));
    assert_eq!(surface.metadata["session"], session.as_str());
    let database = std::fs::read(env.home_path().join("vowp.db")).expect("database bytes");
    assert!(
        !database.windows(code.len()).any(|window| window == code.as_bytes()),
        "the plaintext code must not be stored in the database"
    );

    let (status, list, stderr) = env.run_json(&["surface", "ls"]);
    assert_eq!(status, 0, "surface ls failed: {stderr}");
    let entries = list.as_array().expect("surface ls returns an array");
    let entry = entries
        .iter()
        .find(|entry| entry["id"] == surface_id)
        .expect("the pending surface is listed");
    assert_eq!(entry["node"], "mac-studio");
    assert_eq!(entry["kind"], "opencode");
    assert_eq!(entry["state"], "pending");
    assert_eq!(entry["expires_at"], serde_json::Value::Null);
    let text = serde_json::to_string(&list).expect("json text");
    assert!(!text.contains(code), "surface ls must never print the code: {text}");
}

#[test]
fn surface_revoke_marks_the_token_revoked() {
    let (env, node, session) = seed_pairing_env();
    let value = pair_surface(&env, &session);
    let code = value["code"].as_str().expect("code");
    let surface_id = SurfaceId(value["surface"].as_str().expect("surface id").to_owned());
    let store = env.store();
    let now = store.clock().now();
    let issued = issue_surface_token(&store, &node, &surface_id, &session, "opencode", code, now).expect("issue token");
    assert!(verify_surface_token(&store, &issued.token, now).is_ok());

    let (status, _stdout, stderr) = env.run(&["surface", "revoke", &surface_id.0]);
    assert_eq!(status, 5, "revoke without --yes is refused: {stderr}");
    assert_eq!(
        store.surface_by_id(&surface_id).expect("lookup").expect("row").state,
        SurfaceState::Attached
    );

    let (status, value, stderr) = env.run_json(&["surface", "revoke", &surface_id.0, "--yes"]);
    assert_eq!(status, 0, "surface revoke failed: {stderr}");
    assert_eq!(value["surface"], surface_id.0);
    assert_eq!(value["status"], "revoked");
    assert_eq!(value["tokens_revoked"], 1);
    assert_eq!(
        store.surface_by_id(&surface_id).expect("lookup").expect("row").state,
        SurfaceState::Revoked
    );
    let tokens = store.list_surface_tokens(&surface_id).expect("tokens");
    assert_eq!(tokens.len(), 1);
    assert!(tokens[0].revoked_at.is_some(), "the token row is revoked");
    assert!(verify_surface_token(&store, &issued.token, now).is_err());

    let (status, _stdout, stderr) = env.run(&["surface", "revoke", "01ARZ3NDEKTSV4RRFFQ69G5FAZ", "--yes"]);
    assert_eq!(status, 3, "an unknown surface is not found: {stderr}");
}

#[test]
fn surface_command_requires_a_node_and_session() {
    let (env, _node, session) = seed_pairing_env();
    let session_text = session.to_string();

    let (status, _stdout, stderr) = env.run(&["surface", "pair", "--session", &session_text, "--kind", "opencode"]);
    assert_eq!(status, 2, "a missing --node is invalid input: {stderr}");
    assert!(stderr.contains("--node"), "{stderr}");

    let (status, _stdout, _stderr) = env.run(&["surface", "pair", "--node", "mac-studio", "--kind", "opencode"]);
    assert_eq!(status, 2, "a missing --session is a usage error");

    let (status, _stdout, stderr) = env.run(&[
        "surface",
        "pair",
        "--session",
        &session_text,
        "--node",
        "missing-node",
        "--kind",
        "opencode",
    ]);
    assert_eq!(status, 3, "an unknown node is not found: {stderr}");

    let (status, _stdout, stderr) = env.run(&[
        "surface",
        "pair",
        "--session",
        "01ARZ3NDEKTSV4RRFFQ69G5FAZ",
        "--node",
        "mac-studio",
        "--kind",
        "opencode",
    ]);
    assert_eq!(status, 3, "an unknown session is not found: {stderr}");

    let (status, _stdout, stderr) = env.run(&[
        "surface",
        "pair",
        "--session",
        &session_text,
        "--node",
        "mac-studio",
        "--kind",
        "unknown",
    ]);
    assert_eq!(status, 2, "an unknown kind is invalid input: {stderr}");
}

#[test]
fn surface_ls_never_prints_a_token() {
    let (env, node, session) = seed_pairing_env();
    let value = pair_surface(&env, &session);
    let code = value["code"].as_str().expect("code");
    let surface_id = SurfaceId(value["surface"].as_str().expect("surface id").to_owned());
    let store = env.store();
    let now = store.clock().now();
    let issued = issue_surface_token(&store, &node, &surface_id, &session, "opencode", code, now).expect("issue token");

    let (status, stdout, stderr) = env.run(&["surface", "ls"]);
    assert_eq!(status, 0, "surface ls failed: {stderr}");
    assert!(stdout.contains(&surface_id.0), "{stdout}");
    assert!(stdout.contains("attached"), "{stdout}");
    assert!(stdout.contains(&format!("session.read:{session}")), "{stdout}");
    assert!(
        !stdout.contains(&issued.token),
        "the token must never be printed: {stdout}"
    );
    assert!(
        !stderr.contains(&issued.token),
        "the token must never reach stderr: {stderr}"
    );

    let (status, value, stderr) = env.run_json(&["surface", "ls"]);
    assert_eq!(status, 0, "surface ls --json failed: {stderr}");
    let entry = value
        .as_array()
        .expect("array")
        .iter()
        .find(|entry| entry["id"] == surface_id.0)
        .expect("the attached surface is listed");
    assert_eq!(entry["state"], "attached");
    assert_eq!(entry["scopes"][0], format!("session.read:{session}"));
    assert!(entry["expires_at"].is_string());
    let text = serde_json::to_string(&value).expect("json text");
    assert!(!text.contains(&issued.token), "the token must never be in JSON: {text}");
    assert!(
        !text.contains("\"token\""),
        "the JSON document has no token field: {text}"
    );

    let (status, value, stderr) = env.run_json(&["surface", "ls", "--node", "mac-studio"]);
    assert_eq!(status, 0, "{stderr}");
    assert_eq!(value.as_array().expect("array").len(), 1);
    let (status, value, stderr) = env.run_json(&["surface", "ls", "--node", "other-node"]);
    assert_eq!(status, 0, "{stderr}");
    assert!(value.as_array().expect("array").is_empty());
}

#[test]
fn surface_commands_are_home_only() {
    let home = TempDir::new().expect("temp home");
    let control = TempDir::new().expect("temp control");
    let created_at = Timestamp::parse("2026-11-14T10:00:00Z").expect("timestamp");
    let config = ControlConfig {
        role: "control".to_owned(),
        home: "home-1".to_owned(),
        ssh_target: "home.example".to_owned(),
        remote_home: "/home/dev/.vowp".to_owned(),
        created_at,
    };
    std::fs::write(
        control.path().join("control.json"),
        serde_json::to_string(&config).expect("control JSON"),
    )
    .expect("write control.json");

    let output = ProcessCommand::new(env!("CARGO_BIN_EXE_vowp"))
        .args(["surface", "ls"])
        .env("HOME", home.path())
        .env("VOWP_CONTROL_HOME", control.path())
        .env_remove("VOWP_HOME")
        .env("NO_COLOR", "1")
        .output()
        .expect("run vowp");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(5),
        "a control machine is refused, not routed: {stderr}"
    );
    assert!(stderr.contains("home"), "{stderr}");
    assert!(
        !home.path().join(".vowp/vowp.db").exists(),
        "the control machine must not create a home database"
    );
}

// ---------------------------------------------------------------------------
// The Mac assignment executor (`CoreRunHandler`)
// ---------------------------------------------------------------------------

/// A fake Claude Code adapter: it answers `--version` and then emits the JSONL body.
const ADAPTER_BODY: &str = r#"
printf '%s\n' '{"type":"system","subtype":"init","session_id":"fake-native","model":"fake-model"}'
printf '%s\n' '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hello from the fake adapter"}]},"session_id":"fake-native"}'
printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"finished ok","total_cost_usd":0.01,"usage":{"input_tokens":3,"output_tokens":4}}'
exit "${FAKE_EXIT:-0}"
"#;

/// Runs one git command in `cwd` and returns its stdout; a failure fails the test.
fn git(cwd: &Path, args: &[&str]) -> String {
    let output = ProcessCommand::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} in {} failed: {}",
        cwd.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Returns whether one git command exited successfully.
fn git_ok(cwd: &Path, args: &[&str]) -> bool {
    ProcessCommand::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn ts() -> Timestamp {
    Timestamp::parse("2026-11-14T10:00:00Z").expect("timestamp")
}

/// One hermetic checkout, bare origin, worktree root, fake adapter and agent state.
struct ExecFixture {
    _home: TempDir,
    root: PathBuf,
    checkout: PathBuf,
    origin: PathBuf,
    worktrees: PathBuf,
    state_dir: PathBuf,
    script: PathBuf,
    base: String,
    config: AgentConfig,
}

impl ExecFixture {
    /// Creates the fixture around one fake adapter body.
    fn new(body: &str) -> Self {
        let home = TempDir::new().expect("temp home");
        let root = std::fs::canonicalize(home.path()).expect("canonical home");
        let origin = root.join("origin.git");
        git(&root, &["init", "-q", "--bare", origin.to_str().expect("utf-8 origin")]);
        let checkout = root.join("checkout");
        std::fs::create_dir_all(&checkout).expect("checkout dir");
        git(
            &root,
            &["init", "-q", "-b", "main", checkout.to_str().expect("utf-8 checkout")],
        );
        git(&checkout, &["config", "user.email", "test@example.invalid"]);
        git(&checkout, &["config", "user.name", "VOWP Test"]);
        std::fs::write(checkout.join("a.txt"), "base\n").expect("base file");

        let script = root.join("fake-claude.sh");
        let text =
            format!("#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  printf '1.2.3\\n'\n  exit 0\nfi\n{body}\n");
        std::fs::write(&script, text).expect("fake adapter");
        let mut permissions = std::fs::metadata(&script).expect("fake adapter stat").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).expect("fake adapter mode");

        let project_toml = format!(
            "[project]\nname = \"vowp\"\nremote = \"origin\"\n\n\
             [harnesses.claude-code]\nbinary = \"{}\"\n\n\
             [profiles.planner]\nladder = [\"claude-code:fake-model\"]\n\n\
             [profiles.keeper]\nladder = [\"claude-code:fake-model\"]\n\n\
             [profiles.worker]\nladder = [\"claude-code:fake-model\"]\n\n\
             [profiles.verifier]\nladder = [\"claude-code:fake-model\"]\n",
            script.display()
        );
        std::fs::write(checkout.join("vowp.toml"), project_toml).expect("vowp.toml");
        git(&checkout, &["add", "-A"]);
        git(&checkout, &["commit", "-q", "-m", "base"]);
        let base = git(&checkout, &["rev-parse", "HEAD"]).trim().to_owned();
        git(
            &checkout,
            &["remote", "add", "origin", origin.to_str().expect("utf-8 origin")],
        );
        git(&checkout, &["push", "-q", "origin", "main"]);
        git(&origin, &["symbolic-ref", "HEAD", "refs/heads/main"]);

        let worktrees = root.join("worktrees");
        std::fs::create_dir_all(&worktrees).expect("worktree root");
        let state_dir = root.join(".vowp-agent");
        std::fs::create_dir_all(&state_dir).expect("agent state dir");
        let project = ProjectName::parse("vowp").expect("project");
        let map = AgentPathMap {
            project,
            local_root: checkout.to_str().expect("utf-8 checkout").to_owned(),
            worktree_root: Some(worktrees.to_str().expect("utf-8 worktrees").to_owned()),
            allow: Vec::new(),
        };
        let config = AgentConfig::new(
            "home-1",
            NodeName::parse("mac-studio").expect("node"),
            state_dir.clone(),
            "home.example",
        )
        .with_path_maps(vec![map]);
        config.write().expect("agent.json");
        Self {
            _home: home,
            root,
            checkout,
            origin,
            worktrees,
            state_dir,
            script,
            base,
            config,
        }
    }

    fn handler(&self, environment: BTreeMap<String, String>) -> CoreRunHandler {
        CoreRunHandler::new(self.config.clone())
            .with_reserve(0)
            .with_env(environment)
    }

    fn base_env(&self) -> BTreeMap<String, String> {
        BTreeMap::from([("FAKE_EXIT".to_owned(), "0".to_owned())])
    }

    fn flow_env(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("FAKE_EXIT".to_owned(), "0".to_owned()),
            ("VOWP_TEST_ENV".to_owned(), "environment-value-to-hide".to_owned()),
            ("VOWP_TEST_SECRET".to_owned(), "secret-value-to-hide".to_owned()),
        ])
    }

    fn assignment(&self, job: &str) -> AssignmentPayload {
        let project = ProjectName::parse("vowp").expect("project");
        let job = JobId::parse(job).expect("job");
        AssignmentPayload {
            assignment_id: format!("assignment-{job}"),
            run_id: RunId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAX").expect("run id"),
            session_id: SessionId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAV").expect("session"),
            project: project.clone(),
            job: job.clone(),
            executor: Executor::parse("claude-code:fake-model").expect("executor"),
            purpose: RunPurpose::Start,
            prompt: "implement the test job".to_owned(),
            cwd_ref: PathRef::Worktree { project, job },
            base_commit: self.base.clone(),
            remote: "origin".to_owned(),
            remote_url: String::new(),
            result_transport: vowp_protocol::ResultTransport::Push,
            env_names: vec!["FAKE_EXIT".to_owned()],
            secret_names: Vec::new(),
            epoch: 1,
            deadline: None,
            spool_cap_bytes: 2 * 1024 * 1024,
        }
    }

    fn worktree(&self, job: &str) -> PathBuf {
        self.worktrees.join(job)
    }

    fn head(&self, cwd: &Path) -> String {
        git(cwd, &["rev-parse", "HEAD"]).trim().to_owned()
    }
}

/// A run sink that records lines, states and the final summary in memory.
#[derive(Default)]
struct RecordingSink {
    events: Vec<RemoteEvent>,
    states: Vec<(RemoteRunState, String)>,
    summary: Option<RunSummary>,
    stopped: bool,
}

impl RunSink for RecordingSink {
    fn line(&mut self, line: RemoteLine) -> Result<(), RunSinkError> {
        if let RemoteLine::Event { event } = line {
            self.events.push(event);
        }
        Ok(())
    }

    fn state(&mut self, state: RemoteRunState, detail: &str) -> Result<(), RunSinkError> {
        self.states.push((state, detail.to_owned()));
        Ok(())
    }

    fn finish(&mut self, summary: RunSummary) -> Result<(), RunSinkError> {
        self.summary = Some(summary);
        Ok(())
    }

    fn is_stopped(&self) -> bool {
        self.stopped
    }

    fn spool_bytes(&self) -> u64 {
        0
    }
}

/// Accepts and runs one assignment synchronously, returning the recording sink.
fn run_assignment(handler: &CoreRunHandler, assignment: &AssignmentPayload) -> RecordingSink {
    let accepted = handler.accept(assignment).expect("assignment is accepted");
    let mut sink = RecordingSink::default();
    handler.run(accepted, &mut sink);
    sink
}

/// A fixed agent clock for the fake gateway tests.
struct FixedClock(Timestamp);

impl AgentClock for FixedClock {
    fn now(&self) -> Timestamp {
        self.0
    }

    fn sleep(&self, _duration: Duration) {}
}

#[derive(Default)]
struct GatewayState {
    received: Vec<AgentFrame>,
    summary: Option<TurnSummary>,
}

/// One in-memory gateway: it accepts `AssignRun`, acknowledges every event and stops on
/// the terminal summary.
#[derive(Clone)]
struct FakeGateway {
    shared: Arc<Mutex<GatewayState>>,
    assignment: AssignmentPayload,
}

impl FakeGateway {
    fn new(assignment: AssignmentPayload) -> Self {
        Self {
            shared: Arc::new(Mutex::new(GatewayState::default())),
            assignment,
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, GatewayState> {
        self.shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Transport for FakeGateway {
    fn open(&mut self) -> Result<Box<dyn FrameStream>, AgentError> {
        Ok(Box::new(FakeStream {
            shared: Arc::clone(&self.shared),
            assignment: self.assignment.clone(),
            outbound: VecDeque::new(),
            seq: 1,
        }))
    }
}

struct FakeStream {
    shared: Arc<Mutex<GatewayState>>,
    assignment: AssignmentPayload,
    outbound: VecDeque<AgentFrame>,
    seq: u64,
}

impl FakeStream {
    fn next_seq(&mut self) -> u64 {
        let seq = self.seq;
        self.seq += 1;
        seq
    }
}

impl FrameStream for FakeStream {
    fn recv(&mut self, _timeout: Option<Duration>) -> Result<FrameRead, AgentError> {
        match self.outbound.pop_front() {
            Some(frame) => Ok(FrameRead::Frame(frame)),
            None => Ok(FrameRead::Timeout),
        }
    }

    fn send(&mut self, frame: &AgentFrame) -> Result<(), AgentError> {
        self.shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .received
            .push(frame.clone());
        match frame {
            AgentFrame::AgentHello { .. } => {
                self.outbound.push_back(AgentFrame::HomeChallenge {
                    nonce: "nonce-1".to_owned(),
                    home_id: "home-1".to_owned(),
                    connection_id: "connection-1".to_owned(),
                    server_time: ts(),
                    heartbeat_secs: 10,
                    max_frame_bytes: 1024 * 1024,
                });
            }
            AgentFrame::AgentAuth { .. } => {
                self.outbound.push_back(AgentFrame::HomeWelcome {
                    protocol: AGENT_PROTOCOL.to_owned(),
                    home_id: "home-1".to_owned(),
                    node: NodeName::parse("mac-studio").expect("node"),
                    connection_id: "connection-1".to_owned(),
                });
                self.outbound.push_back(AgentFrame::AssignRun {
                    assignment: self.assignment.clone(),
                    lease_token: "lease-token-for-the-fake-gateway".to_owned(),
                    lease_expires_at: ts().plus(Duration::from_secs(600)),
                });
            }
            AgentFrame::RemoteLine {
                run_id,
                line: RemoteLine::Event { event },
                ..
            } => {
                let seq = self.next_seq();
                self.outbound.push_back(AgentFrame::IngestAck {
                    run_id: run_id.clone(),
                    event_id: Some(event.id.clone()),
                    seq: Some(seq),
                    duplicate: false,
                    lease_revoked: None,
                    error: None,
                });
            }
            AgentFrame::TurnEnd { run_id, summary } => {
                self.shared
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .summary = Some(summary.clone());
                let seq = self.next_seq();
                self.outbound.push_back(AgentFrame::IngestAck {
                    run_id: run_id.clone(),
                    event_id: None,
                    seq: Some(seq),
                    duplicate: false,
                    lease_revoked: None,
                    error: None,
                });
                self.outbound.push_back(AgentFrame::Revoked {
                    reason: "test complete".to_owned(),
                });
            }
            AgentFrame::Ping => self.outbound.push_back(AgentFrame::Pong),
            _ => {}
        }
        Ok(())
    }
}

#[test]
fn agent_run_creates_a_worktree_and_streams_adapter_events() {
    let fixture = ExecFixture::new(ADAPTER_BODY);
    let handler = Arc::new(fixture.handler(fixture.base_env()));
    let assignment = fixture.assignment("agent-exec-stream");
    let gateway = FakeGateway::new(assignment.clone());

    let state_dir = fixture.state_dir.clone();
    let config = AgentConfig::load(&state_dir).expect("agent config");
    let key = NodeKey::load_or_generate(&state_dir).expect("node key");
    let mut client = AgentClient::new(
        state_dir,
        key,
        config,
        Box::new(gateway.clone()),
        Arc::new(FixedClock(ts())),
    );
    let outcome = client
        .run_with_handler(&RunOptions::new(AgentRole::Executor), handler)
        .expect("run loop");
    assert!(
        matches!(outcome, vowp_agent::RunOutcome::Revoked { .. }),
        "the fake gateway revokes after the summary: {outcome:?}"
    );

    let state = gateway.state();
    assert!(
        state
            .received
            .iter()
            .any(|frame| matches!(frame, AgentFrame::RunAccepted { .. })),
        "the gateway accepted AssignRun: {:?}",
        state.received
    );
    let summary = state.summary.as_ref().expect("terminal summary");
    assert_eq!(summary.outcome, RunOutcome::Ok);
    assert_eq!(summary.exit_code, Some(0));
    assert_eq!(summary.native_session_id.as_deref(), Some("fake-native"));
    assert_eq!(summary.result_text.as_deref(), Some("finished ok"));
    assert_eq!(summary.cost.usd, Some(0.01));
    let mut event_ids = std::collections::BTreeSet::new();
    let mut event_texts = Vec::new();
    for frame in &state.received {
        if let AgentFrame::RemoteLine {
            line: RemoteLine::Event { event },
            ..
        } = frame
        {
            assert!(
                event_ids.insert(event.id.as_str().to_owned()),
                "event ids are unique: {}",
                event.id
            );
            event_texts.push(event.payload.to_string());
        }
    }
    assert!(
        event_texts
            .iter()
            .any(|text| text.contains("hello from the fake adapter")),
        "adapter events reached the gateway: {event_texts:?}"
    );

    let worktree = fixture.worktree("agent-exec-stream");
    assert!(worktree.is_dir(), "the job worktree exists");
    assert_eq!(
        git(&worktree, &["symbolic-ref", "--short", "HEAD"]).trim(),
        "job/agent-exec-stream"
    );
    let head = fixture.head(&worktree);
    assert!(!head.is_empty(), "the worktree has a head commit");
}

#[test]
fn agent_run_pushes_only_the_job_branch_and_reports_the_head() {
    let fixture = ExecFixture::new(ADAPTER_BODY);
    let handler = fixture.handler(fixture.base_env());
    let mut assignment = fixture.assignment("agent-exec-push");
    assignment.remote_url = fixture.origin.to_str().expect("utf-8 origin").to_owned();

    let sink = run_assignment(&handler, &assignment);
    let summary = sink.summary.expect("summary");
    let head = fixture.head(&fixture.worktree("agent-exec-push"));
    assert_eq!(summary.head_commit.as_deref(), Some(head.as_str()));
    match summary.push_status {
        Some(PushStatus::Pushed {
            branch,
            remote,
            head_commit,
        }) => {
            assert_eq!(branch, "job/agent-exec-push");
            assert_eq!(remote, "origin");
            assert_eq!(head_commit, head);
        }
        other => panic!("expected a pushed job branch, got {other:?}"),
    }
    assert_eq!(
        git(&fixture.origin, &["rev-parse", "refs/heads/job/agent-exec-push"])
            .trim()
            .to_owned(),
        head
    );
    assert_eq!(
        git(&fixture.origin, &["rev-parse", "refs/heads/main"])
            .trim()
            .to_owned(),
        fixture.base,
        "the default branch is untouched"
    );
    assert!(git(&fixture.origin, &["tag"]).trim().is_empty(), "no tag is pushed");
}

#[test]
fn agent_run_refuses_a_missing_env_name_and_redacts_injected_values() {
    let fixture = ExecFixture::new(
        r#"
printf '%s\n' '{"type":"system","subtype":"init","session_id":"fake-native","model":"fake-model"}'
printf '%s\n' "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"values ${VOWP_TEST_ENV} and ${VOWP_TEST_SECRET}\"}]},\"session_id\":\"fake-native\"}"
printf '%s\n' "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"done ${VOWP_TEST_ENV} ${VOWP_TEST_SECRET}\",\"total_cost_usd\":0}"
exit 0
"#,
    );
    let handler = fixture.handler(fixture.flow_env());

    let mut missing = fixture.assignment("agent-exec-missing-env");
    missing.env_names.push("VOWP_TEST_MISSING".to_owned());
    let rejected = handler.accept(&missing).expect_err("a missing name is refused");
    assert_eq!(rejected.reason, RunRejectReason::EnvMissing);
    assert!(
        rejected.detail.contains("VOWP_TEST_MISSING"),
        "the refusal names the missing variable: {}",
        rejected.detail
    );
    assert!(
        !rejected
            .detail
            .contains(fixture.checkout.to_str().expect("utf-8 checkout")),
        "the refusal carries no absolute path: {}",
        rejected.detail
    );

    let mut assignment = fixture.assignment("agent-exec-redact");
    assignment.env_names.push("VOWP_TEST_ENV".to_owned());
    assignment
        .secret_names
        .push(SecretName::parse("VOWP_TEST_SECRET").expect("secret name"));
    let sink = run_assignment(&handler, &assignment);
    let serialized = serde_json::to_string(&sink.events).expect("events JSON");
    assert!(
        !serialized.contains("environment-value-to-hide") && !serialized.contains("secret-value-to-hide"),
        "injected values never reach the sink: {serialized}"
    );
    assert!(serialized.contains("[REDACTED:VOWP_TEST_ENV]"), "{serialized}");
    assert!(serialized.contains("[REDACTED:VOWP_TEST_SECRET]"), "{serialized}");
    let summary = sink.summary.expect("summary");
    let result = summary.result_text.clone().unwrap_or_default();
    assert!(!result.contains("environment-value-to-hide"), "{result}");
    assert!(!result.contains("secret-value-to-hide"), "{result}");
    assert!(result.contains("[REDACTED:VOWP_TEST_ENV]"), "{result}");
    assert!(result.contains("[REDACTED:VOWP_TEST_SECRET]"), "{result}");
}

#[test]
fn agent_run_keeps_the_worktree_on_push_failure() {
    let fixture = ExecFixture::new(ADAPTER_BODY);
    let handler = fixture.handler(fixture.base_env());
    let mut assignment = fixture.assignment("agent-exec-push-failure");
    assignment.remote_url = fixture
        .root
        .join("another-origin.git")
        .to_str()
        .expect("utf-8")
        .to_owned();

    let sink = run_assignment(&handler, &assignment);
    let summary = sink.summary.expect("summary");
    match summary.push_status {
        Some(PushStatus::Failed { detail }) => {
            assert!(
                detail.contains("origin") || detail.contains("push"),
                "the typed failure explains the push: {detail}"
            );
        }
        other => panic!("expected a failed push, got {other:?}"),
    }
    assert_eq!(summary.outcome, RunOutcome::Error);
    assert!(
        fixture.worktree("agent-exec-push-failure").is_dir(),
        "a failed push keeps the worktree for inspection"
    );
    assert!(
        git_ok(
            &fixture.checkout,
            &[
                "show-ref",
                "--verify",
                "--quiet",
                "refs/heads/job/agent-exec-push-failure"
            ]
        ),
        "the job branch is still local"
    );
}

#[test]
fn agent_run_cleans_up_after_a_terminal_summary() {
    let fixture = ExecFixture::new(ADAPTER_BODY);
    let handler = fixture.handler(fixture.base_env());
    let assignment = fixture.assignment("agent-exec-cleanup");
    let run = assignment.run_id.clone();
    let sink = run_assignment(&handler, &assignment);
    assert!(sink.summary.is_some(), "the summary exists before cleanup");
    let worktree = fixture.worktree("agent-exec-cleanup");
    assert!(worktree.is_dir());
    assert!(git_ok(
        &fixture.checkout,
        &["show-ref", "--verify", "--quiet", "refs/heads/job/agent-exec-cleanup"]
    ));

    handler.cleanup(&run);
    assert!(!worktree.exists(), "cleanup removes only the job worktree");
    assert!(
        !git_ok(
            &fixture.checkout,
            &["show-ref", "--verify", "--quiet", "refs/heads/job/agent-exec-cleanup"]
        ),
        "cleanup removes only the job branch"
    );
    assert!(fixture.checkout.is_dir(), "the main checkout stays");
}

#[test]
fn agent_run_cancel_stops_the_adapter() {
    let fixture = ExecFixture::new(
        r#"
printf '%s\n' '{"type":"system","subtype":"init","session_id":"fake-native","model":"fake-model"}'
printf '%s\n' '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"started"}]},"session_id":"fake-native"}'
printf '%s\n' "$$" > PLACEHOLDER
/bin/sleep 300
"#,
    );
    let started = fixture.root.join("adapter-started");
    let script = std::fs::read_to_string(&fixture.script).expect("fake adapter");
    std::fs::write(
        &fixture.script,
        script.replace("PLACEHOLDER", &started.display().to_string()),
    )
    .expect("rewrite fake adapter");
    let handler = Arc::new(fixture.handler(fixture.base_env()));
    let assignment = fixture.assignment("agent-exec-cancel");
    let run = assignment.run_id.clone();
    let accepted = handler.accept(&assignment).expect("accepted");

    let (sender, receiver) = mpsc::channel();
    let runner = Arc::clone(&handler);
    std::thread::spawn(move || {
        let mut sink = RecordingSink::default();
        runner.run(accepted, &mut sink);
        let _ = sender.send(sink);
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !started.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(started.exists(), "the fake adapter started");
    handler.cancel(&run);
    let sink = receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("the cancelled run returns promptly");
    let summary = sink.summary.expect("the cancelled run still reports a summary");
    assert_eq!(summary.result_text.as_deref(), Some("cancelled"));
    assert_eq!(summary.outcome, RunOutcome::Killed);
    assert_eq!(summary.stop_reason, StopReason::Interrupted);
    assert!(
        fixture.worktree("agent-exec-cancel").is_dir(),
        "cancel keeps the worktree"
    );
}

#[test]
fn agent_run_does_not_touch_the_main_checkout() {
    let fixture = ExecFixture::new(ADAPTER_BODY);
    std::fs::write(fixture.checkout.join("a.txt"), "dirty local change\n").expect("dirty file");
    std::fs::write(fixture.checkout.join("notes.txt"), "untracked\n").expect("untracked file");
    let before_head = fixture.head(&fixture.checkout);
    let before_status = git(&fixture.checkout, &["status", "--porcelain"]);

    let handler = fixture.handler(fixture.base_env());
    let assignment = fixture.assignment("agent-exec-main");
    let sink = run_assignment(&handler, &assignment);
    assert!(sink.summary.is_some());

    assert_eq!(fixture.head(&fixture.checkout), before_head, "HEAD is unchanged");
    assert_eq!(
        git(&fixture.checkout, &["status", "--porcelain"]),
        before_status,
        "the dirty working tree is unchanged"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.checkout.join("a.txt")).expect("dirty file"),
        "dirty local change\n"
    );
    assert!(fixture.worktree("agent-exec-main").is_dir());
    assert!(
        !fixture.worktree("agent-exec-main").starts_with(&fixture.checkout),
        "the worktree lives under the configured worktree root, not in the checkout"
    );
}

#[test]
fn agent_run_rejects_an_unmapped_project() {
    let fixture = ExecFixture::new(ADAPTER_BODY);
    let handler = fixture.handler(fixture.base_env());
    let mut assignment = fixture.assignment("agent-exec-unmapped");
    assignment.project = ProjectName::parse("another-project").expect("project");
    assignment.cwd_ref = PathRef::Project {
        project: assignment.project.clone(),
    };

    let rejected = handler.accept(&assignment).expect_err("an unmapped project is refused");
    assert_eq!(rejected.reason, RunRejectReason::ProjectUnmapped);
    assert!(
        !rejected
            .detail
            .contains(fixture.checkout.to_str().expect("utf-8 checkout")),
        "the refusal carries no absolute path: {}",
        rejected.detail
    );
    assert!(
        !fixture.worktree("agent-exec-unmapped").exists(),
        "a rejected assignment creates no worktree"
    );
}

// ---------------------------------------------------------------------------
// v0.4 CLI surface: WSS enrollment, service, gateway and surface UX
// ---------------------------------------------------------------------------

/// A fake `systemctl` that records every invocation and answers status queries.
struct FakeManager {
    _dir: TempDir,
    log: PathBuf,
}

impl FakeManager {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("fake manager dir");
        let log = dir.path().join("manager.log");
        let script = dir.path().join("systemctl");
        let body = r#"#!/bin/sh
printf '%s\n' "$*" >> "$FAKE_MANAGER_LOG"
if [ "$1" = "--version" ]; then exit 0; fi
if [ "$2" = "show" ]; then
  case "${FAKE_MANAGER_MODE-}" in
    loaded)
      printf 'LoadState=loaded\nActiveState=active\nMainPID=1234\nExecMainStatus=0\n'
      ;;
    stopped)
      printf 'LoadState=loaded\nActiveState=inactive\nMainPID=0\nExecMainStatus=1\n'
      ;;
    *)
      exit 1
      ;;
  esac
fi
exit 0
"#;
        std::fs::write(&script, body).expect("fake systemctl");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        Self { _dir: dir, log }
    }

    fn path(&self) -> &Path {
        self._dir.path()
    }

    fn envs(&self, mode: &str, xdg: &Path) -> Vec<(&'static str, String)> {
        vec![
            ("PATH", self.path().display().to_string()),
            ("FAKE_MANAGER_LOG", self.log.display().to_string()),
            ("FAKE_MANAGER_MODE", mode.to_owned()),
            ("XDG_CONFIG_HOME", xdg.display().to_string()),
        ]
    }

    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

/// One service unit path under the temporary `$XDG_CONFIG_HOME`.
#[cfg(target_os = "linux")]
fn service_unit_path(home: &Path) -> PathBuf {
    home.join(".config/systemd/user/vowp-agent.service")
}

/// Creates a private agent state directory for the service tests.
fn private_state_dir(home: &Path) -> PathBuf {
    let state = home.join(".vowp-agent");
    std::fs::create_dir_all(&state).expect("state dir");
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700)).expect("state mode");
    state
}

#[test]
fn agent_enroll_accepts_wss_and_carries_the_url_and_fingerprint() {
    let cli = Cli::try_parse_from([
        "vowp",
        "agent",
        "enroll",
        "--wss",
        "wss://home.example/agent/v1",
        "--code-file",
        "-",
    ])
    .expect("parse agent enroll --wss");
    match cli.command {
        Command::Agent {
            command: AgentCommand::Enroll(args),
        } => {
            assert!(args.target.is_none(), "the target is optional with --wss");
            assert_eq!(args.wss.as_deref(), Some("wss://home.example/agent/v1"));
            assert!(args.tls_fingerprint.is_none());
        }
        other => panic!("expected agent enroll, got {other:?}"),
    }

    let fingerprint = format!("sha256:{}", "a".repeat(64));
    let cli = Cli::try_parse_from([
        "vowp",
        "agent",
        "enroll",
        "--wss",
        "wss://home.example/agent/v1",
        "--tls-fingerprint",
        &fingerprint,
        "--code-file",
        "-",
    ])
    .expect("parse agent enroll --tls-fingerprint");
    match cli.command {
        Command::Agent {
            command: AgentCommand::Enroll(args),
        } => assert_eq!(args.tls_fingerprint.as_deref(), Some(fingerprint.as_str())),
        other => panic!("expected agent enroll, got {other:?}"),
    }

    assert_eq!(enrollment_transport_config(None, None).expect("ssh default"), None);
    assert_eq!(
        enrollment_transport_config(Some("wss://home.example/agent/v1"), None).expect("system mode"),
        Some(TransportConfig::Wss {
            url: "wss://home.example/agent/v1".to_owned(),
            tls: WssTlsMode::System,
            fingerprint: None,
        })
    );
    assert_eq!(
        enrollment_transport_config(Some("wss://home.example/agent/v1"), Some(&fingerprint)).expect("pin mode"),
        Some(TransportConfig::Wss {
            url: "wss://home.example/agent/v1".to_owned(),
            tls: WssTlsMode::Pin,
            fingerprint: Some(fingerprint),
        })
    );
    assert!(enrollment_transport_config(Some("http://home.example/agent/v1"), None).is_err());
    assert!(enrollment_transport_config(Some("wss://home.example/agent/v1"), Some("sha256:short")).is_err());
}

#[test]
fn agent_enroll_without_target_or_wss_exits_two() {
    let home = TempDir::new().expect("temp home");
    let (code, _stdout, stderr) = run_binary(home.path(), &["agent", "enroll", "--code-file", "-"]);
    assert_eq!(code, 2, "a missing transport is invalid input: {stderr}");
    assert!(stderr.contains("--target") || stderr.contains("--wss"), "{stderr}");
}

#[test]
fn agent_gateway_refuses_a_non_loopback_bind() {
    let home = TempDir::new().expect("temp home");
    let (code, _stdout, stderr) = run_binary(home.path(), &["agent", "gateway", "--listen", "0.0.0.0:0"]);
    assert_eq!(code, 5, "a public bind is refused: {stderr}");
    assert!(stderr.contains("loopback"), "{stderr}");
}

/// One spawned `agent gateway --listen` process, killed if the test panics.
struct GatewayProcess(Child);

impl Drop for GatewayProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Sends one HTTP upgrade request and returns the response head and body.
fn http_upgrade(port: u16, path: &str, protocol: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to the gateway");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("read timeout");
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\
         Sec-WebSocket-Version: 13\r\nSec-WebSocket-Protocol: {protocol}\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).expect("write request");
    stream.flush().expect("flush request");
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    response
}

#[test]
fn agent_gateway_listen_refuses_other_routes_and_subprotocols() {
    let env = TestEnv::new();
    let mut child = ProcessCommand::new(env!("CARGO_BIN_EXE_vowp"))
        .arg("--home")
        .arg(env.home_path())
        .args(["agent", "gateway", "--listen", "127.0.0.1:0"])
        .env("HOME", env.home_path())
        .env("NO_COLOR", "1")
        .env_remove("VOWP_AGENT_HOME")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn agent gateway");
    let stdout = child.stdout.take().expect("gateway stdout");
    let mut guard = GatewayProcess(child);
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).expect("read gateway line");
    assert!(
        line.contains("ws://127.0.0.1:") && line.contains("/agent/v1"),
        "the gateway prints its bound address: {line:?}"
    );
    let address = line
        .split("ws://")
        .nth(1)
        .expect("bound address")
        .split('/')
        .next()
        .expect("host:port");
    let port: u16 = address
        .rsplit_once(':')
        .and_then(|(_, port)| port.parse().ok())
        .expect("gateway port");

    let wrong_path = http_upgrade(port, "/other", "vowp-agent/1");
    assert!(
        wrong_path.starts_with("HTTP/1.1 404"),
        "only /agent/v1 is served: {wrong_path:?}"
    );
    let wrong_protocol = http_upgrade(port, "/agent/v1", "other/1");
    assert!(
        wrong_protocol.starts_with("HTTP/1.1 400"),
        "the vowp-agent/1 subprotocol is required: {wrong_protocol:?}"
    );

    let _ = guard.0.kill();
    let _ = guard.0.wait();
}

#[cfg(target_os = "linux")]
#[test]
fn agent_service_install_print_writes_nothing_and_names_the_manager() {
    let home = TempDir::new().expect("temp home");
    private_state_dir(home.path());
    let manager = FakeManager::new();
    let xdg = home.path().join(".config");
    let envs = manager.envs("loaded", &xdg);
    let (code, stdout, stderr) = run_binary_with(home.path(), &["agent", "service", "install", "--print"], &envs);
    assert_eq!(code, 0, "service install --print failed: {stderr}");
    assert!(stdout.contains("[Service]"), "{stdout}");
    assert!(stdout.contains("ExecStart="), "{stdout}");
    assert!(stdout.contains("systemctl --user daemon-reload"), "{stdout}");
    assert!(
        stdout.contains("systemctl --user enable --now vowp-agent.service"),
        "{stdout}"
    );
    assert!(!service_unit_path(home.path()).exists(), "--print writes nothing");
    assert!(
        manager.log().is_empty(),
        "--print invokes no manager: {}",
        manager.log()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn agent_service_install_writes_one_unit_and_only_now_runs_the_manager() {
    let home = TempDir::new().expect("temp home");
    private_state_dir(home.path());
    let manager = FakeManager::new();
    let xdg = home.path().join(".config");
    let envs = manager.envs("loaded", &xdg);

    let (code, _stdout, stderr) = run_binary_with(home.path(), &["agent", "service", "install"], &envs);
    assert_eq!(code, 0, "service install failed: {stderr}");
    let unit = service_unit_path(home.path());
    assert!(unit.is_file(), "the unit is written at {}", unit.display());
    let mode = std::fs::metadata(&unit).expect("unit metadata").permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "the unit is private");
    let text = std::fs::read_to_string(&unit).expect("unit text");
    assert!(text.contains("generated by vowp agent service install"), "{text}");
    assert!(manager.log().is_empty(), "install without --now invokes no manager");

    let (code, _stdout, stderr) = run_binary_with(home.path(), &["agent", "service", "install", "--now"], &envs);
    assert_eq!(code, 0, "service install --now failed: {stderr}");
    let log = manager.log();
    assert!(log.contains("--user daemon-reload"), "{log}");
    assert!(log.contains("--user enable --now vowp-agent.service"), "{log}");

    std::fs::write(&unit, "[Service]\nDescription=someone else\n").expect("foreign unit");
    let (code, _stdout, stderr) = run_binary_with(home.path(), &["agent", "service", "install"], &envs);
    assert_eq!(code, 4, "a foreign unit is refused without --force: {stderr}");
    assert_eq!(
        std::fs::read_to_string(&unit).expect("foreign unit text"),
        "[Service]\nDescription=someone else\n",
        "the foreign file is not replaced"
    );
    let (code, _stdout, stderr) = run_binary_with(home.path(), &["agent", "service", "install", "--force"], &envs);
    assert_eq!(code, 0, "install --force replaces the foreign unit: {stderr}");
    assert!(
        std::fs::read_to_string(&unit)
            .expect("forced unit text")
            .contains("generated by vowp agent service install"),
        "the forced unit is VOWP's own text"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn agent_service_uninstall_removes_only_a_generated_unit_and_keeps_the_state() {
    let home = TempDir::new().expect("temp home");
    let state = private_state_dir(home.path());
    let token_dir = state.join("surfaces/01ARZ3NDEKTSV4RRFFQ69G5FAW");
    std::fs::create_dir_all(&token_dir).expect("surface dir");
    std::fs::write(token_dir.join("token"), "kept-token-placeholder").expect("token");
    let manager = FakeManager::new();
    let xdg = home.path().join(".config");
    let envs = manager.envs("loaded", &xdg);
    let (code, _stdout, stderr) = run_binary_with(home.path(), &["agent", "service", "install"], &envs);
    assert_eq!(code, 0, "install failed: {stderr}");
    let unit = service_unit_path(home.path());
    assert!(unit.is_file());

    let (code, _stdout, stderr) = run_binary_with(home.path(), &["agent", "service", "uninstall"], &envs);
    assert_eq!(code, 5, "uninstall without --yes is refused: {stderr}");
    assert!(unit.is_file(), "the refused uninstall keeps the unit");

    let (code, _stdout, stderr) = run_binary_with(home.path(), &["agent", "service", "uninstall", "--yes"], &envs);
    assert_eq!(code, 0, "uninstall failed: {stderr}");
    assert!(!unit.exists(), "the generated unit is removed");
    let log = manager.log();
    assert!(log.contains("--user disable --now vowp-agent.service"), "{log}");
    assert!(log.contains("--user daemon-reload"), "{log}");
    assert_eq!(
        std::fs::read_to_string(token_dir.join("token")).expect("token kept"),
        "kept-token-placeholder",
        "uninstall keeps every token"
    );

    std::fs::write(&unit, "[Unit]\nDescription=someone else\n").expect("foreign unit");
    let (code, _stdout, stderr) = run_binary_with(home.path(), &["agent", "service", "uninstall", "--yes"], &envs);
    assert_eq!(code, 4, "a foreign unit is not removed: {stderr}");
    assert!(unit.is_file(), "the foreign file stays");
}

#[cfg(target_os = "linux")]
#[test]
fn agent_service_status_reads_the_manager_and_reports_unavailable_json() {
    let home = TempDir::new().expect("temp home");
    private_state_dir(home.path());
    let manager = FakeManager::new();
    let xdg = home.path().join(".config");
    let loaded = manager.envs("loaded", &xdg);
    let (code, _stdout, stderr) = run_binary_with(home.path(), &["agent", "service", "install"], &loaded);
    assert_eq!(code, 0, "install failed: {stderr}");

    let (code, value, stderr) = run_binary_with(home.path(), &["--json", "agent", "service", "status"], &loaded);
    assert_eq!(code, 0, "{stderr}");
    let value: serde_json::Value = serde_json::from_str(&value).expect("status JSON");
    assert_eq!(value["available"], true);
    assert_eq!(value["unit"], "vowp-agent.service");
    assert_eq!(value["state"], "loaded");
    assert_eq!(value["loaded"], true);
    assert_eq!(value["pid"], 1234);
    assert_eq!(value["last_exit"], 0);
    assert!(value.get("unit_path").is_none(), "JSON stays path-free: {value}");

    let stopped = manager.envs("stopped", &xdg);
    let (code, human, stderr) = run_binary_with(home.path(), &["agent", "service", "status"], &stopped);
    assert_eq!(code, 0, "{stderr}");
    assert!(human.contains("unit:"), "{human}");
    assert!(
        human.contains("loaded: yes"),
        "LoadState=loaded is the manager's loaded state: {human}"
    );
    assert!(human.contains("pid: -"), "{human}");
    assert!(human.contains("last exit: 1"), "{human}");

    let empty = tempfile::tempdir().expect("empty PATH");
    let unavailable = vec![
        ("PATH", empty.path().display().to_string()),
        ("XDG_CONFIG_HOME", xdg.display().to_string()),
    ];
    let (code, value, stderr) = run_binary_with(home.path(), &["--json", "agent", "service", "status"], &unavailable);
    assert_eq!(code, 0, "{stderr}");
    let value: serde_json::Value = serde_json::from_str(&value).expect("unavailable JSON");
    assert_eq!(value["available"], false);
    assert_eq!(value["state"], "unavailable");
}

#[test]
fn agent_service_unsupported_platform_is_exit_five() {
    let code = service_error_exit_code(&ServiceError::UnsupportedPlatform);
    assert_eq!(code, 5);
    assert!(
        ServiceError::UnsupportedPlatform
            .to_string()
            .contains("Linux and macOS")
    );
}

#[test]
fn agent_run_service_without_home_exits_two() {
    let output = ProcessCommand::new(env!("CARGO_BIN_EXE_vowp"))
        .args(["agent", "run", "--service"])
        .env_remove("HOME")
        .env_remove("VOWP_AGENT_HOME")
        .env("NO_COLOR", "1")
        .output()
        .expect("run vowp agent run --service");
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn agent_run_service_refuses_a_loose_state_dir_with_exit_five() {
    let home = TempDir::new().expect("temp home");
    let state_dir = home.path().join(".vowp-agent");
    enrolled_agent_config(home.path());
    NodeKey::load_or_generate(&state_dir).expect("node key");
    std::fs::set_permissions(&state_dir, std::fs::Permissions::from_mode(0o755)).expect("loose mode");

    let (code, _stdout, stderr) = run_binary(home.path(), &["agent", "run", "--service"]);
    assert_eq!(code, 5, "a loose state directory is a refusal: {stderr}");
    assert!(stderr.contains("mode 755"), "{stderr}");
    assert!(stderr.contains("must be 700"), "{stderr}");
    assert!(
        !stderr.contains("config:"),
        "an isolation refusal is not a config error: {stderr}"
    );
}

#[test]
fn agent_run_service_writes_the_run_marker_and_drains_on_sigterm() {
    let home = TempDir::new().expect("temp home");
    let state_dir = home.path().join(".vowp-agent");
    enrolled_agent_config(home.path());
    NodeKey::load_or_generate(&state_dir).expect("node key");

    let tools = tempfile::tempdir().expect("fake tools");
    let ssh = tools.path().join("ssh");
    let log = tools.path().join("ssh.log");
    std::fs::write(&ssh, "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$FAKE_SSH_LOG\"\nexit 1\n").expect("fake ssh");
    std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o755)).expect("chmod ssh");

    let mut child = ProcessCommand::new(env!("CARGO_BIN_EXE_vowp"))
        .args(["agent", "run", "--service", "--foreground"])
        .env("HOME", home.path())
        .env("PATH", tools.path())
        .env("FAKE_SSH_LOG", &log)
        .env("NO_COLOR", "1")
        .env_remove("VOWP_AGENT_HOME")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn agent run --service");
    let marker = state_dir.join("run");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !marker.is_file() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(marker.is_file(), "the service run writes the run marker");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !log.is_file() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(log.is_file(), "the service run opens the configured transport");

    let status = ProcessCommand::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .expect("send SIGTERM");
    assert!(status.success());
    let deadline = Instant::now() + Duration::from_secs(15);
    let exit = loop {
        if let Some(status) = child.try_wait().expect("poll agent") {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the service run did not drain within 15 s");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(exit.code(), Some(0), "a drained service run exits 0");
    assert!(marker.is_file(), "a drain keeps the marker for the next start");
    let log = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(log.contains("vowp agent gateway --stdio"), "{log}");
}

/// Writes one local surface directory with a token, expiry and state.
fn write_local_surface(state_dir: &Path, id: &str, token: &str, expiry: Timestamp, state: &str) {
    let dir = state_dir.join("surfaces").join(id);
    std::fs::create_dir_all(&dir).expect("surface dir");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).expect("surface mode");
    if !token.is_empty() {
        let path = dir.join("token");
        std::fs::write(&path, token).expect("token");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("token mode");
    }
    let path = dir.join("expires_at");
    std::fs::write(&path, expiry.as_rfc3339()).expect("expiry");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("expiry mode");
    let path = dir.join("state");
    std::fs::write(&path, state).expect("state");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("state mode");
}

#[test]
fn agent_status_adds_the_surfaces_array_and_warns_within_seven_days() {
    let home = TempDir::new().expect("temp home");
    enrolled_agent_config(home.path());
    let state_dir = home.path().join(".vowp-agent");
    let now = Timestamp::now();
    let early = now.plus(Duration::from_secs(3 * 24 * 60 * 60));
    let late = now.plus(Duration::from_secs(20 * 24 * 60 * 60));
    let early_token = "AAAAAAAAAAAA-local-fixture".to_owned();
    let late_token = "BBBBBBBBBBBB-local-fixture".to_owned();
    write_local_surface(&state_dir, "01ARZ3NDEKTSV4RRFFQ69G5FAW", &early_token, early, "paired");
    write_local_surface(&state_dir, "01ARZ3NDEKTSV4RRFFQ69G5FAX", &late_token, late, "paired");
    let dead_dir = state_dir.join("surfaces/01ARZ3NDEKTSV4RRFFQ69G5FAY");
    std::fs::create_dir_all(&dead_dir).expect("needs-pair dir");
    std::fs::set_permissions(&dead_dir, std::fs::Permissions::from_mode(0o700)).expect("mode");
    std::fs::write(dead_dir.join("state"), "needs_pair").expect("state");

    let (code, stdout, stderr) = run_binary(home.path(), &["--json", "agent", "status"]);
    assert_eq!(code, 0, "status failed: {stderr}");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("status JSON");
    let surfaces = value["surfaces"].as_array().expect("surfaces array");
    assert_eq!(surfaces.len(), 3, "{value}");
    let first = surfaces
        .iter()
        .find(|row| row["surface"] == "01ARZ3NDEKTSV4RRFFQ69G5FAW")
        .expect("early surface");
    assert_eq!(first["prefix"], "AAAAAAAAAAAA");
    assert_eq!(first["state"], "paired");
    assert_eq!(first["expires_at"], early.as_rfc3339());
    let needs_pair = surfaces
        .iter()
        .find(|row| row["surface"] == "01ARZ3NDEKTSV4RRFFQ69G5FAY")
        .expect("needs-pair surface");
    assert_eq!(needs_pair["state"], "needs_pair");
    assert_eq!(
        value["surface_token_expires_at"],
        early.as_rfc3339(),
        "the earliest active expiry stays the compatibility field"
    );
    assert_eq!(stderr.matches("warning:").count(), 1, "{stderr}");
    assert!(stderr.contains("7 days"), "{stderr}");
    for (label, text) in [("stdout", &stdout), ("stderr", &stderr)] {
        assert!(!text.contains(&early_token), "{label} carries no token: {text}");
        assert!(!text.contains(&late_token), "{label} carries no token: {text}");
    }

    let (code, human, stderr) = run_binary(home.path(), &["agent", "status"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(human.contains("surfaces:"), "{human}");
    assert!(human.contains("01ARZ3NDEKTSV4RRFFQ69G5FAW"), "{human}");
}

/// Creates one attached surface and returns its node, id and stored token.
fn attached_surface(env: &TestEnv, session: &SessionId) -> (NodeName, SurfaceId, String) {
    let value = pair_surface(env, session);
    let code = value["code"].as_str().expect("pairing code");
    let id = SurfaceId(value["surface"].as_str().expect("surface id").to_owned());
    let node = NodeName::parse("mac-studio").expect("node slug");
    let store = env.store();
    let now = store.clock().now();
    let issued = issue_surface_token(&store, &node, &id, session, "opencode", code, now).expect("issue token");
    (node, id, issued.token)
}

#[test]
fn surface_rotate_wait_returns_when_the_frame_is_drained() {
    let (env, node, session) = seed_pairing_env();
    let (_node, surface, _token) = attached_surface(&env, &session);
    let home = env.home_path().to_path_buf();
    let request = surface.0.clone();
    let drainer = std::thread::spawn(move || {
        let spool = vowp_core::agent::AgentFrameSpool::new(&home);
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if spool.read(&node, &request).expect("read queued frame").is_some() {
                spool.complete(&node, &request).expect("drain queued frame");
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    });
    let (code, value, stderr) = env.run_json(&["surface", "rotate", &surface.0, "--wait", "3s"]);
    assert!(
        drainer.join().expect("drainer thread"),
        "the drainer saw the queued frame"
    );
    assert_eq!(code, 0, "surface rotate --wait failed: {stderr}");
    assert_eq!(value["pending"], false, "{value}");
    assert!(value["prefix"].is_string(), "{value}");
    assert!(
        !stderr.contains("rotation_pending"),
        "a drained rotation is not pending: {stderr}"
    );
}

#[test]
fn surface_rotate_wait_reports_rotation_pending_with_exit_seven() {
    let (env, _node, session) = seed_pairing_env();
    let (_node, surface, _token) = attached_surface(&env, &session);
    let (code, stdout, stderr) = env.run(&["surface", "rotate", &surface.0, "--wait", "0s"]);
    assert_eq!(code, 7, "an undrained rotation is exit 7: {stdout} {stderr}");
    assert!(stderr.contains("rotation_pending"), "{stderr}");
    let frame = env
        .home_path()
        .join("agent-frames/mac-studio")
        .join(format!("{}.json", surface.0));
    assert!(frame.is_file(), "the queued frame stays for the next connection");
}

#[test]
fn surface_rotate_grace_is_capped_at_twenty_four_hours() {
    let (env, _node, session) = seed_pairing_env();
    let (_node, surface, old_token) = attached_surface(&env, &session);
    let (code, _stdout, stderr) = env.run(&["surface", "rotate", &surface.0, "--grace", "48h"]);
    assert_eq!(code, 0, "surface rotate --grace failed: {stderr}");
    let store = env.store();
    let now = Timestamp::now();
    let old = store
        .list_surface_tokens(&surface)
        .expect("token rows")
        .into_iter()
        .find(|token| token.token_hash == vowp_core::agent::hash_code(&old_token))
        .expect("old token row");
    let expiry = old.expires_at.expect("old token keeps an expiry");
    assert!(
        expiry.since(now) <= Duration::from_secs(24 * 60 * 60) + Duration::from_secs(2),
        "the grace is capped at 24 hours: {}",
        expiry.as_rfc3339()
    );
    assert!(
        expiry.since(now) >= Duration::from_secs(23 * 60 * 60),
        "a 48h request still grants almost the 24h cap: {}",
        expiry.as_rfc3339()
    );

    // A second rotation with a short grace shortens the replacement token too.
    let (code, value, stderr) = env.run_json(&["surface", "rotate", &surface.0, "--grace", "30m"]);
    assert_eq!(code, 0, "{stderr}");
    let old_expires: Timestamp = serde_json::from_value(value["old_expires_at"].clone()).expect("old expiry");
    let remaining = old_expires.since(Timestamp::now());
    assert!(
        remaining <= Duration::from_secs(30 * 60) + Duration::from_secs(2) && remaining >= Duration::from_secs(29 * 60),
        "the short grace is applied: {}",
        old_expires.as_rfc3339()
    );
}

#[test]
fn surface_rotate_grace_refuses_pending_and_revoked_surfaces() {
    let (env, node, session) = seed_pairing_env();
    let store = env.store();
    let now = store.clock().now();
    let pending = vowp_core::agent::pair::create_pending_surface(
        &store,
        &node,
        &session,
        vowp_protocol::SurfaceKind::Opencode,
        Some("opencode"),
        now,
    )
    .expect("pending surface");
    let (code, _stdout, stderr) = env.run(&["surface", "rotate", &pending.surface.id.0, "--grace", "1h"]);
    assert_eq!(code, 4, "a pending surface is a conflict: {stderr}");

    let (_node, surface, _token) = attached_surface(&env, &session);
    let deps = env.global(false);
    let input = BufferIo::new("");
    let ctx = vowp_cli::Ctx::build(&deps, &input).expect("context");
    vowp_core::agent::pair::revoke_surface(&store, ctx.deps(), &surface, now).expect("revoke");
    let (code, _stdout, stderr) = env.run(&["surface", "rotate", &surface.0, "--grace", "1h"]);
    assert_eq!(code, 5, "a revoked surface is refused: {stderr}");
}

#[test]
fn surface_pair_replace_revokes_the_old_surface_and_clears_the_stale_frame() {
    let (env, _node, session) = seed_pairing_env();
    let (_node, old, old_token) = attached_surface(&env, &session);
    let (code, _stdout, stderr) = env.run(&["surface", "rotate", &old.0]);
    assert_eq!(code, 0, "rotation failed: {stderr}");
    let frame = env
        .home_path()
        .join("agent-frames/mac-studio")
        .join(format!("{}.json", old.0));
    assert!(frame.is_file(), "the rotation is queued before the replace");

    let (code, value, stderr) = env.run_json(&[
        "surface",
        "pair",
        "--session",
        session.as_ref(),
        "--node",
        "mac-studio",
        "--kind",
        "opencode",
        "--app",
        "opencode",
        "--replace",
        &old.0,
    ]);
    assert_eq!(code, 0, "surface pair --replace failed: {stderr}");
    assert_eq!(value["replaced"], old.0);
    let new_id = SurfaceId(value["surface"].as_str().expect("new surface").to_owned());
    assert_ne!(new_id, old);
    assert_eq!(value["code"].as_str().expect("new code").len(), 52);
    assert!(!frame.exists(), "the old surface's queued frame is removed");

    let store = env.store();
    assert_eq!(
        store.surface_by_id(&old).expect("old lookup").expect("old row").state,
        SurfaceState::Revoked
    );
    assert_eq!(
        store
            .surface_by_id(&new_id)
            .expect("new lookup")
            .expect("new row")
            .state,
        SurfaceState::Pending
    );
    assert!(
        store
            .list_surface_tokens(&old)
            .expect("old tokens")
            .iter()
            .all(|token| token.revoked_at.is_some()),
        "every old token is revoked"
    );
    assert!(
        verify_surface_token(&store, &old_token, Timestamp::now()).is_err(),
        "the old token is dead immediately"
    );
}

#[test]
fn surface_ls_shows_prefix_overlap_pending_and_expired_without_a_token() {
    let (env, _node, session) = seed_pairing_env();
    let (_node, surface, _token) = attached_surface(&env, &session);
    let (code, rotated, stderr) = env.run_json(&["surface", "rotate", &surface.0]);
    assert_eq!(code, 0, "rotation failed: {stderr}");
    let new_expiry: Timestamp = serde_json::from_value(rotated["expires_at"].clone()).expect("new expiry");

    let store = env.store();
    let tokens = store.list_surface_tokens(&surface).expect("tokens");
    let newest = tokens
        .iter()
        .filter(|token| token.revoked_at.is_none())
        .max_by_key(|token| token.created_at)
        .expect("newest token");
    let old_expiry = tokens
        .iter()
        .filter(|token| token.revoked_at.is_none())
        .filter_map(|token| token.expires_at)
        .filter(|expiry| *expiry < new_expiry)
        .max()
        .expect("overlap expiry");

    let (code, value, stderr) = env.run_json(&["surface", "ls"]);
    assert_eq!(code, 0, "{stderr}");
    let entry = value
        .as_array()
        .expect("array")
        .iter()
        .find(|entry| entry["id"] == surface.0)
        .expect("surface row");
    assert_eq!(entry["prefix"], newest.prefix);
    assert_eq!(entry["expires_at"], new_expiry.as_rfc3339());
    assert_eq!(entry["overlap_until"], old_expiry.as_rfc3339());
    assert_eq!(entry["pending"], true);
    assert_eq!(entry["delivery_deadline"], new_expiry.as_rfc3339());
    assert_eq!(entry["expired"], false);
    let text = serde_json::to_string(&value).expect("json text");
    assert!(!text.contains("vowp_surf_"), "no token in JSON: {text}");

    let (code, human, stderr) = env.run(&["surface", "ls"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(human.contains("PREFIX"), "{human}");
    assert!(human.contains("OVERLAP"), "{human}");
    assert!(human.contains("PENDING"), "{human}");
    assert!(!human.contains("vowp_surf_"), "no token in human output: {human}");

    let now = Timestamp::now();
    let past = now.minus(Duration::from_secs(60));
    for token in store.list_surface_tokens(&surface).expect("tokens") {
        if token.revoked_at.is_none() {
            store
                .shorten_surface_token_expiry(&token.id, past)
                .expect("expire token");
        }
    }
    let (code, human, stderr) = env.run(&["surface", "ls"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(human.contains("expired"), "an expired token is marked: {human}");
    assert!(human.contains("attached"), "the surface state stays attached: {human}");
    let (code, value, stderr) = env.run_json(&["surface", "ls"]);
    assert_eq!(code, 0, "{stderr}");
    let entry = value
        .as_array()
        .expect("array")
        .iter()
        .find(|entry| entry["id"] == surface.0)
        .expect("surface row");
    assert_eq!(entry["state"], "attached");
    assert_eq!(entry["expired"], true);
    assert!(entry["prefix"].is_null(), "no active prefix: {entry}");
}

#[test]
fn agent_service_install_without_home_exits_two() {
    let output = ProcessCommand::new(env!("CARGO_BIN_EXE_vowp"))
        .args(["agent", "service", "install", "--print"])
        .env_remove("HOME")
        .env_remove("VOWP_AGENT_HOME")
        .env("NO_COLOR", "1")
        .output()
        .expect("run vowp agent service install");
    assert_eq!(output.status.code(), Some(2));
}
