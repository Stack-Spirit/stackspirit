//! Gate probes: ASIF and local-orchestrator capability evidence, never version-only.
//!
//! The D6 probes mirror the shipped VOWP v0.4.0 CLI surface: `surface pair|ls|revoke|rotate`,
//! `agent pair|run|paths|status|service|doctor|trust`, `orchestrator new|ls|stop|resume`, and
//! `mcp --agent-socket`. `agent surface start|attach|detach` is a design sketch, not a
//! command in the shipped CLI, so it must never be required or accepted. `agent trust` is an
//! optional owner-only security action, never a `Ready` gate and never invoked without `--help`.

use std::sync::Mutex;

use async_trait::async_trait;
use spirit_core::engine::gate::{
    AsifGate, GateCache, GateProbeOptions, GateSnapshot, LocalAgentState, LocalOrchestratorGate,
    probe_asif, probe_local_orchestrator,
};
use spirit_core::ssh::command::{CommandOutput, CommandSpec};
use spirit_core::ssh::{CommandRunner, SshOptions, SshTarget};
use spirit_core::time::Timestamp;

const GATE_PHASE3_READY: &str = include_str!("../fixtures/vowp/gate-phase3-ready.json");
const GATE_PHASE3_WAITING: &str = include_str!("../fixtures/vowp/gate-phase3-waiting.json");
const GATE_ASIF_READY: &str = include_str!("../fixtures/vowp/gate-asif-ready.json");
const GATE_NODE_LIST_V03: &str = include_str!("../fixtures/vowp/gate-node-list-v0.3.json");
const GATE_NODE_SHOW_HOME_OFFLINE_V03: &str =
    include_str!("../fixtures/vowp/gate-node-show-v0.3-home-offline.json");

/// A pre-0.4.0 `vowp orchestrator new --help` output: `--node` appears only as the global
/// SSH selector. `--project`/`--home` values are the CLI defaults, not a host.
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

/// The shipped `vowp 0.4.0` `orchestrator new --help` output: the local-node after-help is
/// present. Captured read-only from the installed binary on 2026-11-18.
const ORCHESTRATOR_NEW_HELP_0_4_0: &str = "\
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

For `orchestrator new`, --node <name> names the node that runs the app, not an SSH redirect.
";

/// The shipped `vowp 0.4.0` `surface pair --help` output, including `--replace` and the
/// local-node after-help. Captured read-only from the installed binary on 2026-11-18.
const SURFACE_PAIR_HELP_0_4_0: &str = "\
Create a pending surface and print its one-time pairing code

Usage: vowp surface pair [OPTIONS] --session <ID> --kind <KIND>

Options:
      --json                  Print one machine-readable JSON document on stdout
      --session <ID>          Session the surface will mirror
      --kind <KIND>           Surface kind: `opencode` or `chat`
      --node <NAME>           Run the command on that node over ssh
      --app <APP>             Application name carried into the token scopes
      --project <NAME|PATH>   Project name or path; defaults to the nearest ancestor with `vowp.toml`
      --home <DIR>            Node home (default `$VOWP_HOME`, else `~/.vowp`)
      --replace <SURFACE-ID>  Revoke this surface and create the replacement pending surface in one transaction
      --no-color              Disable colour even when stdout is a terminal
  -q, --quiet                 Suppress non-essential human output
  -v, --verbose...            Increase log verbosity (`-v`, `-vv`)
  -h, --help                  Print help

For this command --node <name> names the node the surface runs on, not an SSH redirect.
";

/// A pre-0.4.0 `vowp surface pair --help`: the after-help is present but `--replace` is not.
const SURFACE_PAIR_HELP_PRE_0_4_0: &str = "\
Create a pending surface and print its one-time pairing code

Usage: vowp surface pair [OPTIONS] --session <ID> --kind <KIND>

Options:
      --session <ID>  Session the surface will mirror
      --kind <KIND>   Surface kind: `opencode` or `chat`
      --app <APP>     Application name carried into the token scopes

For this command --node <name> names the node the surface runs on, not an SSH redirect.
";

const SURFACE_LS_HELP: &str = "\
List surfaces, their scopes and token expiry

Usage: vowp surface ls [OPTIONS]
";

const SURFACE_REVOKE_HELP: &str = "\
Revoke a surface and every token it holds

Usage: vowp surface revoke [OPTIONS] <ID>

Arguments:
  <ID>  Surface id

Options:
      --json                 Print one machine-readable JSON document on stdout
      --yes                  Revoke without asking for confirmation
";

/// The shipped `vowp 0.4.0` `surface rotate --help`; `--wait`/`--grace` and the command
/// after-help are the required capability text. Captured read-only on 2026-11-18.
const SURFACE_ROTATE_HELP: &str = "\
Rotate a surface token, keeping the old token for a ten-minute overlap

Usage: vowp surface rotate [OPTIONS] <ID>

Arguments:
  <ID>  Surface id printed by `vowp surface pair`

Options:
      --json                 Print one machine-readable JSON document on stdout
      --wait <DURATION>      Poll the outbox for this long and exit 7 with `rotation_pending` if still queued
      --grace <DURATION>     Shorten the old tokens to `now + grace` instead of the ten-minute default (max 24 h)
      --node <NAME>          Run the command on that node over ssh
      --project <NAME|PATH>  Project name or path; defaults to the nearest ancestor with `vowp.toml`
      --home <DIR>           Node home (default `$VOWP_HOME`, else `~/.vowp`)
      --no-color             Disable colour even when stdout is a terminal
  -q, --quiet                Suppress non-essential human output
  -v, --verbose...           Increase log verbosity (`-v`, `-vv`)
  -h, --help                 Print help

For this command --node <name> names the node the surface runs on, not an SSH redirect.
";

const MCP_HELP: &str = "\
Run the stdio MCP server or the Mac-side proxy

Usage: vowp mcp [OPTIONS] --surface <ID>

Options:
      --json                 Print one machine-readable JSON document on stdout
      --session <ID>         Session the server writes to (home mode; not with --agent-socket)
      --node <NAME>          Run the command on that node over ssh
      --surface <ID>         Surface id this server represents
      --project <NAME|PATH>  Project name or path; defaults to the nearest ancestor with `vowp.toml`
      --token-file <FILE>    File that holds the lease token (home mode; not with --agent-socket)
      --agent-socket <PATH>  Unix socket served by `vowp agent run`; selects the local MCP proxy
      --home <DIR>           Node home (default `$VOWP_HOME`, else `~/.vowp`)
      --no-color             Disable colour even when stdout is a terminal
  -q, --quiet                Suppress non-essential human output
  -v, --verbose...           Increase log verbosity (`-v`, `-vv`)
  -h, --help                 Print help
";

const ORCHESTRATOR_STOP_HELP: &str = "\
Stop an orchestrator's host and release its lease

Usage: vowp orchestrator stop [OPTIONS] <NAME>
";

const ORCHESTRATOR_RESUME_HELP: &str = "\
Start an orchestrator's host again

Usage: vowp orchestrator resume [OPTIONS] <NAME>
";

const AGENT_PAIR_HELP: &str = "\
Redeem a home surface pairing code and store the token

Usage: vowp agent pair [OPTIONS] --code-file <PATH|-> --surface <ID> --session <ID>

Options:
      --code-file <PATH|->   File holding the one-time pairing code, or `-` for stdin
      --json                 Print one machine-readable JSON document on stdout
      --node <NAME>          Run the command on that node over ssh
      --surface <ID>         Surface id printed by `vowp surface pair`
      --project <NAME|PATH>  Project name or path; defaults to the nearest ancestor with `vowp.toml`
      --session <ID>         Session id printed by `vowp surface pair`
      --app <APP>            Application name; defaults to `agent` when the home did not bind one
      --home <DIR>           Node home (default `$VOWP_HOME`, else `~/.vowp`)
      --no-color             Disable colour even when stdout is a terminal
  -q, --quiet                Suppress non-essential human output
  -v, --verbose...           Increase log verbosity (`-v`, `-vv`)
  -h, --help                 Print help
";

const AGENT_RUN_HELP: &str = "\
Run the agent client until revoked or superseded

Usage: vowp agent run [OPTIONS]

Options:
      --foreground           Keep the process in the terminal (the Phase 0 default)
      --json                 Print one machine-readable JSON document on stdout
      --node <NAME>          Run the command on that node over ssh
      --role <ROLE>          Agent role: `surface`, `executor` or `both`
      --project <NAME|PATH>  Project name or path; defaults to the nearest ancestor with `vowp.toml`
      --service              Run under the service manager: write the `run` marker and drain on SIGTERM
      --home <DIR>           Node home (default `$VOWP_HOME`, else `~/.vowp`)
      --no-color             Disable colour even when stdout is a terminal
  -q, --quiet                Suppress non-essential human output
  -v, --verbose...           Increase log verbosity (`-v`, `-vv`)
  -h, --help                 Print help
";

const AGENT_PATHS_HELP: &str = "\
Add, remove or list the local project path map

Usage: vowp agent paths [OPTIONS] <COMMAND>

Commands:
  add  Add or update one project checkout
  rm   Remove one project from the map
  ls   List the local project path map

Options:
      --json                 Print one machine-readable JSON document on stdout
      --node <NAME>          Run the command on that node over ssh
      --project <NAME|PATH>  Project name or path; defaults to the nearest ancestor with `vowp.toml`
      --home <DIR>           Node home (default `$VOWP_HOME`, else `~/.vowp`)
      --no-color             Disable colour even when stdout is a terminal
  -q, --quiet                Suppress non-essential human output
  -v, --verbose...           Increase log verbosity (`-v`, `-vv`)
  -h, --help                 Print help
";

const AGENT_SERVICE_HELP: &str = "\
Install, remove or inspect the launchd or systemd-user service

Usage: vowp agent service [OPTIONS] <COMMAND>

Commands:
  install    Write exactly one launchd plist or systemd user unit
  uninstall  Stop the unit and remove only a VOWP-generated file
  status     Show the unit path and the manager's reported state
";

const AGENT_DOCTOR_HELP: &str = "\
Report local agent warnings without failing on them

Usage: vowp agent doctor [OPTIONS]
";

/// The shipped `vowp 0.4.0` `agent trust --help` output. Trust is a local-only,
/// owner-confirmed security action for a changed system-mode WSS leaf fingerprint. The
/// probe records this help as optional evidence and never runs the command itself.
const AGENT_TRUST_HELP: &str = "\
Confirm a changed system-mode WSS leaf fingerprint after verifying it out of band

Usage: vowp agent trust [OPTIONS] --fingerprint <SHA256:HEX>

Options:
      --fingerprint <SHA256:HEX>  New leaf fingerprint (`sha256:<64 lowercase hex>`), verified through a trusted channel
      --json                      Print one machine-readable JSON document on stdout
      --node <NAME>               Run the command on that node over ssh
      --project <NAME|PATH>       Project name or path; defaults to the nearest ancestor with `vowp.toml`
      --home <DIR>                Node home (default `$VOWP_HOME`, else `~/.vowp`)
      --no-color                  Disable colour even when stdout is a terminal
  -q, --quiet                     Suppress non-essential human output
  -v, --verbose...                Increase log verbosity (`-v`, `-vv`)
  -h, --help                      Print help
";

/// The shipped `vowp 0.4.0` fresh-home `node show` card for the local home node. Presence is
/// online, but `kind: home` is not an agent and must not satisfy the gate.
const NODE_SHOW_V0_4_0_HOME_ONLINE: &str = r#"{"name":"local","kind":"home","status":"active","presence":"online","labels":[],"capacity":null,"runs":0,"mapped":[],"agent_version":null,"protocol":null,"last_error":null,"last_seen_at":"2026-11-18T16:48:56.078Z"}"#;

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
    // The installed v0.4.0 build reports 0.4.0; a version string is still never evidence
    // by itself. `parse_version_line` also accepts `vowp version`'s build/schema suffix.
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
            output: ok(
                r#"[{"name":"mac-studio","ssh_target":null,"is_local":false,"capabilities":{"vowp":"0.4.0"},"last_seen_at":"2026-11-18T16:48:56.078Z","created_at":"2026-11-18T16:48:56.078Z"}]"#,
            ),
        },
        Route {
            needles: vec!["node", "show", "mac-studio"],
            output: ok(
                r##"{"name":"mac-studio","kind":"agent","status":"active","presence":"online","labels":[],"runs":0,"mapped":["vowp"],"agent_version":"0.4.0","protocol":"vowp-agent/1","last_error":null,"last_seen_at":"2026-11-18T16:48:56.078Z"}"##,
            ),
        },
        Route {
            needles: vec!["offload", "status"],
            output: ok(
                r#"{"generated_at":"2026-11-14T12:00:00.000Z","nodes":[],"assignments":[],"ready":[]}"#,
            ),
        },
        Route {
            needles: vec!["surface", "pair"],
            output: ok(SURFACE_PAIR_HELP_0_4_0),
        },
        Route {
            needles: vec!["surface", "ls"],
            output: ok(SURFACE_LS_HELP),
        },
        Route {
            needles: vec!["surface", "revoke"],
            output: ok(SURFACE_REVOKE_HELP),
        },
        Route {
            needles: vec!["surface", "rotate"],
            output: ok(SURFACE_ROTATE_HELP),
        },
        Route {
            needles: vec!["mcp", "--help"],
            output: ok(MCP_HELP),
        },
        Route {
            needles: vec!["orchestrator", "new"],
            output: ok(ORCHESTRATOR_NEW_HELP_0_4_0),
        },
        Route {
            needles: vec!["orchestrator", "stop"],
            output: ok(ORCHESTRATOR_STOP_HELP),
        },
        Route {
            needles: vec!["orchestrator", "resume"],
            output: ok(ORCHESTRATOR_RESUME_HELP),
        },
        Route {
            needles: vec!["orchestrator", "ls"],
            output: ok("[]"),
        },
    ]
}

fn status_json(connection: &str, protocol: Option<&str>, cache: bool) -> String {
    let protocol = protocol.map_or_else(|| "null".to_owned(), |value| format!("\"{value}\""));
    let cache = if cache {
        r#","cache_entries":0,"cache_bytes":0,"cache_oldest_at":null"#
    } else {
        ""
    };
    format!(
        r#"{{"connection":"{connection}","node":"mac-studio","home":"home-1","protocol":{protocol},"last_heartbeat":"2026-11-14T12:05:00.000Z","last_error":null{cache}}}"#
    )
}

fn local_routes() -> Vec<Route> {
    vec![
        Route {
            needles: vec!["agent", "status"],
            output: ok(&status_json("connected", Some("vowp-agent/1"), true)),
        },
        Route {
            needles: vec!["agent", "pair", "--help"],
            output: ok(AGENT_PAIR_HELP),
        },
        Route {
            needles: vec!["agent", "run", "--help"],
            output: ok(AGENT_RUN_HELP),
        },
        Route {
            needles: vec!["agent", "paths", "--help"],
            output: ok(AGENT_PATHS_HELP),
        },
        Route {
            needles: vec!["agent", "service", "--help"],
            output: ok(AGENT_SERVICE_HELP),
        },
        Route {
            needles: vec!["agent", "doctor", "--help"],
            output: ok(AGENT_DOCTOR_HELP),
        },
        Route {
            needles: vec!["agent", "trust", "--help"],
            output: ok(AGENT_TRUST_HELP),
        },
    ]
}

fn local_runner() -> RouteRunner {
    RouteRunner::new(local_routes(), failed(1, "not installed"))
}

fn local_runner_with_status(status: &str) -> RouteRunner {
    RouteRunner::new(
        vec![Route {
            needles: vec!["agent", "status"],
            output: ok(status),
        }],
        failed(1, "not installed"),
    )
}

#[tokio::test]
async fn gates_asif_probe_waiting_when_export_flag_is_unsupported() {
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
async fn gates_asif_probe_ready_export_only_when_import_help_is_ambiguous() {
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
async fn gates_asif_probe_ready_both_when_export_and_import_are_proven() {
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
async fn gates_asif_probe_never_ready_from_version_alone() {
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
async fn gates_asif_probe_marks_inconsistent_help_as_incompatible() {
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
async fn gates_phase3_probe_ready_with_the_installed_vowp_0_4_0_surface() {
    let runner = RouteRunner::new(home_routes(), failed(1, "unexpected"));
    let agent = local_runner();
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
            for capability in [
                "node_card",
                "offload_status",
                "surface_pair",
                "surface_list",
                "surface_revoke",
                "surface_rotate",
                "mcp_agent_socket",
                "orchestrator_new_node",
                "orchestrator_lifecycle",
                "orchestrator_read",
                "local_agent_connected",
                "local_surface_pair",
                "local_agent_run",
                "backfill",
                "local_path_map",
                "local_cache",
            ] {
                assert!(
                    capabilities.contains(&capability.to_owned()),
                    "missing capability {capability}: {capabilities:?}"
                );
            }
            assert!(
                evidence
                    .iter()
                    .any(|line| line.contains("vowp surface pair --help: exit 0"))
            );
            assert!(
                evidence
                    .iter()
                    .any(|line| line.contains("vowp surface rotate --help: exit 0"))
            );
            assert!(
                evidence
                    .iter()
                    .any(|line| line.contains("vowp orchestrator new --help: exit 0"))
            );
            assert!(
                evidence
                    .iter()
                    .any(|line| line.contains("vowp agent trust --help: exit 0 (optional)"))
            );
            assert!(
                evidence
                    .iter()
                    .any(|line| line.contains("cache stats reported"))
            );
            assert!(
                evidence.iter().all(|line| !line.contains("agent surface")),
                "the stale agent surface command must never be probed: {evidence:?}"
            );
        }
        other => panic!("expected ready, got {other:?}"),
    }
    assert_eq!(local.state, LocalAgentState::Connected);
    assert!(runner.calls().len() >= 13);
    assert!(
        runner
            .calls()
            .iter()
            .all(|spec| !remote_argv(spec).contains("'agent' 'surface'")),
        "the stale agent surface command must never be invoked"
    );
    let agent_calls = agent.calls();
    assert!(
        agent_calls
            .iter()
            .any(|spec| spec.args == ["agent", "trust", "--help"]),
        "the trust capability is proven from its help only"
    );
    assert!(
        agent_calls
            .iter()
            .all(|spec| !spec.args.iter().any(|arg| arg == "trust")
                || spec.args.iter().any(|arg| arg == "--help")),
        "the probe must never run `vowp agent trust` without --help"
    );
}

#[tokio::test]
async fn gates_phase3_probe_treats_agent_trust_as_optional_evidence() {
    // A local build without `agent trust` still has the SSH surface D6 needs.
    let mut routes = local_routes();
    routes.retain(|route| route.needles != ["agent", "trust", "--help"]);
    let agent = RouteRunner::new(routes, failed(2, "unknown command"));
    let runner = RouteRunner::new(home_routes(), failed(1, "unexpected"));
    let (gate, _) = probe_local_orchestrator(&runner, Some(&agent), &options()).await;
    match gate {
        LocalOrchestratorGate::Ready { evidence, .. } => {
            assert!(evidence.iter().any(|line| {
                line.contains("vowp agent trust --help:")
                    && line.contains("(optional)")
                    && !line.contains("exit 0 (optional)")
            }));
        }
        other => panic!("expected ready without trust, got {other:?}"),
    }

    // A help that omits the fingerprint flag is recorded, not accepted as proof.
    let mut routes = local_routes();
    for route in &mut routes {
        if route.needles == ["agent", "trust", "--help"] {
            route.output = ok(
                "Confirm a changed system-mode WSS leaf fingerprint\n\nUsage: vowp agent trust [OPTIONS]\n",
            );
        }
    }
    let agent = RouteRunner::new(routes, failed(2, "unknown command"));
    let runner = RouteRunner::new(home_routes(), failed(1, "unexpected"));
    let (gate, _) = probe_local_orchestrator(&runner, Some(&agent), &options()).await;
    match gate {
        LocalOrchestratorGate::Ready { evidence, .. } => {
            assert!(evidence.iter().any(|line| {
                line.contains("vowp agent trust --help: required flag not advertised (optional)")
            }));
        }
        other => panic!("expected ready with an unproven trust help, got {other:?}"),
    }
}

#[tokio::test]
async fn gates_phase3_probe_rejects_an_online_home_node_from_vowp_0_4_0() {
    // The shipped 0.4.0 fresh home reports its only node as kind=home, status=active,
    // presence=online. Presence alone is never proof of a local agent.
    let routes = vec![
        version_route(),
        Route {
            needles: vec!["node", "ls"],
            output: ok(
                r#"[{"name":"local","ssh_target":null,"is_local":true,"capabilities":{"vowp":"0.4.0"},"last_seen_at":"2026-11-18T16:48:56.078Z","created_at":"2026-11-18T16:48:56.078Z"}]"#,
            ),
        },
        Route {
            needles: vec!["node", "show", "local"],
            output: ok(NODE_SHOW_V0_4_0_HOME_ONLINE),
        },
    ];
    let runner = RouteRunner::new(routes, failed(2, "unknown command"));
    let (gate, _) = probe_local_orchestrator(&runner, None, &options()).await;
    match gate {
        LocalOrchestratorGate::WaitingForVowpPhase3 {
            missing,
            detected_version,
            evidence,
        } => {
            assert_eq!(missing, vec!["no active local agent node".to_owned()]);
            assert_eq!(detected_version.as_deref(), Some("0.4.0"));
            assert!(
                evidence
                    .iter()
                    .any(|line| line.contains("kind=home") && line.contains("presence=online")),
                "{evidence:?}"
            );
        }
        other => panic!("expected waiting on a home node, got {other:?}"),
    }
}

#[tokio::test]
async fn gates_phase3_probe_waits_when_surface_pair_cannot_mint_a_replacement() {
    let mut routes = home_routes();
    for route in &mut routes {
        if route.needles == ["surface", "pair"] {
            route.output = ok(SURFACE_PAIR_HELP_PRE_0_4_0);
        }
    }
    let runner = RouteRunner::new(routes, failed(1, "unexpected"));
    let agent = local_runner();
    let (gate, _) = probe_local_orchestrator(&runner, Some(&agent), &options()).await;
    match gate {
        LocalOrchestratorGate::WaitingForVowpPhase3 {
            missing, evidence, ..
        } => {
            assert!(missing.contains(&"surface pairing".to_owned()));
            assert!(
                evidence
                    .iter()
                    .any(|line| line.contains("required flags/text not advertised"))
            );
        }
        other => panic!("expected waiting, got {other:?}"),
    }
}

#[tokio::test]
async fn gates_phase3_probe_waits_when_offload_status_is_not_the_documented_shape() {
    let mut routes = home_routes();
    for route in &mut routes {
        if route.needles == ["offload", "status"] {
            route.output = ok(r#"{"enabled":true,"spool_bytes":0}"#);
        }
    }
    let runner = RouteRunner::new(routes, failed(1, "unexpected"));
    let agent = local_runner();
    let (gate, _) = probe_local_orchestrator(&runner, Some(&agent), &options()).await;
    match gate {
        LocalOrchestratorGate::WaitingForVowpPhase3 {
            missing, evidence, ..
        } => {
            assert!(missing.contains(&"offload executor (Phase 2)".to_owned()));
            assert!(
                evidence
                    .iter()
                    .any(|line| line.contains("not the documented shape"))
            );
        }
        other => panic!("expected waiting, got {other:?}"),
    }
}

#[tokio::test]
async fn gates_phase3_probe_waits_when_each_home_surface_command_is_absent() {
    for (first, second, missing_label) in [
        ("surface", "pair", "surface pairing"),
        ("surface", "ls", "surface read model"),
        ("surface", "revoke", "surface revocation"),
        ("surface", "rotate", "surface rotation"),
        ("mcp", "--help", "MCP proxy"),
        ("orchestrator", "new", "orchestrator materialization"),
        ("orchestrator", "stop", "orchestrator lifecycle"),
        ("orchestrator", "resume", "orchestrator lifecycle"),
    ] {
        let mut routes = home_routes();
        routes.retain(|route| route.needles != [first, second]);
        let runner = RouteRunner::new(routes, failed(2, "unknown command"));
        let agent = local_runner();
        let (gate, _) = probe_local_orchestrator(&runner, Some(&agent), &options()).await;
        match gate {
            LocalOrchestratorGate::WaitingForVowpPhase3 { missing, .. } => {
                assert!(
                    missing.contains(&missing_label.to_owned()),
                    "missing {missing_label}: {missing:?}"
                );
            }
            other => panic!("expected waiting for {missing_label}, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn gates_phase3_probe_waits_when_mcp_agent_socket_is_not_advertised() {
    let mut routes = home_routes();
    for route in &mut routes {
        if route.needles == ["mcp", "--help"] {
            route.output = ok("Usage: vowp mcp [OPTIONS] --surface <ID>\n");
        }
    }
    let runner = RouteRunner::new(routes, failed(1, "unexpected"));
    let agent = local_runner();
    let (gate, _) = probe_local_orchestrator(&runner, Some(&agent), &options()).await;
    match gate {
        LocalOrchestratorGate::WaitingForVowpPhase3 {
            missing, evidence, ..
        } => {
            assert!(missing.contains(&"MCP proxy".to_owned()));
            assert!(
                evidence
                    .iter()
                    .any(|line| line.contains("required flags/text not advertised"))
            );
        }
        other => panic!("expected waiting, got {other:?}"),
    }
}

#[tokio::test]
async fn gates_phase3_probe_rejects_the_global_ssh_node_as_local_node_proof() {
    let mut routes = home_routes();
    for route in &mut routes {
        if route.needles == ["orchestrator", "new"] {
            route.output = ok(ORCHESTRATOR_NEW_HELP_GLOBAL_NODE_ONLY);
        }
    }
    let runner = RouteRunner::new(routes, failed(1, "unexpected"));
    let agent = local_runner();
    let (gate, _) = probe_local_orchestrator(&runner, Some(&agent), &options()).await;
    match gate {
        LocalOrchestratorGate::WaitingForVowpPhase3 {
            missing, evidence, ..
        } => {
            assert!(missing.contains(&"orchestrator materialization".to_owned()));
            assert!(
                evidence
                    .iter()
                    .any(|line| line.contains("required flags/text not advertised"))
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
async fn gates_phase3_probe_rejects_the_global_ssh_node_on_surface_rotate_help() {
    let mut routes = home_routes();
    for route in &mut routes {
        if route.needles == ["surface", "rotate"] {
            route.output = ok(
                "Rotate a surface token\n\nUsage: vowp surface rotate [OPTIONS] <ID>\n\nOptions:\n      --wait <DURATION>   Poll the outbox\n      --grace <DURATION>  Shorten the old tokens\n      --node <NAME>       Run the command on that node over ssh\n",
            );
        }
    }
    let runner = RouteRunner::new(routes, failed(1, "unexpected"));
    let agent = local_runner();
    let (gate, _) = probe_local_orchestrator(&runner, Some(&agent), &options()).await;
    match gate {
        LocalOrchestratorGate::WaitingForVowpPhase3 { missing, .. } => {
            assert!(missing.contains(&"surface rotation".to_owned()));
        }
        other => panic!("expected waiting, got {other:?}"),
    }
}

#[tokio::test]
async fn gates_phase3_probe_waits_when_orchestrator_read_model_is_not_an_array() {
    let mut routes = home_routes();
    for route in &mut routes {
        if route.needles == ["orchestrator", "ls"] {
            route.output = ok(r#"{"orchestrators":[]}"#);
        }
    }
    let runner = RouteRunner::new(routes, failed(1, "unexpected"));
    let agent = local_runner();
    let (gate, _) = probe_local_orchestrator(&runner, Some(&agent), &options()).await;
    match gate {
        LocalOrchestratorGate::WaitingForVowpPhase3 {
            missing, evidence, ..
        } => {
            assert!(missing.contains(&"orchestrator read model".to_owned()));
            assert!(evidence.iter().any(|line| line.contains("not an array")));
        }
        other => panic!("expected waiting, got {other:?}"),
    }
}

#[tokio::test]
async fn gates_phase3_probe_waits_when_local_agent_is_not_connected() {
    let runner = RouteRunner::new(home_routes(), failed(1, "unexpected"));
    let (gate, local) = probe_local_orchestrator(&runner, None, &options()).await;
    match gate {
        LocalOrchestratorGate::WaitingForVowpPhase3 { missing, .. } => {
            assert!(missing.contains(&"local agent connection".to_owned()));
        }
        other => panic!("expected waiting, got {other:?}"),
    }
    assert_eq!(local.state, LocalAgentState::NotInstalled);

    let stopped = local_runner_with_status(&status_json("stopped", Some("vowp-agent/1"), true));
    let (gate, local) = probe_local_orchestrator(&runner, Some(&stopped), &options()).await;
    match gate {
        LocalOrchestratorGate::WaitingForVowpPhase3 { missing, .. } => {
            assert!(missing.contains(&"local agent connection".to_owned()));
        }
        other => panic!("expected waiting for stopped agent, got {other:?}"),
    }
    assert_eq!(local.state, LocalAgentState::Stopped);
}

#[tokio::test]
async fn gates_phase3_probe_waits_when_local_agent_commands_or_cache_are_missing() {
    for (needle, missing_label) in [
        (["agent", "pair", "--help"], "local surface pairing"),
        (["agent", "run", "--help"], "local agent run"),
        (["agent", "paths", "--help"], "local path map"),
    ] {
        let mut routes = local_routes();
        routes.retain(|route| route.needles != needle);
        let agent = RouteRunner::new(routes, failed(2, "unknown command"));
        let runner = RouteRunner::new(home_routes(), failed(1, "unexpected"));
        let (gate, _) = probe_local_orchestrator(&runner, Some(&agent), &options()).await;
        match gate {
            LocalOrchestratorGate::WaitingForVowpPhase3 { missing, .. } => {
                assert!(
                    missing.contains(&missing_label.to_owned()),
                    "missing {missing_label}: {missing:?}"
                );
            }
            other => panic!("expected waiting for {missing_label}, got {other:?}"),
        }
    }

    let no_cache = local_runner_with_status(&status_json("connected", Some("vowp-agent/1"), false));
    let runner = RouteRunner::new(home_routes(), failed(1, "unexpected"));
    let (gate, _) = probe_local_orchestrator(&runner, Some(&no_cache), &options()).await;
    match gate {
        LocalOrchestratorGate::WaitingForVowpPhase3 {
            missing, evidence, ..
        } => {
            assert!(missing.contains(&"local cache".to_owned()));
            assert!(
                evidence
                    .iter()
                    .any(|line| line.contains("cache stats not reported"))
            );
        }
        other => panic!("expected waiting for local cache, got {other:?}"),
    }
}

#[tokio::test]
async fn gates_phase3_probe_waits_when_local_protocol_is_missing() {
    let agent = local_runner_with_status(&status_json("connected", None, true));
    let runner = RouteRunner::new(home_routes(), failed(1, "unexpected"));
    let (gate, _) = probe_local_orchestrator(&runner, Some(&agent), &options()).await;
    match gate {
        LocalOrchestratorGate::WaitingForVowpPhase3 { missing, .. } => {
            assert!(missing.contains(&"local agent protocol".to_owned()));
        }
        other => panic!("expected waiting, got {other:?}"),
    }
}

#[tokio::test]
async fn gates_phase3_probe_never_ready_from_version_alone() {
    let only_version = RouteRunner::new(vec![version_route()], failed(1, "not found"));
    let agent = local_runner();
    let (gate, _) = probe_local_orchestrator(&only_version, Some(&agent), &options()).await;
    match gate {
        LocalOrchestratorGate::WaitingForVowpPhase3 {
            detected_version, ..
        } => assert_eq!(detected_version.as_deref(), Some("0.4.0")),
        other => panic!("expected waiting from version alone, got {other:?}"),
    }
}

#[tokio::test]
async fn gates_phase3_probe_waits_on_the_v0_3_offline_home_node() {
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
async fn gates_phase3_probe_does_not_accept_an_offline_agent_or_a_missing_agent_version() {
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
async fn gates_phase3_probe_incompatible_on_wrong_agent_protocol() {
    let mut routes = home_routes();
    for route in &mut routes {
        if route.needles.contains(&"show") {
            route.output = ok(
                r#"{"name":"mac-studio","kind":"agent","status":"active","presence":"online","protocol":"vowp-agent/2"}"#,
            );
        }
    }
    let runner = RouteRunner::new(routes, failed(1, "unexpected"));
    let agent = local_runner();
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
async fn gates_phase3_probe_does_not_accept_agent_status_and_node_model_alone() {
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
    let agent = local_runner();
    let (gate, local) = probe_local_orchestrator(&runner, Some(&agent), &options()).await;
    assert!(matches!(
        gate,
        LocalOrchestratorGate::WaitingForVowpPhase3 { .. }
    ));
    assert_eq!(local.state, LocalAgentState::Connected);
}

#[tokio::test]
async fn gates_phase3_probe_passes_the_active_project_to_node_reads() {
    let runner = RouteRunner::new(home_routes(), failed(1, "unexpected"));
    let agent = local_runner();
    let (gate, _) = probe_local_orchestrator(&runner, Some(&agent), &options()).await;
    assert!(matches!(gate, LocalOrchestratorGate::Ready { .. }));
    let calls = runner.calls();
    for needle in ["'node' 'ls'", "'node' 'show'"] {
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
async fn gates_phase3_probe_waits_without_a_project_and_skips_global_node_reads() {
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
async fn gates_phase3_probe_never_requires_node_paths_and_reads_cache_from_agent_status() {
    let runner = RouteRunner::new(home_routes(), failed(1, "unexpected"));
    let agent = local_runner();
    let (gate, _) = probe_local_orchestrator(&runner, Some(&agent), &options()).await;
    assert!(matches!(gate, LocalOrchestratorGate::Ready { .. }));
    let calls = runner.calls();
    assert!(
        calls
            .iter()
            .all(|spec| !remote_argv(spec).contains("'node' 'paths'")),
        "the merged CLI has no `vowp node paths`; local cache is a Mac-side read"
    );
}

#[tokio::test]
async fn gates_phase3_probe_marks_a_malformed_node_inventory_or_card_as_incompatible() {
    let mut routes = home_routes();
    for route in &mut routes {
        if route.needles == ["node", "ls"] {
            route.output = ok(r#"{"nodes":[]}"#);
        }
    }
    let runner = RouteRunner::new(routes, failed(1, "unexpected"));
    let agent = local_runner();
    let (gate, _) = probe_local_orchestrator(&runner, Some(&agent), &options()).await;
    assert!(matches!(gate, LocalOrchestratorGate::Incompatible { .. }));

    let mut routes = home_routes();
    for route in &mut routes {
        if route.needles.contains(&"show") {
            route.output = ok(r#"{"name":"mac-studio"}"#);
        }
    }
    let runner = RouteRunner::new(routes, failed(1, "unexpected"));
    let (gate, _) = probe_local_orchestrator(&runner, Some(&agent), &options()).await;
    assert!(matches!(gate, LocalOrchestratorGate::Incompatible { .. }));
}

#[tokio::test]
async fn gates_phase3_probe_separates_home_capability_from_local_agent_status() {
    let runner = RouteRunner::new(home_routes(), failed(1, "unexpected"));
    let (gate, local) = probe_local_orchestrator(&runner, None, &options()).await;
    assert!(matches!(
        gate,
        LocalOrchestratorGate::WaitingForVowpPhase3 { .. }
    ));
    assert_eq!(local.state, LocalAgentState::NotInstalled);

    let missing = RouteRunner::new(vec![version_route()], failed(1, "not installed"));
    let agent = local_runner();
    let (gate, local) = probe_local_orchestrator(&missing, Some(&agent), &options()).await;
    assert!(!matches!(gate, LocalOrchestratorGate::Ready { .. }));
    assert_eq!(local.state, LocalAgentState::Connected);
}

#[tokio::test]
async fn gates_local_agent_states_map_exactly() {
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
        let status = status_json(connection, Some("vowp-agent/1"), true);
        let agent = local_runner_with_status(&status);
        let (_, local) = probe_local_orchestrator(&runner, Some(&agent), &options()).await;
        assert_eq!(local.state, expected, "{connection}");
    }
    let not_enrolled = RouteRunner::new(vec![], failed(3, "no enrollment"));
    let (_, local) = probe_local_orchestrator(&runner, Some(&not_enrolled), &options()).await;
    assert_eq!(local.state, LocalAgentState::NotEnrolled);
    let wrong_protocol =
        local_runner_with_status(&status_json("connected", Some("vowp-agent/2"), true));
    let (_, local) = probe_local_orchestrator(&runner, Some(&wrong_protocol), &options()).await;
    assert_eq!(local.state, LocalAgentState::Incompatible);
}

#[tokio::test]
async fn gates_local_agent_last_error_is_scrubbed_before_display() {
    let status = r#"{"connection":"connected","node":"mac-studio","home":"home-1","protocol":"vowp-agent/1","last_heartbeat":"2026-11-14T12:05:00.000Z","last_error":"open /Users/example/.vowp-agent/state failed; refresh_token=abcdef","cache_entries":0,"cache_bytes":0}"#;
    let agent = local_runner_with_status(status);
    let runner = RouteRunner::new(home_routes(), failed(1, "unexpected"));
    let (_, local) = probe_local_orchestrator(&runner, Some(&agent), &options()).await;
    let error = local.last_error.expect("last_error");
    assert!(!error.contains("/Users/"), "{error}");
    assert!(!error.contains("abcdef"), "{error}");
}

#[tokio::test]
async fn gates_gate_evidence_has_no_paths_or_secrets() {
    let runner = RouteRunner::new(home_routes(), failed(1, "unexpected"));
    let agent = local_runner();
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
        assert!(!line.contains("agent surface"), "{line}");
        assert!(line.len() < 300, "{line}");
    }
    // No evidence for a status-only probe can be Ready.
    let only_version = RouteRunner::new(vec![version_route()], failed(1, "not found"));
    let (gate, _) = probe_local_orchestrator(&only_version, None, &options()).await;
    assert!(!matches!(gate, LocalOrchestratorGate::Ready { .. }));
}

#[test]
fn gates_gate_cache_expires_and_downgrades() {
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
fn gates_ready_fixture_enables_d6_but_waiting_fixture_stays_disabled() {
    let mut snapshot: GateSnapshot = serde_json::from_str(GATE_PHASE3_READY).unwrap();
    snapshot.enforce_local_orchestrator_flag();
    assert!(matches!(
        snapshot.local_orchestrator,
        LocalOrchestratorGate::Ready { .. }
    ));
    assert!(snapshot.features.local_orchestrator_actions);
    assert!(snapshot.local_orchestrator_enabled());

    let mut waiting: GateSnapshot = serde_json::from_str(GATE_PHASE3_WAITING).unwrap();
    waiting.enforce_local_orchestrator_flag();
    assert!(!waiting.features.local_orchestrator_actions);
    assert!(!waiting.local_orchestrator_enabled());

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
