//! Phase 0 home gateway tests.
//!
//! Every test uses a temporary node home and an in-memory duplex; no test opens the real
//! `~/.vowp`, starts tmux, touches the network or signs with a real node key.

use std::collections::VecDeque;
use std::io::{BufReader, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use ed25519_dalek::{Signer, SigningKey};
use tempfile::TempDir;
use vowp_core::agent::gateway::{self, GatewayOptions, GatewayOutcome};
use vowp_core::agent::{
    EnrollRequest, auth, create_pending, create_pending_surface, hash_code, issue_surface_token, redeem, revoke,
    revoke_surface,
};
use vowp_core::{AdapterRegistry, CoreError, Deps, FixedRng, SystemGit, SystemRunner};
use vowp_protocol::{
    AgentCapabilities, AgentFrame, AgentPathMap, Clock, ErrorKind, FakeClock, LeaseMode, MemoryLog, NodeName,
    NodePresence, NodeStatus, ProjectName, Session, SessionId, SessionRole, SessionState, SurfaceId, SurfaceKind,
    SurfaceScope, SurfaceState, Timestamp,
};
use vowp_store::{NodePaths, Store, hash_lease_token};

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
    AgentCapabilities {
        os: "linux".to_owned(),
        arch: "x86_64".to_owned(),
        agent_version: "0.3.0".to_owned(),
        harnesses: Default::default(),
        git_version: Some("git version 2.47.0".to_owned()),
        labels: vec!["test".to_owned()],
        secret_names: Vec::new(),
        projects: Vec::new(),
    }
}

fn test_deps(root: &Path, clock: Arc<FakeClock>, log: Arc<MemoryLog>) -> Deps {
    Deps {
        paths: NodePaths::new(root),
        config: Arc::new(vowp_protocol::VowpConfig::with_defaults_for_tests()),
        node: NodeName::parse("home-1").expect("home node"),
        clock,
        log,
        adapters: Arc::new(AdapterRegistry::new()),
        windows: Arc::new(vowp_tmux::fake::FakeWindows::new()),
        git: Arc::new(SystemGit::default()),
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
        let store = test_store(root, &clock);
        let log = Arc::new(MemoryLog::new());
        let deps = test_deps(root, clock.clone(), log.clone());
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
        test_store(&self.root, &self.clock)
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
