//! Connect/probe failure mapping and the complete typed exit-code table.

mod common;

use std::sync::Arc;

use spirit_cli::exit_code_for;
use spirit_core::connection::ConnectionSnapshot;
use spirit_core::error::{SpiritError, SpiritErrorCode};
use spirit_core::host::HostProbe;
use spirit_core::ssh::FakeCommandRunner;
use spirit_core::ssh::command::CommandOutput;

use common::{add_host, empty_env, env_with_runner, fleet_status_json, run_cli};

#[tokio::test]
async fn connect_maps_ssh_auth_to_exit_five_and_a_typed_error() {
    let directory = tempfile::tempdir().expect("temp dir");
    let root = directory.path().join("stackspirit");
    let (add_env, _runner) = empty_env(&root);
    let host = add_host(&add_env, "demo", "dev@example.com").await;

    let runner = Arc::new(FakeCommandRunner::new(vec![CommandOutput::fail(
        255,
        "Permission denied (publickey).\n",
    )]));
    let env = env_with_runner(&root, runner);
    let (result, out, err) = run_cli(
        &["spirit", "host", "connect", host.id.as_str(), "--json"],
        &env,
    )
    .await;
    let error = result.expect_err("auth failure must fail the command");
    assert_eq!(error.exit, 5);
    assert_eq!(error.error.code, SpiritErrorCode::SshAuth);
    assert!(out.is_empty());
    let parsed: SpiritError = serde_json::from_str(err.trim()).expect("error JSON");
    assert_eq!(parsed.code, SpiritErrorCode::SshAuth);
    assert!(parsed.message.contains("authentication"));
}

#[tokio::test]
async fn connect_maps_a_missing_vowp_binary_to_exit_five() {
    let directory = tempfile::tempdir().expect("temp dir");
    let root = directory.path().join("stackspirit");
    let (add_env, _runner) = empty_env(&root);
    let host = add_host(&add_env, "demo", "dev@example.com").await;

    let runner = Arc::new(FakeCommandRunner::new(vec![CommandOutput::fail(
        127,
        "vowp: command not found\n",
    )]));
    let env = env_with_runner(&root, runner);
    let (result, _out, _err) = run_cli(
        &["spirit", "host", "connect", host.id.as_str(), "--json"],
        &env,
    )
    .await;
    let error = result.expect_err("missing vowp must fail");
    assert_eq!(error.exit, 5);
    assert_eq!(error.error.code, SpiritErrorCode::VowpMissing);
}

#[tokio::test]
async fn connect_success_reports_a_connected_snapshot_and_marks_the_host() {
    let directory = tempfile::tempdir().expect("temp dir");
    let root = directory.path().join("stackspirit");
    let (add_env, _runner) = empty_env(&root);
    let host = add_host(&add_env, "demo", "dev@example.com").await;

    let runner = Arc::new(FakeCommandRunner::new(vec![
        CommandOutput::ok("vowp 0.4.0\n"),
        CommandOutput::ok(fleet_status_json()),
    ]));
    let env = env_with_runner(&root, runner);
    let (result, out, err) = run_cli(
        &["spirit", "host", "connect", host.id.as_str(), "--json"],
        &env,
    )
    .await;
    assert!(result.is_ok(), "connect failed: {err}");
    let snapshot: ConnectionSnapshot = serde_json::from_str(&out).expect("connection JSON");
    assert_eq!(snapshot.state, "connected");
    assert_eq!(snapshot.host_id.as_ref(), Some(&host.id));
    assert_eq!(snapshot.latency.expect("latency").samples, 1);

    let hosts: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("hosts.json")).expect("host store"))
            .expect("host store JSON");
    assert!(hosts["hosts"][0]["last_connected_at"].is_string());
}

#[tokio::test]
async fn probe_maps_a_host_key_failure_to_exit_five_and_keeps_the_probe_output() {
    let directory = tempfile::tempdir().expect("temp dir");
    let root = directory.path().join("stackspirit");
    let (add_env, _runner) = empty_env(&root);
    let host = add_host(&add_env, "demo", "dev@example.com").await;

    let runner = Arc::new(FakeCommandRunner::new(vec![CommandOutput::fail(
        255,
        "Host key verification failed.\n",
    )]));
    let env = env_with_runner(&root, runner);
    let (result, out, err) = run_cli(
        &["spirit", "host", "probe", host.id.as_str(), "--json"],
        &env,
    )
    .await;
    let error = result.expect_err("failed probe must exit non-zero");
    assert_eq!(error.exit, 5);
    let probe: HostProbe = serde_json::from_str(&out).expect("probe JSON");
    assert!(!probe.save_allowed());
    assert!(!probe.auth_ok);
    assert!(
        probe
            .stage("ssh")
            .is_some_and(|stage| !stage.ok && stage.detail.contains("host key")),
        "{probe:?}"
    );
    assert!(err.contains("remote"));
}

#[tokio::test]
async fn probe_success_passes_every_required_stage() {
    let directory = tempfile::tempdir().expect("temp dir");
    let root = directory.path().join("stackspirit");
    let (add_env, _runner) = empty_env(&root);
    let host = add_host(&add_env, "demo", "dev@example.com").await;

    let runner = Arc::new(FakeCommandRunner::new(vec![
        CommandOutput::ok("vowp 0.4.0\n"),
        CommandOutput::ok(fleet_status_json()),
    ]));
    let env = env_with_runner(&root, runner);
    let (result, out, err) = run_cli(
        &["spirit", "host", "probe", host.id.as_str(), "--json"],
        &env,
    )
    .await;
    assert!(result.is_ok(), "probe failed: {err}");
    let probe: HostProbe = serde_json::from_str(&out).expect("probe JSON");
    assert!(probe.reachable);
    assert!(probe.auth_ok);
    assert!(probe.host_key_ok);
    assert!(probe.save_allowed());
}

#[test]
fn every_core_error_code_maps_to_a_documented_exit_code() {
    let cases = [
        (SpiritErrorCode::InvalidInput, 2),
        (SpiritErrorCode::NotFound, 4),
        (SpiritErrorCode::HostNotFound, 4),
        (SpiritErrorCode::Conflict, 3),
        (SpiritErrorCode::Refused, 3),
        (SpiritErrorCode::Gate, 3),
        (SpiritErrorCode::HostKeyMismatch, 3),
        (SpiritErrorCode::Limit, 5),
        (SpiritErrorCode::Auth, 5),
        (SpiritErrorCode::SshAuth, 5),
        (SpiritErrorCode::SshUnavailable, 5),
        (SpiritErrorCode::Remote, 5),
        (SpiritErrorCode::RemoteTimeout, 5),
        (SpiritErrorCode::VowpMissing, 5),
        (SpiritErrorCode::VowpIncompatible, 5),
        (SpiritErrorCode::VowpCommand, 5),
        (SpiritErrorCode::Parse, 5),
        (SpiritErrorCode::Keychain, 5),
        (SpiritErrorCode::Engine, 5),
        (SpiritErrorCode::Io, 5),
        (SpiritErrorCode::Internal, 5),
    ];
    for (code, expected) in cases {
        assert_eq!(exit_code_for(code), expected, "{code:?}");
    }
}
