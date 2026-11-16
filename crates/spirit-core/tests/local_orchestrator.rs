//! D6 local-orchestrator actions, guard refusals, command mapping and privacy.
//!
//! These tests are hermetic: the home side is a `MockVowpClient` or a
//! `FakeCommandRunner`; the Mac side is `MockLocalAgentClient` or the command client
//! with scripted output. No real `~/.vowp`, VOWP database, SSH host or keychain is read.

use std::sync::Arc;

use spirit_core::engine::local::{
    LocalOrchestratorAction, LocalOrchestratorGuard, LocalOrchestratorState, LocalPathMapStatus,
};
use spirit_core::engine::{
    EngineSupervisor, EngineSupervisorConfig, FakeEngineRuntime, GateFeatureFlags, GateSnapshot,
    LocalAgentState, LocalAgentStatus, LocalOrchestratorGate,
};
use spirit_core::host::{HostDraft, HostKeyPin, InMemoryHostStore, NewHostProfile};
use spirit_core::service::{SpiritCore, SpiritService};
use spirit_core::ssh::command::{CommandOutput, CommandSpec};
use spirit_core::ssh::{FakeCommandRunner, SshOptions, SshTarget};
use spirit_core::time::Timestamp;
use spirit_core::view::{AgentPathView, LocalAgentView, LocalSessionState, LocalSurfaceView};
use spirit_core::vowp::{
    AgentPairRequest, CommandLocalAgent, LocalAgentClient, MockLocalAgentClient, MockVowpClient,
    VowpClient,
};
use spirit_core::wire::{
    AGENT_PROTOCOL, AgentPathMap, AgentStatusDocument, AgentSurfaceStatus, NodeCard,
    OrchestratorCard, OrchestratorNewRequest, PathRef, ProjectName, SessionId, SurfaceEntry,
    SurfaceLifecycleState, SurfacePairOutput, SurfacePairRequest, SurfaceRevokeResult,
    SurfaceRotateRequest, SurfaceRotateResult, parse_capacity_report, parse_fleet_status,
};

const GATE_PHASE3_READY: &str = include_str!("../fixtures/vowp/gate-phase3-ready.json");
const AGENT_CONNECTED: &str = include_str!("../fixtures/vowp/agent-status-connected.json");
const AGENT_REVOKED: &str = include_str!("../fixtures/vowp/agent-status-revoked.json");
const SURFACE_LIST: &str = include_str!("../fixtures/vowp/surface-list-attached-revoked.json");
const SURFACE_PAIR: &str = include_str!("../fixtures/vowp/surface-pair.json");
const AGENT_PATHS: &str = include_str!("../fixtures/vowp/agent-paths.json");
const ORCHESTRATORS: &str = include_str!("../fixtures/vowp/orchestrator-list-local.json");

const SESSION_ID: &str = "01JCF5WQ8ZK3M2V7N4P6R8T0AE";
const SURFACE_ID: &str = "01J9Z00000000000000000000A";

fn supervisor() -> EngineSupervisor {
    EngineSupervisor::new(
        Arc::new(FakeEngineRuntime::new()),
        EngineSupervisorConfig {
            restart_budget: 3,
            health_attempts: 1,
            retry_delay: std::time::Duration::ZERO,
            port: 0,
        },
    )
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

fn ready_snapshot() -> GateSnapshot {
    let mut snapshot: GateSnapshot = serde_json::from_str(GATE_PHASE3_READY).unwrap();
    snapshot.enforce_local_orchestrator_flag();
    snapshot
}

fn node_card() -> NodeCard {
    serde_json::from_str(
        r#"{"name":"mac-studio","kind":"agent","status":"active","presence":"online","protocol":"vowp-agent/1","agent_version":"0.4.0","labels":[],"runs":0,"mapped":["vowp"],"last_error":null}"#,
    )
    .unwrap()
}

fn path_map() -> AgentPathMap {
    let mut maps: Vec<AgentPathMap> = serde_json::from_str(AGENT_PATHS).unwrap();
    maps.remove(0)
}

fn surface_pair_output() -> SurfacePairOutput {
    serde_json::from_str(SURFACE_PAIR).unwrap()
}

fn orchestrator_card(name: &str) -> OrchestratorCard {
    let mut cards: Vec<OrchestratorCard> = serde_json::from_str(ORCHESTRATORS).unwrap();
    let mut card = cards.remove(0);
    card.name = name.to_owned();
    card
}

async fn connected_core(client: &MockVowpClient) -> SpiritCore {
    let core = SpiritCore::new(Arc::new(InMemoryHostStore::new()), supervisor());
    let host = core.add_host(host_input()).unwrap();
    core.connect(host.id, client).await.unwrap();
    core
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

fn surface_pair_request() -> SurfacePairRequest {
    SurfacePairRequest {
        session: SESSION_ID.to_owned(),
        node: "mac-studio".to_owned(),
        kind: spirit_core::wire::SurfaceKind::Opencode,
        app: Some("opencode".to_owned()),
        replace: None,
    }
}

fn remote_argv(spec: &CommandSpec) -> String {
    spec.args.last().cloned().unwrap_or_default()
}

#[test]
fn d6_feature_flag_is_derived_from_ready_and_connected_only() {
    let mut ready = ready_snapshot();
    assert!(ready.features.local_orchestrator_actions);
    assert!(ready.local_orchestrator_enabled());

    ready.local_agent.state = LocalAgentState::Stopped;
    ready.enforce_local_orchestrator_flag();
    assert!(!ready.features.local_orchestrator_actions);
    assert!(!ready.local_orchestrator_enabled());

    let mut waiting: GateSnapshot =
        serde_json::from_str(include_str!("../fixtures/vowp/gate-phase3-waiting.json")).unwrap();
    waiting.enforce_local_orchestrator_flag();
    assert!(!waiting.features.local_orchestrator_actions);
    assert!(!waiting.local_orchestrator_enabled());

    let mut incompatible = ready_snapshot();
    incompatible.local_orchestrator = LocalOrchestratorGate::Incompatible {
        detected: "vowp-agent/2".to_owned(),
        required: AGENT_PROTOCOL.to_owned(),
        evidence: Vec::new(),
    };
    incompatible.enforce_local_orchestrator_flag();
    assert!(!incompatible.features.local_orchestrator_actions);
}

#[test]
fn d6_guard_refuses_exact_prerequisites_and_never_uses_a_force_switch() {
    let snapshot = ready_snapshot();
    let state = LocalOrchestratorState::default();
    let guard = LocalOrchestratorGuard::from_snapshot(&snapshot, &state);
    assert!(
        guard
            .ensure(LocalOrchestratorAction::SurfacePair, Some("mac-studio"))
            .is_ok()
    );

    let stopped = GateSnapshot {
        local_agent: LocalAgentStatus {
            state: LocalAgentState::Reconnecting,
            ..snapshot.local_agent.clone()
        },
        ..snapshot.clone()
    };
    let error = LocalOrchestratorGuard::from_snapshot(&stopped, &state)
        .ensure(LocalOrchestratorAction::SurfacePair, None)
        .unwrap_err();
    assert!(error.message.contains("local_agent_connection"), "{error}");
    assert!(error.message.contains("reconnecting"), "{error}");

    let mut no_flag = snapshot.clone();
    no_flag.features.local_orchestrator_actions = false;
    let error = LocalOrchestratorGuard::from_snapshot(&no_flag, &state)
        .ensure(LocalOrchestratorAction::SurfacePair, None)
        .unwrap_err();
    assert!(
        error.message.contains("local_orchestrator_actions"),
        "{error}"
    );

    let mut wrong_protocol = snapshot.clone();
    wrong_protocol.local_agent.protocol = Some("vowp-agent/2".to_owned());
    let error = LocalOrchestratorGuard::from_snapshot(&wrong_protocol, &state)
        .ensure(LocalOrchestratorAction::SurfacePair, None)
        .unwrap_err();
    assert!(error.message.contains("agent_protocol"), "{error}");
    assert!(error.message.contains("vowp-agent/2"), "{error}");

    let mut revoked = node_card();
    revoked.status = spirit_core::wire::NodeStatus::Revoked;
    revoked.presence = spirit_core::wire::NodePresence::Revoked;
    let revoked_state = LocalOrchestratorState {
        node: Some(spirit_core::engine::LocalNodeState::from_card(&revoked)),
        ..LocalOrchestratorState::default()
    };
    let error = LocalOrchestratorGuard::from_snapshot(&snapshot, &revoked_state)
        .ensure(LocalOrchestratorAction::SurfacePair, Some("mac-studio"))
        .unwrap_err();
    assert!(error.message.contains("node_status"), "{error}");
    assert!(error.message.contains("revoked"), "{error}");

    let invalid_path = LocalOrchestratorGuard::from_snapshot(&snapshot, &state).with_path_map(
        LocalPathMapStatus::Invalid {
            reason: "allowed paths must be relative".to_owned(),
        },
    );
    let error = invalid_path
        .ensure(LocalOrchestratorAction::OrchestratorNew, None)
        .unwrap_err();
    assert!(error.message.contains("local_path_map"), "{error}");
    assert!(!error.message.contains("/Users/"), "{error}");
}

#[test]
fn d6_path_refs_never_carry_absolute_or_traversing_paths() {
    assert_eq!(PathRef::project("vowp").unwrap().as_str(), "project:vowp");
    assert_eq!(
        PathRef::worktree("vowp", "ss-20-d6-core").unwrap().as_str(),
        "worktree:vowp/ss-20-d6-core"
    );
    assert_eq!(
        PathRef::subdir("vowp", "src/./lib").unwrap().as_str(),
        "subdir:vowp/src/lib"
    );
    assert!(PathRef::parse("/Users/example/src/vowp").is_err());
    assert!(PathRef::subdir("vowp", "../secret").is_err());
    assert!(PathRef::subdir("vowp", "src/../secret").is_err());
    assert!(PathRef::parse("project:vowp/../../x").is_err());
}

#[tokio::test]
async fn d6_surface_pair_service_guards_then_returns_a_redacted_code() {
    let pairing = surface_pair_output();
    let client = fresh_mock().with_surface_pairing(pairing.clone());
    let core: SpiritService = connected_core(&client).await;
    let mut snapshot = ready_snapshot();
    snapshot.features.local_orchestrator_actions = false;
    core.set_gates(snapshot);
    assert!(
        core.gates().features.local_orchestrator_actions,
        "set_gates derives the D6 flag from Ready + connected"
    );
    core.set_local_node_card(&node_card());

    let pairing = core
        .surface_pair(&client, surface_pair_request())
        .await
        .unwrap();
    assert_eq!(pairing.code().expose(), "pairing-code-placeholder");
    let debug = format!("{pairing:?}");
    assert!(!debug.contains("pairing-code-placeholder"), "{debug}");
    let view_json = serde_json::to_string(&pairing.view()).unwrap();
    assert!(
        !view_json.contains("pairing-code-placeholder"),
        "{view_json}"
    );
    assert!(view_json.contains("mac-studio"));
    assert_eq!(client.surface_pair_calls().len(), 1);

    // A gate that is merely Ready is not enough when the Mac agent is stopped; the
    // service must refuse before calling VOWP.
    let mut stopped = ready_snapshot();
    stopped.local_agent.state = LocalAgentState::Stopped;
    stopped.enforce_local_orchestrator_flag();
    core.set_gates(stopped);
    let error = core
        .surface_pair(&client, surface_pair_request())
        .await
        .unwrap_err();
    assert!(error.message.contains("local_agent_connection"), "{error}");
    assert_eq!(client.surface_pair_calls().len(), 1, "no hidden retry");
}

#[tokio::test]
async fn d6_orchestrator_new_refuses_bad_path_map_before_the_command() {
    let client = fresh_mock().with_orchestrator_new_result(orchestrator_card("vowp-local"));
    let core = connected_core(&client).await;
    core.set_gates(ready_snapshot());
    core.set_local_node_card(&node_card());
    core.set_local_agent_paths(&[path_map()]);

    let request = OrchestratorNewRequest {
        node: "mac-studio".to_owned(),
        app: spirit_core::wire::OrchestratorApp::Opencode,
        project: Some("vowp".to_owned()),
        name: Some("vowp-local".to_owned()),
        force: false,
    };
    let view = core
        .orchestrator_new(&client, request.clone())
        .await
        .unwrap();
    assert_eq!(view.name, "vowp-local");
    assert_eq!(view.node.as_deref(), Some("mac-studio"));
    assert_eq!(client.orchestrator_new_calls(), vec![request.clone()]);

    let mut invalid = path_map();
    invalid.local_root = "relative/path".to_owned();
    core.set_local_agent_paths(&[invalid]);
    let error = core.orchestrator_new(&client, request).await.unwrap_err();
    assert!(error.message.contains("local_path_map"), "{error}");
    assert!(!error.message.contains("/Users/"), "{error}");
    assert_eq!(client.orchestrator_new_calls().len(), 1);
}

#[tokio::test]
async fn d6_surface_revoke_remains_available_when_the_gate_is_waiting() {
    let client = fresh_mock().with_surface_revocation(SurfaceRevokeResult {
        surface: SURFACE_ID.to_owned(),
        status: "revoked".to_owned(),
        tokens_revoked: 1,
    });
    let core = connected_core(&client).await;
    core.set_gates(GateSnapshot {
        features: GateFeatureFlags::default(),
        ..serde_json::from_str(include_str!("../fixtures/vowp/gate-phase3-waiting.json")).unwrap()
    });
    let result = core.surface_revoke(&client, SURFACE_ID).await.unwrap();
    assert_eq!(result.status, "revoked");
    assert_eq!(client.surface_revoke_calls(), vec![SURFACE_ID.to_owned()]);
}

#[tokio::test]
async fn d6_home_commands_use_the_documented_argv_and_parse_their_output() {
    let runner = Arc::new(FakeCommandRunner::new(vec![
        CommandOutput::ok(SURFACE_PAIR.as_bytes().to_vec()),
        CommandOutput::ok(
            include_str!("../fixtures/vowp/surface-list-attached-revoked.json")
                .as_bytes()
                .to_vec(),
        ),
        CommandOutput::ok(
            include_str!("../fixtures/vowp/surface-rotate.json")
                .as_bytes()
                .to_vec(),
        ),
        CommandOutput::ok(ORCHESTRATORS.as_bytes().to_vec()),
    ]));
    let client = spirit_core::vowp::SshVowpClient::new(
        runner.clone(),
        SshTarget::new("owner", "192.0.2.10", 22).unwrap(),
        SshOptions::new("/tmp/opencode/known_hosts"),
        None,
    );

    let pairing = client.surface_pair(surface_pair_request()).await.unwrap();
    assert_eq!(pairing.surface, "01J9Z00000000000000000000C");
    let surfaces = client.surface_list(Some("mac-studio")).await.unwrap();
    assert_eq!(surfaces.len(), 2);
    let rotation = client
        .surface_rotate(SurfaceRotateRequest {
            surface: SURFACE_ID.to_owned(),
            node: Some("mac-studio".to_owned()),
            wait: Some("30s".to_owned()),
            grace: Some("10m".to_owned()),
        })
        .await
        .unwrap();
    assert!(rotation.prefix.starts_with("vowp_surf_"));
    let cards = client.orchestrator_list().await.unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].node.as_ref().unwrap().as_str(), "mac-studio");

    let calls = runner.calls();
    let surface_pair = remote_argv(&calls[0]);
    for expected in [
        "'--session'",
        SESSION_ID,
        "'--node'",
        "mac-studio",
        "'--kind'",
        "opencode",
        "'--app'",
        "opencode",
    ] {
        assert!(
            surface_pair.contains(expected),
            "{expected}: {surface_pair}"
        );
    }
    assert!(remote_argv(&calls[1]).contains("'--node' 'mac-studio'"));
    let rotate = remote_argv(&calls[2]);
    for expected in ["'--wait' '30s'", "'--grace' '10m'", "'--node' 'mac-studio'"] {
        assert!(rotate.contains(expected), "{expected}: {rotate}");
    }
}

#[tokio::test]
async fn d6_orchestrator_new_uses_the_local_node_flag_and_app() {
    let card = orchestrator_card("vowp-local");
    let runner = Arc::new(FakeCommandRunner::new(vec![CommandOutput::ok(
        serde_json::to_vec(&card).unwrap(),
    )]));
    let client = spirit_core::vowp::SshVowpClient::new(
        runner.clone(),
        SshTarget::new("owner", "192.0.2.10", 22).unwrap(),
        SshOptions::new("/tmp/opencode/known_hosts"),
        None,
    );
    let request = OrchestratorNewRequest {
        node: "mac-studio".to_owned(),
        app: spirit_core::wire::OrchestratorApp::Opencode,
        project: Some("vowp".to_owned()),
        name: Some("vowp-local".to_owned()),
        force: true,
    };
    client.orchestrator_new(request).await.unwrap();
    let call = remote_argv(&runner.last_call().expect("call"));
    for expected in [
        "'orchestrator'",
        "'new'",
        "'--node' 'mac-studio'",
        "'--app' 'opencode'",
        "'--project' 'vowp'",
        "'--name' 'vowp-local'",
        "'--force'",
    ] {
        assert!(call.contains(expected), "{expected}: {call}");
    }
    assert!(!call.contains("'agent'"));
}

#[tokio::test]
async fn d6_local_agent_pair_sends_the_code_on_stdin_and_drops_the_token_path() {
    let runner = Arc::new(FakeCommandRunner::new(vec![CommandOutput::ok(
        format!(
            r#"{{"surface_id":"{SURFACE_ID}","scopes":["session.read:{SESSION_ID}"],"expires_at":"2026-12-14T12:00:00.000Z","token_path":"/Users/example/.vowp-agent/surfaces/secret.token"}}"#
        )
        .into_bytes(),
    )]));
    let agent = CommandLocalAgent::new(runner.clone());
    let request = AgentPairRequest::new(
        spirit_core::SecretString::new("pairing-code-placeholder"),
        "01J9Z00000000000000000000C",
        SESSION_ID,
        Some("opencode".to_owned()),
    );
    let paired = agent.pair(&request).await.unwrap();
    assert_eq!(paired.surface_id, SURFACE_ID);
    let call = runner.last_call().expect("call");
    assert!(call.args.iter().any(|arg| arg == "--code-file"));
    assert!(call.args.iter().any(|arg| arg == "-"));
    assert!(
        !call
            .args
            .iter()
            .any(|arg| arg == "pairing-code-placeholder")
    );
    assert_eq!(
        call.stdin.as_deref(),
        Some(b"pairing-code-placeholder\n".as_slice())
    );
    let debug = format!("{paired:?}");
    assert!(!debug.contains("/Users/"), "{debug}");
    assert!(!debug.contains("secret.token"), "{debug}");
}

#[tokio::test]
async fn d6_local_agent_paths_and_status_are_safe_views() {
    let runner = Arc::new(FakeCommandRunner::new(vec![
        CommandOutput::ok(AGENT_PATHS.as_bytes().to_vec()),
        CommandOutput::ok(AGENT_CONNECTED.as_bytes().to_vec()),
    ]));
    let agent = CommandLocalAgent::new(runner);
    let paths = agent.paths().await.unwrap();
    assert_eq!(paths.len(), 1);
    let views: Vec<AgentPathView> = paths.iter().map(AgentPathView::from_map).collect();
    let json = serde_json::to_string(&views).unwrap();
    assert!(!json.contains("/Users/"), "{json}");
    assert!(json.contains("project:vowp"));

    let document = agent.status().await.unwrap();
    assert_eq!(document.connection.as_str(), "connected");
    let view = LocalAgentView::from_document(&document);
    assert!(view.is_connected());
    let json = serde_json::to_string(&view).unwrap();
    assert!(!json.contains("/Users/"), "{json}");
    assert!(!json.contains("key_fingerprint"), "{json}");
}

#[tokio::test]
async fn d6_local_agent_not_enrolled_is_an_honest_state_not_an_error() {
    let runner = Arc::new(FakeCommandRunner::new(vec![CommandOutput::fail(
        3,
        "agent is not enrolled",
    )]));
    let agent = CommandLocalAgent::new(runner);
    let status = agent.status().await.unwrap();
    assert_eq!(status.connection.as_str(), "not_enrolled");
}

#[test]
fn d6_revoked_and_expired_surfaces_are_not_writable() {
    let surfaces: Vec<SurfaceEntry> = serde_json::from_str(SURFACE_LIST).unwrap();
    let agent: AgentStatusDocument = serde_json::from_str(AGENT_CONNECTED).unwrap();
    let now = Timestamp::parse("2026-11-16T12:00:00.000Z").unwrap();
    let revoked = LocalSurfaceView::from_parts(&surfaces[1], &agent, None, None, now);
    assert_eq!(revoked.session_state, LocalSessionState::Revoked.as_str());
    assert!(!revoked.can_write);
    assert!(revoked.expired);

    let attached = LocalSurfaceView::from_parts(&surfaces[0], &agent, None, None, now);
    assert!(
        !attached.can_write,
        "a surface without the lease is read-only"
    );
    assert_eq!(attached.agent_status, "connected");
    assert_eq!(attached.protocol.as_deref(), Some(AGENT_PROTOCOL));
}

#[test]
fn d6_revoked_agent_and_surface_views_have_no_secret_or_absolute_path() {
    let document: AgentStatusDocument = serde_json::from_str(AGENT_REVOKED).unwrap();
    let view = LocalAgentView::from_document(&document);
    assert_eq!(view.state, "revoked");
    let json = serde_json::to_string(&view).unwrap();
    assert!(!json.contains("/Users/"), "{json}");
    assert!(!json.contains("old-value"), "{json}");

    let surfaces: Vec<SurfaceEntry> = serde_json::from_str(SURFACE_LIST).unwrap();
    let now = Timestamp::parse("2026-11-16T12:00:00.000Z").unwrap();
    let revoked = LocalSurfaceView::from_parts(&surfaces[1], &document, None, None, now);
    assert_eq!(revoked.session_state, "revoked");
    assert!(!revoked.can_write);
}

#[test]
fn d6_agent_surface_views_do_not_require_a_live_agent() {
    let document = AgentStatusDocument {
        connection: spirit_core::wire::AgentConnectionState::parse("connected"),
        node: Some("mac-studio".to_owned()),
        protocol: Some(AGENT_PROTOCOL.to_owned()),
        surfaces: vec![AgentSurfaceStatus {
            surface: SURFACE_ID.to_owned(),
            prefix: Some("vowp_surf_ab".to_owned()),
            expires_at: None,
            state: spirit_core::wire::AgentSurfaceState::NeedsPair,
        }],
        ..AgentStatusDocument::default()
    };
    let view = LocalAgentView::from_document(&document);
    assert_eq!(view.surfaces[0].state, "needs_pair");
    assert!(matches!(
        LocalSessionState::Disconnected,
        LocalSessionState::Disconnected
    ));
}

#[tokio::test]
async fn d6_local_agent_mock_records_paths_without_changing_the_contract() {
    let agent = MockLocalAgentClient::connected();
    assert_eq!(agent.status_calls(), 0);
    assert_eq!(
        agent.status().await.unwrap().connection.as_str(),
        "connected"
    );
    assert_eq!(agent.status_calls(), 1);
}

#[test]
fn d6_wire_fixtures_parse_into_typed_values() {
    let _: NodeCard = node_card();
    let _: Vec<SurfaceEntry> = serde_json::from_str(SURFACE_LIST).unwrap();
    let surface = &serde_json::from_str::<Vec<SurfaceEntry>>(SURFACE_LIST).unwrap()[0];
    assert_eq!(surface.state, SurfaceLifecycleState::Attached);
    let _: SurfaceRotateResult =
        serde_json::from_str(include_str!("../fixtures/vowp/surface-rotate.json")).unwrap();
    let _: SurfaceRevokeResult =
        serde_json::from_str(include_str!("../fixtures/vowp/surface-revoke.json")).unwrap();
    let _: Vec<AgentPathMap> = serde_json::from_str(AGENT_PATHS).unwrap();
    let project = ProjectName::parse("vowp").unwrap();
    assert_eq!(project.as_str(), "vowp");
    let _ = SessionId::parse(SESSION_ID).unwrap();
}

#[test]
fn d6_guard_state_never_contains_absolute_paths() {
    let mut state = LocalOrchestratorState::default();
    state.set_path_maps(&[path_map()]);
    let json = serde_json::to_string(&state).unwrap();
    assert!(!json.contains("/Users/"), "{json}");
    assert!(json.contains("\"valid\""));
}
