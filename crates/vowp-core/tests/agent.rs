//! Phase 0 home gateway tests.
//!
//! Every test uses a temporary node home and an in-memory duplex; no test opens the real
//! `~/.vowp`, starts tmux, touches the network or signs with a real node key.

mod support;

use std::collections::VecDeque;
use std::io::{BufReader, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use ed25519_dalek::{Signer, SigningKey};
use support::fake_git::FakeGit;
use tempfile::TempDir;
use vowp_core::agent::gateway::{self, GatewayOptions, GatewayOutcome};
use vowp_core::agent::{
    EnrollRequest, auth, create_pending, create_pending_surface, hash_code, issue_surface_token, redeem, revoke,
    revoke_surface,
};
use vowp_core::{AdapterRegistry, CoreError, Deps, FixedRng, SystemRunner};
use vowp_protocol::{
    Actor, AgentCapabilities, AgentCapacity, AgentFrame, AgentPathMap, Clock, Cost, ErrorKind, EventBody, EventId,
    EventSource, FakeClock, HarnessKind, JobId, JobKind, JobState, LeaseMode, MemoryLog, NewJob, NodeName,
    NodePresence, NodeStatus, PathRef, Placement, Project, ProjectName, RemoteEvent, RemoteLine, Role, RunId,
    RunOutcome, RunPurpose, SecretName, Session, SessionId, SessionRole, SessionState, StopReason, SurfaceId,
    SurfaceKind, SurfaceScope, SurfaceState, Timestamp, TurnSummary, VowpConfig,
};
use vowp_store::session_log::SessionLog;
use vowp_store::{NodePath, NodePaths, StateChange, Store, hash_lease_token};

fn ts() -> Timestamp {
    Timestamp::parse("2026-11-14T10:00:00Z").expect("timestamp")
}

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[7_u8; 32])
}

fn public_hex(key: &SigningKey) -> String {
    key.verifying_key()
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn signature_hex(key: &SigningKey, nonce: &str, home: &str, connection: &str) -> String {
    key.sign(&auth::handshake_payload(nonce, home, connection))
        .to_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn capabilities() -> AgentCapabilities {
    let mut harnesses = std::collections::BTreeMap::new();
    harnesses.insert("opencode".to_owned(), "1.0.0".to_owned());
    AgentCapabilities {
        os: "linux".to_owned(),
        arch: "x86_64".to_owned(),
        agent_version: "0.3.0".to_owned(),
        harnesses,
        git_version: Some("git version 2.47.0".to_owned()),
        labels: vec!["test".to_owned()],
        secret_names: vec![SecretName::parse("BUILD_TOKEN").expect("secret name")],
        projects: vec![ProjectName::parse("vowp").expect("project")],
    }
}

fn test_deps(root: &Path, clock: Arc<FakeClock>, log: Arc<MemoryLog>) -> Deps {
    test_deps_with_config(root, clock, log, VowpConfig::with_defaults_for_tests())
}

fn test_deps_with_config(root: &Path, clock: Arc<FakeClock>, log: Arc<MemoryLog>, config: VowpConfig) -> Deps {
    Deps {
        paths: NodePaths::new(root),
        config: Arc::new(config),
        node: NodeName::parse("home-1").expect("home node"),
        clock,
        log,
        adapters: Arc::new(AdapterRegistry::new()),
        windows: Arc::new(vowp_tmux::fake::FakeWindows::new()),
        git: Arc::new(FakeGit::new().with_head_sha("0123456789abcdef0123456789abcdef01234567")),
        cmd: Arc::new(SystemRunner),
        rng: Arc::new(FixedRng(0)),
    }
}

fn test_store(root: &Path, clock: &Arc<FakeClock>) -> Store {
    Store::open(&NodePaths::new(root))
        .expect("store opens")
        .with_clock(clock.clone())
}

fn node_name(name: &str) -> NodeName {
    NodeName::parse(name).expect("node slug")
}

fn seed_session(store: &Store, node: &NodeName, project: &str, id: &str, now: Timestamp) -> SessionId {
    let id = SessionId::parse(id).expect("session id");
    store
        .upsert_session(&Session {
            id: id.clone(),
            project: Some(ProjectName::parse(project).expect("project")),
            job_id: None,
            role: Some(SessionRole::User),
            title: "surface pairing".to_owned(),
            parent_session_id: None,
            node: node.clone(),
            cwd: "/tmp/vowp".to_owned(),
            state: SessionState::Idle,
            executor: None,
            last_seq: 0,
            last_capsule_id: None,
            log_bytes: 0,
            created_at: now,
            updated_at: now,
        })
        .expect("session");
    id
}

fn hello(name: &NodeName, key: &SigningKey) -> AgentFrame {
    AgentFrame::AgentHello {
        node: name.clone(),
        key_id: public_hex(key),
        agent_version: "0.3.0".to_owned(),
        os: "linux".to_owned(),
        arch: "x86_64".to_owned(),
        labels: vec!["test".to_owned()],
        capabilities: capabilities(),
    }
}

fn enroll_frame(code: &str, key: &SigningKey) -> AgentFrame {
    AgentFrame::Enroll {
        code: code.to_owned(),
        public_key: public_hex(key),
        agent_version: "0.3.0".to_owned(),
        os: "linux".to_owned(),
        arch: "x86_64".to_owned(),
        labels: vec!["test".to_owned()],
        capabilities: capabilities(),
        paths: vec![AgentPathMap {
            project: ProjectName::parse("vowp").expect("project"),
            local_root: "/home/dev/vowp".to_owned(),
            worktree_root: None,
            allow: Vec::new(),
        }],
    }
}

/// Creates a pending node and redeems its code directly, leaving an active credential.
fn activate(root: &Path, clock: &Arc<FakeClock>, name: &NodeName, key: &SigningKey) {
    let store = test_store(root, clock);
    let pending = create_pending(&store, name, &["darwin-arm64".to_owned()], clock.now()).expect("pending node");
    let request = EnrollRequest {
        code: pending.code,
        public_key: public_hex(key),
        agent_version: "0.3.0".to_owned(),
        capabilities: capabilities(),
        paths: Vec::new(),
    };
    redeem(&store, &request, clock.now()).expect("redeem code");
}

/// One controllable in-memory reader.
#[derive(Clone)]
struct TestReader {
    state: Arc<(Mutex<VecDeque<u8>>, Condvar)>,
    closed: Arc<AtomicBool>,
}

impl TestReader {
    fn new() -> Self {
        Self {
            state: Arc::new((Mutex::new(VecDeque::new()), Condvar::new())),
            closed: Arc::new(AtomicBool::new(false)),
        }
    }

    fn push(&self, frame: &AgentFrame) {
        let mut line = serde_json::to_vec(frame).expect("frame JSON");
        line.push(b'\n');
        let (queue, ready) = &*self.state;
        let mut queue = queue.lock().expect("input lock");
        queue.extend(line);
        ready.notify_all();
    }

    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        let (_, ready) = &*self.state;
        ready.notify_all();
    }
}

impl Read for TestReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let (queue, ready) = &*self.state;
        let mut queue = queue.lock().expect("input lock");
        while queue.is_empty() && !self.closed.load(Ordering::SeqCst) {
            queue = ready.wait(queue).expect("input wait");
        }
        if queue.is_empty() {
            return Ok(0);
        }
        let count = buf.len().min(queue.len());
        for slot in buf.iter_mut().take(count) {
            *slot = queue.pop_front().expect("queued byte");
        }
        Ok(count)
    }
}

/// One shared writer that tests can inspect without consuming it.
#[derive(Clone, Default)]
struct TestWriter {
    bytes: Arc<Mutex<Vec<u8>>>,
}

impl TestWriter {
    fn frames(&self) -> Vec<AgentFrame> {
        let bytes = self.bytes.lock().expect("output lock");
        String::from_utf8_lossy(&bytes)
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    fn wait_frame(&self, predicate: impl Fn(&AgentFrame) -> bool, timeout: Duration) -> AgentFrame {
        self.wait_frame_after(0, predicate, timeout)
    }

    fn wait_frame_after(&self, start: usize, predicate: impl Fn(&AgentFrame) -> bool, timeout: Duration) -> AgentFrame {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(frame) = self.frames().into_iter().skip(start).find(|frame| predicate(frame)) {
                return frame;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for a frame; saw {:?}",
                self.frames()
            );
            thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Write for TestWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.bytes.lock().expect("output lock").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// One running gateway connection.
struct Gateway {
    input: TestReader,
    output: TestWriter,
    log: Arc<MemoryLog>,
    clock: Arc<FakeClock>,
    root: std::path::PathBuf,
    handle: Option<JoinHandle<Result<GatewayOutcome, CoreError>>>,
}

impl Gateway {
    fn start(root: &Path, clock: Arc<FakeClock>) -> Self {
        Self::start_with_config(root, clock, VowpConfig::with_defaults_for_tests())
    }

    fn start_with_config(root: &Path, clock: Arc<FakeClock>, config: VowpConfig) -> Self {
        let log = Arc::new(MemoryLog::new());
        let deps = test_deps_with_config(root, clock.clone(), log.clone(), config);
        let store = deps.open_store().expect("bound store");
        let input = TestReader::new();
        let output = TestWriter::default();
        let reader = BufReader::new(input.clone());
        let writer = output.clone();
        let options = GatewayOptions {
            home_id: "home-1".to_owned(),
            heartbeat_secs: 10,
            max_frame_bytes: 1024 * 1024,
            handshake_timeout: Duration::from_secs(2),
        };
        let handle = thread::spawn(move || gateway::serve_with_options(&store, &deps, reader, writer, options));
        Self {
            input,
            output,
            log,
            clock,
            root: root.to_path_buf(),
            handle: Some(handle),
        }
    }

    fn store(&self) -> Store {
        let log = Arc::new(MemoryLog::new());
        test_deps(&self.root, self.clock.clone(), log)
            .open_store()
            .expect("bound store")
    }

    fn send(&self, frame: &AgentFrame) {
        self.input.push(frame);
    }

    fn expect(&self, predicate: impl Fn(&AgentFrame) -> bool) -> AgentFrame {
        self.output.wait_frame(predicate, Duration::from_secs(2))
    }

    fn frame_count(&self) -> usize {
        self.output.frames().len()
    }

    fn expect_after(&self, start: usize, predicate: impl Fn(&AgentFrame) -> bool) -> AgentFrame {
        self.output.wait_frame_after(start, predicate, Duration::from_secs(2))
    }

    fn finish(mut self) -> Result<GatewayOutcome, CoreError> {
        self.input.close();
        self.handle
            .take()
            .expect("gateway thread")
            .join()
            .expect("gateway thread panicked")
    }
}

fn authenticate(gateway: &Gateway, name: &NodeName, key: &SigningKey) {
    gateway.send(&hello(name, key));
    let AgentFrame::HomeChallenge {
        nonce,
        home_id,
        connection_id,
        ..
    } = gateway.expect(|frame| matches!(frame, AgentFrame::HomeChallenge { .. }))
    else {
        panic!("expected HomeChallenge");
    };
    let signature = signature_hex(key, &nonce, &home_id, &connection_id);
    gateway.send(&AgentFrame::AgentAuth { signature });
    gateway.expect(|frame| matches!(frame, AgentFrame::HomeWelcome { .. }));
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !condition() {
        assert!(Instant::now() < deadline, "condition was not met in time");
        thread::sleep(Duration::from_millis(5));
    }
}

/// Pairs one surface and returns its id and plaintext surface token.
fn pair_surface(store: &Store, node: &NodeName, session: &SessionId, app: &str, now: Timestamp) -> (SurfaceId, String) {
    let pending =
        create_pending_surface(store, node, session, SurfaceKind::Opencode, Some(app), now).expect("pending surface");
    let issued = issue_surface_token(store, node, &pending.surface.id, session, app, &pending.code, now)
        .expect("issued surface token");
    (issued.surface_id, issued.token)
}

/// Sends one `LeaseAcquire` frame for a paired surface.
fn acquire_frame(surface: &SurfaceId, session: &SessionId, token: &str, mode: LeaseMode) -> AgentFrame {
    AgentFrame::LeaseAcquire {
        surface: surface.clone(),
        session: session.clone(),
        surface_token: token.to_owned(),
        mode,
    }
}

/// Reads the first `LeaseGrant` after `start`.
fn expect_grant(gateway: &Gateway, start: usize) -> (String, Timestamp, u64) {
    let AgentFrame::LeaseGrant {
        lease_token,
        expires_at,
        tail_seq,
    } = gateway.expect_after(start, |frame| matches!(frame, AgentFrame::LeaseGrant { .. }))
    else {
        panic!("expected LeaseGrant");
    };
    (lease_token, expires_at, tail_seq)
}

/// Creates a session row and an empty log file so `begin_turn` can append.
fn chat_session(root: &Path, store: &Store, node: &NodeName, id: &str, clock: &Arc<FakeClock>) -> SessionId {
    let session = seed_session(store, node, "vowp", id, clock.now());
    let log = SessionLog::create(&NodePaths::new(root), &session, clock.clone()).expect("session log");
    log.sync().expect("sync empty log");
    session
}

/// Builds a `TurnBegin` for the paired surface.
fn turn_begin_frame(surface: &SurfaceId, session: &SessionId, lease: &str, cwd_ref: PathRef) -> AgentFrame {
    AgentFrame::TurnBegin {
        surface: surface.clone(),
        session: session.clone(),
        lease_token: lease.to_owned(),
        prompt: "hello from the Mac".to_owned(),
        executor: "opencode:deepseek/deepseek-flash#max".to_owned(),
        cwd_ref,
        job: None,
        purpose: RunPurpose::Start,
    }
}

/// Reads the first `TurnGrant` after `start`.
fn expect_turn_grant(gateway: &Gateway, start: usize) -> (RunId, SessionId, String, u64) {
    let AgentFrame::TurnGrant {
        run_id,
        session,
        lease_token,
        tail_seq,
        ..
    } = gateway.expect_after(start, |frame| matches!(frame, AgentFrame::TurnGrant { .. }))
    else {
        panic!("expected TurnGrant");
    };
    (run_id, session, lease_token, tail_seq)
}

/// Builds the first `RemoteLine` of a run.
fn hello_line(session: &SessionId, run: &RunId, lease: &str) -> RemoteLine {
    RemoteLine::Hello {
        session_id: session.clone(),
        run_id: run.clone(),
        executor: "opencode:deepseek/deepseek-flash#max".to_owned(),
        node: node_name("mac-studio"),
        spool_id: "test-spool".to_owned(),
        lease: lease.to_owned(),
    }
}

/// Builds one assistant event with a fixed body.
fn assistant_event(id: &EventId, text: &str) -> RemoteEvent {
    let body = EventBody::AssistantMessage(vowp_protocol::AssistantMessage { text: text.to_owned() });
    RemoteEvent {
        id: id.clone(),
        ts: ts(),
        event_type: body.event_type(),
        actor: Actor::Assistant,
        source: EventSource {
            harness: HarnessKind::Opencode,
            model: Some("deepseek/deepseek-flash".to_owned()),
            node: Some(node_name("mac-studio")),
            native_session_id: None,
        },
        payload: body.to_value(),
        raw: None,
        replayable: true,
    }
}

/// Builds an event line for a run.
fn event_line(run: &RunId, event: RemoteEvent) -> AgentFrame {
    AgentFrame::RemoteLine {
        run_id: run.clone(),
        epoch: 1,
        line: RemoteLine::Event { event },
    }
}

/// Builds the terminal `TurnEnd` for a run.
fn turn_end_frame(run: &RunId) -> AgentFrame {
    AgentFrame::TurnEnd {
        run_id: run.clone(),
        summary: TurnSummary {
            exit_code: Some(0),
            outcome: RunOutcome::Ok,
            native_session_id: Some("ses_mac".to_owned()),
            cost: Cost::unknown(),
            limit: None,
            result_text: Some("done".to_owned()),
            stop_reason: StopReason::EndTurn,
            usage: None,
        },
    }
}

/// Reads one session's canonical log.
fn read_log(root: &Path, session: &SessionId) -> Vec<vowp_protocol::EventEnvelope> {
    SessionLog::open_read(&NodePaths::new(root), session)
        .expect("open session log")
        .read_all()
        .expect("read session log")
}

/// Waits for an `IngestAck` after `start`.
fn expect_ack(
    gateway: &Gateway,
    start: usize,
) -> (
    RunId,
    Option<EventId>,
    Option<u64>,
    bool,
    Option<String>,
    Option<String>,
) {
    let AgentFrame::IngestAck {
        run_id,
        event_id,
        seq,
        duplicate,
        lease_revoked,
        error,
    } = gateway.expect_after(start, |frame| matches!(frame, AgentFrame::IngestAck { .. }))
    else {
        panic!("expected IngestAck");
    };
    (run_id, event_id, seq, duplicate, lease_revoked, error)
}

#[test]
fn gateway_enrolls_a_node_with_a_valid_code() {
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    let name = node_name("mac-studio");
    let key = signing_key();
    let store = test_store(home.path(), &clock);
    let pending = create_pending(&store, &name, &["darwin-arm64".to_owned()], clock.now()).expect("pending node");

    let gateway = Gateway::start(home.path(), clock.clone());
    gateway.send(&enroll_frame(&pending.code, &key));
    let AgentFrame::Enrolled {
        home_id,
        node,
        protocol,
        heartbeat_secs,
        ..
    } = gateway.expect(|frame| matches!(frame, AgentFrame::Enrolled { .. }))
    else {
        panic!("expected Enrolled");
    };
    assert_eq!(home_id, "home-1");
    assert_eq!(node, name);
    assert_eq!(protocol, vowp_protocol::AGENT_PROTOCOL);
    assert_eq!(heartbeat_secs, 10);
    assert_eq!(
        gateway.finish().expect("gateway"),
        GatewayOutcome::Enrolled { node: name.clone() }
    );

    let store = test_store(home.path(), &clock);
    let card = store.node_card(&name, clock.now()).expect("card").expect("node exists");
    assert_eq!(card.status, NodeStatus::Active);
    assert_eq!(card.kind, vowp_protocol::NodeKind::Agent);
    assert_eq!(card.labels, vec!["darwin-arm64".to_owned()]);
    let credential = store
        .node_credential(&name, &public_hex(&key))
        .expect("credential")
        .expect("credential exists");
    assert_eq!(credential.public_key, public_hex(&key));
    assert!(credential.revoked_at.is_none());
    assert!(
        store
            .consume_enrollment_code(&hash_code(&pending.code), clock.now())
            .expect("code lookup")
            .is_none(),
        "the code must be single-use"
    );
    assert_eq!(store.list_node_paths(&name).expect("paths").len(), 1);
}

#[test]
fn gateway_refuses_an_expired_or_used_code() {
    let key = signing_key();

    // Expired: the ten-minute window has passed.
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    let name = node_name("expired-node");
    let store = test_store(home.path(), &clock);
    let pending = create_pending(&store, &name, &[], clock.now()).expect("pending node");
    clock.advance(Duration::from_secs(601));
    let gateway = Gateway::start(home.path(), clock.clone());
    gateway.send(&enroll_frame(&pending.code, &key));
    let AgentFrame::Error { retryable, .. } = gateway.expect(|frame| matches!(frame, AgentFrame::Error { .. })) else {
        panic!("expected Error");
    };
    assert!(!retryable);
    assert_eq!(
        gateway.finish().expect("gateway"),
        GatewayOutcome::Closed { node: None }
    );

    // Used: the same code was already redeemed.
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    let name = node_name("used-node");
    let store = test_store(home.path(), &clock);
    let pending = create_pending(&store, &name, &[], clock.now()).expect("pending node");
    let request = EnrollRequest {
        code: pending.code.clone(),
        public_key: public_hex(&key),
        agent_version: "0.3.0".to_owned(),
        capabilities: capabilities(),
        paths: Vec::new(),
    };
    redeem(&store, &request, clock.now()).expect("first redemption");
    let gateway = Gateway::start(home.path(), clock.clone());
    gateway.send(&enroll_frame(&pending.code, &key));
    let AgentFrame::Error { retryable, .. } = gateway.expect(|frame| matches!(frame, AgentFrame::Error { .. })) else {
        panic!("expected Error");
    };
    assert!(!retryable);
    gateway.finish().expect("gateway");
}

#[test]
fn gateway_auth_rejects_a_bad_signature_and_a_replayed_nonce() {
    let key = signing_key();
    let name = node_name("mac-studio");
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    activate(home.path(), &clock, &name, &key);

    // A signature over the wrong bytes is refused.
    let gateway = Gateway::start(home.path(), clock.clone());
    gateway.send(&hello(&name, &key));
    let AgentFrame::HomeChallenge {
        home_id, connection_id, ..
    } = gateway.expect(|frame| matches!(frame, AgentFrame::HomeChallenge { .. }))
    else {
        panic!("expected HomeChallenge");
    };
    let bad = signature_hex(&key, "another-nonce", &home_id, &connection_id);
    gateway.send(&AgentFrame::AgentAuth { signature: bad });
    let AgentFrame::Error { kind, .. } = gateway.expect(|frame| matches!(frame, AgentFrame::Error { .. })) else {
        panic!("expected Error");
    };
    assert_eq!(kind, ErrorKind::Auth);
    assert_eq!(
        gateway.finish().expect("gateway"),
        GatewayOutcome::Closed { node: None }
    );

    // A signature captured on one challenge cannot authenticate a new connection.
    let first = Gateway::start(home.path(), clock.clone());
    first.send(&hello(&name, &key));
    let AgentFrame::HomeChallenge {
        nonce: first_nonce,
        home_id: first_home,
        connection_id: first_connection,
        ..
    } = first.expect(|frame| matches!(frame, AgentFrame::HomeChallenge { .. }))
    else {
        panic!("expected first HomeChallenge");
    };
    let captured = signature_hex(&key, &first_nonce, &first_home, &first_connection);
    first.send(&AgentFrame::AgentAuth {
        signature: captured.clone(),
    });
    first.expect(|frame| matches!(frame, AgentFrame::HomeWelcome { .. }));
    first.finish().expect("first gateway");

    let replay = Gateway::start(home.path(), clock.clone());
    replay.send(&hello(&name, &key));
    let AgentFrame::HomeChallenge {
        nonce: replay_nonce, ..
    } = replay.expect(|frame| matches!(frame, AgentFrame::HomeChallenge { .. }))
    else {
        panic!("expected replay HomeChallenge");
    };
    assert_ne!(replay_nonce, first_nonce, "each connection gets a fresh nonce");
    replay.send(&AgentFrame::AgentAuth { signature: captured });
    let AgentFrame::Error { kind, .. } = replay.expect(|frame| matches!(frame, AgentFrame::Error { .. })) else {
        panic!("expected Error");
    };
    assert_eq!(kind, ErrorKind::Auth);
    replay.finish().expect("replay gateway");
}

#[test]
fn gateway_heartbeat_updates_presence_and_capacity() {
    let key = signing_key();
    let name = node_name("mac-studio");
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    activate(home.path(), &clock, &name, &key);

    let gateway = Gateway::start(home.path(), clock.clone());
    authenticate(&gateway, &name, &key);
    let capacity = vowp_protocol::AgentCapacity {
        cpu_cores: 12,
        load1: 0.5,
        mem_total_bytes: 32 * 1024 * 1024 * 1024,
        mem_free_bytes: 16 * 1024 * 1024 * 1024,
        disk_free_bytes: 200 * 1024 * 1024 * 1024,
        busy_slots: 1,
        max_slots: 4,
        on_battery: Some(false),
        thermal_state: Some("nominal".to_owned()),
    };
    gateway.send(&AgentFrame::Heartbeat {
        at: clock.now(),
        capacity: capacity.clone(),
        pending_spool_bytes: 42,
    });

    let store = gateway.store();
    wait_until(|| {
        store
            .node_card(&name, clock.now())
            .expect("card")
            .is_some_and(|card| card.capacity.is_some())
    });
    let card = store.node_card(&name, clock.now()).expect("card").expect("node");
    assert_eq!(card.presence, NodePresence::Online);
    assert_eq!(card.capacity, Some(capacity.clone()));
    assert_eq!(card.agent_version.as_deref(), Some("0.3.0"));
    assert_eq!(card.protocol.as_deref(), Some(vowp_protocol::AGENT_PROTOCOL));

    // Presence is computed from the home clock; an old heartbeat goes offline.
    clock.advance(Duration::from_secs(100));
    let card = store.node_card(&name, clock.now()).expect("card").expect("node");
    assert_eq!(card.presence, NodePresence::Offline);

    // A new heartbeat brings it back and replaces the capacity snapshot.
    let mut updated = capacity.clone();
    updated.busy_slots = 2;
    gateway.send(&AgentFrame::Heartbeat {
        at: clock.now(),
        capacity: updated.clone(),
        pending_spool_bytes: 0,
    });
    wait_until(|| {
        store
            .node_card(&name, clock.now())
            .expect("card")
            .is_some_and(|card| card.capacity.as_ref() == Some(&updated))
    });
    let card = store.node_card(&name, clock.now()).expect("card").expect("node");
    assert_eq!(card.presence, NodePresence::Online);
    assert_eq!(card.capacity, Some(updated));
    gateway.finish().expect("gateway");
}

#[test]
fn gateway_revoke_marks_and_closes() {
    let key = signing_key();
    let name = node_name("mac-studio");
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    activate(home.path(), &clock, &name, &key);

    let gateway = Gateway::start(home.path(), clock.clone());
    authenticate(&gateway, &name, &key);
    let store = gateway.store();
    assert_eq!(revoke(&store, &name, clock.now()).expect("revoke"), 1);

    gateway.send(&AgentFrame::Heartbeat {
        at: clock.now(),
        capacity: vowp_protocol::AgentCapacity {
            cpu_cores: 4,
            load1: 0.1,
            mem_total_bytes: 1024,
            mem_free_bytes: 512,
            disk_free_bytes: 1024,
            busy_slots: 0,
            max_slots: 1,
            on_battery: None,
            thermal_state: None,
        },
        pending_spool_bytes: 0,
    });
    let AgentFrame::Revoked { reason } = gateway.expect(|frame| matches!(frame, AgentFrame::Revoked { .. })) else {
        panic!("expected Revoked");
    };
    assert!(reason.contains("revoked"));
    gateway.finish().expect("gateway");

    let card = store.node_card(&name, clock.now()).expect("card").expect("node");
    assert_eq!(card.status, NodeStatus::Revoked);
    assert_eq!(card.presence, NodePresence::Revoked);
    let credential = store
        .node_credential(&name, &public_hex(&key))
        .expect("credential")
        .expect("credential exists");
    assert!(credential.revoked_at.is_some());
}

#[test]
fn gateway_unknown_or_revoked_node_is_refused() {
    let key = signing_key();

    // Unknown: no node row at all.
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    let name = node_name("ghost");
    let gateway = Gateway::start(home.path(), clock.clone());
    gateway.send(&hello(&name, &key));
    let AgentFrame::Error { kind, .. } = gateway.expect(|frame| matches!(frame, AgentFrame::Error { .. })) else {
        panic!("expected Error");
    };
    assert_eq!(kind, ErrorKind::NotFound);
    assert_eq!(
        gateway.finish().expect("gateway"),
        GatewayOutcome::Closed { node: None }
    );

    // Revoked: the node exists but its status is revoked.
    let name = node_name("revoked-node");
    activate(home.path(), &clock, &name, &key);
    let store = test_store(home.path(), &clock);
    revoke(&store, &name, clock.now()).expect("revoke");
    let gateway = Gateway::start(home.path(), clock.clone());
    gateway.send(&hello(&name, &key));
    gateway.expect(|frame| matches!(frame, AgentFrame::Revoked { .. }));
    assert_eq!(
        gateway.finish().expect("gateway"),
        GatewayOutcome::Closed { node: None }
    );
}

const SESSION: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

#[test]
fn gateway_surface_pair_issues_and_persists_hash() {
    let key = signing_key();
    let name = node_name("mac-studio");
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    activate(home.path(), &clock, &name, &key);
    let store = test_store(home.path(), &clock);
    let session = seed_session(&store, &name, "vowp", SESSION, clock.now());
    let pending = create_pending_surface(
        &store,
        &name,
        &session,
        SurfaceKind::Opencode,
        Some("opencode"),
        clock.now(),
    )
    .expect("pending surface");

    let gateway = Gateway::start(home.path(), clock.clone());
    authenticate(&gateway, &name, &key);
    gateway.send(&AgentFrame::SurfacePair {
        code: pending.code.clone(),
        surface_id: pending.surface.id.clone(),
        session: session.clone(),
        app: "opencode".to_owned(),
    });
    let AgentFrame::SurfaceIssued {
        surface_id,
        token,
        scopes,
        expires_at,
    } = gateway.expect(|frame| matches!(frame, AgentFrame::SurfaceIssued { .. }))
    else {
        panic!("expected SurfaceIssued");
    };
    assert_eq!(surface_id, pending.surface.id);
    assert!(token.starts_with("vowp_surf_"));
    assert_eq!(
        scopes,
        vec![
            SurfaceScope::new(format!("session.read:{SESSION}")),
            SurfaceScope::new(format!("session.write:{SESSION}")),
            SurfaceScope::new("project.read:vowp"),
            SurfaceScope::new("fleet.read"),
        ]
    );
    assert_eq!(expires_at, clock.now().plus(vowp_core::agent::SURFACE_TOKEN_TTL));

    let token_hash = hash_code(&token);
    let stored = store
        .surface_token_by_hash(&token_hash)
        .expect("hash lookup")
        .expect("token row");
    assert_eq!(stored.token_hash, token_hash);
    assert_ne!(stored.token_hash, token);
    assert_eq!(stored.surface_id, pending.surface.id);
    assert_eq!(stored.scopes, scopes);
    assert_eq!(stored.expires_at, Some(expires_at));
    assert!(store.surface_token_by_hash(&token).expect("plaintext lookup").is_none());
    assert_eq!(store.list_surface_tokens(&pending.surface.id).expect("tokens").len(), 1);
    let surface = store
        .surface_by_id(&pending.surface.id)
        .expect("surface lookup")
        .expect("surface row");
    assert_eq!(surface.state, SurfaceState::Attached);
    assert_eq!(
        gateway.finish().expect("gateway"),
        GatewayOutcome::Closed { node: Some(name) }
    );
}

#[test]
fn gateway_surface_pair_refuses_a_bad_or_replayed_code() {
    let key = signing_key();
    let name = node_name("mac-studio");
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    activate(home.path(), &clock, &name, &key);
    let store = test_store(home.path(), &clock);
    let session = seed_session(&store, &name, "vowp", SESSION, clock.now());
    let pending = create_pending_surface(
        &store,
        &name,
        &session,
        SurfaceKind::Opencode,
        Some("opencode"),
        clock.now(),
    )
    .expect("pending surface");

    let gateway = Gateway::start(home.path(), clock.clone());
    authenticate(&gateway, &name, &key);

    // Malformed code.
    gateway.send(&AgentFrame::SurfacePair {
        code: "not-a-code".to_owned(),
        surface_id: pending.surface.id.clone(),
        session: session.clone(),
        app: "opencode".to_owned(),
    });
    let AgentFrame::Error { kind, retryable, .. } = gateway.expect(|frame| matches!(frame, AgentFrame::Error { .. }))
    else {
        panic!("expected Error");
    };
    assert_eq!(kind, ErrorKind::InvalidInput);
    assert!(!retryable);

    // Well-shaped but unknown code.
    let start = gateway.frame_count();
    gateway.send(&AgentFrame::SurfacePair {
        code: "a".repeat(52),
        surface_id: pending.surface.id.clone(),
        session: session.clone(),
        app: "opencode".to_owned(),
    });
    let AgentFrame::Error { kind, retryable, .. } =
        gateway.expect_after(start, |frame| matches!(frame, AgentFrame::Error { .. }))
    else {
        panic!("expected Error");
    };
    assert_eq!(kind, ErrorKind::Refused);
    assert!(!retryable);

    // A refusal keeps the authenticated connection open.
    gateway.send(&AgentFrame::Ping);
    gateway.expect(|frame| matches!(frame, AgentFrame::Pong));

    // First use succeeds.
    gateway.send(&AgentFrame::SurfacePair {
        code: pending.code.clone(),
        surface_id: pending.surface.id.clone(),
        session: session.clone(),
        app: "opencode".to_owned(),
    });
    gateway.expect(|frame| matches!(frame, AgentFrame::SurfaceIssued { .. }));

    // A replay of the same code is refused and does not mint a second token.
    let start = gateway.frame_count();
    gateway.send(&AgentFrame::SurfacePair {
        code: pending.code.clone(),
        surface_id: pending.surface.id.clone(),
        session: session.clone(),
        app: "opencode".to_owned(),
    });
    let AgentFrame::Error {
        kind,
        message,
        retryable,
    } = gateway.expect_after(start, |frame| matches!(frame, AgentFrame::Error { .. }))
    else {
        panic!("expected Error");
    };
    assert_eq!(kind, ErrorKind::Refused);
    assert!(!retryable);
    assert!(!message.contains(&pending.code));
    assert_eq!(store.list_surface_tokens(&pending.surface.id).expect("tokens").len(), 1);

    // An expired code is refused too.
    let expired = create_pending_surface(
        &store,
        &name,
        &session,
        SurfaceKind::Opencode,
        Some("opencode"),
        clock.now(),
    )
    .expect("expired surface");
    clock.advance(Duration::from_secs(601));
    let start = gateway.frame_count();
    gateway.send(&AgentFrame::SurfacePair {
        code: expired.code.clone(),
        surface_id: expired.surface.id.clone(),
        session: session.clone(),
        app: "opencode".to_owned(),
    });
    let AgentFrame::Error { kind, .. } = gateway.expect_after(start, |frame| matches!(frame, AgentFrame::Error { .. }))
    else {
        panic!("expected Error");
    };
    assert_eq!(kind, ErrorKind::Refused);
    assert!(
        store
            .list_surface_tokens(&expired.surface.id)
            .expect("tokens")
            .is_empty()
    );

    let start = gateway.frame_count();
    gateway.send(&AgentFrame::Ping);
    gateway.expect_after(start, |frame| matches!(frame, AgentFrame::Pong));
    gateway.finish().expect("gateway");
}

#[test]
fn gateway_surface_pair_binds_the_connection_node() {
    let key = signing_key();
    let other_key = SigningKey::from_bytes(&[9_u8; 32]);
    let mac_studio = node_name("mac-studio");
    let mac_mini = node_name("mac-mini");
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    activate(home.path(), &clock, &mac_studio, &key);
    activate(home.path(), &clock, &mac_mini, &other_key);
    let store = test_store(home.path(), &clock);
    let session = seed_session(&store, &mac_studio, "vowp", SESSION, clock.now());
    let pending = create_pending_surface(
        &store,
        &mac_mini,
        &session,
        SurfaceKind::Opencode,
        Some("opencode"),
        clock.now(),
    )
    .expect("pending surface");

    let gateway = Gateway::start(home.path(), clock.clone());
    authenticate(&gateway, &mac_studio, &key);
    gateway.send(&AgentFrame::SurfacePair {
        code: pending.code.clone(),
        surface_id: pending.surface.id.clone(),
        session: session.clone(),
        app: "opencode".to_owned(),
    });
    let AgentFrame::Error { kind, .. } = gateway.expect(|frame| matches!(frame, AgentFrame::Error { .. })) else {
        panic!("expected Error");
    };
    assert_eq!(kind, ErrorKind::Refused);
    assert!(
        store
            .list_surface_tokens(&pending.surface.id)
            .expect("tokens")
            .is_empty()
    );
    assert_eq!(
        store
            .surface_by_id(&pending.surface.id)
            .expect("surface lookup")
            .expect("surface row")
            .state,
        SurfaceState::Pending
    );
    gateway.finish().expect("gateway");

    // The refusal rolled the code back: the surface's own node can still redeem it.
    let issued = issue_surface_token(
        &store,
        &mac_mini,
        &pending.surface.id,
        &session,
        "opencode",
        &pending.code,
        clock.now(),
    )
    .expect("code survives a wrong-node refusal");
    assert_eq!(issued.surface_id, pending.surface.id);
}

#[test]
fn gateway_surface_pair_refuses_a_revoked_surface() {
    let key = signing_key();
    let name = node_name("mac-studio");
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    activate(home.path(), &clock, &name, &key);
    let store = test_store(home.path(), &clock);
    let session = seed_session(&store, &name, "vowp", SESSION, clock.now());
    let pending = create_pending_surface(
        &store,
        &name,
        &session,
        SurfaceKind::Opencode,
        Some("opencode"),
        clock.now(),
    )
    .expect("pending surface");
    revoke_surface(&store, &pending.surface.id, clock.now()).expect("revoke surface");

    let gateway = Gateway::start(home.path(), clock.clone());
    authenticate(&gateway, &name, &key);
    gateway.send(&AgentFrame::SurfacePair {
        code: pending.code.clone(),
        surface_id: pending.surface.id.clone(),
        session: session.clone(),
        app: "opencode".to_owned(),
    });
    let AgentFrame::SurfaceRevoked { surface_id, reason } =
        gateway.expect(|frame| matches!(frame, AgentFrame::SurfaceRevoked { .. }))
    else {
        panic!("expected SurfaceRevoked");
    };
    assert_eq!(surface_id, pending.surface.id);
    assert!(reason.contains("revoked"));
    assert!(
        store
            .list_surface_tokens(&pending.surface.id)
            .expect("tokens")
            .is_empty()
    );

    // The surface refusal keeps the connection open.
    let start = gateway.frame_count();
    gateway.send(&AgentFrame::Ping);
    gateway.expect_after(start, |frame| matches!(frame, AgentFrame::Pong));
    gateway.finish().expect("gateway");
}

#[test]
fn surface_token_never_appears_in_logs() {
    let key = signing_key();
    let name = node_name("mac-studio");
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    activate(home.path(), &clock, &name, &key);
    let store = test_store(home.path(), &clock);
    let session = seed_session(&store, &name, "vowp", SESSION, clock.now());
    let pending = create_pending_surface(
        &store,
        &name,
        &session,
        SurfaceKind::Opencode,
        Some("opencode"),
        clock.now(),
    )
    .expect("pending surface");
    let code = pending.code.clone();

    let gateway = Gateway::start(home.path(), clock.clone());
    let log = gateway.log.clone();
    authenticate(&gateway, &name, &key);
    gateway.send(&AgentFrame::SurfacePair {
        code: code.clone(),
        surface_id: pending.surface.id.clone(),
        session: session.clone(),
        app: "opencode".to_owned(),
    });
    let AgentFrame::SurfaceIssued { token, .. } =
        gateway.expect(|frame| matches!(frame, AgentFrame::SurfaceIssued { .. }))
    else {
        panic!("expected SurfaceIssued");
    };

    // A refused replay must not log the code or the live token either.
    gateway.send(&AgentFrame::SurfacePair {
        code: code.clone(),
        surface_id: pending.surface.id.clone(),
        session: session.clone(),
        app: "opencode".to_owned(),
    });
    gateway.expect(|frame| matches!(frame, AgentFrame::Error { .. }));
    gateway.finish().expect("gateway");

    let lines = log.lines();
    assert!(
        lines.iter().any(|(_, _, message)| message.contains("paired")),
        "the pairing itself is logged: {lines:?}"
    );
    for (_, target, message) in &lines {
        assert!(!target.contains(&code), "the code must not appear in a log target");
        assert!(!message.contains(&code), "the code must not appear in a log message");
        assert!(!target.contains(&token), "the token must not appear in a log target");
        assert!(!message.contains(&token), "the token must not appear in a log message");
    }

    assert!(
        store.surface_token_by_hash(&token).expect("plaintext lookup").is_none(),
        "the plaintext token is not a stored hash"
    );
    let stored = store
        .surface_token_by_hash(&hash_code(&token))
        .expect("hash lookup")
        .expect("token row");
    assert_ne!(stored.token_hash, token);
}

#[test]
fn lease_acquire_grants_and_heartbeats() {
    let key = signing_key();
    let name = node_name("mac-studio");
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    activate(home.path(), &clock, &name, &key);
    let store = test_store(home.path(), &clock);
    let session = seed_session(&store, &name, "vowp", SESSION, clock.now());
    let (surface, surface_token) = pair_surface(&store, &name, &session, "opencode", clock.now());

    let gateway = Gateway::start(home.path(), clock.clone());
    authenticate(&gateway, &name, &key);
    gateway.send(&acquire_frame(&surface, &session, &surface_token, LeaseMode::Acquire));
    let (lease_token, expires_at, tail_seq) = expect_grant(&gateway, 0);

    assert_eq!(lease_token.len(), 52, "two ULIDs");
    assert_eq!(expires_at, clock.now().plus(Duration::from_secs(60)));
    assert_eq!(tail_seq, 0);
    let stored = store.lease(&session).expect("lease lookup").expect("held");
    assert_eq!(stored.holder, "opencode");
    assert_eq!(stored.surface_id.as_ref(), Some(&surface));
    assert_eq!(stored.token, hash_lease_token(&lease_token));
    assert_ne!(stored.token, lease_token, "the plaintext token is not stored");
    assert_eq!(stored.token.len(), 64);

    let acquired = stored.heartbeat_at;
    clock.advance(Duration::from_secs(5));
    gateway.send(&AgentFrame::LeaseHeartbeat {
        surface: surface.clone(),
        session: session.clone(),
        token: lease_token.clone(),
    });
    let heartbeat_store = gateway.store();
    wait_until(|| {
        heartbeat_store
            .lease(&session)
            .expect("lease lookup")
            .is_some_and(|lease| lease.heartbeat_at > acquired)
    });
    let start = gateway.frame_count();
    gateway.send(&AgentFrame::Ping);
    gateway.expect_after(start, |frame| matches!(frame, AgentFrame::Pong));
    assert!(
        !gateway
            .output
            .frames()
            .iter()
            .any(|frame| { matches!(frame, AgentFrame::LeaseLost { .. } | AgentFrame::LeaseRevoked { .. }) }),
        "a valid heartbeat produces no loss notice"
    );
    gateway.finish().expect("gateway");
}

#[test]
fn lease_conflict_without_takeover() {
    let key = signing_key();
    let name = node_name("mac-studio");
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    activate(home.path(), &clock, &name, &key);
    let store = test_store(home.path(), &clock);
    let session = seed_session(&store, &name, "vowp", SESSION, clock.now());
    let (first, first_token) = pair_surface(&store, &name, &session, "opencode", clock.now());
    let (second, second_token) = pair_surface(&store, &name, &session, "claude", clock.now());

    let gateway = Gateway::start(home.path(), clock.clone());
    authenticate(&gateway, &name, &key);
    gateway.send(&acquire_frame(&first, &session, &first_token, LeaseMode::Acquire));
    let (first_lease_token, _, _) = expect_grant(&gateway, 0);
    let before = store.lease(&session).expect("lease lookup").expect("held");

    let start = gateway.frame_count();
    gateway.send(&acquire_frame(&second, &session, &second_token, LeaseMode::Acquire));
    let AgentFrame::Error {
        kind,
        message,
        retryable,
    } = gateway.expect_after(start, |frame| matches!(frame, AgentFrame::Error { .. }))
    else {
        panic!("expected Error");
    };
    assert_eq!(kind, ErrorKind::Conflict);
    assert!(!retryable);
    assert!(message.contains("opencode"), "conflict names the holder: {message}");
    assert!(!message.contains(&first_lease_token), "no lease token is echoed");
    assert!(!message.contains(&second_token), "no surface token is echoed");

    let after = store.lease(&session).expect("lease lookup").expect("held");
    assert_eq!(after.surface_id.as_ref(), Some(&first));
    assert_eq!(after.token, before.token, "the refused acquire changed nothing");

    // The refusal keeps the authenticated connection open.
    let start = gateway.frame_count();
    gateway.send(&AgentFrame::Ping);
    gateway.expect_after(start, |frame| matches!(frame, AgentFrame::Pong));
    gateway.finish().expect("gateway");
}

#[test]
fn lease_takeover_pushes_lease_revoked() {
    let key = signing_key();
    let name = node_name("mac-studio");
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    activate(home.path(), &clock, &name, &key);
    let store = test_store(home.path(), &clock);
    let session = seed_session(&store, &name, "vowp", SESSION, clock.now());
    let (old, old_surface_token) = pair_surface(&store, &name, &session, "opencode", clock.now());
    let (new, new_surface_token) = pair_surface(&store, &name, &session, "claude", clock.now());

    let gateway = Gateway::start(home.path(), clock.clone());
    authenticate(&gateway, &name, &key);
    gateway.send(&acquire_frame(&old, &session, &old_surface_token, LeaseMode::Acquire));
    let (old_lease_token, _, _) = expect_grant(&gateway, 0);

    let start = gateway.frame_count();
    gateway.send(&acquire_frame(&new, &session, &new_surface_token, LeaseMode::Takeover));
    let AgentFrame::LeaseRevoked { reason, holder } =
        gateway.expect_after(start, |frame| matches!(frame, AgentFrame::LeaseRevoked { .. }))
    else {
        panic!("expected LeaseRevoked");
    };
    assert!(reason.contains("taken over"), "{reason}");
    assert_eq!(holder, "claude");
    assert!(!reason.contains(&old_lease_token));
    let (new_lease_token, _, _) = expect_grant(&gateway, start);

    let current = store.lease(&session).expect("lease lookup").expect("held");
    assert_eq!(current.surface_id.as_ref(), Some(&new));
    assert_eq!(current.holder, "claude");
    assert_eq!(current.token, hash_lease_token(&new_lease_token));

    // The old surface's next heartbeat is refused and names the new holder.
    clock.advance(Duration::from_secs(5));
    let start = gateway.frame_count();
    gateway.send(&AgentFrame::LeaseHeartbeat {
        surface: old.clone(),
        session: session.clone(),
        token: old_lease_token.clone(),
    });
    let AgentFrame::LeaseRevoked { reason, holder } =
        gateway.expect_after(start, |frame| matches!(frame, AgentFrame::LeaseRevoked { .. }))
    else {
        panic!("expected LeaseRevoked");
    };
    assert_eq!(holder, "claude");
    assert!(reason.contains("read-only"), "{reason}");
    assert_eq!(
        store.lease(&session).expect("lease lookup").expect("held").token,
        hash_lease_token(&new_lease_token),
        "the old heartbeat did not touch the new row"
    );

    // The old surface's release leaves the new holder's row alone.
    let start = gateway.frame_count();
    gateway.send(&AgentFrame::LeaseRelease {
        surface: old.clone(),
        session: session.clone(),
        token: old_lease_token,
    });
    gateway.expect_after(start, |frame| matches!(frame, AgentFrame::LeaseRevoked { .. }));
    assert_eq!(
        store.lease(&session).expect("lease lookup").expect("held").holder,
        "claude"
    );

    // The new holder can release normally.
    let start = gateway.frame_count();
    gateway.send(&AgentFrame::LeaseRelease {
        surface: new,
        session: session.clone(),
        token: new_lease_token,
    });
    let release_store = gateway.store();
    wait_until(|| release_store.lease(&session).expect("lease lookup").is_none());
    gateway.send(&AgentFrame::Ping);
    gateway.expect_after(start, |frame| matches!(frame, AgentFrame::Pong));
    gateway.finish().expect("gateway");
}

#[test]
fn lease_release_removes_the_row() {
    let key = signing_key();
    let name = node_name("mac-studio");
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    activate(home.path(), &clock, &name, &key);
    let store = test_store(home.path(), &clock);
    let session = seed_session(&store, &name, "vowp", SESSION, clock.now());
    let (surface, surface_token) = pair_surface(&store, &name, &session, "opencode", clock.now());

    let gateway = Gateway::start(home.path(), clock.clone());
    authenticate(&gateway, &name, &key);
    gateway.send(&acquire_frame(&surface, &session, &surface_token, LeaseMode::Acquire));
    let (lease_token, _, _) = expect_grant(&gateway, 0);
    assert!(store.lease(&session).expect("lease lookup").is_some());

    gateway.send(&AgentFrame::LeaseRelease {
        surface: surface.clone(),
        session: session.clone(),
        token: lease_token.clone(),
    });
    let release_store = gateway.store();
    wait_until(|| release_store.lease(&session).expect("lease lookup").is_none());
    assert!(store.lease(&session).expect("lease lookup").is_none());

    // A second release is idempotent; a heartbeat after release is LeaseLost.
    gateway.send(&AgentFrame::LeaseRelease {
        surface: surface.clone(),
        session: session.clone(),
        token: lease_token.clone(),
    });
    let start = gateway.frame_count();
    gateway.send(&AgentFrame::LeaseHeartbeat {
        surface,
        session: session.clone(),
        token: lease_token,
    });
    let AgentFrame::LeaseLost { reason } =
        gateway.expect_after(start, |frame| matches!(frame, AgentFrame::LeaseLost { .. }))
    else {
        panic!("expected LeaseLost");
    };
    assert!(reason.contains("read-only"), "{reason}");
    gateway.finish().expect("gateway");
}

#[test]
fn surface_token_must_match_the_paired_surface() {
    let key = signing_key();
    let name = node_name("mac-studio");
    let other_key = SigningKey::from_bytes(&[9_u8; 32]);
    let other_name = node_name("mac-mini");
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    activate(home.path(), &clock, &name, &key);
    activate(home.path(), &clock, &other_name, &other_key);
    let store = test_store(home.path(), &clock);
    let session = seed_session(&store, &name, "vowp", SESSION, clock.now());
    let (first, first_token) = pair_surface(&store, &name, &session, "opencode", clock.now());
    let (second, second_token) = pair_surface(&store, &name, &session, "claude", clock.now());
    let other_session = seed_session(&store, &name, "vowp", "01ARZ3NDEKTSV4RRFFQ69G5FAZ", clock.now());
    let (_, other_session_token) = pair_surface(&store, &name, &other_session, "opencode", clock.now());
    let (foreign, foreign_token) = pair_surface(&store, &other_name, &session, "opencode", clock.now());

    let gateway = Gateway::start(home.path(), clock.clone());
    authenticate(&gateway, &name, &key);

    // A token for surface B cannot acquire as surface A.
    let start = gateway.frame_count();
    gateway.send(&acquire_frame(&first, &session, &second_token, LeaseMode::Acquire));
    let AgentFrame::Error { kind, .. } = gateway.expect_after(start, |frame| matches!(frame, AgentFrame::Error { .. }))
    else {
        panic!("expected Error");
    };
    assert_eq!(kind, ErrorKind::Refused);

    // A token paired for another session cannot acquire this one.
    let start = gateway.frame_count();
    gateway.send(&acquire_frame(
        &first,
        &session,
        &other_session_token,
        LeaseMode::Acquire,
    ));
    let AgentFrame::Error { kind, .. } = gateway.expect_after(start, |frame| matches!(frame, AgentFrame::Error { .. }))
    else {
        panic!("expected Error");
    };
    assert_eq!(kind, ErrorKind::Refused);

    // A surface on another node cannot acquire over this connection.
    let start = gateway.frame_count();
    gateway.send(&acquire_frame(&foreign, &session, &foreign_token, LeaseMode::Acquire));
    let AgentFrame::Error { kind, .. } = gateway.expect_after(start, |frame| matches!(frame, AgentFrame::Error { .. }))
    else {
        panic!("expected Error");
    };
    assert_eq!(kind, ErrorKind::Refused);

    // The real pairing still works and no refused attempt created a lease.
    assert!(store.lease(&session).expect("lease lookup").is_none());
    gateway.send(&acquire_frame(&first, &session, &first_token, LeaseMode::Acquire));
    let (lease_token, _, _) = expect_grant(&gateway, start);
    assert_eq!(lease_token.len(), 52);
    assert_eq!(
        store
            .lease(&session)
            .expect("lease lookup")
            .expect("held")
            .surface_id
            .as_ref(),
        Some(&first)
    );
    let _ = second;
    gateway.finish().expect("gateway");
}

#[test]
fn lease_token_is_stored_only_as_a_hash() {
    let key = signing_key();
    let name = node_name("mac-studio");
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    activate(home.path(), &clock, &name, &key);
    let store = test_store(home.path(), &clock);
    let session = seed_session(&store, &name, "vowp", SESSION, clock.now());
    let (surface, surface_token) = pair_surface(&store, &name, &session, "opencode", clock.now());

    let gateway = Gateway::start(home.path(), clock.clone());
    let log = gateway.log.clone();
    authenticate(&gateway, &name, &key);
    gateway.send(&acquire_frame(&surface, &session, &surface_token, LeaseMode::Acquire));
    let (lease_token, _, _) = expect_grant(&gateway, 0);
    let start = gateway.frame_count();
    gateway.send(&AgentFrame::LeaseHeartbeat {
        surface,
        session: session.clone(),
        token: lease_token.clone(),
    });
    gateway.send(&AgentFrame::Ping);
    gateway.expect_after(start, |frame| matches!(frame, AgentFrame::Pong));
    gateway.finish().expect("gateway");

    let stored = store.lease(&session).expect("lease lookup").expect("held");
    assert_eq!(stored.token.len(), 64, "SHA-256 hex is 64 characters");
    assert!(
        stored
            .token
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
        "the stored value is lowercase hex: {}",
        stored.token
    );
    assert_eq!(stored.token, hash_lease_token(&lease_token));
    assert_ne!(stored.token, lease_token, "the plaintext token is not stored");
    for (_, target, message) in log.lines() {
        assert!(!target.contains(&lease_token), "the lease token reached a log target");
        assert!(!message.contains(&lease_token), "the lease token reached a log message");
    }
}

#[test]
fn lease_stale_after_sixty_seconds() {
    let key = signing_key();
    let name = node_name("mac-studio");
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    activate(home.path(), &clock, &name, &key);
    let store = test_store(home.path(), &clock);
    let session = seed_session(&store, &name, "vowp", SESSION, clock.now());
    let (old, old_token) = pair_surface(&store, &name, &session, "opencode", clock.now());
    let (new, new_token) = pair_surface(&store, &name, &session, "claude", clock.now());

    let gateway = Gateway::start(home.path(), clock.clone());
    authenticate(&gateway, &name, &key);
    gateway.send(&acquire_frame(&old, &session, &old_token, LeaseMode::Acquire));
    let (old_lease_token, _, _) = expect_grant(&gateway, 0);

    // 59.999 seconds old is live.
    clock.advance(Duration::from_secs(60) - Duration::from_millis(1));
    let start = gateway.frame_count();
    gateway.send(&acquire_frame(&new, &session, &new_token, LeaseMode::Acquire));
    let AgentFrame::Error { kind, .. } = gateway.expect_after(start, |frame| matches!(frame, AgentFrame::Error { .. }))
    else {
        panic!("expected Error");
    };
    assert_eq!(kind, ErrorKind::Conflict);

    // Exactly 60 seconds is stale and may be replaced without takeover.
    clock.advance(Duration::from_millis(1));
    let start = gateway.frame_count();
    gateway.send(&acquire_frame(&new, &session, &new_token, LeaseMode::Acquire));
    let (new_lease_token, expires_at, _) = expect_grant(&gateway, start);
    assert_eq!(expires_at, clock.now().plus(Duration::from_secs(60)));
    assert_ne!(new_lease_token, old_lease_token);
    let current = store.lease(&session).expect("lease lookup").expect("held");
    assert_eq!(current.surface_id.as_ref(), Some(&new));
    assert_eq!(current.token, hash_lease_token(&new_lease_token));

    // The old token is gone; its heartbeat is refused.
    let start = gateway.frame_count();
    gateway.send(&AgentFrame::LeaseHeartbeat {
        surface: old,
        session: session.clone(),
        token: old_lease_token,
    });
    gateway.expect_after(start, |frame| matches!(frame, AgentFrame::LeaseRevoked { .. }));
    gateway.finish().expect("gateway");
}

/// Pairs a surface, acquires its lease and begins one local chat turn.
fn begin_local_chat(
    gateway: &Gateway,
    store: &Store,
    name: &NodeName,
    session: &SessionId,
    cwd_ref: PathRef,
) -> (SurfaceId, String, RunId, String) {
    let (surface, surface_token) = pair_surface(store, name, session, "chat", ts());
    gateway.send(&acquire_frame(&surface, session, &surface_token, LeaseMode::Acquire));
    let (lease_token, _, _) = expect_grant(gateway, 0);
    let start = gateway.frame_count();
    gateway.send(&turn_begin_frame(&surface, session, &lease_token, cwd_ref));
    let (run, granted_session, granted_lease, _) = expect_turn_grant(gateway, start);
    assert_eq!(granted_session, *session);
    assert_eq!(granted_lease, lease_token);
    (surface, surface_token, run, lease_token)
}

#[test]
fn local_chat_turn_appends_to_the_home_log() {
    let key = signing_key();
    let name = node_name("mac-studio");
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    activate(home.path(), &clock, &name, &key);
    let store = test_store(home.path(), &clock);
    let session = chat_session(home.path(), &store, &name, SESSION, &clock);

    let gateway = Gateway::start(home.path(), clock.clone());
    authenticate(&gateway, &name, &key);
    let (_, _, run, lease) = begin_local_chat(
        &gateway,
        &store,
        &name,
        &session,
        PathRef::parse("project:vowp").expect("path ref"),
    );

    gateway.send(&AgentFrame::RemoteLine {
        run_id: run.clone(),
        epoch: 1,
        line: hello_line(&session, &run, &lease),
    });
    let event = assistant_event(&EventId::new(), "the turn landed");
    let event_id = event.id.clone();
    let start = gateway.frame_count();
    gateway.send(&event_line(&run, event));
    let (ack_run, ack_id, ack_seq, duplicate, _, error) = expect_ack(&gateway, start);
    assert_eq!(ack_run, run);
    assert_eq!(ack_id, Some(event_id.clone()));
    assert!(!duplicate);
    assert!(error.is_none());
    assert!(ack_seq.is_some());

    let start = gateway.frame_count();
    gateway.send(&turn_end_frame(&run));
    let (_, _, _, duplicate, _, error) = expect_ack(&gateway, start);
    assert!(!duplicate);
    assert!(error.is_none());

    let events = read_log(home.path(), &session);
    for pair in events.windows(2) {
        assert_eq!(pair[1].seq, pair[0].seq + 1, "the home log is gapless");
    }
    let assistants: Vec<_> = events
        .iter()
        .filter_map(|event| match event.body() {
            Ok(EventBody::AssistantMessage(message)) => Some(message.text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(assistants, vec!["the turn landed".to_owned()]);
    let run_row = gateway.store().require_run(&run).expect("run row");
    assert_eq!(run_row.outcome, Some(RunOutcome::Ok));
    assert_eq!(run_row.session_id.as_ref(), Some(&session));
    gateway.finish().expect("gateway");
    for event in &events {
        let text = serde_json::to_string(event).expect("event JSON");
        assert!(!text.contains("/Users/"), "no Mac absolute path: {text}");
    }
}

#[test]
fn local_chat_turn_begin_requires_a_live_lease() {
    let key = signing_key();
    let name = node_name("mac-studio");
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    activate(home.path(), &clock, &name, &key);
    let store = test_store(home.path(), &clock);
    let session = chat_session(home.path(), &store, &name, SESSION, &clock);

    let gateway = Gateway::start(home.path(), clock.clone());
    authenticate(&gateway, &name, &key);
    let (surface, _) = pair_surface(&store, &name, &session, "chat", ts());
    let start = gateway.frame_count();
    gateway.send(&turn_begin_frame(
        &surface,
        &session,
        &"A".repeat(52),
        PathRef::parse("project:vowp").expect("path ref"),
    ));
    let AgentFrame::Error { kind, message, .. } =
        gateway.expect_after(start, |frame| matches!(frame, AgentFrame::Error { .. }))
    else {
        panic!("expected an Error frame");
    };
    assert_eq!(kind, ErrorKind::Refused);
    assert!(message.contains("no live lease"), "{message}");
    assert!(
        !gateway
            .output
            .frames()
            .iter()
            .any(|frame| matches!(frame, AgentFrame::TurnGrant { .. })),
        "no turn is granted without a lease"
    );
    gateway.finish().expect("gateway");
}

#[test]
fn local_chat_replays_after_an_ssh_drop_without_gaps() {
    let key = signing_key();
    let name = node_name("mac-studio");
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    activate(home.path(), &clock, &name, &key);
    let store = test_store(home.path(), &clock);
    let session = chat_session(home.path(), &store, &name, SESSION, &clock);

    let gateway = Gateway::start(home.path(), clock.clone());
    authenticate(&gateway, &name, &key);
    let (surface, _, run, lease) = begin_local_chat(
        &gateway,
        &store,
        &name,
        &session,
        PathRef::parse("project:vowp").expect("path ref"),
    );
    gateway.send(&AgentFrame::RemoteLine {
        run_id: run.clone(),
        epoch: 1,
        line: hello_line(&session, &run, &lease),
    });

    let first = EventId::new();
    let second = EventId::new();
    let third = EventId::new();
    let first_event = assistant_event(&first, "one");
    let second_event = assistant_event(&second, "two");

    let start = gateway.frame_count();
    gateway.send(&event_line(&run, first_event.clone()));
    let (_, _, first_seq, duplicate, _, _) = expect_ack(&gateway, start);
    assert!(!duplicate);
    let start = gateway.frame_count();
    gateway.send(&event_line(&run, second_event.clone()));
    let (_, _, second_seq, duplicate, _, _) = expect_ack(&gateway, start);
    assert!(!duplicate);
    assert!(first_seq < second_seq);

    // The SSH transport dropped and the agent reconnected: a second gateway process
    // starts with an empty in-memory map. The heartbeat proves the lease, the fresh
    // hello resumes the run, and both unacknowledged lines replay as duplicates.
    gateway.finish().expect("first gateway");
    let gateway = Gateway::start(home.path(), clock.clone());
    authenticate(&gateway, &name, &key);
    gateway.send(&AgentFrame::LeaseHeartbeat {
        surface,
        session: session.clone(),
        token: lease.clone(),
    });
    gateway.send(&AgentFrame::RemoteLine {
        run_id: run.clone(),
        epoch: 1,
        line: hello_line(&session, &run, &lease),
    });

    let start = gateway.frame_count();
    gateway.send(&event_line(&run, first_event));
    let (_, first_ack, _, duplicate, _, _) = expect_ack(&gateway, start);
    assert!(duplicate, "the first replayed event is a duplicate");
    assert_eq!(first_ack, Some(first.clone()));
    let start = gateway.frame_count();
    gateway.send(&event_line(&run, second_event));
    let (_, second_ack, _, duplicate, _, _) = expect_ack(&gateway, start);
    assert!(duplicate, "the second replayed event is a duplicate");
    assert_eq!(second_ack, Some(second.clone()));

    let start = gateway.frame_count();
    gateway.send(&event_line(&run, assistant_event(&third, "three")));
    let (_, _, third_seq, duplicate, _, _) = expect_ack(&gateway, start);
    assert!(!duplicate);
    assert!(second_seq < third_seq);

    let start = gateway.frame_count();
    gateway.send(&turn_end_frame(&run));
    expect_ack(&gateway, start);
    gateway.finish().expect("gateway");

    let events = read_log(home.path(), &session);
    for pair in events.windows(2) {
        assert_eq!(pair[1].seq, pair[0].seq + 1, "no gap after replay");
    }
    let ids: Vec<EventId> = events
        .iter()
        .map(|event| event.id.clone())
        .filter(|id| id == &first || id == &second || id == &third)
        .collect();
    assert_eq!(ids, vec![first, second, third], "each replayed id appears once");
}

#[test]
fn local_chat_enforces_the_64_event_window_and_replenishes_it() {
    let key = signing_key();
    let name = node_name("mac-studio");
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    activate(home.path(), &clock, &name, &key);
    let store = test_store(home.path(), &clock);
    let session = chat_session(home.path(), &store, &name, SESSION, &clock);

    let gateway = Gateway::start(home.path(), clock.clone());
    authenticate(&gateway, &name, &key);
    let (surface, _, run, lease) = begin_local_chat(
        &gateway,
        &store,
        &name,
        &session,
        PathRef::parse("project:vowp").expect("path ref"),
    );
    gateway.send(&AgentFrame::RemoteLine {
        run_id: run.clone(),
        epoch: 1,
        line: hello_line(&session, &run, &lease),
    });

    let window_start = gateway.frame_count();
    let mut accepted = Vec::new();
    for index in 0..64 {
        let id = EventId::new();
        let start = gateway.frame_count();
        gateway.send(&event_line(&run, assistant_event(&id, &format!("event {index}"))));
        let (_, ack_id, _, duplicate, _, _) = expect_ack(&gateway, start);
        assert_eq!(ack_id, Some(id.clone()));
        assert!(!duplicate);
        accepted.push(id);
    }
    // The 64th event's Credit arrives before its acknowledgement.
    gateway.expect_after(window_start, |frame| {
        matches!(frame, AgentFrame::Credit { count: 64, .. })
    });

    let refused = EventId::new();
    let start = gateway.frame_count();
    gateway.send(&event_line(&run, assistant_event(&refused, "past the window")));
    let AgentFrame::Error {
        kind,
        message,
        retryable,
    } = gateway.expect_after(start, |frame| matches!(frame, AgentFrame::Error { .. }))
    else {
        panic!("expected a window error");
    };
    assert_eq!(kind, ErrorKind::Conflict);
    assert!(retryable);
    assert!(message.contains("64-event in-flight window"), "{message}");
    assert!(
        !gateway.output.frames().iter().any(|frame| matches!(
            frame,
            AgentFrame::IngestAck { event_id: Some(id), .. } if id == &refused
        )),
        "the refused event was not appended"
    );

    // The lease heartbeat is the agent's confirmation that it consumed the Credit.
    gateway.send(&AgentFrame::LeaseHeartbeat {
        surface,
        session: session.clone(),
        token: lease.clone(),
    });
    let start = gateway.frame_count();
    gateway.send(&event_line(&run, assistant_event(&refused, "past the window")));
    let (_, ack_id, _, duplicate, _, _) = expect_ack(&gateway, start);
    assert_eq!(ack_id, Some(refused.clone()));
    assert!(!duplicate);
    accepted.push(refused);

    let start = gateway.frame_count();
    gateway.send(&turn_end_frame(&run));
    expect_ack(&gateway, start);
    gateway.finish().expect("gateway");

    let events = read_log(home.path(), &session);
    let appended: Vec<EventId> = events
        .iter()
        .map(|event| event.id.clone())
        .filter(|id| accepted.contains(id))
        .collect();
    assert_eq!(appended, accepted, "each windowed event appears exactly once");
}

#[test]
fn local_chat_rejects_a_duplicate_event_id_with_different_content() {
    let key = signing_key();
    let name = node_name("mac-studio");
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    activate(home.path(), &clock, &name, &key);
    let store = test_store(home.path(), &clock);
    let session = chat_session(home.path(), &store, &name, SESSION, &clock);

    let gateway = Gateway::start(home.path(), clock.clone());
    authenticate(&gateway, &name, &key);
    let (_, _, run, lease) = begin_local_chat(
        &gateway,
        &store,
        &name,
        &session,
        PathRef::parse("project:vowp").expect("path ref"),
    );
    gateway.send(&AgentFrame::RemoteLine {
        run_id: run.clone(),
        epoch: 1,
        line: hello_line(&session, &run, &lease),
    });

    let event_id = EventId::new();
    let start = gateway.frame_count();
    gateway.send(&event_line(&run, assistant_event(&event_id, "original")));
    expect_ack(&gateway, start);

    let start = gateway.frame_count();
    gateway.send(&event_line(&run, assistant_event(&event_id, "different")));
    let (_, ack_id, _, _, revoked, error) = expect_ack(&gateway, start);
    assert!(ack_id.is_none());
    assert!(revoked.is_none());
    assert!(
        error
            .as_deref()
            .is_some_and(|message| message.contains("duplicate-conflict")),
        "the ack names the conflict: {error:?}"
    );
    gateway.expect_after(start, |frame| matches!(frame, AgentFrame::RunLost { .. }));

    // The run is fenced: a later line is never appended.
    let start = gateway.frame_count();
    gateway.send(&event_line(&run, assistant_event(&EventId::new(), "late")));
    gateway.expect_after(start, |frame| matches!(frame, AgentFrame::RunLost { .. }));
    gateway.finish().expect("gateway");

    let events = read_log(home.path(), &session);
    let texts: Vec<_> = events
        .iter()
        .filter_map(|event| match event.body() {
            Ok(EventBody::AssistantMessage(message)) => Some(message.text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, vec!["original".to_owned()], "no conflicting or late content");
}

#[test]
fn local_chat_takeover_fences_the_old_run() {
    let key = signing_key();
    let name = node_name("mac-studio");
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    activate(home.path(), &clock, &name, &key);
    let store = test_store(home.path(), &clock);
    let session = chat_session(home.path(), &store, &name, SESSION, &clock);

    let gateway = Gateway::start(home.path(), clock.clone());
    authenticate(&gateway, &name, &key);
    let (_, _, run, lease) = begin_local_chat(
        &gateway,
        &store,
        &name,
        &session,
        PathRef::parse("project:vowp").expect("path ref"),
    );
    gateway.send(&AgentFrame::RemoteLine {
        run_id: run.clone(),
        epoch: 1,
        line: hello_line(&session, &run, &lease),
    });
    let start = gateway.frame_count();
    gateway.send(&event_line(&run, assistant_event(&EventId::new(), "before takeover")));
    expect_ack(&gateway, start);

    let (phone, phone_token) = pair_surface(&store, &name, &session, "chat", ts());
    let start = gateway.frame_count();
    gateway.send(&acquire_frame(&phone, &session, &phone_token, LeaseMode::Takeover));
    gateway.expect_after(start, |frame| matches!(frame, AgentFrame::LeaseRevoked { .. }));
    gateway.expect_after(start, |frame| matches!(frame, AgentFrame::RunLost { .. }));
    expect_grant(&gateway, start);
    // The new surface holds the lease.
    let current = store.lease(&session).expect("lease lookup").expect("held");
    assert_eq!(current.surface_id.as_ref(), Some(&phone));

    let late = EventId::new();
    let start = gateway.frame_count();
    gateway.send(&event_line(&run, assistant_event(&late, "after takeover")));
    gateway.expect_after(start, |frame| matches!(frame, AgentFrame::RunLost { .. }));
    gateway.finish().expect("gateway");

    let events = read_log(home.path(), &session);
    assert!(
        !events.iter().any(|event| event.id == late),
        "a fenced run never appends a later line"
    );
    let texts: Vec<_> = events
        .iter()
        .filter_map(|event| match event.body() {
            Ok(EventBody::AssistantMessage(message)) => Some(message.text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, vec!["before takeover".to_owned()]);
}

#[test]
fn local_chat_uses_a_path_ref_not_an_absolute_path() {
    let key = signing_key();
    let name = node_name("mac-studio");
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    activate(home.path(), &clock, &name, &key);
    let store = test_store(home.path(), &clock);
    let session = chat_session(home.path(), &store, &name, SESSION, &clock);

    let gateway = Gateway::start(home.path(), clock.clone());
    authenticate(&gateway, &name, &key);
    let (surface, surface_token) = pair_surface(&store, &name, &session, "chat", ts());
    gateway.send(&acquire_frame(&surface, &session, &surface_token, LeaseMode::Acquire));
    let (lease, _, _) = expect_grant(&gateway, 0);

    let frame = turn_begin_frame(
        &surface,
        &session,
        &lease,
        PathRef::parse("worktree:vowp/local-chat").expect("path ref"),
    );
    let wire = serde_json::to_string(&frame).expect("TurnBegin JSON");
    assert!(wire.contains("\"cwd_ref\":\"worktree:vowp/local-chat\""), "{wire}");
    assert!(!wire.contains("/Users/"), "no absolute path in the frame: {wire}");

    // A path ref for another project is refused rather than silently used.
    let other = AgentFrame::TurnBegin {
        surface: surface.clone(),
        session: session.clone(),
        lease_token: lease.clone(),
        prompt: "wrong project".to_owned(),
        executor: "opencode:deepseek/deepseek-flash#max".to_owned(),
        cwd_ref: PathRef::parse("project:another").expect("path ref"),
        job: None,
        purpose: RunPurpose::Continue,
    };
    let start = gateway.frame_count();
    gateway.send(&other);
    let AgentFrame::Error { kind, .. } = gateway.expect_after(start, |frame| matches!(frame, AgentFrame::Error { .. }))
    else {
        panic!("expected Error");
    };
    assert_eq!(kind, ErrorKind::Refused);

    let start = gateway.frame_count();
    gateway.send(&frame);
    let (run, _, _, _) = expect_turn_grant(&gateway, start);

    // The home records the run without the Mac's working directory.
    let row = gateway.store().require_run(&run).expect("run row");
    if let Some(cwd) = &row.cwd {
        assert!(!cwd.contains("/Users/"), "the run row has no Mac path: {cwd}");
    }
    gateway.finish().expect("gateway");
}

/// Inserts one ready offload job under `project`.
fn insert_ready_offload_job(store: &Store, project: &ProjectName, id: &str) -> JobId {
    let new = NewJob {
        id: JobId::parse(id).expect("job id"),
        title: format!("job {id}"),
        kind: JobKind::Build,
        role: Role::Worker,
        profile: Some("worker".to_owned()),
        needs: Vec::new(),
        owns: vec!["src/**".to_owned()],
        reads: Vec::new(),
        gates: None,
        priority: None,
        loop_: None,
        brief_text: None,
    };
    let job = store.insert_job(project, &new, "test").expect("insert job");
    store
        .set_job_state(
            &job.id,
            JobState::Ready,
            &StateChange {
                note: None,
                parked_reason: None,
            },
        )
        .expect("ready job");
    job.id
}

/// One heartbeat with the given slot occupancy.
fn offload_heartbeat(busy_slots: u32, max_slots: u32) -> AgentFrame {
    AgentFrame::Heartbeat {
        at: ts(),
        capacity: AgentCapacity {
            cpu_cores: 8,
            load1: 0.1,
            mem_total_bytes: 16 * 1024 * 1024 * 1024,
            mem_free_bytes: 8 * 1024 * 1024 * 1024,
            disk_free_bytes: 100 * 1024 * 1024 * 1024,
            busy_slots,
            max_slots,
            on_battery: Some(false),
            thermal_state: Some("nominal".to_owned()),
        },
        pending_spool_bytes: 0,
    }
}

#[test]
fn heartbeat_dispatches_at_most_the_node_slots() {
    let key = signing_key();
    let name = node_name("mac-studio");
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    let project = ProjectName::parse("vowp").expect("project");
    let setup = test_store(home.path(), &clock);
    setup
        .upsert_project(&Project {
            name: project.clone(),
            path: home.path().join("project").display().to_string(),
            remote_url: Some("https://example.invalid/acme/vowp.git".to_owned()),
            default_branch: "main".to_owned(),
            config: serde_json::json!({}),
            created_at: ts(),
            updated_at: ts(),
        })
        .expect("project row");
    activate(home.path(), &clock, &name, &key);
    setup
        .upsert_node_path(&NodePath {
            node: name.clone(),
            project: project.clone(),
            local_root: "/srv/vowp".to_owned(),
            worktree_root: "/srv/vowp-wt".to_owned(),
            allow: Vec::new(),
            updated_at: ts(),
        })
        .expect("path map");
    for id in ["offload-a", "offload-b", "offload-c"] {
        insert_ready_offload_job(&setup, &project, id);
    }

    let mut config = VowpConfig::with_defaults_for_tests();
    config.project.name = project.clone();
    config.project.path = Some(home.path().join("project").display().to_string());
    config.offload.enabled = true;
    config.offload.default = Placement::Any;
    // Keep the fleet cap above the node's two slots so this test measures the node limit.
    config.offload.max_remote_runs = 3;
    let gateway = Gateway::start_with_config(home.path(), clock.clone(), config);
    authenticate(&gateway, &name, &key);
    gateway.send(&offload_heartbeat(0, 2));

    let count_assign = |gateway: &Gateway| {
        gateway
            .output
            .frames()
            .into_iter()
            .filter(|frame| matches!(frame, AgentFrame::AssignRun { .. }))
            .count()
    };
    wait_until(|| count_assign(&gateway) == 2);
    thread::sleep(Duration::from_millis(150));
    assert_eq!(count_assign(&gateway), 2, "one heartbeat sends only the free slots");

    // The two sent rows occupy the slots even though the agent reports idle capacity.
    gateway.send(&offload_heartbeat(0, 2));
    thread::sleep(Duration::from_millis(150));
    assert_eq!(count_assign(&gateway), 2, "a second heartbeat must not over-dispatch");

    let store = gateway.store();
    let assignments = store.assignments_for_node(&name).expect("assignments");
    assert_eq!(assignments.len(), 3, "all three jobs are durably queued");
    assert_eq!(
        assignments
            .iter()
            .filter(|assignment| assignment.state == vowp_store::repo_offload::AgentAssignmentState::Sent)
            .count(),
        2
    );
    gateway.finish().expect("gateway");
}

#[test]
fn agent_second_connection_supersedes_the_first() {
    let key = signing_key();
    let name = node_name("mac-studio");
    let home = TempDir::new().expect("temp home");
    let clock = Arc::new(FakeClock::at(ts()));
    activate(home.path(), &clock, &name, &key);

    let first = Gateway::start(home.path(), clock.clone());
    authenticate(&first, &name, &key);
    let second = Gateway::start(home.path(), clock.clone());
    authenticate(&second, &name, &key);

    let AgentFrame::Superseded { connection_id } = first.expect(|frame| matches!(frame, AgentFrame::Superseded { .. }))
    else {
        panic!("expected Superseded");
    };
    assert!(!connection_id.is_empty());
    let outcome = first.finish().expect("first gateway closes cleanly");
    assert!(matches!(outcome, GatewayOutcome::Closed { node: Some(node) } if node == name));
    second.finish().expect("second gateway");
}
