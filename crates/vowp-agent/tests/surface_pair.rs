//! Surface pairing tests for the Mac-side agent client.
//!
//! The fake home speaks the public [`Transport`]/[`FrameStream`] boundary in memory: no SSH,
//! network, store or real `~/.vowp-agent` is touched.

use std::collections::{HashSet, VecDeque};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use vowp_agent::{
    AgentClient, AgentClock, AgentConfig, AgentError, FrameRead, FrameStream, NodeKey, SurfacePairRequest, Transport,
};
use vowp_protocol::{AGENT_PROTOCOL, AgentFrame, ErrorKind, NodeName, SessionId, SurfaceId, SurfaceScope, Timestamp};

const CODE: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const SURFACE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
const SESSION: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

/// Builds a fake token at runtime so no secret-shaped literal is committed.
fn token() -> String {
    format!("vowp_surf_{}", "A".repeat(43))
}

fn ts() -> Timestamp {
    Timestamp::parse("2026-11-14T10:00:00Z").expect("timestamp")
}

#[derive(Debug, Clone, Copy)]
struct FixedClock(Timestamp);

impl AgentClock for FixedClock {
    fn now(&self) -> Timestamp {
        self.0
    }

    fn sleep(&self, _duration: Duration) {}
}

#[derive(Default)]
struct FakeState {
    connections: usize,
    used_codes: HashSet<String>,
    received: Vec<AgentFrame>,
}

#[derive(Clone)]
struct FakeHome {
    shared: Arc<Mutex<FakeState>>,
    clock: FixedClock,
    surface_id: SurfaceId,
    token: String,
    scopes: Vec<SurfaceScope>,
    expires_at: Timestamp,
}

impl FakeHome {
    fn new() -> Self {
        Self {
            shared: Arc::new(Mutex::new(FakeState::default())),
            clock: FixedClock(ts()),
            surface_id: SurfaceId(SURFACE.to_owned()),
            token: token(),
            scopes: vec![
                SurfaceScope::new(format!("session.read:{SESSION}")),
                SurfaceScope::new(format!("session.write:{SESSION}")),
                SurfaceScope::new("fleet.read"),
            ],
            expires_at: ts().plus(Duration::from_secs(30 * 24 * 60 * 60)),
        }
    }

    fn frames(&self) -> Vec<AgentFrame> {
        self.shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .received
            .clone()
    }

    fn surface_pairs(&self) -> Vec<(String, SurfaceId, SessionId, String)> {
        self.frames()
            .into_iter()
            .filter_map(|frame| match frame {
                AgentFrame::SurfacePair {
                    code,
                    surface_id,
                    session,
                    app,
                } => Some((code, surface_id, session, app)),
                _ => None,
            })
            .collect()
    }
}

impl Transport for FakeHome {
    fn open(&mut self) -> Result<Box<dyn FrameStream>, AgentError> {
        let connection = {
            let mut state = self.shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            state.connections += 1;
            state.connections
        };
        Ok(Box::new(FakeStream {
            shared: Arc::clone(&self.shared),
            clock: self.clock,
            surface_id: self.surface_id.clone(),
            token: self.token.clone(),
            scopes: self.scopes.clone(),
            expires_at: self.expires_at,
            connection,
            outbound: VecDeque::new(),
        }))
    }
}

struct FakeStream {
    shared: Arc<Mutex<FakeState>>,
    clock: FixedClock,
    surface_id: SurfaceId,
    token: String,
    scopes: Vec<SurfaceScope>,
    expires_at: Timestamp,
    connection: usize,
    outbound: VecDeque<AgentFrame>,
}

impl FrameStream for FakeStream {
    fn recv(&mut self, _timeout: Option<Duration>) -> Result<FrameRead, AgentError> {
        match self.outbound.pop_front() {
            Some(frame) => Ok(FrameRead::Frame(frame)),
            None => Ok(FrameRead::Closed),
        }
    }

    fn send(&mut self, frame: &AgentFrame) -> Result<(), AgentError> {
        self.shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .received
            .push(frame.clone());
        match frame {
            AgentFrame::AgentHello { .. } => {
                self.outbound.push_back(AgentFrame::HomeChallenge {
                    nonce: format!("nonce-{}", self.connection),
                    home_id: "home-1".to_owned(),
                    connection_id: format!("connection-{}", self.connection),
                    server_time: self.clock.now(),
                    heartbeat_secs: 10,
                    max_frame_bytes: 1024 * 1024,
                });
            }
            AgentFrame::AgentAuth { .. } => {
                self.outbound.push_back(AgentFrame::HomeWelcome {
                    protocol: AGENT_PROTOCOL.to_owned(),
                    home_id: "home-1".to_owned(),
                    node: NodeName::parse("mac-studio").expect("node slug"),
                    connection_id: format!("connection-{}", self.connection),
                });
            }
            AgentFrame::SurfacePair { code, .. } => {
                let fresh = self
                    .shared
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .used_codes
                    .insert(code.clone());
                if fresh {
                    self.outbound.push_back(AgentFrame::SurfaceIssued {
                        surface_id: self.surface_id.clone(),
                        token: self.token.clone(),
                        scopes: self.scopes.clone(),
                        expires_at: self.expires_at,
                    });
                } else {
                    self.outbound.push_back(AgentFrame::Error {
                        kind: ErrorKind::Refused,
                        message: "pairing code is invalid, expired or already used".to_owned(),
                        retryable: false,
                    });
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn client(dir: &Path, home: FakeHome) -> AgentClient {
    let config = AgentConfig::new(
        "home-1",
        NodeName::parse("mac-studio").expect("node slug"),
        dir.to_path_buf(),
        "home.example",
    );
    config.write().expect("agent.json");
    let key = NodeKey::load_or_generate(dir).expect("node key");
    AgentClient::new(
        dir.to_path_buf(),
        key,
        config,
        Box::new(home),
        Arc::new(FixedClock(ts())),
    )
}

fn request() -> SurfacePairRequest {
    SurfacePairRequest::new(
        CODE,
        SurfaceId(SURFACE.to_owned()),
        SessionId::parse(SESSION).expect("session id"),
        "opencode",
    )
}

#[test]
fn agent_pair_writes_0600_token_and_prints_no_secret() {
    let dir = tempfile::tempdir().expect("temp dir");
    let home = FakeHome::new();
    let mut client = client(dir.path(), home.clone());
    let request = request();

    let paired = client.pair(&request).expect("pair");
    assert_eq!(paired.surface_id, request.surface_id);
    assert_eq!(paired.scopes, home.scopes);
    assert_eq!(paired.expires_at, home.expires_at);
    assert_eq!(
        paired.token_path,
        AgentConfig::surface_token_path(dir.path(), &request.surface_id)
    );

    let token = token();
    let stored = std::fs::read_to_string(&paired.token_path).expect("token file");
    assert_eq!(stored.trim(), token.as_str());
    assert!(
        !stored.contains(CODE),
        "the code must never be written next to the token"
    );
    let mode = std::fs::metadata(&paired.token_path)
        .expect("token metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "the surface token must be 0600");
    let expiry = std::fs::read_to_string(AgentConfig::surface_expiry_path(dir.path(), &request.surface_id))
        .expect("expiry file");
    assert_eq!(
        Timestamp::parse(expiry.trim()).expect("expiry timestamp"),
        home.expires_at
    );

    let debug = format!("{paired:?}");
    assert!(
        !debug.contains(token.as_str()),
        "the result Debug must not carry the token: {debug}"
    );
    let debug = format!("{request:?}");
    assert!(
        !debug.contains(CODE),
        "the request Debug must not carry the code: {debug}"
    );

    let pairs = home.surface_pairs();
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].0, CODE);
    assert_eq!(pairs[0].1, request.surface_id);
    assert_eq!(pairs[0].2, request.session);
    assert_eq!(pairs[0].3, "opencode");
}

#[test]
fn agent_pair_refuses_a_replayed_code() {
    let dir = tempfile::tempdir().expect("temp dir");
    let home = FakeHome::new();
    let mut client = client(dir.path(), home.clone());
    let request = request();

    let paired = client.pair(&request).expect("first pairing");
    let stored = std::fs::read_to_string(&paired.token_path).expect("token file");

    let error = client.pair(&request).expect_err("a replayed code is refused");
    match error {
        AgentError::Remote {
            kind,
            message,
            retryable,
        } => {
            assert_eq!(kind, ErrorKind::Refused);
            assert!(!retryable);
            assert!(!message.contains(CODE), "the refusal must not echo the code: {message}");
        }
        other => panic!("expected a typed refusal, got {other:?}"),
    }
    assert_eq!(
        std::fs::read_to_string(&paired.token_path).expect("token file"),
        stored,
        "a refused replay must not rewrite the stored token"
    );
    assert_eq!(home.surface_pairs().len(), 2, "both attempts are sent");
}

#[test]
fn agent_pair_surfaces_path_is_private() {
    let dir = tempfile::tempdir().expect("temp dir");
    let home = FakeHome::new();
    let mut client = client(dir.path(), home.clone());
    let request = request();
    let paired = client.pair(&request).expect("pair");

    for path in [
        dir.path().to_path_buf(),
        AgentConfig::surfaces_dir(dir.path()),
        AgentConfig::surface_dir(dir.path(), &request.surface_id),
    ] {
        let mode = std::fs::metadata(&path)
            .expect("surface directory")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700, "{} must be private", path.display());
    }
    for path in [
        paired.token_path.clone(),
        AgentConfig::surface_expiry_path(dir.path(), &request.surface_id),
    ] {
        let mode = std::fs::metadata(&path).expect("surface file").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "{} must be private", path.display());
    }
}
