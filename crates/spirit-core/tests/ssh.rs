//! SSH argv, quoting, argv-only execution, caps and timeouts.

use std::sync::Arc;
use std::time::Duration;

use spirit_core::error::SpiritErrorCode;
use spirit_core::ssh::command::{CommandOutput, CommandSpec, remote_command};
use spirit_core::ssh::{
    CommandRunner, FakeCommandRunner, SshOptions, SshTarget, SystemCommandRunner, ssh_command,
};
use spirit_core::vowp::{SshVowpClient, VowpClient};
use spirit_core::wire::TargetMode;

const FRESH: &str = include_str!("../fixtures/vowp/fleet-status-fresh.json");
const CAPACITY: &str = include_str!("../fixtures/vowp/capacity-report-full.json");
const QUEUE: &str = include_str!("../fixtures/vowp/fleet-queue.json");
const INBOX: &str = include_str!("../fixtures/vowp/fleet-inbox.json");

fn target() -> SshTarget {
    SshTarget::new("owner", "192.0.2.10", 2222).unwrap()
}

fn options(identity: bool) -> SshOptions {
    SshOptions::new("/tmp/opencode/known_hosts")
        .with_identity_file(identity.then(|| std::path::PathBuf::from("/tmp/opencode/id_ed25519")))
}

fn ssh_argv() -> Vec<String> {
    vec!["vowp".to_owned(), "--version".to_owned()]
}

#[test]
fn ssh_local_process_uses_an_argv_array() {
    let spec = ssh_command(
        &target(),
        &options(false),
        &ssh_argv(),
        Duration::from_secs(20),
    )
    .unwrap();
    assert_eq!(spec.program, "ssh");
    assert!(spec.args.iter().all(|arg| !arg.contains('\n')));
    assert_eq!(spec.args.last().unwrap(), "'vowp' '--version'");
    assert!(!spec.args.iter().any(|arg| arg == "-c"));
    // The destination is a separate argv element, not a shell word.
    let destination = spec
        .args
        .iter()
        .position(|arg| arg == "owner@192.0.2.10")
        .unwrap();
    assert_eq!(spec.args[destination - 1], "--");
}

#[test]
fn ssh_remote_command_quotes_dynamic_arguments() {
    let argv = vec![
        "vowp".to_owned(),
        "--project".to_owned(),
        "vowp".to_owned(),
        "--json".to_owned(),
        "inbox".to_owned(),
        "answer".to_owned(),
        "01JCF5WQ8ZK3M2V7N4P6R8T0AC".to_owned(),
        "a reason with spaces and 'quotes'".to_owned(),
    ];
    let command = remote_command(&argv).unwrap();
    assert!(command.contains("'a reason with spaces and '\\''quotes'\\'''"));
    assert!(command.starts_with("'vowp'"));
}

#[test]
fn ssh_remote_command_has_no_pipeline_or_operators() {
    let argv = vec!["vowp".to_owned(), "--version".to_owned()];
    let command = remote_command(&argv).unwrap();
    for forbidden in ['|', '&', ';', '<', '>', '$', '`'] {
        assert!(!command.contains(forbidden), "unexpected {forbidden}");
    }
    // User text with metacharacters remains inside a single quoted argument.
    let argv = vec![
        "vowp".to_owned(),
        "job".to_owned(),
        "retry".to_owned(),
        "x; rm -rf /".to_owned(),
    ];
    let command = remote_command(&argv).unwrap();
    assert!(command.ends_with("'x; rm -rf /'"));
    assert!(!command.contains(" && "));
}

#[test]
fn ssh_identities_only_is_set_only_with_an_identity_file() {
    let without = ssh_command(
        &target(),
        &options(false),
        &ssh_argv(),
        Duration::from_secs(20),
    )
    .unwrap();
    assert!(!without.args.iter().any(|arg| arg == "IdentitiesOnly=yes"));
    assert!(!without.args.iter().any(|arg| arg == "-i"));

    let with = ssh_command(
        &target(),
        &options(true),
        &ssh_argv(),
        Duration::from_secs(20),
    )
    .unwrap();
    assert!(with.args.iter().any(|arg| arg == "IdentitiesOnly=yes"));
    let identity_index = with.args.iter().position(|arg| arg == "-i").unwrap();
    assert_eq!(with.args[identity_index + 1], "/tmp/opencode/id_ed25519");
}

#[test]
fn ssh_argv_uses_strict_host_key_checking() {
    let spec = ssh_command(
        &target(),
        &options(false),
        &ssh_argv(),
        Duration::from_secs(20),
    )
    .unwrap();
    assert!(spec.args.iter().any(|arg| arg == "BatchMode=yes"));
    assert!(
        spec.args
            .iter()
            .any(|arg| arg == "StrictHostKeyChecking=yes")
    );
    assert!(
        spec.args
            .iter()
            .any(|arg| arg == "UserKnownHostsFile=/tmp/opencode/known_hosts")
    );
    let port_index = spec.args.iter().position(|arg| arg == "-p").unwrap();
    assert_eq!(spec.args[port_index + 1], "2222");
    assert!(
        !spec
            .args
            .iter()
            .any(|arg| arg == "StrictHostKeyChecking=accept-new")
    );
}

#[tokio::test]
async fn ssh_read_command_uses_the_expected_vowp_json_forms() {
    let fake = Arc::new(FakeCommandRunner::new(vec![
        CommandOutput::ok(FRESH),
        CommandOutput::ok(CAPACITY),
        CommandOutput::ok(QUEUE),
        CommandOutput::ok(INBOX),
    ]));
    let client = SshVowpClient::new(
        fake.clone(),
        target(),
        options(false),
        Some("/srv/vowp-home".to_owned()),
    );
    client.fleet_status().await.unwrap();
    client.capacity_report().await.unwrap();
    client.fleet_queue().await.unwrap();
    client.fleet_inbox(true).await.unwrap();
    let calls = fake.calls();
    assert_eq!(calls.len(), 4);
    let commands: Vec<String> = calls
        .iter()
        .map(|call| call.args.last().unwrap().clone())
        .collect();
    assert!(commands[0].contains("'--home' '/srv/vowp-home' '--json' 'fleet' 'status'"));
    assert!(commands[1].contains("'--json' 'capacity' 'report'"));
    assert!(commands[2].contains("'--json' 'fleet' 'queue'"));
    assert!(commands[3].contains("'--json' 'fleet' 'inbox' '--all'"));
    assert!(commands.iter().all(|command| command.starts_with("'vowp'")));
}

#[tokio::test]
async fn live_client_keeps_parse_warnings_until_the_caller_drains_them() {
    let mut status: serde_json::Value = serde_json::from_str(FRESH).unwrap();
    status["snapshot"]["supervisor"]
        .as_object_mut()
        .unwrap()
        .remove("pid");
    let status_body = serde_json::to_vec(&status).unwrap();

    let mut capacity: serde_json::Value = serde_json::from_str(CAPACITY).unwrap();
    capacity.as_object_mut().unwrap().remove("reload_pending");
    let capacity_body = serde_json::to_vec(&capacity).unwrap();

    let mut queue: serde_json::Value = serde_json::from_str(QUEUE).unwrap();
    queue[0]["state"] = serde_json::Value::String("quantum".to_owned());
    let queue_body = serde_json::to_vec(&queue).unwrap();

    let mut inbox: serde_json::Value = serde_json::from_str(INBOX).unwrap();
    inbox[0]["kind"] = serde_json::Value::String("quantum".to_owned());
    let inbox_body = serde_json::to_vec(&inbox).unwrap();

    let fake = Arc::new(FakeCommandRunner::new(vec![
        CommandOutput::ok(status_body),
        CommandOutput::ok(capacity_body),
        CommandOutput::ok(queue_body),
        CommandOutput::ok(inbox_body),
    ]));
    let client = SshVowpClient::new(fake, target(), options(false), None);

    let _ = client.fleet_status().await.unwrap();
    let warnings = client.take_warnings();
    assert!(
        warnings
            .iter()
            .any(|warning| warning.path == "snapshot.supervisor.pid"),
        "fleet status warnings must not be dropped: {warnings:?}"
    );
    assert!(
        client.take_warnings().is_empty(),
        "draining clears the list"
    );

    let _ = client.capacity_report().await.unwrap();
    let warnings = client.take_warnings();
    assert!(
        warnings
            .iter()
            .any(|warning| warning.path == "reload_pending"),
        "capacity report warnings must not be dropped: {warnings:?}"
    );

    let _ = client.fleet_queue().await.unwrap();
    let warnings = client.take_warnings();
    assert!(
        warnings.iter().any(|warning| warning.path == "[0].state"),
        "fleet queue warnings must not be dropped: {warnings:?}"
    );

    let _ = client.fleet_inbox(true).await.unwrap();
    let warnings = client.take_warnings();
    assert!(
        warnings.iter().any(|warning| warning.path == "[0].kind"),
        "fleet inbox warnings must not be dropped: {warnings:?}"
    );
}

#[tokio::test]
async fn ssh_write_commands_quote_user_text_and_validate_input() {
    let fake = Arc::new(FakeCommandRunner::new(vec![
        CommandOutput::ok("{}"),
        CommandOutput::ok(
            serde_json::to_vec(&serde_json::json!({
                "mode": "fixed",
                "target_workers": 9,
                "advised_workers": 8,
                "is_override": true,
                "set_by": "stackspirit",
                "set_at": "2026-11-14T12:00:00.000Z",
                "reason": "owner asked"
            }))
            .unwrap(),
        ),
    ]));
    let client = SshVowpClient::new(fake.clone(), target(), options(false), None);
    client
        .job_action("vowp", "ss-1-core", spirit_core::wire::JobAction::Retry)
        .await
        .unwrap();
    client
        .set_target(9, TargetMode::Fixed, true, "owner asked")
        .await
        .unwrap();
    let calls = fake.calls();
    let job = calls[0].args.last().unwrap();
    assert!(job.contains("'--project' 'vowp' '--json' 'job' 'retry' 'ss-1-core'"));
    let target_call = calls[1].args.last().unwrap();
    assert!(target_call.contains("'--target' '9' '--mode' 'fixed' '--override'"));
    assert!(target_call.contains("'--reason' 'owner asked' '--by' 'stackspirit'"));
    assert!(
        client
            .job_action("../bad", "job", spirit_core::wire::JobAction::Retry)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn ssh_output_cap_is_a_typed_error() {
    let runner = SystemCommandRunner;
    let spec = CommandSpec::new("printf", vec!["0123456789".to_owned()]).with_output_cap(4);
    let error = runner.run(&spec).await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Parse);
    assert!(error.message.contains("output_too_large"));
}

#[tokio::test]
async fn ssh_timeout_is_retryable_remote_timeout() {
    let runner = SystemCommandRunner;
    let spec =
        CommandSpec::new("sleep", vec!["2".to_owned()]).with_timeout(Duration::from_millis(40));
    let error = runner.run(&spec).await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::RemoteTimeout);
    assert!(error.retryable);
    assert!(error.message.contains("did not finish"));
}

#[tokio::test]
async fn ssh_timeout_kills_the_child_and_returns_remote_timeout() {
    let runner = SystemCommandRunner;
    let before = std::time::Instant::now();
    let spec =
        CommandSpec::new("sleep", vec!["5".to_owned()]).with_timeout(Duration::from_millis(50));
    let error = runner.run(&spec).await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::RemoteTimeout);
    assert!(before.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn ssh_transport_failure_maps_to_remote_or_host_key_errors() {
    let fake = Arc::new(FakeCommandRunner::new(vec![CommandOutput::fail(
        255,
        "Host key verification failed.",
    )]));
    let client = SshVowpClient::new(fake, target(), options(false), None);
    let error = client.probe().await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::HostKeyMismatch);
}

#[tokio::test]
async fn refused_and_invalid_input_exits_are_not_retryable() {
    let fake = Arc::new(FakeCommandRunner::new(vec![
        CommandOutput::fail(5, "lane is guarded"),
        CommandOutput::fail(2, "invalid capacity mode"),
        CommandOutput::fail(7, "remote failure"),
    ]));
    let client = SshVowpClient::new(fake, target(), options(false), None);

    let error = client.fleet_queue().await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Refused);
    assert!(!error.retryable, "refused must wait for changed input");

    let error = client.fleet_queue().await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::InvalidInput);
    assert!(!error.retryable, "invalid input cannot succeed on a retry");

    let error = client.fleet_queue().await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Remote);
    assert!(error.retryable, "remote failures still retry with backoff");
}

#[test]
fn ssh_target_validation_rejects_injection_shapes() {
    assert!(SshTarget::new("owner", "192.0.2.10", 22).is_ok());
    assert!(SshTarget::new("owner", "2001:db8::1", 22).is_ok());
    assert!(SshTarget::new("owner name", "192.0.2.10", 22).is_err());
    assert!(SshTarget::new("owner", "-oProxyCommand=evil", 22).is_err());
    assert!(SshTarget::new("owner", "host;reboot", 22).is_err());
    assert!(SshTarget::new("owner", "192.0.2.10", 0).is_err());
    assert_eq!(target().known_hosts_pattern(), "[192.0.2.10]:2222");
}
