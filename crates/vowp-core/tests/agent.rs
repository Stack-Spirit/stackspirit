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
use vowp_core::agent::{EnrollRequest, auth, create_pending, hash_code, redeem, revoke};
use vowp_core::{AdapterRegistry, CoreError, Deps, FixedRng, SystemGit, SystemRunner};
use vowp_protocol::{
    AgentCapabilities, AgentFrame, AgentPathMap, Clock, ErrorKind, FakeClock, MemoryLog, NodeName, NodePresence,
    NodeStatus, ProjectName, Timestamp,
};
use vowp_store::{NodePaths, Store};

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

fn test_deps(root: &Path, clock: Arc<FakeClock>) -> Deps {
    Deps {
        paths: NodePaths::new(root),
        config: Arc::new(vowp_protocol::VowpConfig::with_defaults_for_tests()),
        node: NodeName::parse("home-1").expect("home node"),
        clock,
        log: Arc::new(MemoryLog::new()),
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
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(frame) = self.frames().into_iter().find(|frame| predicate(frame)) {
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
    clock: Arc<FakeClock>,
    root: std::path::PathBuf,
    handle: Option<JoinHandle<Result<GatewayOutcome, CoreError>>>,
}

impl Gateway {
    fn start(root: &Path, clock: Arc<FakeClock>) -> Self {
        let store = test_store(root, &clock);
        let deps = test_deps(root, clock.clone());
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
