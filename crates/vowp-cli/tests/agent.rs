//! `vowp agent` CLI tests.
//!
//! Every test uses a temporary `HOME` or in-process parsing; none reads the real
//! `~/.vowp-agent`, opens a store or starts ssh.

use std::path::Path;
use std::process::Command as ProcessCommand;

use clap::Parser;
use tempfile::TempDir;
use vowp_agent::{AgentConfig, AgentStatus, ConnectionState};
use vowp_cli::BufferIo;
use vowp_cli::cli::{AgentCommand, Cli, Command};
use vowp_cli::node_ctx::ControlConfig;
use vowp_protocol::{AGENT_PROTOCOL, NodeKind, NodeName, NodeStatus, ProjectName, Timestamp};
use vowp_store::{NodeCredential, NodePath};

mod common;

use common::TestEnv;

fn run_binary(home: &Path, args: &[&str]) -> (i32, String, String) {
    let output = ProcessCommand::new(env!("CARGO_BIN_EXE_vowp"))
        .args(args)
        .env("HOME", home)
        .env_remove("VOWP_AGENT_HOME")
        .env("NO_COLOR", "1")
        .output()
        .expect("run vowp");
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
    };
    status.write(&state_dir).expect("status.json");
    std::fs::create_dir_all(state_dir.join("spool")).expect("spool dir");
    std::fs::write(state_dir.join("spool/pending.json"), vec![b'x'; 42]).expect("spool bytes");

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
            assert_eq!(args.target, "home.example");
            assert_eq!(args.code_file, "-");
        }
        other => panic!("expected agent enroll, got {other:?}"),
    }
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
