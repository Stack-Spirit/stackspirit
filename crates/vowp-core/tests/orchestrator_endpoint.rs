//! Orchestrator endpoint discovery: the home document, the local service file and the
//! security rule that no card or snapshot carries the app password.
//!
//! Every test uses a temporary node home, a private live `host.json`/`service.json` and a
//! process id that is this test process. No test starts an app, opens a socket, reaches a
//! node or reads the real home directory.

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use support::fake_git::FakeGit;
use tempfile::TempDir;
use vowp_core::surface::orchestrator::Orchestrator;
use vowp_core::{AdapterRegistry, Deps, FixedRng, SystemRunner};
use vowp_protocol::{
    Clock, FakeClock, FleetStatus, Node, NodeName, NodeStatus, OrchestratorApp, OrchestratorLocation,
    OrchestratorNodeState, Project, ProjectName, SessionId, Timestamp, VowpConfig,
};
use vowp_store::{NodePaths, Store};
use vowp_tmux::fake::FakeWindows;

const SESSION: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const NATIVE: &str = "ses_native_endpoint";

fn ts() -> Timestamp {
    Timestamp::parse("2026-11-19T12:00:00Z").expect("test timestamp")
}

fn session() -> SessionId {
    SessionId::parse(SESSION).expect("session id")
}

fn home_node() -> NodeName {
    NodeName::parse("home-1").expect("home node")
}

fn agent_node() -> NodeName {
    NodeName::parse("mac-studio").expect("agent node")
}

/// A temporary home with one registered project and one local node row.
struct Fixture {
    _home: TempDir,
    _project: TempDir,
    deps: Deps,
}

impl Fixture {
    fn new() -> Self {
        let home = tempfile::tempdir().expect("temp home");
        let project = tempfile::tempdir().expect("temp project");
        let paths = NodePaths::new(home.path());
        paths.ensure().expect("node paths");
        let clock = FakeClock::at(ts());
        let store = Store::open(&paths)
            .expect("store opens")
            .with_clock(Arc::new(clock.clone()));
        let now = clock.now();
        let local = home_node();
        store
            .upsert_node(&Node {
                name: local.clone(),
                ssh_target: None,
                is_local: true,
                capabilities: serde_json::json!({}),
                last_seen_at: Some(now),
                created_at: now,
            })
            .expect("local node");
        let project_name = ProjectName::parse("vowp").expect("project name");
        let mut config = VowpConfig::with_defaults_for_tests();
        config.project.name = project_name.clone();
        config.project.path = Some(project.path().display().to_string());
        store
            .upsert_project(&Project {
                name: project_name.clone(),
                path: project.path().display().to_string(),
                remote_url: None,
                default_branch: "main".to_owned(),
                config: serde_json::to_value(&config).expect("config json"),
                created_at: now,
                updated_at: now,
            })
            .expect("project row");
        Store::create_project_db(&paths, &project_name).expect("project db");
        let deps = Deps {
            paths,
            config: Arc::new(config),
            node: local,
            clock: Arc::new(clock),
            log: Arc::new(vowp_protocol::MemoryLog::new()),
            adapters: Arc::new(AdapterRegistry::new()),
            windows: Arc::new(FakeWindows::new()),
            git: Arc::new(FakeGit::new()),
            cmd: Arc::new(SystemRunner),
            rng: Arc::new(FixedRng(0)),
        };
        Self {
            _home: home,
            _project: project,
            deps,
        }
    }

    fn store(&self) -> Store {
        Store::open_global(&self.deps.paths).expect("global store")
    }

    /// Registers one active, online agent node.
    fn add_agent_node(&self) {
        let store = self.store();
        let now = self.deps.clock.now();
        let node = agent_node();
        store
            .insert_pending_node(&node, &["darwin-arm64".to_owned()], now)
            .expect("pending node");
        store
            .set_node_status(&node, NodeStatus::Active, "", now)
            .expect("active node");
        store
            .touch_node(&node, now, Some("0.4.0"), Some("vowp-agent/1"), &serde_json::json!({}))
            .expect("node heartbeat");
    }
}

/// Writes one private `orchestrator.json` in the home's own shape.
fn write_orchestrator(deps: &Deps, name: &str, node: Option<NodeName>, state: OrchestratorNodeState) -> PathBuf {
    let dir = deps.paths.root().join("orchestrators").join(name);
    create_private_dir(&dir);
    let value = serde_json::json!({
        "name": name,
        "app": OrchestratorApp::Opencode,
        "session_id": session(),
        "project": null,
        "executor": null,
        "state": state,
        "created_at": deps.clock.now(),
        "join": {
            "desktop": format!("vowp orchestrator resume {name}"),
            "phone": format!("vowp chat {SESSION}"),
            "chat": format!("vowp chat --agent --session {SESSION}"),
        },
        "surface_id": null,
        "node": node,
        "last_request_id": null,
    });
    write_private(
        &dir.join("orchestrator.json"),
        &serde_json::to_vec(&value).expect("state json"),
    );
    dir
}

/// Writes a live `host.json` shaped like the opencode host's own file.
fn write_live_host(dir: &Path, port: u16) {
    let value = serde_json::json!({
        "kind": "opencode",
        "name": "main",
        "pid": std::process::id(),
        "port": port,
        "url": format!("http://127.0.0.1:{port}"),
        "started_at": ts(),
        "workdir": dir.display().to_string(),
        "service_json": dir.join(".opencode/state/opencode/service.json").display().to_string(),
        "native_session_id": NATIVE,
        "vowp_session": SESSION,
    });
    write_private(&dir.join("host.json"), &serde_json::to_vec(&value).expect("host json"));
}

/// Writes the private opencode `service.json` below `dir`.
fn write_service(dir: &Path, url: &str, port: u16, password: &str) {
    let path = dir.join(".opencode/state/opencode/service.json");
    if let Some(parent) = path.parent() {
        create_private_dir(parent);
    }
    let value = serde_json::json!({
        "version": "2.0.28",
        "url": url,
        "port": port,
        "pid": std::process::id(),
        "password": password,
    });
    write_private(&path, &serde_json::to_vec(&value).expect("service json"));
}

fn create_private_dir(dir: &Path) {
    fs::create_dir_all(dir).expect("private dir");
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).expect("private dir mode");
}

fn write_private(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).expect("private file");
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).expect("private file mode");
}

fn manager(fixture: &Fixture) -> Orchestrator {
    Orchestrator::new(fixture.deps.clone())
}

#[test]
fn orchestrator_endpoint_home_is_metadata_only_for_a_node() {
    let fixture = Fixture::new();
    fixture.add_agent_node();
    let dir = write_orchestrator(&fixture.deps, "mac", Some(agent_node()), OrchestratorNodeState::Running);
    // Bait: even a live-looking local host and service file must be ignored for a node
    // orchestrator. The home never reads a node's files.
    write_live_host(&dir, 45454);
    let password = format!("vowp-endpoint-{}", "A".repeat(24));
    write_service(&dir, "http://127.0.0.1:45454", 45454, &password);

    let endpoint = manager(&fixture).endpoint("mac").expect("node endpoint");
    assert_eq!(endpoint.name, "mac");
    assert_eq!(endpoint.app, OrchestratorApp::Opencode);
    assert_eq!(endpoint.session_id, session());
    assert_eq!(endpoint.location, OrchestratorLocation::Node);
    assert_eq!(endpoint.node.as_ref().map(NodeName::as_str), Some("mac-studio"));
    assert_eq!(endpoint.state, OrchestratorNodeState::Running);
    assert!(endpoint.url.is_none(), "a node document has no url: {endpoint:?}");
    assert!(
        endpoint.native_session_id.is_none(),
        "a node document has no native id: {endpoint:?}"
    );
    assert!(endpoint.auth.is_none(), "a node document has no auth: {endpoint:?}");
}

#[test]
fn orchestrator_endpoint_home_reads_the_live_local_service_file() {
    let fixture = Fixture::new();
    let dir = write_orchestrator(&fixture.deps, "main", None, OrchestratorNodeState::Running);
    write_live_host(&dir, 43111);
    let password = format!("vowp-endpoint-{}", "B".repeat(24));
    write_service(&dir, "http://localhost:43111", 43111, &password);

    let orchestrator = manager(&fixture);
    let endpoint = orchestrator.endpoint("main").expect("home endpoint");
    assert_eq!(endpoint.location, OrchestratorLocation::Home);
    assert_eq!(endpoint.node, None);
    assert_eq!(endpoint.session_id, session());
    assert_eq!(endpoint.state, OrchestratorNodeState::Running);
    assert_eq!(endpoint.url.as_deref(), Some("http://127.0.0.1:43111"));
    assert_eq!(endpoint.native_session_id.as_deref(), Some(NATIVE));
    let auth = endpoint.auth.as_ref().expect("live local auth");
    assert_eq!(auth.header, "Authorization");
    assert_eq!(auth.scheme, "Basic");
    assert_eq!(auth.username, "opencode");
    assert_eq!(auth.password, password);

    // The explicit endpoint document is the only place the password appears.
    assert!(
        serde_json::to_string(&endpoint)
            .expect("endpoint JSON")
            .contains(&password),
        "the explicit endpoint document carries the password"
    );
    let cards = orchestrator.list().expect("orchestrator list");
    let cards_json =
        serde_json::to_string(&cards.iter().map(|info| &info.card).collect::<Vec<_>>()).expect("cards JSON");
    assert!(
        !cards_json.contains(&password),
        "orchestrator ls never carries the password"
    );
    assert!(
        !format!("{endpoint:?}").contains(&password),
        "Debug redacts the password"
    );
}

#[test]
fn orchestrator_endpoint_stopped_or_unknown_has_no_credentials() {
    let fixture = Fixture::new();
    let dir = write_orchestrator(&fixture.deps, "stopped", None, OrchestratorNodeState::Stopped);
    // A stale service file from an earlier run must not be treated as a live endpoint.
    let stale = format!("vowp-endpoint-{}", "C".repeat(24));
    write_service(&dir, "http://127.0.0.1:45555", 45555, &stale);
    let orchestrator = manager(&fixture);
    let endpoint = orchestrator.endpoint("stopped").expect("stopped endpoint");
    assert_eq!(endpoint.location, OrchestratorLocation::Home);
    assert_eq!(endpoint.state, OrchestratorNodeState::Stopped);
    assert!(endpoint.url.is_none(), "a stopped host has no url");
    assert!(endpoint.native_session_id.is_none(), "a stopped host has no native id");
    assert!(endpoint.auth.is_none(), "a stopped host has no auth");
    assert!(!serde_json::to_string(&endpoint).expect("stopped JSON").contains(&stale));

    let unknown = orchestrator.endpoint("missing").expect_err("unknown is not found");
    let text = unknown.to_string();
    assert!(
        text.contains("orchestrator missing"),
        "the error names the orchestrator: {text}"
    );

    // A corrupt local record is a typed, path-free failure naming the orchestrator.
    let broken = write_orchestrator(&fixture.deps, "broken", None, OrchestratorNodeState::Running);
    write_private(&broken.join("orchestrator.json"), b"{\"name\":");
    let error = orchestrator.endpoint("broken").expect_err("a corrupt record is typed");
    let text = error.to_string();
    assert!(
        text.contains("orchestrator broken"),
        "the error names the orchestrator: {text}"
    );
    for forbidden in [broken.display().to_string(), "orchestrator.json".to_owned()] {
        assert!(!text.contains(&forbidden), "the error leaked {forbidden:?}: {text}");
    }

    // A corrupt service file is a typed, path-free failure.
    let corrupt = write_orchestrator(&fixture.deps, "corrupt", None, OrchestratorNodeState::Running);
    write_live_host(&corrupt, 45666);
    let path = corrupt.join(".opencode/state/opencode/service.json");
    if let Some(parent) = path.parent() {
        create_private_dir(parent);
    }
    write_private(&path, b"{\"url\":\"http://127.0.0.1:45666\",\"password\":");
    let error = orchestrator
        .endpoint("corrupt")
        .expect_err("a corrupt service file is typed");
    let text = error.to_string();
    assert!(
        text.contains("orchestrator corrupt"),
        "the error names the orchestrator and failure class: {text}"
    );
    for forbidden in [
        corrupt.display().to_string(),
        path.display().to_string(),
        "45666".to_owned(),
        "service.json".to_owned(),
    ] {
        assert!(!text.contains(&forbidden), "the error leaked {forbidden:?}: {text}");
    }

    // A service file wider than 0600 is not trusted as local auth material.
    let wide = serde_json::json!({
        "version": "2.0.28",
        "url": "http://127.0.0.1:45666",
        "port": 45666,
        "pid": std::process::id(),
        "password": format!("vowp-endpoint-{}", "Z".repeat(24)),
    });
    fs::write(&path, serde_json::to_vec(&wide).expect("wide service json")).expect("wide service");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("wide service mode");
    let error = orchestrator
        .endpoint("corrupt")
        .expect_err("a wide service file is typed");
    let text = error.to_string();
    assert!(
        text.contains("orchestrator corrupt"),
        "the error names the orchestrator: {text}"
    );
    assert!(!text.contains("45666"), "the error carries no port: {text}");
    assert!(
        !text.contains(&corrupt.display().to_string()),
        "the error carries no path: {text}"
    );

    // A non-loopback URL is refused rather than returned as an endpoint.
    let public = write_orchestrator(&fixture.deps, "public", None, OrchestratorNodeState::Running);
    write_live_host(&public, 45777);
    write_service(&public, "http://0.0.0.0:45777", 45777, &stale);
    let error = orchestrator
        .endpoint("public")
        .expect_err("non-loopback URL is refused");
    let text = error.to_string();
    assert!(!text.contains("0.0.0.0"), "the error carries no address: {text}");
    assert!(!text.contains(&stale), "the error carries no password");
}

#[test]
fn orchestrator_card_snapshot_and_ls_never_contain_the_endpoint_password() {
    let fixture = Fixture::new();
    let dir = write_orchestrator(&fixture.deps, "main", None, OrchestratorNodeState::Running);
    write_live_host(&dir, 43222);
    let password = format!("vowp-endpoint-{}", "D".repeat(24));
    write_service(&dir, "http://127.0.0.1:43222", 43222, &password);

    let orchestrator = manager(&fixture);
    let endpoint = orchestrator.endpoint("main").expect("live endpoint");
    assert_eq!(endpoint.auth.as_ref().expect("auth").password, password);

    let cards = orchestrator.list().expect("orchestrator list");
    assert_eq!(cards.len(), 1, "one orchestrator card");
    let cards_json =
        serde_json::to_string(&cards.iter().map(|info| &info.card).collect::<Vec<_>>()).expect("cards JSON");
    assert!(
        !cards_json.contains(&password),
        "orchestrator ls never carries the password"
    );

    let store = fixture.store();
    let snapshot = vowp_core::fleet::build_snapshot(&fixture.deps, &store, fixture.deps.clock.now()).expect("snapshot");
    assert_eq!(snapshot.orchestrators.len(), 1, "the snapshot has the card");
    let snapshot_json = serde_json::to_string(&snapshot).expect("snapshot JSON");
    assert!(
        !snapshot_json.contains(&password),
        "snapshot.json never carries the password"
    );
    let status = FleetStatus {
        snapshot,
        source: "local".to_owned(),
        fetched_at: fixture.deps.clock.now(),
        stale: false,
    };
    let status_json = serde_json::to_string(&status).expect("fleet status JSON");
    assert!(
        !status_json.contains(&password),
        "fleet status never carries the password"
    );
}
