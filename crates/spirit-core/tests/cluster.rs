//! Hermetic cluster tests: form, health and promotion over the fake SSH runner.
//!
//! No test opens a real SSH connection, reads `~/.vowp`, or mutates a live server. The
//! runner records every [`CommandSpec`] so the tests can assert the exact argv, stdin
//! and ordering, and can prove that a pairing code never reaches argv.

use std::sync::Arc;
use std::time::Duration;

use spirit_core::cluster::{
    BackupPolicy, CLUSTER_BACKUP_SCHEDULE, ClusterAgentState, ClusterConfig, ClusterFormRequest,
    ClusterHostRole, ClusterProject, ClusterService, ClusterTotalsSource, ClusterWorker,
    ClusterWorkerProject, ClusterWorkerRole, HostProfileId, PROMOTE_CONFIRMATION_TOKEN,
    PromoteRequest,
};
use spirit_core::host::{
    HostDraft, HostKeyPin, HostProfile, HostStore, InMemoryHostStore, NewHostProfile,
};
use spirit_core::ssh::command::{CommandOutput, CommandSpec};
use spirit_core::ssh::{FakeCommandRunner, SshTarget};
use spirit_core::time::{FakeClock, Timestamp};
use spirit_core::wire::ProjectName;

const NODE_CODE: &str = "one-time-pairing-code-0123456789abcdefghijklmnopqrstuvwxyz";
const HOME_CONFIG: &str = r#"[project]
name = "demo"
default_branch = "main"

[offload]
enabled = false
max_remote_runs = 5
result_transport = "push"
pause_on_home_brake = true

[offload.projects.demo]
placement = "any"
allow_kinds = ["build", "test"]
"#;

fn pin() -> HostKeyPin {
    HostKeyPin {
        algorithm: "ssh-ed25519".to_owned(),
        sha256: "SHA256:AbCdEfGhIjKlMnOpQrStUvWxYz0123456789ABCDEFG".to_owned(),
        pinned_at: Timestamp::epoch(),
    }
}

fn add_host(
    store: &Arc<dyn HostStore>,
    label: &str,
    host: &str,
    vowp_home: Option<&str>,
) -> HostProfile {
    store
        .add(NewHostProfile {
            draft: HostDraft {
                label: label.to_owned(),
                ssh: SshTarget::new("owner", host, 22).unwrap(),
                identity_file: None,
                vowp_home: vowp_home.map(str::to_owned),
                default_project: Some("demo".to_owned()),
            },
            host_key: pin(),
        })
        .unwrap()
}

fn empty_store() -> Arc<dyn HostStore> {
    Arc::new(InMemoryHostStore::new())
}

fn form_config(
    home: &HostProfile,
    workers: &[(&HostProfile, &str, ClusterWorkerRole)],
) -> ClusterConfig {
    ClusterConfig {
        home: home.id.clone(),
        workers: workers
            .iter()
            .map(|(host, node_name, role)| ClusterWorker {
                host: host.id.clone(),
                node_name: (*node_name).to_owned(),
                role: *role,
                agent_state: ClusterAgentState::Pending,
            })
            .collect(),
        backup: BackupPolicy {
            every: Duration::from_secs(24 * 60 * 60),
            keep: 7,
        },
    }
}

fn form_request(config: ClusterConfig, worker: &HostProfile) -> ClusterFormRequest {
    ClusterFormRequest {
        config,
        home_project: ClusterProject {
            name: ProjectName::parse("demo").unwrap(),
            root: "/srv/demo".to_owned(),
        },
        worker_projects: vec![ClusterWorkerProject {
            worker: worker.id.clone(),
            project: ProjectName::parse("demo").unwrap(),
            root: "/srv/demo".to_owned(),
            worktree_root: None,
            allow: vec!["src".to_owned()],
        }],
        backup_target: Some("backup@192.0.2.20:/srv/vowp-backups".to_owned()),
    }
}

fn remote_argv(spec: &CommandSpec) -> String {
    spec.args.last().cloned().unwrap_or_default()
}

fn runner(responses: Vec<CommandOutput>) -> Arc<FakeCommandRunner> {
    Arc::new(FakeCommandRunner::new(responses))
}

fn node_enroll_json() -> CommandOutput {
    CommandOutput::ok(
        format!(
            r#"{{"node":"worker-1","code":"{NODE_CODE}","expires_at":"2026-11-19T12:10:00.000Z","command":"vowp agent enroll --target owner@192.0.2.10 --code-file -"}}"#
        )
        .into_bytes(),
    )
}

fn agent_enroll_json() -> CommandOutput {
    CommandOutput::ok(
        br#"{"node":"worker-1","home":"vps","protocol":"vowp-agent/1","heartbeat_secs":10}"#
            .to_vec(),
    )
}

fn paths_add_json() -> CommandOutput {
    CommandOutput::ok(
        br#"{"project":"demo","local_root":"/srv/demo","worktree_root":null,"allow":["src"]}"#
            .to_vec(),
    )
}

fn version_ok() -> CommandOutput {
    CommandOutput::ok(b"vowp 0.4.0\n".to_vec())
}

fn stat_ok(at: Timestamp) -> CommandOutput {
    CommandOutput::ok(format!("{}\n", at.unix_secs()).into_bytes())
}

#[tokio::test]
async fn form_with_two_servers_writes_the_expected_commands_and_config() {
    let store = empty_store();
    let home = add_host(&store, "home-vps", "192.0.2.10", None);
    let worker = add_host(&store, "worker-vps", "192.0.2.20", None);
    let config = form_config(&home, &[(&worker, "worker-1", ClusterWorkerRole::Backup)]);
    let request = form_request(config, &worker);

    let runner = runner(vec![
        version_ok(),                                       // Home preflight
        version_ok(),                                       // Worker preflight
        CommandOutput::fail(1, b"No such file".to_vec()),   // cat stackspirit.toml
        CommandOutput::ok(HOME_CONFIG.as_bytes().to_vec()), // cat vowp.toml
        node_enroll_json(),                                 // node enroll on Home
        agent_enroll_json(),                                // agent enroll on Worker
        CommandOutput::ok(Vec::new()),                      // agent service install
        paths_add_json(),                                   // agent paths add
        CommandOutput::ok(Vec::new()),                      // tee
        CommandOutput::ok(Vec::new()),                      // schedule add
    ]);
    let service = ClusterService::new(store, runner.clone(), "/tmp/opencode/known_hosts");

    let formed = service.form(&request).await.unwrap();
    assert_eq!(formed.backup_schedule, CLUSTER_BACKUP_SCHEDULE);
    assert_eq!(formed.config.workers.len(), 1);
    assert_eq!(
        formed.config.workers[0].agent_state,
        ClusterAgentState::Enrolled
    );

    let calls = runner.calls();
    assert_eq!(calls.len(), 10);

    assert!(remote_argv(&calls[0]).contains("'--version'"));
    assert!(remote_argv(&calls[1]).contains("'--version'"));
    assert_eq!(remote_argv(&calls[2]), "'cat' '/srv/demo/stackspirit.toml'");
    assert_eq!(remote_argv(&calls[3]), "'cat' '/srv/demo/vowp.toml'");

    let enroll = remote_argv(&calls[4]);
    assert!(enroll.contains("'node' 'enroll' 'worker-1'"), "{enroll}");
    assert!(!enroll.contains(NODE_CODE), "{enroll}");

    let agent_enroll = remote_argv(&calls[5]);
    for expected in [
        "'agent' 'enroll'",
        "'--target' 'owner@192.0.2.10'",
        "'--code-file' '-'",
    ] {
        assert!(
            agent_enroll.contains(expected),
            "{expected}: {agent_enroll}"
        );
    }
    assert!(!agent_enroll.contains(NODE_CODE), "{agent_enroll}");
    assert_eq!(
        calls[5].stdin.as_deref(),
        Some(format!("{NODE_CODE}\n").as_bytes())
    );

    let install = remote_argv(&calls[6]);
    assert!(
        install.contains("'agent' 'service' 'install' '--now'"),
        "{install}"
    );

    let paths = remote_argv(&calls[7]);
    for expected in [
        "'agent' 'paths' 'add'",
        "'--project' '/srv/demo'",
        "'--name' 'demo'",
        "'--allow' 'src'",
    ] {
        assert!(paths.contains(expected), "{expected}: {paths}");
    }

    let tee = remote_argv(&calls[8]);
    assert_eq!(tee, "'tee' '/srv/demo/vowp.toml'");
    let written = String::from_utf8(calls[8].stdin.clone().unwrap()).unwrap();
    assert!(written.contains("enabled = true"));
    assert!(written.contains("default = \"any\""));
    assert!(written.contains("result_transport = \"bundle\""));
    assert!(written.contains("max_remote_runs = 5"), "{written}");
    assert!(written.contains("pause_on_home_brake = true"), "{written}");
    assert!(written.contains("[offload.projects.demo]"), "{written}");
    assert!(
        written.contains("allow_kinds = [\"build\", \"test\"]"),
        "{written}"
    );
    assert!(!written.contains("default_placement"));

    let schedule = remote_argv(&calls[9]);
    for expected in [
        "'schedule' 'add' 'every 1d' 'command'",
        "'--name' 'cluster-backup'",
        "'--' 'vowp' 'backup' '--to' 'backup@192.0.2.20:/srv/vowp-backups'",
    ] {
        assert!(schedule.contains(expected), "{expected}: {schedule}");
    }

    let debug = format!("{formed:?}");
    assert!(!debug.contains(NODE_CODE), "{debug}");
}

#[tokio::test]
async fn form_prefers_and_writes_back_stackspirit_toml_when_present() {
    let store = empty_store();
    let home = add_host(&store, "home-vps", "192.0.2.10", None);
    let worker = add_host(&store, "worker-vps", "192.0.2.20", None);
    let config = form_config(&home, &[(&worker, "worker-1", ClusterWorkerRole::Backup)]);
    let request = form_request(config, &worker);

    let runner = runner(vec![
        version_ok(),                                       // Home preflight
        version_ok(),                                       // Worker preflight
        CommandOutput::ok(HOME_CONFIG.as_bytes().to_vec()), // cat stackspirit.toml
        node_enroll_json(),                                 // node enroll on Home
        agent_enroll_json(),                                // agent enroll on Worker
        CommandOutput::ok(Vec::new()),                      // agent service install
        paths_add_json(),                                   // agent paths add
        CommandOutput::ok(Vec::new()),                      // tee
        CommandOutput::ok(Vec::new()),                      // schedule add
    ]);
    let service = ClusterService::new(store, runner.clone(), "/tmp/opencode/known_hosts");

    let formed = service.form(&request).await.unwrap();
    assert_eq!(formed.backup_schedule, CLUSTER_BACKUP_SCHEDULE);
    assert_eq!(formed.config.workers.len(), 1);
    assert_eq!(
        formed.config.workers[0].agent_state,
        ClusterAgentState::Enrolled
    );

    let calls = runner.calls();
    assert_eq!(calls.len(), 9);

    assert!(remote_argv(&calls[0]).contains("'--version'"));
    assert!(remote_argv(&calls[1]).contains("'--version'"));
    assert_eq!(remote_argv(&calls[2]), "'cat' '/srv/demo/stackspirit.toml'");

    let enroll = remote_argv(&calls[3]);
    assert!(enroll.contains("'node' 'enroll' 'worker-1'"), "{enroll}");
    assert!(!enroll.contains(NODE_CODE), "{enroll}");

    let agent_enroll = remote_argv(&calls[4]);
    for expected in [
        "'agent' 'enroll'",
        "'--target' 'owner@192.0.2.10'",
        "'--code-file' '-'",
    ] {
        assert!(
            agent_enroll.contains(expected),
            "{expected}: {agent_enroll}"
        );
    }
    assert!(!agent_enroll.contains(NODE_CODE), "{agent_enroll}");
    assert_eq!(
        calls[4].stdin.as_deref(),
        Some(format!("{NODE_CODE}\n").as_bytes())
    );

    let install = remote_argv(&calls[5]);
    assert!(
        install.contains("'agent' 'service' 'install' '--now'"),
        "{install}"
    );

    let paths = remote_argv(&calls[6]);
    for expected in [
        "'agent' 'paths' 'add'",
        "'--project' '/srv/demo'",
        "'--name' 'demo'",
        "'--allow' 'src'",
    ] {
        assert!(paths.contains(expected), "{expected}: {paths}");
    }

    let tee = remote_argv(&calls[7]);
    assert_eq!(tee, "'tee' '/srv/demo/stackspirit.toml'");
    let written = String::from_utf8(calls[7].stdin.clone().unwrap()).unwrap();
    assert!(written.contains("enabled = true"));
    assert!(written.contains("default = \"any\""));
    assert!(written.contains("result_transport = \"bundle\""));
    assert!(written.contains("max_remote_runs = 5"), "{written}");
    assert!(written.contains("pause_on_home_brake = true"), "{written}");
    assert!(written.contains("[offload.projects.demo]"), "{written}");
    assert!(
        written.contains("allow_kinds = [\"build\", \"test\"]"),
        "{written}"
    );
    assert!(!written.contains("default_placement"));

    let schedule = remote_argv(&calls[8]);
    for expected in [
        "'schedule' 'add' 'every 1d' 'command'",
        "'--name' 'cluster-backup'",
        "'--' 'vowp' 'backup' '--to' 'backup@192.0.2.20:/srv/vowp-backups'",
    ] {
        assert!(schedule.contains(expected), "{expected}: {schedule}");
    }

    let debug = format!("{formed:?}");
    assert!(!debug.contains(NODE_CODE), "{debug}");
}

#[tokio::test]
async fn form_refuses_fewer_than_two_saved_servers_before_any_command() {
    let store = empty_store();
    let home = add_host(&store, "home-vps", "192.0.2.10", None);
    let config = ClusterConfig {
        home: home.id.clone(),
        workers: vec![ClusterWorker {
            host: HostProfileId::new(),
            node_name: "worker-1".to_owned(),
            role: ClusterWorkerRole::Worker,
            agent_state: ClusterAgentState::Pending,
        }],
        backup: BackupPolicy::default(),
    };
    let request = ClusterFormRequest {
        config,
        home_project: ClusterProject {
            name: ProjectName::parse("demo").unwrap(),
            root: "/srv/demo".to_owned(),
        },
        worker_projects: Vec::new(),
        backup_target: Some("backup@192.0.2.20:/srv/vowp-backups".to_owned()),
    };
    let runner = runner(Vec::new());
    let service = ClusterService::new(store, runner.clone(), "/tmp/opencode/known_hosts");

    let error = service.form(&request).await.unwrap_err();
    assert_eq!(error.code, spirit_core::SpiritErrorCode::Refused);
    assert_eq!(runner.call_count(), 0);
}

#[tokio::test]
async fn form_refuses_home_as_worker_and_duplicate_node_names() {
    let store = empty_store();
    let home = add_host(&store, "home-vps", "192.0.2.10", None);
    let worker = add_host(&store, "worker-vps", "192.0.2.20", None);
    let runner = runner(Vec::new());
    let service = ClusterService::new(store.clone(), runner.clone(), "/tmp/opencode/known_hosts");

    let mut home_as_worker =
        form_config(&home, &[(&worker, "worker-1", ClusterWorkerRole::Worker)]);
    home_as_worker.workers[0].host = home.id.clone();
    let error = service
        .form(&form_request(home_as_worker, &worker))
        .await
        .unwrap_err();
    assert_eq!(error.code, spirit_core::SpiritErrorCode::InvalidInput);
    assert_eq!(runner.call_count(), 0);

    let mut duplicate_nodes =
        form_config(&home, &[(&worker, "worker-1", ClusterWorkerRole::Worker)]);
    duplicate_nodes.workers.push(ClusterWorker {
        host: HostProfileId::new(),
        node_name: "worker-1".to_owned(),
        role: ClusterWorkerRole::Worker,
        agent_state: ClusterAgentState::Pending,
    });
    let error = service
        .form(&form_request(duplicate_nodes, &worker))
        .await
        .unwrap_err();
    assert_eq!(error.code, spirit_core::SpiritErrorCode::InvalidInput);
    assert_eq!(runner.call_count(), 0);

    let same_target = add_host(&store, "same-vps", "192.0.2.10", None);
    let same_target_config = form_config(
        &home,
        &[(&same_target, "worker-2", ClusterWorkerRole::Worker)],
    );
    let error = service
        .form(&form_request(same_target_config, &same_target))
        .await
        .unwrap_err();
    assert_eq!(error.code, spirit_core::SpiritErrorCode::InvalidInput);
    assert_eq!(runner.call_count(), 0);
}

#[tokio::test]
async fn form_refuses_a_worker_whose_ssh_login_fails_before_any_mutation() {
    let store = empty_store();
    let home = add_host(&store, "home-vps", "192.0.2.10", None);
    let worker = add_host(&store, "worker-vps", "192.0.2.20", None);
    let config = form_config(&home, &[(&worker, "worker-1", ClusterWorkerRole::Worker)]);
    let request = form_request(config, &worker);

    let runner = runner(vec![
        version_ok(),
        CommandOutput::fail(255, "owner@192.0.2.20: Permission denied (publickey)."),
    ]);
    let service = ClusterService::new(store, runner.clone(), "/tmp/opencode/known_hosts");

    let error = service.form(&request).await.unwrap_err();
    assert_eq!(error.code, spirit_core::SpiritErrorCode::SshAuth);
    assert_eq!(runner.call_count(), 2);
    assert!(
        runner
            .calls()
            .iter()
            .all(|call| !remote_argv(call).contains("'node' 'enroll'"))
    );
}

#[tokio::test]
async fn form_never_puts_the_pairing_code_in_an_error_or_debug_output() {
    let store = empty_store();
    let home = add_host(&store, "home-vps", "192.0.2.10", None);
    let worker = add_host(&store, "worker-vps", "192.0.2.20", None);
    let config = form_config(&home, &[(&worker, "worker-1", ClusterWorkerRole::Worker)]);
    let request = form_request(config, &worker);

    let runner = runner(vec![
        version_ok(),
        version_ok(),
        CommandOutput::ok(HOME_CONFIG.as_bytes().to_vec()),
        node_enroll_json(),
        CommandOutput::fail(5, "refused"),
    ]);
    let service = ClusterService::new(store, runner.clone(), "/tmp/opencode/known_hosts");

    let error = service.form(&request).await.unwrap_err();
    assert_eq!(error.code, spirit_core::SpiritErrorCode::Refused);
    let rendered = format!("{error} {error:?}");
    assert!(!rendered.contains(NODE_CODE), "{rendered}");
    assert!(!rendered.contains("one-time-pairing"), "{rendered}");
    assert!(!format!("{request:?}").contains(NODE_CODE));
    assert!(
        !runner
            .calls()
            .iter()
            .any(|call| call.args.iter().any(|arg| arg.contains(NODE_CODE))),
        "the code must be stdin-only"
    );
    assert_eq!(
        runner.calls()[4].stdin.as_deref(),
        Some(format!("{NODE_CODE}\n").as_bytes())
    );
}

fn health_config(home: &HostProfile, worker: &HostProfile) -> ClusterConfig {
    form_config(home, &[(worker, "worker-1", ClusterWorkerRole::Worker)])
}

fn node_list_json() -> CommandOutput {
    CommandOutput::ok(
        br#"[
            {"name":"vps","ssh_target":null,"is_local":true,"capabilities":{},"last_seen_at":"2026-11-19T12:00:00.000Z","created_at":"2026-11-01T00:00:00.000Z"},
            {"name":"worker-1","ssh_target":"owner@192.0.2.20","is_local":false,"capabilities":{},"last_seen_at":"2026-11-19T12:00:00.000Z","created_at":"2026-11-01T00:00:00.000Z"}
        ]"#
        .to_vec(),
    )
}

fn offload_json(
    presence: &str,
    slots_busy: u32,
    slots_max: u32,
    last_error: Option<&str>,
) -> CommandOutput {
    CommandOutput::ok(
        serde_json::json!({
            "generated_at": "2026-11-19T12:00:00.000Z",
            "nodes": [{
                "name": "worker-1",
                "status": "active",
                "presence": presence,
                "mapped": ["demo"],
                "slots_busy": slots_busy,
                "slots_max": slots_max,
                "disk_free_bytes": 1000,
                "on_battery": null,
                "thermal_state": null,
                "last_error": last_error,
            }],
            "assignments": [],
            "ready": [],
        })
        .to_string()
        .into_bytes(),
    )
}

fn capacity_json(with_cluster: bool) -> CommandOutput {
    let mut value: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/vowp/capacity-report-full.json")).unwrap();
    if with_cluster {
        value["cluster"] = serde_json::json!({
            "online_worker_nodes": 1,
            "target_workers": 10,
            "busy_workers": 6,
            "cpu_cores": 16,
            "mem_total_bytes": 34359738368_u64,
            "mem_available_bytes": 17179869184_u64,
            "disk_free_bytes": 107374182400_u64,
        });
    }
    CommandOutput::ok(serde_json::to_vec(&value).unwrap())
}

fn fleet_json(with_cluster: bool) -> CommandOutput {
    let mut value: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/vowp/fleet-status-fresh.json")).unwrap();
    if with_cluster {
        value["snapshot"]["capacity"]["cluster"] = serde_json::json!({
            "online_worker_nodes": 1,
            "target_workers": 10,
            "busy_workers": 6,
            "cpu_cores": 16,
            "mem_total_bytes": 34359738368_u64,
            "mem_available_bytes": 17179869184_u64,
            "disk_free_bytes": 107374182400_u64,
        });
    }
    CommandOutput::ok(serde_json::to_vec(&value).unwrap())
}

#[tokio::test]
async fn health_maps_pooled_totals_and_degrades_when_cluster_is_absent() {
    let store = empty_store();
    let home = add_host(&store, "home-vps", "192.0.2.10", None);
    let worker = add_host(&store, "worker-vps", "192.0.2.20", None);
    let config = health_config(&home, &worker);

    let runner = runner(vec![
        fleet_json(false),
        node_list_json(),
        capacity_json(false),
        offload_json(
            "online",
            2,
            4,
            Some("failed to read /home/owner/secret.txt"),
        ),
    ]);
    let service = ClusterService::new(store, runner.clone(), "/tmp/opencode/known_hosts");

    let health = service.health(&config).await.unwrap();
    assert_eq!(health.home_node, "vps");
    assert_eq!(health.online_workers, 1);
    assert_eq!(health.home_target_workers, 8);
    assert_eq!(health.worker_slots, 4);
    assert_eq!(health.pooled_target_workers, 12);
    assert_eq!(health.busy_workers, 3);
    assert!(!health.cluster_totals_present);
    assert_eq!(health.totals_source, ClusterTotalsSource::Computed);
    assert!(
        health
            .warnings
            .iter()
            .any(|warning| warning.contains("did not report pooled cluster totals")),
        "{:?}",
        health.warnings
    );
    assert_eq!(health.hosts.len(), 2);
    assert_eq!(health.hosts[0].role, ClusterHostRole::Home);
    assert_eq!(health.hosts[1].role, ClusterHostRole::Worker);
    assert_eq!(health.hosts[1].agent_state, ClusterAgentState::Enrolled);
    assert_eq!(health.hosts[1].slots_busy, 2);
    assert_eq!(health.hosts[1].slots_max, 4);

    let json = serde_json::to_string(&health).unwrap();
    assert!(!json.contains("/home/owner"), "{json}");
    assert!(!json.contains("secret.txt"), "{json}");
    assert!(!json.contains(NODE_CODE), "{json}");
    assert!(!json.contains("192.0.2.20"), "{json}");
}

#[tokio::test]
async fn health_prefers_vowp_cluster_totals_when_present() {
    let store = empty_store();
    let home = add_host(&store, "home-vps", "192.0.2.10", None);
    let worker = add_host(&store, "worker-vps", "192.0.2.20", None);
    let config = health_config(&home, &worker);

    let runner = runner(vec![
        fleet_json(true),
        node_list_json(),
        capacity_json(true),
        offload_json("online", 2, 4, None),
    ]);
    let service = ClusterService::new(store, runner.clone(), "/tmp/opencode/known_hosts");

    let health = service.health(&config).await.unwrap();
    assert!(health.cluster_totals_present);
    assert_eq!(health.totals_source, ClusterTotalsSource::VowpCluster);
    assert_eq!(health.online_workers, 1);
    assert_eq!(health.home_target_workers, 8);
    assert_eq!(health.worker_slots, 2);
    assert_eq!(health.pooled_target_workers, 10);
    assert_eq!(health.busy_workers, 6);
    assert!(
        !health
            .warnings
            .iter()
            .any(|warning| warning.contains("did not report")),
        "{:?}",
        health.warnings
    );
}

#[tokio::test]
async fn health_marks_an_offline_worker_without_counting_it_as_online() {
    let store = empty_store();
    let home = add_host(&store, "home-vps", "192.0.2.10", None);
    let worker = add_host(&store, "worker-vps", "192.0.2.20", None);
    let config = health_config(&home, &worker);

    let runner = runner(vec![
        fleet_json(false),
        node_list_json(),
        capacity_json(false),
        offload_json("offline", 2, 4, None),
    ]);
    let service = ClusterService::new(store, runner.clone(), "/tmp/opencode/known_hosts");

    let health = service.health(&config).await.unwrap();
    assert_eq!(health.online_workers, 0);
    assert_eq!(
        health.worker_slots, 0,
        "last-known offline slots must not count as pooled capacity"
    );
    assert_eq!(health.pooled_target_workers, 8);
    assert_eq!(
        health.busy_workers, 1,
        "offline worker slots are not part of the pooled busy total"
    );
    assert_eq!(health.hosts[1].agent_state, ClusterAgentState::Offline);
    assert_eq!(health.hosts[1].presence.as_str(), "offline");
    assert_eq!(
        health.hosts[1].slots_max, 4,
        "the per-host row keeps the last-known capacity"
    );
}

#[tokio::test]
async fn health_sanitizes_unknown_enum_values_instead_of_leaking_them() {
    let store = empty_store();
    let home = add_host(&store, "home-vps", "192.0.2.10", None);
    let worker = add_host(&store, "worker-vps", "192.0.2.20", None);
    let config = health_config(&home, &worker);

    let offload = serde_json::json!({
        "generated_at": "2026-11-19T12:00:00.000Z",
        "nodes": [{
            "name": "worker-1",
            "status": "/home/owner/private-status",
            "presence": "token-abc",
            "mapped": ["demo"],
            "slots_busy": 1,
            "slots_max": 2,
            "last_error": null,
        }],
        "assignments": [],
        "ready": [],
    });
    let runner = runner(vec![
        fleet_json(false),
        node_list_json(),
        capacity_json(false),
        CommandOutput::ok(serde_json::to_vec(&offload).unwrap()),
    ]);
    let service = ClusterService::new(store, runner.clone(), "/tmp/opencode/known_hosts");

    let health = service.health(&config).await.unwrap();
    assert_eq!(health.hosts[1].status.as_str(), "unknown");
    assert_eq!(health.hosts[1].presence.as_str(), "unknown");
    assert_eq!(health.hosts[1].agent_state, ClusterAgentState::Unknown);
    let json = serde_json::to_string(&health).unwrap();
    assert!(!json.contains("/home/owner"), "{json}");
    assert!(!json.contains("token-abc"), "{json}");
}

fn promote_fixture() -> (Arc<dyn HostStore>, ClusterConfig, HostProfileId) {
    let store = empty_store();
    let home = add_host(&store, "home-vps", "192.0.2.10", None);
    let worker = add_host(
        &store,
        "worker-vps",
        "192.0.2.20",
        Some("/srv/vowp-restored"),
    );
    let config = form_config(&home, &[(&worker, "worker-1", ClusterWorkerRole::Backup)]);
    (store, config, worker.id.clone())
}

fn promote_request(
    config: ClusterConfig,
    target: HostProfileId,
    backup_at: Timestamp,
) -> PromoteRequest {
    PromoteRequest {
        config,
        target,
        backup_dir: "/srv/vowp-backups/2026-11-19".to_owned(),
        backup_at: Some(backup_at),
        new_home: None,
        confirmation: PROMOTE_CONFIRMATION_TOKEN.to_owned(),
    }
}

#[tokio::test]
async fn promote_requires_the_explicit_confirmation_token_before_any_command() {
    let (store, config, target) = promote_fixture();
    let now = Timestamp::parse("2026-11-19T12:00:00.000Z").unwrap();
    let runner = runner(Vec::new());
    let service = ClusterService::new(store, runner.clone(), "/tmp/opencode/known_hosts")
        .with_clock(Arc::new(FakeClock::at(now)));

    let mut request = promote_request(config, target, now.minus_secs(60 * 60));
    request.confirmation = "yes".to_owned();
    let error = service.promote(&request).await.unwrap_err();
    assert_eq!(error.code, spirit_core::SpiritErrorCode::Refused);
    assert_eq!(runner.call_count(), 0);

    request.confirmation = String::new();
    let error = service.promote(&request).await.unwrap_err();
    assert_eq!(error.code, spirit_core::SpiritErrorCode::Refused);
    assert_eq!(runner.call_count(), 0);
}

#[tokio::test]
async fn promote_refuses_a_stale_backup_before_any_command() {
    let (store, config, target) = promote_fixture();
    let now = Timestamp::parse("2026-11-19T12:00:00.000Z").unwrap();
    let runner = runner(Vec::new());
    let service = ClusterService::new(store, runner.clone(), "/tmp/opencode/known_hosts")
        .with_clock(Arc::new(FakeClock::at(now)));

    let request = promote_request(config, target, now.minus_secs(2 * 24 * 60 * 60));
    let error = service.promote(&request).await.unwrap_err();
    assert_eq!(error.code, spirit_core::SpiritErrorCode::Refused);
    assert!(error.message.contains("stale"), "{error}");
    assert_eq!(runner.call_count(), 0);
}

#[tokio::test]
async fn promote_refuses_an_unreachable_target_without_draining_home() {
    let (store, config, target) = promote_fixture();
    let now = Timestamp::parse("2026-11-19T12:00:00.000Z").unwrap();
    let runner = runner(vec![CommandOutput::fail(
        255,
        "owner@192.0.2.20: Permission denied (publickey).",
    )]);
    let service = ClusterService::new(store, runner.clone(), "/tmp/opencode/known_hosts")
        .with_clock(Arc::new(FakeClock::at(now)));

    let request = promote_request(config, target, now.minus_secs(60 * 60));
    let error = service.promote(&request).await.unwrap_err();
    assert_eq!(error.code, spirit_core::SpiritErrorCode::SshAuth);
    assert_eq!(runner.call_count(), 1);
    assert!(!remote_argv(&runner.calls()[0]).contains("'down'"));
}

#[tokio::test]
async fn promote_reads_the_remote_backup_mtime_when_no_timestamp_is_supplied() {
    let (store, config, target) = promote_fixture();
    let now = Timestamp::parse("2026-11-19T12:00:00.000Z").unwrap();
    let runner = runner(vec![
        version_ok(),
        stat_ok(now.minus_secs(60 * 60)),
        CommandOutput::ok(Vec::new()),
        CommandOutput::ok(Vec::new()),
        CommandOutput::ok(Vec::new()),
        fleet_json(false),
    ]);
    let service = ClusterService::new(store, runner.clone(), "/tmp/opencode/known_hosts")
        .with_clock(Arc::new(FakeClock::at(now)));

    let mut request = promote_request(config, target.clone(), now.minus_secs(60 * 60));
    request.backup_at = None;
    let outcome = service.promote(&request).await.unwrap();
    assert_eq!(outcome.new_home, target);
    assert_eq!(outcome.backup_at, now.minus_secs(60 * 60));

    let calls = runner.calls();
    assert_eq!(calls.len(), 6);
    let stat = remote_argv(&calls[1]);
    assert!(
        stat.contains("'stat' '-c' '%Y' '/srv/vowp-backups/2026-11-19'"),
        "{stat}"
    );
    assert!(remote_argv(&calls[2]).contains("'down' '--drain'"));
}

#[tokio::test]
async fn promote_drains_home_restores_the_worker_and_returns_the_new_identity() {
    let (store, config, target) = promote_fixture();
    let now = Timestamp::parse("2026-11-19T12:00:00.000Z").unwrap();
    let runner = runner(vec![
        version_ok(),
        stat_ok(now.minus_secs(60 * 60)),
        CommandOutput::ok(Vec::new()),
        CommandOutput::ok(Vec::new()),
        CommandOutput::ok(Vec::new()),
        fleet_json(false),
    ]);
    let service = ClusterService::new(store, runner.clone(), "/tmp/opencode/known_hosts")
        .with_clock(Arc::new(FakeClock::at(now)));

    let request = promote_request(config, target.clone(), now.minus_secs(60 * 60));
    let outcome = service.promote(&request).await.unwrap();
    assert_eq!(outcome.new_home, target);
    assert_eq!(outcome.node_name, "vps");
    assert_eq!(outcome.home_dir, "/srv/vowp-restored");
    assert_eq!(outcome.backup_at, now.minus_secs(60 * 60));

    let calls = runner.calls();
    assert_eq!(calls.len(), 6);
    assert!(remote_argv(&calls[0]).contains("'--version'"));
    assert!(remote_argv(&calls[1]).contains("'stat' '-c' '%Y' '/srv/vowp-backups/2026-11-19'"));
    assert!(remote_argv(&calls[2]).contains("'down' '--drain'"));
    let restore = remote_argv(&calls[3]);
    for expected in [
        "'backup' '--restore' '/srv/vowp-backups/2026-11-19'",
        "'--to-home' '/srv/vowp-restored'",
    ] {
        assert!(restore.contains(expected), "{expected}: {restore}");
    }
    let up = remote_argv(&calls[4]);
    assert!(up.contains("'--home' '/srv/vowp-restored' 'up'"), "{up}");
    let status = remote_argv(&calls[5]);
    assert!(
        status.contains("'--home' '/srv/vowp-restored' '--json' 'fleet' 'status'"),
        "{status}"
    );
}
