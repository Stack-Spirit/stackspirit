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
use vowp_protocol::{AGENT_PROTOCOL, NodeName, Timestamp};

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
