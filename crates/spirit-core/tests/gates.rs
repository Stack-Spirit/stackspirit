//! Gate probes: ASIF and local-orchestrator capability evidence, never version-only.

use std::sync::Mutex;

use async_trait::async_trait;
use spirit_core::engine::gate::{
    AsifGate, GateCache, GateFeatureFlags, GateProbeOptions, GateSnapshot, LocalAgentState,
    LocalOrchestratorGate, probe_asif, probe_local_orchestrator,
};
use spirit_core::ssh::command::{CommandOutput, CommandSpec};
use spirit_core::ssh::{CommandRunner, SshOptions, SshTarget};
use spirit_core::time::Timestamp;

const GATE_PHASE3_READY: &str = include_str!("../fixtures/vowp/gate-phase3-ready.json");
const GATE_ASIF_READY: &str = include_str!("../fixtures/vowp/gate-asif-ready.json");
const GATE_NODE_LIST_V03: &str = include_str!("../fixtures/vowp/gate-node-list-v0.3.json");
const GATE_NODE_SHOW_HOME_OFFLINE_V03: &str =
    include_str!("../fixtures/vowp/gate-node-show-v0.3-home-offline.json");

/// The installed `vowp orchestrator new --help` output (2026-11-16): `--node` appears only
/// as the global SSH selector. `--project`/`--home` values are the CLI defaults, not a host.
const ORCHESTRATOR_NEW_HELP_GLOBAL_NODE_ONLY: &str = "\
Create and start an orchestrator

Usage: vowp orchestrator new [OPTIONS] --app <APP>

Options:
      --json                 Print one machine-readable JSON document on stdout
      --name <NAME>          Orchestrator name; defaults to the project name, or `orchestrator`
      --node <NAME>          Run the command on that node over ssh
      --project <NAME>       Project to bind, when any
      --app <APP>            App to run: `opencode`, `claude-code` or `chat`
      --executor <EXECUTOR>  Executor to use
      --home <DIR>           Node home (default `$VOWP_HOME`, else `~/.vowp`)
      --force                Replace an orchestrator with the same name
      --no-color             Disable colour even when stdout is a terminal
  -q, --quiet                Suppress non-essential human output
  -v, --verbose...           Increase log verbosity (`-v`, `-vv`)
  -h, --help                 Print help
";

struct Route {
    needles: Vec<&'static str>,
    output: CommandOutput,
}

struct RouteRunner {
    routes: Vec<Route>,
    default: CommandOutput,
    calls: Mutex<Vec<CommandSpec>>,
}

impl RouteRunner {
    fn new(routes: Vec<Route>, default: CommandOutput) -> Self {
        Self {
            routes,
            default,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<CommandSpec> {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl CommandRunner for RouteRunner {
    async fn run(
        &self,
        spec: &CommandSpec,
    ) -> Result<CommandOutput, spirit_core::error::SpiritError> {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(spec.clone());
        let haystack = format!("{} {}", spec.program, spec.args.join(" "));
        let route = self
            .routes
            .iter()
            .find(|route| route.needles.iter().all(|needle| haystack.contains(needle)));
        Ok(route.map_or_else(|| self.default.clone(), |route| route.output.clone()))
    }
}

fn out(status: i32, stdout: &str, stderr: &str) -> CommandOutput {
    CommandOutput {
        status,
        stdout: stdout.as_bytes().to_vec(),
        stderr: stderr.as_bytes().to_vec(),
        duration_ms: 1,
        stderr_truncated: false,
    }
}

fn ok(stdout: &str) -> CommandOutput {
    out(0, stdout, "")
}

fn failed(status: i32, stderr: &str) -> CommandOutput {
    out(status, "", stderr)
}

/// The remote command an SSH spec would execute, already shell-quoted.
fn remote_argv(spec: &CommandSpec) -> String {
    spec.args.last().cloned().unwrap_or_default()
}

fn options() -> GateProbeOptions {
    GateProbeOptions {
        target: SshTarget::new("owner", "192.0.2.10", 22).unwrap(),
        ssh: SshOptions::new("/tmp/opencode/known_hosts"),
        home: None,
        active_project: Some("vowp".to_owned()),
    }
}

fn version_route() -> Route {
    Route {
        needles: vec!["--version"],
        output: ok("vowp 0.4.0\n"),
    }
}

fn asif_routes(export: CommandOutput, import_help: CommandOutput) -> Vec<Route> {
    vec![
        version_route(),
        Route {
            needles: vec!["session", "export"],
            output: export,
        },
        Route {
            needles: vec!["session", "import", "--help"],
            output: import_help,
        },
    ]
}

fn home_routes() -> Vec<Route> {
    vec![
        version_route(),
        Route {
            needles: vec!["node", "ls"],
            output: ok(r#"[{"name":"mac-studio","ssh_target":"mac@192.0.2.20"}]"#),
        },
        Route {
            needles: vec!["node", "show", "mac-studio"],
            output: ok(
                r##"{"name":"mac-studio","kind":"agent","status":"active","presence":"online","labels":[],"runs":0,"mapped":["vowp"],"agent_version":"0.4.0","protocol":"vowp-agent/1","last_error":null}"##,
            ),
        },
        Route {
            needles: vec!["offload", "status"],
            output: ok(r#"{"enabled":true,"spool_bytes":0}"#),
        },
        Route {
            needles: vec!["surface", "start"],
            output: ok("Usage: vowp agent surface start"),
        },
        Route {
            needles: vec!["surface", "attach"],
            output: ok("Usage: vowp agent surface attach"),
        },
        Route {
            needles: vec!["mcp", "--help"],
            output: ok("Usage: vowp mcp --agent-socket <path> --surface <id>"),
        },
        Route {
            needles: vec!["orchestrator", "new"],
            output: ok(
                "Create and start an orchestrator\n\nUsage: vowp orchestrator new [OPTIONS] --app <APP>\n\nOptions:\n      --node <NAME>          Run the command on that node over ssh\n\nFor `orchestrator new`, --node <name> names the node that runs the app, not an SSH redirect.",
            ),
        },
        Route {
            needles: vec!["node", "paths"],
            output: ok(r#"{"project":"/srv/vowp"}"#),
        },
        Route {
            needles: vec!["orchestrator", "ls"],
            output: ok("[]"),
        },
    ]
}

fn local_runner(status: &str, connection: &str, protocol: &str) -> RouteRunner {
    RouteRunner::new(
        vec![Route {
            needles: vec!["agent", "status"],
            output: ok(&format!(
                r#"{{"connection":"{connection}","node":"mac-studio","home":"vps","protocol":"{protocol}","last_heartbeat":"2026-11-14T12:05:00.000Z","last_error":null}}"#
            )),
        }],
        failed(1, status),
    )
}

#[tokio::test]
async fn asif_probe_waiting_when_export_flag_is_unsupported() {
    let runner = RouteRunner::new(
        asif_routes(
            failed(2, "error: invalid value 'asif' for '--format <FORMAT>'"),
            ok("Usage: vowp session import <path>"),
        ),
        failed(1, "unexpected"),
    );
    let gate = probe_asif(&runner, &options()).await;
    match gate {
        AsifGate::WaitingForVowpAsif {
            missing,
            detected_version,
            ..
        } => {
            assert_eq!(
                missing,
                vec!["v4-asif-core".to_owned(), "v4-asif-cli".to_owned()]
            );
            assert_eq!(detected_version.as_deref(), Some("0.4.0"));
        }
        other => panic!("expected waiting, got {other:?}"),
    }
}

#[tokio::test]
async fn asif_probe_ready_export_only_when_import_help_is_ambiguous() {
    let runner = RouteRunner::new(
        asif_routes(
            failed(3, "session not found"),
            ok("Usage: vowp session import <path> --format <FORMAT>"),
        ),
        failed(1, "unexpected"),
    );
    let gate = probe_asif(&runner, &options()).await;
    match gate {
        AsifGate::Ready { export, import, .. } => {
            assert!(export);
            assert!(!import);
        }
        other => panic!("expected partial ready, got {other:?}"),
    }
}

#[tokio::test]
async fn asif_probe_ready_both_when_export_and_import_are_proven() {
    let runner = RouteRunner::new(
        asif_routes(
            failed(3, "session not found"),
            ok(
                "Usage: vowp session import <path> --format <FORMAT>\n\nPossible values: asif, native",
            ),
        ),
        failed(1, "unexpected"),
    );
    let gate = probe_asif(&runner, &options()).await;
    match gate {
        AsifGate::Ready {
            export,
            import,
            evidence,
            ..
        } => {
            assert!(export);
            assert!(import);
            assert!(evidence.iter().any(|line| line.contains("exit 3")));
        }
        other => panic!("expected ready, got {other:?}"),
    }
}

#[tokio::test]
async fn asif_probe_never_ready_from_version_alone() {
    let runner = RouteRunner::new(vec![version_route()], failed(1, "command not found"));
    let gate = probe_asif(&runner, &options()).await;
    assert!(!matches!(gate, AsifGate::Ready { .. }));
    assert!(matches!(
        gate,
        AsifGate::WaitingForVowpAsif { .. } | AsifGate::Unknown
    ));

    let mut no_project = options();
    no_project.active_project = None;
    let gate = probe_asif(&runner, &no_project).await;
    assert!(matches!(gate, AsifGate::Unknown));
}

#[tokio::test]
async fn asif_probe_marks_inconsistent_help_as_incompatible() {
    let runner = RouteRunner::new(
        asif_routes(
            failed(2, "error: invalid value 'asif' for '--format <FORMAT>'"),
            ok("Usage: vowp session import <path> --format <FORMAT>\nPossible values: asif"),
        ),
        failed(1, "unexpected"),
    );
    let gate = probe_asif(&runner, &options()).await;
    assert!(matches!(gate, AsifGate::Incompatible { .. }));
}

#[tokio::test]
async fn phase3_probe_waiting_when_surface_command_is_absent() {
    let mut routes = home_routes();
    routes.retain(|route| !route.needles.contains(&"surface") || !route.needles.contains(&"start"));
    let runner = RouteRunner::new(routes, failed(2, "unknown command"));
    let agent = local_runner("", "connected", "vowp-agent/1");
    let (gate, local) = probe_local_orchestrator(&runner, Some(&agent), &options()).await;
    match gate {
        LocalOrchestratorGate::WaitingForVowpPhase3 { missing, .. } => {
            assert!(missing.contains(&"local opencode app host".to_owned()));
        }
        other => panic!("expected waiting, got {other:?}"),
    }
    assert_eq!(local.state, LocalAgentState::Connected);
}

#[tokio::test]
async fn phase3_probe_incompatible_on_wrong_agent_protocol() {
    let mut routes = home_routes();
    for route in &mut routes {
        if route.needles.contains(&"show") {
            route.output = ok(
                r#"{"name":"mac-studio","kind":"agent","status":"active","presence":"online","protocol":"vowp-agent/2"}"#,
            );
        }
    }
    let runner = RouteRunner::new(routes, failed(1, "unexpected"));
    let agent = local_runner("", "connected", "vowp-agent/1");
    let (gate, _) = probe_local_orchestrator(&runner, Some(&agent), &options()).await;
    match gate {
        LocalOrchestratorGate::Incompatible {
            detected, required, ..
        } => {
            assert_eq!(detected, "vowp-agent/2");
            assert_eq!(required, "vowp-agent/1");
        }
        other => panic!("expected incompatible, got {other:?}"),
    }
}

#[tokio::test]
async fn phase3_probe_does_not_accept_agent_status_and_node_model_alone() {
    let routes = vec![
        version_route(),
        Route {
            needles: vec!["node", "ls"],
            output: ok(r#"[{"name":"mac-studio"}]"#),
        },
        Route {
            needles: vec!["node", "show", "mac-studio"],
            output: ok(
                r#"{"name":"mac-studio","kind":"agent","status":"active","presence":"online","protocol":"vowp-agent/1"}"#,
            ),
        },
    ];
    let runner = RouteRunner::new(routes, failed(2, "unknown command"));
    let agent = local_runner("", "connected", "vowp-agent/1");
    let (gate, local) = probe_local_orchestrator(&runner, Some(&agent), &options()).await;
    assert!(matches!(
        gate,
        LocalOrchestratorGate::WaitingForVowpPhase3 { .. }
    ));
    assert_eq!(local.state, LocalAgentState::Connected);
}

#[tokio::test]
async fn phase3_probe_ready_only_with_offload_and_surface_capabilities() {
    let runner = RouteRunner::new(home_routes(), failed(1, "unexpected"));
    let agent = local_runner("", "connected", "vowp-agent/1");
    let (gate, local) = probe_local_orchestrator(&runner, Some(&agent), &options()).await;
    match gate {
        LocalOrchestratorGate::Ready {
            agent_protocol,
            node,
            capabilities,
            evidence,
        } => {
            assert_eq!(agent_protocol, "vowp-agent/1");
            assert_eq!(node.as_deref(), Some("mac-studio"));
            assert!(capabilities.contains(&"offload_status".to_owned()));
            assert!(capabilities.contains(&"surface_start".to_owned()));
            assert!(capabilities.contains(&"mcp_agent_socket".to_owned()));
            assert!(capabilities.contains(&"orchestrator_new_node".to_owned()));
            assert!(evidence.iter().any(|line| line.contains("offload status")));
            assert!(
                evidence
                    .iter()
                    .any(|line| line.contains("local-node meaning present"))
            );
        }
        other => panic!("expected ready, got {other:?}"),
    }
    assert_eq!(local.state, LocalAgentState::Connected);
    assert!(runner.calls().len() >= 9);
}

#[tokio::test]
async fn phase3_probe_passes_the_active_project_to_node_reads() {
    let runner = RouteRunner::new(home_routes(), failed(1, "unexpected"));
    let (gate, _) = probe_local_orchestrator(&runner, None, &options()).await;
    assert!(matches!(gate, LocalOrchestratorGate::Ready { .. }));
    let calls = runner.calls();
    for needle in ["'node' 'ls'", "'node' 'show'", "'node' 'paths'"] {
        let command = calls
            .iter()
            .map(remote_argv)
            .find(|command| command.contains(needle))
            .unwrap_or_else(|| panic!("no {needle} call"));
        assert!(
            command.contains("'--project' 'vowp'"),
            "project not passed to {needle}: {command}"
        );
    }
}

#[tokio::test]
async fn phase3_probe_waits_without_a_project_and_skips_global_node_reads() {
    let runner = RouteRunner::new(home_routes(), failed(1, "unexpected"));
    let mut no_project = options();
    no_project.active_project = None;
    let (gate, _) = probe_local_orchestrator(&runner, None, &no_project).await;
    match gate {
        LocalOrchestratorGate::WaitingForVowpPhase3 {
            missing, evidence, ..
        } => {
            assert_eq!(missing, vec!["active project".to_owned()]);
            assert!(
                evidence
                    .iter()
                    .any(|line| line.contains("no usable active project"))
            );
            assert!(evidence.iter().any(|line| line.contains("not attempted")));
        }
        other => panic!("expected waiting, got {other:?}"),
    }
    let calls = runner.calls();
    assert!(
        calls
            .iter()
            .all(|spec| !remote_argv(spec).contains("'node'")),
        "a bare global node read must not run without a project: {calls:?}"
    );
}

#[tokio::test]
async fn phase3_probe_waits_on_the_v0_3_offline_home_node() {
    let mut routes = home_routes();
    for route in &mut routes {
        if route.needles == ["node", "ls"] {
            route.output = ok(GATE_NODE_LIST_V03);
        }
        if route.needles.contains(&"show") {
            route.needles = vec!["node", "show"];
            route.output = ok(GATE_NODE_SHOW_HOME_OFFLINE_V03);
        }
    }
    let runner = RouteRunner::new(routes, failed(1, "unexpected"));
    let (gate, _) = probe_local_orchestrator(&runner, None, &options()).await;
    match gate {
        LocalOrchestratorGate::WaitingForVowpPhase3 {
            missing, evidence, ..
        } => {
            assert!(missing.contains(&"no active local agent node".to_owned()));
            assert!(
                evidence
                    .iter()
                    .any(|line| line.contains("kind=home") && line.contains("presence=offline"))
            );
        }
        other => panic!("expected waiting, got {other:?}"),
    }
}

#[tokio::test]
async fn phase3_probe_does_not_accept_an_offline_agent_or_a_missing_agent_version() {
    let offline_agent = r#"{"name":"mac-studio","kind":"agent","status":"active","presence":"offline","agent_version":"0.4.0","protocol":"vowp-agent/1"}"#;
    let mut routes = home_routes();
    for route in &mut routes {
        if route.needles.contains(&"show") {
            route.output = ok(offline_agent);
        }
    }
    let runner = RouteRunner::new(routes, failed(1, "unexpected"));
    let (gate, _) = probe_local_orchestrator(&runner, None, &options()).await;
    match gate {
        LocalOrchestratorGate::WaitingForVowpPhase3 {
            missing, evidence, ..
        } => {
            assert!(missing.contains(&"no active local agent node".to_owned()));
            assert!(evidence.iter().any(|line| line.contains("not online")));
        }
        other => panic!("expected waiting, got {other:?}"),
    }

    let no_agent_version = r#"{"name":"mac-studio","kind":"agent","status":"active","presence":"online","agent_version":null,"protocol":"vowp-agent/1"}"#;
    let mut routes = home_routes();
    for route in &mut routes {
        if route.needles.contains(&"show") {
            route.output = ok(no_agent_version);
        }
    }
    let runner = RouteRunner::new(routes, failed(1, "unexpected"));
    let (gate, _) = probe_local_orchestrator(&runner, None, &options()).await;
    match gate {
        LocalOrchestratorGate::WaitingForVowpPhase3 {
            missing, evidence, ..
        } => {
            assert!(missing.contains(&"agent version not reported".to_owned()));
            assert!(evidence.iter().any(|line| line.contains("not reported")));
        }
        other => panic!("expected waiting, got {other:?}"),
    }
}

#[tokio::test]
async fn phase3_probe_rejects_the_global_ssh_node_as_local_node_proof() {
    let mut routes = home_routes();
    for route in &mut routes {
        if route.needles == ["orchestrator", "new"] {
            route.output = ok(ORCHESTRATOR_NEW_HELP_GLOBAL_NODE_ONLY);
        }
    }
    let runner = RouteRunner::new(routes, failed(1, "unexpected"));
    let (gate, _) = probe_local_orchestrator(&runner, None, &options()).await;
    match gate {
        LocalOrchestratorGate::WaitingForVowpPhase3 {
            missing, evidence, ..
        } => {
            assert!(missing.contains(&"orchestrator materialization".to_owned()));
            assert!(
                evidence
                    .iter()
                    .any(|line| line.contains("global SSH selector"))
            );
            assert!(
                !evidence
                    .iter()
                    .any(|line| line.contains("local-node meaning present"))
            );
        }
        other => panic!("expected waiting, got {other:?}"),
    }
}

#[tokio::test]
async fn phase3_probe_reports_missing_node_paths_as_missing_local_cache() {
    let mut routes = home_routes();
    for route in &mut routes {
        if route.needles == ["node", "paths"] {
            route.output = failed(2, "error: unrecognized subcommand 'paths'");
        }
    }
    let runner = RouteRunner::new(routes, failed(1, "unexpected"));
    let (gate, _) = probe_local_orchestrator(&runner, None, &options()).await;
    match gate {
        LocalOrchestratorGate::WaitingForVowpPhase3 {
            missing, evidence, ..
        } => {
            assert!(missing.contains(&"local cache".to_owned()));
            assert!(
                evidence
                    .iter()
                    .any(|line| line == "vowp node paths --json: exit 2 (unsupported subcommand)")
            );
            assert!(!missing.contains(&"orchestrator materialization".to_owned()));
        }
        other => panic!("expected waiting, got {other:?}"),
    }
}

#[tokio::test]
async fn phase3_probe_requires_orchestrator_new_node() {
    let mut routes = home_routes();
    for route in &mut routes {
        if route.needles == ["orchestrator", "new"] {
            route.output = ok("Usage: vowp orchestrator new [--name <name>]");
        }
    }
    let runner = RouteRunner::new(routes, failed(1, "unexpected"));
    let (gate, _) = probe_local_orchestrator(&runner, None, &options()).await;
    match gate {
        LocalOrchestratorGate::WaitingForVowpPhase3 { missing, .. } => {
            assert!(missing.contains(&"orchestrator materialization".to_owned()));
        }
        other => panic!("expected waiting, got {other:?}"),
    }
}

#[tokio::test]
async fn phase3_probe_reports_the_orchestrator_read_model_label() {
    let mut routes = home_routes();
    for route in &mut routes {
        if route.needles == ["orchestrator", "ls"] {
            route.output = failed(1, "unknown command");
        }
    }
    let runner = RouteRunner::new(routes, failed(1, "unexpected"));
    let (gate, _) = probe_local_orchestrator(&runner, None, &options()).await;
    match gate {
        LocalOrchestratorGate::WaitingForVowpPhase3 { missing, .. } => {
            assert!(
                missing.contains(&"orchestrator read model".to_owned()),
                "VOWP-CONTRACT.md §11.3.4 step 8 names this capability: {missing:?}"
            );
            assert!(!missing.contains(&"backfill".to_owned()));
        }
        other => panic!("expected waiting, got {other:?}"),
    }
}

#[tokio::test]
async fn phase3_probe_separates_home_capability_from_local_agent_status() {
    let runner = RouteRunner::new(home_routes(), failed(1, "unexpected"));
    let (gate, local) = probe_local_orchestrator(&runner, None, &options()).await;
    assert!(matches!(gate, LocalOrchestratorGate::Ready { .. }));
    assert_eq!(local.state, LocalAgentState::NotInstalled);

    let missing = RouteRunner::new(vec![version_route()], failed(1, "not installed"));
    let agent = local_runner("", "connected", "vowp-agent/1");
    let (gate, local) = probe_local_orchestrator(&missing, Some(&agent), &options()).await;
    assert!(!matches!(gate, LocalOrchestratorGate::Ready { .. }));
    assert_eq!(local.state, LocalAgentState::Connected);
}

#[tokio::test]
async fn local_agent_states_map_exactly() {
    let runner = RouteRunner::new(home_routes(), failed(1, "unexpected"));
    for (connection, expected) in [
        ("connected", LocalAgentState::Connected),
        ("connecting", LocalAgentState::Connecting),
        ("reconnecting", LocalAgentState::Reconnecting),
        ("revoked", LocalAgentState::Revoked),
        ("superseded", LocalAgentState::Superseded),
        ("stopped", LocalAgentState::Stopped),
        ("unknown", LocalAgentState::Unknown),
    ] {
        let agent = local_runner("", connection, "vowp-agent/1");
        let (_, local) = probe_local_orchestrator(&runner, Some(&agent), &options()).await;
        assert_eq!(local.state, expected, "{connection}");
    }
    let not_enrolled = RouteRunner::new(vec![], failed(3, "no enrollment"));
    let (_, local) = probe_local_orchestrator(&runner, Some(&not_enrolled), &options()).await;
    assert_eq!(local.state, LocalAgentState::NotEnrolled);
    let wrong_protocol = local_runner("", "connected", "vowp-agent/2");
    let (_, local) = probe_local_orchestrator(&runner, Some(&wrong_protocol), &options()).await;
    assert_eq!(local.state, LocalAgentState::Incompatible);
}

#[tokio::test]
async fn gate_evidence_has_no_paths_or_secrets() {
    let runner = RouteRunner::new(home_routes(), failed(1, "unexpected"));
    let agent = local_runner("", "connected", "vowp-agent/1");
    let (gate, _) = probe_local_orchestrator(&runner, Some(&agent), &options()).await;
    let evidence = match gate {
        LocalOrchestratorGate::Ready { evidence, .. }
        | LocalOrchestratorGate::WaitingForVowpPhase3 { evidence, .. }
        | LocalOrchestratorGate::Incompatible { evidence, .. } => evidence,
        other => panic!("unexpected {other:?}"),
    };
    assert!(!evidence.is_empty());
    for line in evidence {
        assert!(!line.contains("/home/"), "{line}");
        assert!(!line.contains("/Users/"), "{line}");
        assert!(!line.contains("known_hosts"), "{line}");
        assert!(!line.contains(&["s", "k", "-"].concat()), "{line}");
        assert!(!line.contains(&["g", "hp", "_"].concat()), "{line}");
        assert!(!line.contains(&["to", "ken="].concat()), "{line}");
        assert!(line.len() < 300, "{line}");
    }
    // No evidence for a status-only probe can be Ready.
    let only_version = RouteRunner::new(vec![version_route()], failed(1, "not found"));
    let (gate, _) = probe_local_orchestrator(&only_version, None, &options()).await;
    assert!(!matches!(gate, LocalOrchestratorGate::Ready { .. }));
}

#[test]
fn gate_cache_expires_and_downgrades() {
    let cache = GateCache::new();
    let probed = Timestamp::parse("2026-11-14T12:00:00.000Z").unwrap();
    let mut snapshot: GateSnapshot = serde_json::from_str(GATE_PHASE3_READY).unwrap();
    snapshot.probed_at = probed;
    cache.put(snapshot);
    let fresh = cache.get(probed.plus_secs(600)).unwrap();
    assert!(matches!(
        fresh.local_orchestrator,
        LocalOrchestratorGate::Ready { .. }
    ));
    assert!(cache.get(probed.plus_secs(601)).is_none());
    assert!(
        cache
            .get(Timestamp::parse("2026-11-14T12:10:01.000Z").unwrap())
            .is_none()
    );

    let mut waiting: GateSnapshot = serde_json::from_str(GATE_ASIF_READY).unwrap();
    waiting.probed_at = probed.plus_secs(700);
    cache.put(waiting);
    let downgraded = cache.get(probed.plus_secs(701)).unwrap();
    assert!(matches!(
        downgraded.local_orchestrator,
        LocalOrchestratorGate::WaitingForVowpPhase3 { .. }
    ));
}

#[test]
fn gate_ready_fixture_still_disables_unimplemented_action() {
    let snapshot: GateSnapshot = serde_json::from_str(GATE_PHASE3_READY).unwrap();
    assert!(matches!(
        snapshot.local_orchestrator,
        LocalOrchestratorGate::Ready { .. }
    ));
    assert_eq!(snapshot.features, GateFeatureFlags::default());
    assert!(!snapshot.local_orchestrator_enabled());
    let asif: GateSnapshot = serde_json::from_str(GATE_ASIF_READY).unwrap();
    assert!(matches!(
        asif.asif,
        AsifGate::Ready {
            export: true,
            import: true,
            ..
        }
    ));
    assert!(!asif.asif_export_enabled());
    assert!(!asif.asif_import_enabled());
}
