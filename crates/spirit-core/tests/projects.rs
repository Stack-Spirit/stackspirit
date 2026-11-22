//! One-step `vowp project add`: exact argv, path-free parsing, validation and gating.

use std::sync::Arc;

use spirit_core::engine::{EngineSupervisor, EngineSupervisorConfig, FakeEngineRuntime};
use spirit_core::host::{HostDraft, HostKeyPin, InMemoryHostStore, NewHostProfile};
use spirit_core::service::SpiritCore;
use spirit_core::ssh::command::{CommandOutput, CommandSpec};
use spirit_core::ssh::{FakeCommandRunner, SshOptions, SshTarget};
use spirit_core::time::Timestamp;
use spirit_core::vowp::{
    MockVowpClient, ProjectAddRequest, ProjectSource, SshVowpClient, VowpClient,
    parse_project_bootstrap,
};
use spirit_core::wire::{OrchestratorApp, parse_capacity_report, parse_fleet_status};

const HOME_PATH: &str = "/home/owner/projects/demo-app";

fn bootstrap_json(with_orchestrator: bool) -> Vec<u8> {
    let orchestrator = if with_orchestrator {
        r#"{"name":"demo-app-chat","session_id":"01JCF5WQ8ZK3M2V7N4P6R8T0AE","project":"demo-app","app":"chat","state":"running","node":null,"join":{"desktop":"d","phone":"p","chat":"vowp chat --agent --session 01JCF5WQ8ZK3M2V7N4P6R8T0AE"}}"#
    } else {
        "null"
    };
    format!(
        r#"{{"project":"demo-app","path":"{HOME_PATH}","initialized":true,"orchestrator":{orchestrator}}}"#
    )
    .into_bytes()
}

fn ssh_client(runner: Arc<FakeCommandRunner>) -> SshVowpClient {
    SshVowpClient::new(
        runner,
        SshTarget::new("owner", "192.0.2.10", 22).unwrap(),
        SshOptions::new("/tmp/opencode/known_hosts"),
        None,
    )
}

fn remote_argv(call: &CommandSpec) -> String {
    call.args.last().cloned().unwrap_or_default()
}

fn folder_request() -> ProjectAddRequest {
    ProjectAddRequest {
        name: "demo-app".to_owned(),
        source: ProjectSource::Folder {
            path: HOME_PATH.to_owned(),
        },
        init: true,
        orchestrator: None,
    }
}

fn git_request(url: &str) -> ProjectAddRequest {
    ProjectAddRequest {
        name: "demo-app".to_owned(),
        source: ProjectSource::Git {
            url: url.to_owned(),
        },
        init: true,
        orchestrator: Some(OrchestratorApp::Chat),
    }
}

fn fresh_mock() -> MockVowpClient {
    let status =
        parse_fleet_status(include_str!("../fixtures/vowp/fleet-status-fresh.json").as_bytes())
            .unwrap()
            .value;
    let capacity = parse_capacity_report(
        include_str!("../fixtures/vowp/capacity-report-full.json").as_bytes(),
    )
    .unwrap()
    .value;
    MockVowpClient::new(status, capacity)
}

fn core() -> SpiritCore {
    let supervisor = EngineSupervisor::new(
        Arc::new(FakeEngineRuntime::new()),
        EngineSupervisorConfig {
            restart_budget: 3,
            health_attempts: 1,
            retry_delay: std::time::Duration::ZERO,
            port: 0,
        },
    );
    SpiritCore::new(Arc::new(InMemoryHostStore::new()), supervisor)
}

fn host_input() -> NewHostProfile {
    NewHostProfile {
        draft: HostDraft {
            label: "primary-vps".to_owned(),
            ssh: SshTarget::new("owner", "192.0.2.10", 22).unwrap(),
            identity_file: None,
            vowp_home: None,
            default_project: Some("vowp".to_owned()),
        },
        host_key: HostKeyPin {
            algorithm: "ssh-ed25519".to_owned(),
            sha256: "SHA256:AbCdEfGhIjKlMnOpQrStUvWxYz0123456789ABCDEFG".to_owned(),
            pinned_at: Timestamp::epoch(),
        },
    }
}

#[tokio::test]
async fn folder_and_git_add_use_the_exact_argv() {
    let runner = Arc::new(FakeCommandRunner::new(vec![
        CommandOutput::ok(bootstrap_json(false)),
        CommandOutput::ok(bootstrap_json(true)),
    ]));
    let client = ssh_client(runner.clone());
    client.project_add(folder_request()).await.unwrap();
    client
        .project_add(git_request("https://example.com/org/demo-app.git"))
        .await
        .unwrap();
    let calls = runner.calls();
    assert_eq!(
        remote_argv(&calls[0]),
        format!("'vowp' '--json' 'project' 'add' '{HOME_PATH}' '--name' 'demo-app' '--init'")
    );
    assert_eq!(
        remote_argv(&calls[1]),
        "'vowp' '--json' 'project' 'add' '--git' 'https://example.com/org/demo-app.git' '--name' 'demo-app' '--init' '--orchestrator' 'chat'"
    );
}

#[test]
fn parse_drops_the_home_path() {
    let outcome = parse_project_bootstrap(&bootstrap_json(true)).unwrap();
    assert_eq!(outcome.project, "demo-app");
    assert!(outcome.initialized);
    assert_eq!(outcome.orchestrator.as_ref().unwrap().app, "chat");
    let serialized = serde_json::to_string(&outcome).unwrap();
    assert!(!serialized.contains(HOME_PATH));
    assert!(!serialized.contains("path"));
    assert!(
        parse_project_bootstrap(&bootstrap_json(false))
            .unwrap()
            .orchestrator
            .is_none()
    );
}

#[test]
fn parse_rejects_a_bad_project_name_and_malformed_documents() {
    let bad = br#"{"project":"Bad Name;rm","path":"/x","initialized":true,"orchestrator":null}"#;
    assert!(parse_project_bootstrap(bad).is_err());
    assert!(parse_project_bootstrap(b"not json").is_err());
    assert!(parse_project_bootstrap(br#"{"project":"demo-app","path":"/x"}"#).is_err());
}

#[tokio::test]
async fn invalid_requests_are_rejected_before_the_client_call() {
    let mock = fresh_mock();
    let core = core();
    let host = core.add_host(host_input()).unwrap();
    core.connect(host.id, &mock).await.unwrap();
    let mut bad_name = folder_request();
    bad_name.name = "-bad name".to_owned();
    let bad_requests = [
        bad_name,
        git_request("https://user:secret@example.com/org/repo.git"),
        git_request("https://token@example.com/org/repo.git"),
        git_request(""),
        git_request("--upload-pack=evil"),
        ProjectAddRequest {
            source: ProjectSource::Folder {
                path: "relative/dir".to_owned(),
            },
            ..folder_request()
        },
        ProjectAddRequest {
            source: ProjectSource::Folder {
                path: String::new(),
            },
            ..folder_request()
        },
    ];
    for request in bad_requests {
        assert!(core.project_add(&mock, request).await.is_err());
    }
    assert!(mock.project_add_calls().is_empty());

    let runner = Arc::new(FakeCommandRunner::new(vec![]));
    let ssh = ssh_client(runner.clone());
    assert!(
        ssh.project_add(git_request("https://u:p@example.com/r.git"))
            .await
            .is_err()
    );
    assert_eq!(runner.call_count(), 0);
}

#[tokio::test]
async fn ssh_git_urls_without_a_password_are_allowed() {
    for url in [
        "ssh://git@example.com/org/repo.git",
        "git@example.com:org/repo.git",
    ] {
        assert!(git_request(url).validate().is_ok(), "{url}");
    }
}

#[tokio::test]
async fn missing_command_maps_to_not_supported() {
    for output in [
        CommandOutput::fail(127, b"vowp: command not found".to_vec()),
        CommandOutput::fail(2, b"error: unrecognized subcommand 'add'".to_vec()),
    ] {
        let runner = Arc::new(FakeCommandRunner::new(vec![output]));
        let error = ssh_client(runner)
            .project_add(folder_request())
            .await
            .unwrap_err();
        assert!(error.message.starts_with("not_supported:"), "{error}");
    }
}

#[tokio::test]
async fn service_requires_a_connected_host_and_mock_records_a_path_free_outcome() {
    let mock = fresh_mock();
    let core = core();
    assert!(core.project_add(&mock, folder_request()).await.is_err());
    assert!(mock.project_add_calls().is_empty());

    let host = core.add_host(host_input()).unwrap();
    core.connect(host.id, &mock).await.unwrap();
    let outcome = core.project_add(&mock, folder_request()).await.unwrap();
    assert_eq!(mock.project_add_calls(), vec![folder_request()]);
    assert_eq!(outcome.project, "demo-app");
    assert!(!serde_json::to_string(&outcome).unwrap().contains("/home/"));
}
