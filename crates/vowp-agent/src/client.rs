//! The frame client: enrollment, handshake, heartbeat, reconnect and status.
//!
//! Spec: `docs/design/v0.4/local-orchestrator.md` §4.2, §4.6, §8.2–§8.5 and §12.2. All I/O goes
//! through a [`Transport`]; tests inject an in-memory home instead of opening SSH.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use vowp_protocol::{
    AGENT_PROTOCOL, AgentCapabilities, AgentCapacity, AgentFrame, AgentPathMap, AgentRole, ErrorKind, NodeName,
    SessionId, SurfaceId, SurfaceScope, Timestamp,
};

use crate::config::{AgentConfig, ensure_private_dir, write_private, write_surface_binding};
use crate::error::AgentError;
use crate::executor::{RunDriver, RunHandler};
use crate::key::NodeKey;
use crate::status::{AgentStatus, ConnectionState, pending_spool_bytes, surface_token_expiry};
use crate::transport::{FrameRead, FrameStream, MAX_FRAME_BYTES, SshTransport, Transport};

/// Agent crate version sent in `AgentHello` and `Enroll`.
pub const AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Default heartbeat interval when the home does not name one.
pub const HEARTBEAT_SECS: u64 = 10;
/// A connection that lasted this long resets the reconnect backoff.
pub const STABLE_CONNECTION: Duration = Duration::from_secs(60);
/// Longest reconnect delay.
pub const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// How long `pair` waits for the home's answer after the handshake.
pub const PAIR_TIMEOUT: Duration = Duration::from_secs(30);

/// The clock and sleeper the client uses. Tests inject a fake clock.
pub trait AgentClock: Send + Sync {
    /// Returns the current instant.
    fn now(&self) -> Timestamp;
    /// Sleeps for `duration`, advancing fake time in tests.
    fn sleep(&self, duration: Duration);
}

/// The real clock backed by the operating system.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemAgentClock;

impl AgentClock for SystemAgentClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// Exponential reconnect backoff: 1, 2, 4 … 60 seconds with up to 25 % jitter.
#[derive(Debug, Clone)]
pub struct Backoff {
    base: Duration,
    max: Duration,
    next: Duration,
    state: u64,
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new()
    }
}

impl Backoff {
    /// Creates a time-seeded backoff.
    pub fn new() -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        Self::seeded(seed)
    }

    /// Creates a deterministic backoff for tests.
    pub fn seeded(seed: u64) -> Self {
        Self {
            base: Duration::from_secs(1),
            max: MAX_BACKOFF,
            next: Duration::from_secs(1),
            state: seed | 1,
        }
    }

    /// Returns the next delay and doubles the base for the following attempt.
    pub fn next_delay(&mut self) -> Duration {
        let base = self.next;
        let span = u64::try_from(base.as_millis() / 4).unwrap_or(u64::MAX);
        let jitter = if span == 0 { 0 } else { self.random() % (span + 1) };
        let delay = base.saturating_add(Duration::from_millis(jitter));
        self.next = base.saturating_mul(2).min(self.max);
        delay
    }

    /// Resets the next delay to one second after a stable connection.
    pub fn reset(&mut self) {
        self.next = self.base;
    }

    fn random(&mut self) -> u64 {
        let mut value = self.state;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.state = value;
        value
    }
}

/// What `vowp agent run` passed on the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOptions {
    /// Requested role. Phase 0 accepts it and keeps the connection open for later assignment work.
    pub role: AgentRole,
    /// Keep the process in the terminal. Phase 0 always runs in the foreground.
    pub foreground: bool,
}

impl RunOptions {
    /// Creates options with the Phase 0 foreground default.
    pub fn new(role: AgentRole) -> Self {
        Self { role, foreground: true }
    }
}

/// Why the run loop stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome {
    /// The home revoked the node credential.
    Revoked {
        /// Human-readable reason.
        reason: String,
    },
    /// Another connection took over.
    Superseded {
        /// Connection that replaced this one.
        connection_id: String,
    },
}

/// The surface metadata a Mac needs to redeem a home pairing code.
///
/// The code is a secret. `Debug` redacts it; callers must never print the struct with its
/// fields expanded by hand.
#[derive(Clone, PartialEq, Eq)]
pub struct SurfacePairRequest {
    /// One-time pairing code read from the code file; never logged or echoed.
    pub code: String,
    /// Surface the owner created on the home.
    pub surface_id: SurfaceId,
    /// Session the surface will mirror.
    pub session: SessionId,
    /// Application name, for example `opencode`.
    pub app: String,
}

impl SurfacePairRequest {
    /// Builds one pairing request.
    pub fn new(code: impl Into<String>, surface_id: SurfaceId, session: SessionId, app: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            surface_id,
            session,
            app: app.into(),
        }
    }
}

impl std::fmt::Debug for SurfacePairRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SurfacePairRequest")
            .field("code", &"[REDACTED]")
            .field("surface_id", &self.surface_id)
            .field("session", &self.session)
            .field("app", &self.app)
            .finish()
    }
}

/// A surface token stored by the agent. The token itself is never carried in this value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairedSurface {
    /// Surface the token belongs to.
    pub surface_id: SurfaceId,
    /// Scopes granted to the token.
    pub scopes: Vec<SurfaceScope>,
    /// When the token expires.
    pub expires_at: Timestamp,
    /// Mode-0600 file that holds the token, for display only.
    pub token_path: PathBuf,
}

/// The result of a successful enrollment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Enrollment {
    /// Node name assigned by the home.
    pub node: NodeName,
    /// Home identity.
    pub home: String,
    /// Negotiated protocol.
    pub protocol: String,
    /// Heartbeat interval in seconds.
    pub heartbeat_secs: u64,
}

/// One enrolled agent connection.
pub struct AgentClient {
    state_dir: PathBuf,
    key: NodeKey,
    config: Option<AgentConfig>,
    transport: Box<dyn Transport>,
    clock: Arc<dyn AgentClock>,
    connection: Option<Connection>,
    status: AgentStatus,
    seen_nonces: HashSet<String>,
    backoff: Backoff,
    pending_path_maps: Vec<AgentPathMap>,
    sent_path_maps: Vec<AgentPathMap>,
    path_map_updates: bool,
    awaiting_path_map_reply: bool,
}

struct Connection {
    stream: Box<dyn FrameStream>,
    heartbeat_interval: Duration,
    max_frame_bytes: usize,
}

enum ServeOutcome {
    Stopped(RunOutcome),
    Disconnected(String),
    Failed(AgentError),
}

impl AgentClient {
    /// Builds a client for an already enrolled config.
    pub fn new(
        state_dir: PathBuf,
        key: NodeKey,
        config: AgentConfig,
        transport: Box<dyn Transport>,
        clock: Arc<dyn AgentClock>,
    ) -> Self {
        let status = base_status(&state_dir, Some(&config), &key);
        let pending_path_maps = config.path_maps.clone();
        Self {
            state_dir,
            key,
            config: Some(config),
            transport,
            clock,
            connection: None,
            status,
            seen_nonces: HashSet::new(),
            backoff: Backoff::new(),
            pending_path_maps,
            // A fresh process reports the saved map after its first handshake, so a project
            // added while the agent was stopped reaches the home without a re-enrollment.
            sent_path_maps: Vec::new(),
            path_map_updates: true,
            awaiting_path_map_reply: false,
        }
    }

    /// Builds a client with no config, for `vowp agent enroll`.
    pub fn unenrolled(
        state_dir: PathBuf,
        key: NodeKey,
        transport: Box<dyn Transport>,
        clock: Arc<dyn AgentClock>,
    ) -> Self {
        let status = base_status(&state_dir, None, &key);
        Self {
            state_dir,
            key,
            config: None,
            transport,
            clock,
            connection: None,
            status,
            seen_nonces: HashSet::new(),
            backoff: Backoff::new(),
            pending_path_maps: Vec::new(),
            sent_path_maps: Vec::new(),
            path_map_updates: true,
            awaiting_path_map_reply: false,
        }
    }

    /// Loads `agent.json` and `node.key` and builds the real SSH transport.
    pub fn from_state_dir(state_dir: PathBuf, clock: Arc<dyn AgentClock>) -> Result<Self, AgentError> {
        let config = AgentConfig::load(&state_dir)?;
        let key = NodeKey::load(&state_dir)?;
        let target = config.ssh_target()?.to_owned();
        Ok(Self::new(
            state_dir,
            key,
            config,
            Box::new(SshTransport::new(target)),
            clock,
        ))
    }

    /// Replaces the reconnect backoff (tests use [`Backoff::seeded`]).
    pub fn with_backoff(mut self, backoff: Backoff) -> Self {
        self.backoff = backoff;
        self
    }

    /// Sets the project path map sent by the next [`AgentClient::enroll`].
    ///
    /// Callers validate the maps with [`AgentConfig::validate_path_maps`] before enrolling.
    /// On an already enrolled client the maps replace the in-memory config; the file changes
    /// only when the caller writes it.
    pub fn with_path_maps(mut self, path_maps: Vec<AgentPathMap>) -> Self {
        self.pending_path_maps = path_maps.clone();
        if let Some(config) = self.config.as_mut() {
            config.path_maps = path_maps;
        }
        self
    }

    /// Returns the current project path map.
    pub fn path_maps(&self) -> &[AgentPathMap] {
        self.config
            .as_ref()
            .map(|config| config.path_maps.as_slice())
            .unwrap_or(&self.pending_path_maps)
    }

    /// Redeems a one-time code, reconnects and performs the handshake.
    pub fn enroll(&mut self, target: &str, code: &str) -> Result<Enrollment, AgentError> {
        validate_enrollment_code(code)?;
        if target.trim().is_empty() {
            return Err(AgentError::Config("ssh target must not be empty".to_owned()));
        }
        let public_key = self.key.public_key_hex();
        let path_maps = self.path_maps().to_vec();
        AgentConfig::validate_path_maps(&self.state_dir, &path_maps)?;
        let mut stream = self.transport.open()?;
        stream.send(&AgentFrame::Enroll {
            code: code.to_owned(),
            public_key,
            agent_version: AGENT_VERSION.to_owned(),
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            labels: Vec::new(),
            capabilities: capabilities(&path_maps),
            paths: path_maps.clone(),
        })?;
        let enrolled = match next_handshake_frame(&mut *stream)? {
            AgentFrame::Enrolled {
                home_id,
                node,
                protocol,
                server_time: _,
                heartbeat_secs,
            } => Enrollment {
                node,
                home: home_id,
                protocol,
                heartbeat_secs,
            },
            AgentFrame::Revoked { reason } => return Err(AgentError::Revoked(reason)),
            AgentFrame::Superseded { connection_id } => return Err(AgentError::Superseded(connection_id)),
            AgentFrame::Error {
                kind,
                message,
                retryable,
            } => return Err(remote_error(kind, message, retryable)),
            _ => {
                return Err(AgentError::Protocol("expected Enrolled from the home".to_owned()));
            }
        };
        drop(stream);

        let config = AgentConfig::new(
            enrolled.home.clone(),
            enrolled.node.clone(),
            self.state_dir.clone(),
            target,
        )
        .with_path_maps(path_maps);
        self.config = Some(config.clone());
        if let Err(error) = self.connect() {
            self.config = None;
            return Err(error);
        }
        config.write()?;
        self.pending_path_maps = config.path_maps.clone();
        self.sent_path_maps = config.path_maps.clone();
        self.status = base_status(&self.state_dir, Some(&config), &self.key);
        self.status.connection = ConnectionState::Stopped;
        self.save_status()?;
        Ok(enrolled)
    }

    /// Redeems one surface pairing code over the agent connection.
    ///
    /// The client performs the normal handshake, sends one `SurfacePair` frame with the
    /// request's surface, session and app, and writes the returned token to
    /// `surfaces/<surface_id>/token` mode 0600. The token is never printed, logged or
    /// returned; the [`PairedSurface`] result names only its path, scopes and expiry.
    pub fn pair(&mut self, request: &SurfacePairRequest) -> Result<PairedSurface, AgentError> {
        if self.config.is_none() {
            return Err(AgentError::NotEnrolled("agent.json is missing".to_owned()));
        }
        validate_pairing_code(&request.code)?;
        validate_surface_id(&request.surface_id)?;
        if request.app.is_empty() || request.app.chars().count() > 64 || request.app.chars().any(char::is_control) {
            return Err(AgentError::Config(
                "app must be a printable name of at most 64 characters".to_owned(),
            ));
        }

        self.connect()?;
        let result = self.redeem_surface(request);
        // Pairing is a one-shot command; close the SSH child instead of leaving it idle.
        self.connection = None;
        result
    }

    fn redeem_surface(&mut self, request: &SurfacePairRequest) -> Result<PairedSurface, AgentError> {
        self.send_frame(&AgentFrame::SurfacePair {
            code: request.code.clone(),
            surface_id: request.surface_id.clone(),
            session: request.session.clone(),
            app: request.app.clone(),
        })?;
        let (token, scopes, expires_at) = match self.recv_frame(Some(PAIR_TIMEOUT))? {
            FrameRead::Frame(AgentFrame::SurfaceIssued {
                surface_id,
                token,
                scopes,
                expires_at,
            }) => {
                if surface_id != request.surface_id {
                    return Err(AgentError::Protocol(
                        "the home issued a token for another surface".to_owned(),
                    ));
                }
                if token.is_empty() {
                    return Err(AgentError::Protocol(
                        "the home returned an empty surface token".to_owned(),
                    ));
                }
                (token, scopes, expires_at)
            }
            FrameRead::Frame(AgentFrame::SurfaceRevoked { reason, .. }) => {
                return Err(AgentError::Remote {
                    kind: ErrorKind::Refused,
                    message: format!("surface {} was revoked: {reason}", request.surface_id),
                    retryable: false,
                });
            }
            FrameRead::Frame(AgentFrame::Revoked { reason }) => return Err(AgentError::Revoked(reason)),
            FrameRead::Frame(AgentFrame::Superseded { connection_id }) => {
                return Err(AgentError::Superseded(connection_id));
            }
            FrameRead::Frame(AgentFrame::Error {
                kind,
                message,
                retryable,
            }) => return Err(remote_error(kind, message, retryable)),
            FrameRead::Frame(_) => {
                return Err(AgentError::Protocol("expected SurfaceIssued from the home".to_owned()));
            }
            FrameRead::Timeout => {
                return Err(AgentError::Transport(
                    "the home did not answer the surface pairing request".to_owned(),
                ));
            }
            FrameRead::Closed => {
                return Err(AgentError::Transport(
                    "the home closed the connection during surface pairing".to_owned(),
                ));
            }
        };
        let token_path = write_surface_token(&self.state_dir, &request.surface_id, &token, expires_at)?;
        write_surface_binding(&self.state_dir, &request.surface_id, &request.session)?;
        Ok(PairedSurface {
            surface_id: request.surface_id.clone(),
            scopes,
            expires_at,
            token_path,
        })
    }

    /// Opens a transport and performs `AgentHello` → `HomeChallenge` → `AgentAuth` → `HomeWelcome`.
    pub fn connect(&mut self) -> Result<(), AgentError> {
        let config = self
            .config
            .clone()
            .ok_or_else(|| AgentError::NotEnrolled("agent.json is missing".to_owned()))?;
        self.connection = None;
        let mut stream = self.transport.open()?;
        stream.send(&AgentFrame::AgentHello {
            node: config.node.clone(),
            key_id: self.key.public_key_hex(),
            agent_version: AGENT_VERSION.to_owned(),
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            labels: Vec::new(),
            capabilities: capabilities(&config.path_maps),
        })?;

        let challenge = match next_handshake_frame(&mut *stream)? {
            frame @ AgentFrame::HomeChallenge { .. } => frame,
            AgentFrame::Revoked { reason } => return Err(AgentError::Revoked(reason)),
            AgentFrame::Superseded { connection_id } => return Err(AgentError::Superseded(connection_id)),
            AgentFrame::Error {
                kind,
                message,
                retryable,
            } => return Err(remote_error(kind, message, retryable)),
            _ => {
                return Err(AgentError::Protocol("expected HomeChallenge from the home".to_owned()));
            }
        };
        let AgentFrame::HomeChallenge {
            nonce,
            home_id,
            connection_id,
            server_time: _,
            heartbeat_secs,
            max_frame_bytes,
        } = challenge
        else {
            return Err(AgentError::Protocol("expected HomeChallenge".to_owned()));
        };
        if nonce.is_empty() {
            return Err(AgentError::Protocol("HomeChallenge nonce is empty".to_owned()));
        }
        if !self.seen_nonces.insert(nonce.clone()) {
            return Err(AgentError::ReplayNonce);
        }
        stream.send(&AgentFrame::AgentAuth {
            signature: self.key.sign_handshake(&nonce, &home_id, &connection_id),
        })?;

        let welcome = match next_handshake_frame(&mut *stream)? {
            frame @ AgentFrame::HomeWelcome { .. } => frame,
            AgentFrame::Revoked { reason } => return Err(AgentError::Revoked(reason)),
            AgentFrame::Superseded { connection_id } => return Err(AgentError::Superseded(connection_id)),
            AgentFrame::Error {
                kind,
                message,
                retryable,
            } => return Err(remote_error(kind, message, retryable)),
            _ => {
                return Err(AgentError::Protocol("expected HomeWelcome from the home".to_owned()));
            }
        };
        let AgentFrame::HomeWelcome {
            protocol,
            home_id: welcome_home,
            node,
            connection_id: welcome_connection,
        } = welcome
        else {
            return Err(AgentError::Protocol("expected HomeWelcome".to_owned()));
        };
        if protocol != AGENT_PROTOCOL {
            return Err(AgentError::Protocol(format!(
                "unsupported home protocol {protocol}; this agent speaks {AGENT_PROTOCOL}"
            )));
        }
        if welcome_home != home_id || welcome_connection != connection_id {
            return Err(AgentError::Auth("HomeWelcome does not match the challenge".to_owned()));
        }
        if node != config.node {
            return Err(AgentError::Auth(format!(
                "home welcomed node {node}, but this agent enrolled as {}",
                config.node
            )));
        }

        let heartbeat_secs = if heartbeat_secs == 0 {
            HEARTBEAT_SECS
        } else {
            heartbeat_secs
        };
        self.connection = Some(Connection {
            stream,
            heartbeat_interval: Duration::from_secs(heartbeat_secs),
            max_frame_bytes: usize::try_from(max_frame_bytes).unwrap_or(MAX_FRAME_BYTES),
        });
        Ok(())
    }

    /// Reports path-map changes since the last report.
    ///
    /// `vowp agent paths add` and `rm` edit `agent.json` while `vowp agent run` is connected.
    /// Each heartbeat reloads the file and sends one [`AgentFrame::PathMapUpdate`] for every
    /// project that was added or changed. A home that predates the frame answers `Error`; the
    /// client then stops sending updates on this process instead of reconnecting in a loop.
    pub fn refresh_path_maps(&mut self) -> Result<usize, AgentError> {
        if self.connection.is_none() || !self.path_map_updates {
            return Ok(0);
        }
        let current = match AgentConfig::load(&self.state_dir) {
            Ok(config) => config.path_maps,
            Err(AgentError::NotEnrolled(_)) => return Ok(0),
            // `vowp agent paths add|rm` rewrites the file in place. A heartbeat that catches
            // the write mid-flight, or a hand edit that is not valid yet, keeps the last good
            // map and retries on the next heartbeat instead of ending the run loop.
            Err(AgentError::Json(_) | AgentError::Config(_)) => return Ok(0),
            Err(error) => return Err(error),
        };
        if let Some(config) = self.config.as_mut() {
            config.path_maps = current.clone();
        }
        let changed: Vec<AgentPathMap> = current
            .iter()
            .filter(|map| !self.sent_path_maps.iter().any(|sent| sent == *map))
            .cloned()
            .collect();
        if changed.is_empty() {
            self.sent_path_maps = current;
            return Ok(0);
        }
        for map in &changed {
            self.send_frame(&AgentFrame::PathMapUpdate { map: map.clone() })?;
        }
        self.sent_path_maps = current;
        self.awaiting_path_map_reply = true;
        Ok(changed.len())
    }

    /// Sends one heartbeat and updates `status.json`.
    pub fn heartbeat(&mut self) -> Result<Timestamp, AgentError> {
        let now = self.clock.now();
        let spool = pending_spool_bytes(&self.state_dir);
        self.send_frame(&AgentFrame::Heartbeat {
            at: now,
            capacity: collect_capacity(&self.state_dir),
            pending_spool_bytes: spool,
        })?;
        self.refresh_path_maps()?;
        self.status.connection = ConnectionState::Connected;
        self.status.last_heartbeat = Some(now);
        self.status.pending_spool_bytes = spool;
        if let Some(expiry) = surface_token_expiry(&self.state_dir) {
            self.status.surface_token_expires_at = Some(expiry);
        }
        self.save_status()?;
        Ok(now)
    }

    /// Runs until the home revokes the node or another connection takes over.
    ///
    /// This Phase 0 entry point has no executor behind it: an `AssignRun` frame is refused
    /// with [`RunRejectReason::Other`](vowp_protocol::RunRejectReason::Other) and never
    /// starts a handler. Use [`AgentClient::run_with_handler`] to run assignments.
    pub fn run(&mut self, options: &RunOptions) -> Result<RunOutcome, AgentError> {
        self.run_with(RunDriver::rejecting(), options)
    }

    /// Runs the connection loop with an injected [`RunHandler`].
    ///
    /// `AssignRun` frames are validated and accepted, the handler runs off the frame loop,
    /// every line is spooled before it is sent, and a reconnect replays the first
    /// unacknowledged line. The lease token stays in memory and is never handed to the
    /// handler.
    pub fn run_with_handler(
        &mut self,
        options: &RunOptions,
        handler: Arc<dyn RunHandler>,
    ) -> Result<RunOutcome, AgentError> {
        self.run_with(RunDriver::new(handler), options)
    }

    fn run_with(&mut self, mut driver: RunDriver, _options: &RunOptions) -> Result<RunOutcome, AgentError> {
        if self.config.is_none() {
            return Err(AgentError::NotEnrolled("agent.json is missing".to_owned()));
        }
        let mut backoff = self.backoff.clone();
        self.status = base_status(&self.state_dir, self.config.as_ref(), &self.key);
        self.status.connection = ConnectionState::Connecting;
        self.save_status()?;

        loop {
            let connected_at = match self.connect() {
                Ok(()) => {
                    self.status.connection = ConnectionState::Connected;
                    self.status.last_error = None;
                    self.save_status()?;
                    Some(self.clock.now())
                }
                Err(AgentError::Revoked(reason)) => {
                    self.status.connection = ConnectionState::Revoked;
                    self.status.last_error = Some(reason.clone());
                    self.save_status()?;
                    return Ok(RunOutcome::Revoked { reason });
                }
                Err(AgentError::Superseded(connection_id)) => {
                    self.status.connection = ConnectionState::Superseded;
                    self.status.last_error = Some(format!("superseded by {connection_id}"));
                    self.save_status()?;
                    return Ok(RunOutcome::Superseded { connection_id });
                }
                Err(error) if error.is_fatal() => {
                    self.status.connection = ConnectionState::Stopped;
                    self.status.last_error = Some(error.to_string());
                    self.save_status()?;
                    return Err(error);
                }
                Err(error) => {
                    self.status.connection = ConnectionState::Reconnecting;
                    self.status.last_error = Some(error.to_string());
                    self.save_status()?;
                    None
                }
            };

            if let Some(connected_at) = connected_at {
                let served = match driver.resume(self) {
                    Ok(()) => self.serve(&mut driver),
                    Err(error) => serve_outcome_from_error(error),
                };
                match served {
                    ServeOutcome::Stopped(outcome) => return Ok(outcome),
                    ServeOutcome::Failed(error) => {
                        self.status.connection = ConnectionState::Stopped;
                        self.status.last_error = Some(error.to_string());
                        self.save_status()?;
                        return Err(error);
                    }
                    ServeOutcome::Disconnected(message) => {
                        driver.note_disconnect(self);
                        self.status.connection = ConnectionState::Reconnecting;
                        self.status.last_error = Some(message);
                        self.save_status()?;
                        if self.clock.now().since(connected_at) >= STABLE_CONNECTION {
                            backoff.reset();
                        }
                    }
                }
            }

            let delay = backoff.next_delay();
            self.status.connection = ConnectionState::Reconnecting;
            self.save_status()?;
            self.clock.sleep(delay);
        }
    }

    /// Returns the current local status, refreshing spool and surface expiry.
    pub fn status(&self) -> AgentStatus {
        let mut status = self.status.clone();
        status.pending_spool_bytes = pending_spool_bytes(&self.state_dir);
        if let Some(expiry) = surface_token_expiry(&self.state_dir) {
            status.surface_token_expires_at = Some(expiry);
        }
        status
    }

    /// Returns the agent state directory.
    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    /// Returns the node name this agent enrolled as.
    pub fn node(&self) -> NodeName {
        self.config
            .as_ref()
            .map(|config| config.node.clone())
            .or_else(|| self.status.node.clone())
            // invariant: "agent" is a valid node slug and an enrolled client has one.
            .unwrap_or_else(|| NodeName::parse("agent").expect("invariant: 'agent' is a valid node slug"))
    }

    /// Sleeps through the injected clock (tests advance fake time).
    pub(crate) fn sleep(&self, duration: Duration) {
        self.clock.sleep(duration);
    }

    /// Returns the injected clock's current instant.
    pub(crate) fn clock_now(&self) -> Timestamp {
        self.clock.now()
    }

    fn serve(&mut self, driver: &mut RunDriver) -> ServeOutcome {
        if let Err(error) = self.heartbeat() {
            return disconnected_or_failed(error);
        }
        loop {
            if let Err(error) = driver.pump(self) {
                return serve_outcome_from_error(error);
            }
            let interval = self
                .connection
                .as_ref()
                .map(|connection| connection.heartbeat_interval)
                .unwrap_or(Duration::from_secs(HEARTBEAT_SECS));
            let last = self.status.last_heartbeat.unwrap_or_else(|| self.clock.now());
            let mut remaining = interval.saturating_sub(self.clock.now().since(last));
            if driver.has_open_runs() {
                remaining = remaining.min(crate::executor::RUN_POLL);
            }
            let read = match self.recv_frame(Some(remaining)) {
                Ok(read) => read,
                Err(error) => return disconnected_or_failed(error),
            };
            match read {
                FrameRead::Timeout => {
                    if self.clock.now().since(last) >= interval
                        && let Err(error) = self.heartbeat()
                    {
                        return disconnected_or_failed(error);
                    }
                }
                FrameRead::Closed => return ServeOutcome::Disconnected("connection closed".to_owned()),
                FrameRead::Frame(frame) => {
                    if self.awaiting_path_map_reply && !matches!(&frame, AgentFrame::Error { .. }) {
                        self.awaiting_path_map_reply = false;
                    }
                    match frame {
                        AgentFrame::Ping => {
                            if let Err(error) = self.send_frame(&AgentFrame::Pong) {
                                return disconnected_or_failed(error);
                            }
                        }
                        AgentFrame::AssignRun {
                            assignment,
                            lease_token,
                            lease_expires_at,
                        } => {
                            if let Err(error) = driver.handle_assign(self, assignment, lease_token, lease_expires_at) {
                                return disconnected_or_failed(error);
                            }
                        }
                        AgentFrame::CancelRun { run_id, epoch, reason } => {
                            driver.handle_cancel(self, &run_id, epoch, &reason)
                        }
                        AgentFrame::RunLost { run_id, reason } => driver.handle_run_lost(self, &run_id, &reason),
                        AgentFrame::Cleanup { run_id, job, .. } => driver.handle_cleanup(self, &run_id, &job),
                        AgentFrame::Credit { run_id, count } => driver.handle_credit(&run_id, count),
                        AgentFrame::IngestAck {
                            run_id,
                            event_id,
                            seq,
                            duplicate,
                            lease_revoked,
                            error,
                        } => driver.handle_ack(&run_id, event_id.as_ref(), seq, duplicate, lease_revoked, error),
                        AgentFrame::Revoked { reason } => {
                            self.status.connection = ConnectionState::Revoked;
                            self.status.last_error = Some(reason.clone());
                            if let Err(error) = self.save_status() {
                                return ServeOutcome::Failed(error);
                            }
                            return ServeOutcome::Stopped(RunOutcome::Revoked { reason });
                        }
                        AgentFrame::Superseded { connection_id } => {
                            self.status.connection = ConnectionState::Superseded;
                            self.status.last_error = Some(format!("superseded by {connection_id}"));
                            if let Err(error) = self.save_status() {
                                return ServeOutcome::Failed(error);
                            }
                            return ServeOutcome::Stopped(RunOutcome::Superseded { connection_id });
                        }
                        AgentFrame::Error {
                            kind,
                            message,
                            retryable,
                        } => {
                            if self.awaiting_path_map_reply {
                                // Older homes answer an unknown `PathMapUpdate` with `Error`
                                // and close the connection. Keep the connection usable by
                                // not sending more map updates; the next process retries.
                                self.awaiting_path_map_reply = false;
                                self.path_map_updates = false;
                                self.sent_path_maps = self.path_maps().to_vec();
                                continue;
                            }
                            let error = remote_error(kind, message, retryable);
                            return disconnected_or_failed(error);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    pub(crate) fn send_frame(&mut self, frame: &AgentFrame) -> Result<(), AgentError> {
        let connection = self
            .connection
            .as_mut()
            .ok_or_else(|| AgentError::Transport("not connected".to_owned()))?;
        if serde_json::to_vec(frame)?.len() + 1 > connection.max_frame_bytes {
            return Err(AgentError::FrameTooLarge {
                size: serde_json::to_vec(frame)?.len() + 1,
                max: connection.max_frame_bytes,
            });
        }
        connection.stream.send(frame)
    }

    pub(crate) fn recv_frame(&mut self, timeout: Option<Duration>) -> Result<FrameRead, AgentError> {
        self.connection
            .as_mut()
            .ok_or_else(|| AgentError::Transport("not connected".to_owned()))?
            .stream
            .recv(timeout)
    }

    fn save_status(&self) -> Result<(), AgentError> {
        self.status.write(&self.state_dir)
    }
}

/// Validates the one-time enrollment code shape without retaining or printing it.
pub fn validate_enrollment_code(code: &str) -> Result<(), AgentError> {
    validate_code_shape(code, "enrollment")
}

/// Validates the one-time surface pairing code shape without retaining or printing it.
pub fn validate_pairing_code(code: &str) -> Result<(), AgentError> {
    validate_code_shape(code, "pairing")
}

/// Validates the 26-character ULID shape of a surface id before it is used as a path segment.
pub fn validate_surface_id(surface: &SurfaceId) -> Result<(), AgentError> {
    if surface.0.len() == 26 && surface.0.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        Ok(())
    } else {
        Err(AgentError::Config("surface id must be a 26-character ULID".to_owned()))
    }
}

fn validate_code_shape(code: &str, kind: &str) -> Result<(), AgentError> {
    if code.len() == 52 && code.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        Ok(())
    } else {
        Err(AgentError::Config(format!("{kind} code must be 52 characters")))
    }
}

/// Writes the token and its expiry below `state_dir/surfaces/<id>` with private modes.
fn write_surface_token(
    state_dir: &Path,
    surface: &SurfaceId,
    token: &str,
    expires_at: Timestamp,
) -> Result<PathBuf, AgentError> {
    ensure_private_dir(state_dir)?;
    ensure_private_dir(&AgentConfig::surfaces_dir(state_dir))?;
    ensure_private_dir(&AgentConfig::surface_dir(state_dir, surface))?;
    let token_path = AgentConfig::surface_token_path(state_dir, surface);
    write_private(&token_path, format!("{token}\n").as_bytes())?;
    write_private(
        &AgentConfig::surface_expiry_path(state_dir, surface),
        expires_at.as_rfc3339().as_bytes(),
    )?;
    Ok(token_path)
}

fn base_status(state_dir: &Path, config: Option<&AgentConfig>, key: &NodeKey) -> AgentStatus {
    AgentStatus {
        connection: ConnectionState::Unknown,
        node: config.map(|config| config.node.clone()),
        home: config.map(|config| config.home.clone()),
        protocol: Some(AGENT_PROTOCOL.to_owned()),
        last_heartbeat: None,
        last_error: None,
        pending_spool_bytes: pending_spool_bytes(state_dir),
        key_fingerprint: Some(key.fingerprint()),
        surface_token_expires_at: surface_token_expiry(state_dir),
    }
}

fn next_handshake_frame(stream: &mut dyn FrameStream) -> Result<AgentFrame, AgentError> {
    match stream.recv(None)? {
        FrameRead::Frame(frame) => Ok(frame),
        FrameRead::Timeout => Err(AgentError::Transport(
            "the home did not answer the handshake".to_owned(),
        )),
        FrameRead::Closed => Err(AgentError::Transport(
            "the home closed the connection during the handshake".to_owned(),
        )),
    }
}

fn disconnected_or_failed(error: AgentError) -> ServeOutcome {
    if error.is_fatal() {
        ServeOutcome::Failed(error)
    } else {
        ServeOutcome::Disconnected(error.to_string())
    }
}

/// Maps a run-loop error to the connection outcome; a revoke or takeover ends the loop.
fn serve_outcome_from_error(error: AgentError) -> ServeOutcome {
    match error {
        AgentError::Revoked(reason) => ServeOutcome::Stopped(RunOutcome::Revoked { reason }),
        AgentError::Superseded(connection_id) => ServeOutcome::Stopped(RunOutcome::Superseded { connection_id }),
        error => disconnected_or_failed(error),
    }
}

fn remote_error(kind: ErrorKind, message: String, retryable: bool) -> AgentError {
    if kind == ErrorKind::Auth {
        AgentError::Auth(message)
    } else {
        AgentError::Remote {
            kind,
            message,
            retryable,
        }
    }
}

fn capabilities(path_maps: &[AgentPathMap]) -> AgentCapabilities {
    AgentCapabilities {
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        agent_version: AGENT_VERSION.to_owned(),
        harnesses: BTreeMap::new(),
        git_version: None,
        labels: Vec::new(),
        secret_names: Vec::new(),
        projects: path_maps.iter().map(|map| map.project.clone()).collect(),
    }
}

fn collect_capacity(state_dir: &Path) -> AgentCapacity {
    let cpu_cores = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .min(u32::MAX as usize) as u32;
    let (mem_total_bytes, mem_free_bytes) = read_meminfo();
    AgentCapacity {
        cpu_cores,
        load1: read_load1(),
        mem_total_bytes,
        mem_free_bytes,
        disk_free_bytes: read_disk_free(state_dir),
        busy_slots: 0,
        max_slots: cpu_cores.max(1),
        on_battery: None,
        thermal_state: None,
    }
}

fn read_meminfo() -> (u64, u64) {
    let Ok(text) = std::fs::read_to_string("/proc/meminfo") else {
        return (0, 0);
    };
    let total = meminfo_value(&text, "MemTotal:").unwrap_or(0);
    let free = meminfo_value(&text, "MemAvailable:")
        .or_else(|| meminfo_value(&text, "MemFree:"))
        .unwrap_or(0);
    (total, free)
}

fn meminfo_value(text: &str, key: &str) -> Option<u64> {
    text.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        if fields.next()? != key {
            return None;
        }
        let kib = fields.next()?.parse::<u64>().ok()?;
        Some(kib.saturating_mul(1024))
    })
}

fn read_load1() -> f64 {
    let Ok(text) = std::fs::read_to_string("/proc/loadavg") else {
        return 0.0;
    };
    text.split_whitespace()
        .next()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(0.0)
}

fn read_disk_free(state_dir: &Path) -> u64 {
    let Ok(output) = std::process::Command::new("df").arg("-Pk").arg(state_dir).output() else {
        return 0;
    };
    if !output.status.success() {
        return 0;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let Some(line) = text.lines().nth(1) else {
        return 0;
    };
    line.split_whitespace()
        .nth(3)
        .and_then(|value| value.parse::<u64>().ok())
        .map(|kib| kib.saturating_mul(1024))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;

    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    use super::*;
    use crate::key::handshake_payload;

    #[derive(Clone)]
    struct TestClock {
        inner: Arc<Mutex<TestClockState>>,
    }

    struct TestClockState {
        now: Timestamp,
        sleeps: Vec<Duration>,
    }

    impl TestClock {
        fn new() -> Self {
            Self {
                inner: Arc::new(Mutex::new(TestClockState {
                    now: Timestamp::from_unix_secs(1_700_000_000),
                    sleeps: Vec::new(),
                })),
            }
        }

        fn advance(&self, duration: Duration) {
            let mut state = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            state.now = state.now.plus(duration);
        }

        fn sleeps(&self) -> Vec<Duration> {
            self.inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .sleeps
                .clone()
        }
    }

    impl AgentClock for TestClock {
        fn now(&self) -> Timestamp {
            self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner).now
        }

        fn sleep(&self, duration: Duration) {
            let mut state = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            state.sleeps.push(duration);
            state.now = state.now.plus(duration);
        }
    }

    #[derive(Clone)]
    struct FakeConfig {
        node: NodeName,
        home_id: String,
        protocol: String,
        heartbeat_secs: u64,
        max_frame_bytes: u64,
        drop_connections: usize,
        reject_signature: bool,
        replay_nonce: bool,
        revoke_after_heartbeats: Option<usize>,
        supersede_after_welcome: bool,
    }

    impl Default for FakeConfig {
        fn default() -> Self {
            Self {
                node: NodeName::parse("mac-studio").expect("node slug"),
                home_id: "home-1".to_owned(),
                protocol: AGENT_PROTOCOL.to_owned(),
                heartbeat_secs: HEARTBEAT_SECS,
                max_frame_bytes: MAX_FRAME_BYTES as u64,
                drop_connections: 0,
                reject_signature: false,
                replay_nonce: false,
                revoke_after_heartbeats: None,
                supersede_after_welcome: false,
            }
        }
    }

    #[derive(Default)]
    struct FakeState {
        connections: usize,
        received: Vec<AgentFrame>,
        auth_ok: usize,
        auth_bad: usize,
        heartbeats: usize,
    }

    #[derive(Clone)]
    struct FakeHome {
        shared: Arc<Mutex<FakeState>>,
        config: FakeConfig,
        clock: TestClock,
    }

    impl FakeHome {
        fn new(config: FakeConfig, clock: TestClock) -> Self {
            Self {
                shared: Arc::new(Mutex::new(FakeState::default())),
                config,
                clock,
            }
        }

        fn frames(&self) -> Vec<AgentFrame> {
            self.shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .received
                .clone()
        }

        fn connections(&self) -> usize {
            self.shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .connections
        }

        fn auth_ok(&self) -> usize {
            self.shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .auth_ok
        }

        fn auth_bad(&self) -> usize {
            self.shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .auth_bad
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
                config: self.config.clone(),
                clock: self.clock.clone(),
                connection,
                outbound: VecDeque::new(),
                key_id: None,
                challenge: None,
                close_after_welcome: false,
                heartbeats: 0,
            }))
        }
    }

    struct FakeStream {
        shared: Arc<Mutex<FakeState>>,
        config: FakeConfig,
        clock: TestClock,
        connection: usize,
        outbound: VecDeque<AgentFrame>,
        key_id: Option<String>,
        challenge: Option<AgentFrame>,
        close_after_welcome: bool,
        heartbeats: usize,
    }

    impl FakeStream {
        fn verify_auth(&self, signature: &str) -> bool {
            let Some(key_id) = &self.key_id else {
                return false;
            };
            let Some(AgentFrame::HomeChallenge {
                nonce,
                home_id,
                connection_id,
                ..
            }) = &self.challenge
            else {
                return false;
            };
            let Ok(signature) = hex::decode(signature) else {
                return false;
            };
            let Ok(signature) = Signature::from_slice(&signature) else {
                return false;
            };
            let Ok(public) = hex::decode(key_id) else {
                return false;
            };
            let Ok(public) = <[u8; 32]>::try_from(public.as_slice()) else {
                return false;
            };
            let Ok(key) = VerifyingKey::from_bytes(&public) else {
                return false;
            };
            key.verify(&handshake_payload(nonce, home_id, connection_id), &signature)
                .is_ok()
        }
    }

    impl FrameStream for FakeStream {
        fn recv(&mut self, timeout: Option<Duration>) -> Result<FrameRead, AgentError> {
            if let Some(frame) = self.outbound.pop_front() {
                return Ok(FrameRead::Frame(frame));
            }
            if self.close_after_welcome {
                return Ok(FrameRead::Closed);
            }
            match timeout {
                Some(duration) => {
                    self.clock.advance(duration);
                    Ok(FrameRead::Timeout)
                }
                None => Ok(FrameRead::Closed),
            }
        }

        fn send(&mut self, frame: &AgentFrame) -> Result<(), AgentError> {
            {
                let mut state = self.shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                state.received.push(frame.clone());
            }
            match frame {
                AgentFrame::Enroll { public_key, .. } => {
                    if public_key.len() == 64 && public_key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                        self.outbound.push_back(AgentFrame::Enrolled {
                            home_id: self.config.home_id.clone(),
                            node: self.config.node.clone(),
                            protocol: self.config.protocol.clone(),
                            server_time: Timestamp::from_unix_secs(1_700_000_000),
                            heartbeat_secs: self.config.heartbeat_secs,
                        });
                    } else {
                        self.outbound.push_back(AgentFrame::Error {
                            kind: ErrorKind::InvalidInput,
                            message: "public key must be 32 bytes in lowercase hex".to_owned(),
                            retryable: false,
                        });
                    }
                }
                AgentFrame::AgentHello { key_id, .. } => {
                    self.key_id = Some(key_id.clone());
                    let nonce = if self.config.replay_nonce {
                        "replayed-nonce".to_owned()
                    } else {
                        format!("nonce-{}", self.connection)
                    };
                    let challenge = AgentFrame::HomeChallenge {
                        nonce,
                        home_id: self.config.home_id.clone(),
                        connection_id: format!("connection-{}", self.connection),
                        server_time: Timestamp::from_unix_secs(1_700_000_000),
                        heartbeat_secs: self.config.heartbeat_secs,
                        max_frame_bytes: self.config.max_frame_bytes,
                    };
                    self.challenge = Some(challenge.clone());
                    self.outbound.push_back(challenge);
                }
                AgentFrame::AgentAuth { signature } => {
                    let valid = self.verify_auth(signature);
                    if valid && !self.config.reject_signature {
                        self.shared
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .auth_ok += 1;
                        self.outbound.push_back(AgentFrame::HomeWelcome {
                            protocol: self.config.protocol.clone(),
                            home_id: self.config.home_id.clone(),
                            node: self.config.node.clone(),
                            connection_id: format!("connection-{}", self.connection),
                        });
                        if self.connection <= self.config.drop_connections {
                            self.close_after_welcome = true;
                        }
                        if self.config.supersede_after_welcome {
                            self.outbound.push_back(AgentFrame::Superseded {
                                connection_id: "newer-connection".to_owned(),
                            });
                        }
                    } else {
                        self.shared
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .auth_bad += 1;
                        self.outbound.push_back(AgentFrame::Error {
                            kind: ErrorKind::Auth,
                            message: "bad signature".to_owned(),
                            retryable: false,
                        });
                    }
                }
                AgentFrame::Heartbeat { .. } => {
                    self.heartbeats += 1;
                    self.shared
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .heartbeats += 1;
                    if self.connection > self.config.drop_connections
                        && self.config.revoke_after_heartbeats == Some(self.heartbeats)
                    {
                        self.outbound.push_back(AgentFrame::Revoked {
                            reason: "revoked by owner".to_owned(),
                        });
                    }
                }
                _ => {}
            }
            Ok(())
        }
    }

    fn test_config(dir: &Path) -> AgentConfig {
        AgentConfig::new(
            "home-1",
            NodeName::parse("mac-studio").expect("node slug"),
            dir.to_path_buf(),
            "home.example",
        )
    }

    fn enrolled_client(dir: &Path, home: &FakeHome, clock: &TestClock) -> AgentClient {
        let key = NodeKey::load_or_generate(dir).expect("node key");
        AgentClient::new(
            dir.to_path_buf(),
            key,
            test_config(dir),
            Box::new(home.clone()),
            Arc::new(clock.clone()),
        )
    }

    fn unenrolled_client(dir: &Path, home: &FakeHome, clock: &TestClock) -> AgentClient {
        let key = NodeKey::load_or_generate(dir).expect("node key");
        AgentClient::unenrolled(dir.to_path_buf(), key, Box::new(home.clone()), Arc::new(clock.clone()))
    }

    #[test]
    fn agent_enroll_sends_a_public_key_and_stores_the_node() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clock = TestClock::new();
        let home = FakeHome::new(FakeConfig::default(), clock.clone());
        let key = NodeKey::load_or_generate(dir.path()).expect("node key");
        let public_key = key.public_key_hex();
        let mut client = unenrolled_client(dir.path(), &home, &clock);
        let code = "A".repeat(52);

        let enrollment = client.enroll("home.example", &code).expect("enroll");
        assert_eq!(enrollment.node.as_ref(), "mac-studio");
        assert_eq!(enrollment.home, "home-1");
        assert_eq!(enrollment.protocol, AGENT_PROTOCOL);

        let config = AgentConfig::load(dir.path()).expect("stored config");
        assert_eq!(config.node, enrollment.node);
        assert_eq!(config.home, enrollment.home);
        assert_eq!(home.connections(), 2, "enroll then reconnect and handshake");

        let enroll = home
            .frames()
            .into_iter()
            .find_map(|frame| match frame {
                AgentFrame::Enroll { public_key, code, .. } => Some((public_key, code)),
                _ => None,
            })
            .expect("Enroll frame");
        assert_eq!(enroll.0, public_key);
        assert_eq!(enroll.1, code);

        let mode = std::fs::metadata(AgentConfig::file_path(dir.path()))
            .expect("agent.json")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "agent.json must be private");
        assert!(AgentStatus::load(dir.path()).expect("status").is_some());
    }

    #[test]
    fn agent_handshake_rejects_a_bad_signature() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clock = TestClock::new();
        let config = FakeConfig {
            reject_signature: true,
            ..FakeConfig::default()
        };
        let home = FakeHome::new(config, clock.clone());
        let mut client = enrolled_client(dir.path(), &home, &clock);

        let error = client.connect().expect_err("bad signature is rejected");
        assert!(matches!(error, AgentError::Auth(_)), "got {error:?}");
        assert_eq!(home.auth_bad(), 1);
        assert_eq!(home.auth_ok(), 0);
    }

    #[test]
    fn agent_handshake_rejects_a_replayed_nonce() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clock = TestClock::new();
        let config = FakeConfig {
            replay_nonce: true,
            ..FakeConfig::default()
        };
        let home = FakeHome::new(config, clock.clone());
        let mut client = enrolled_client(dir.path(), &home, &clock);

        client.connect().expect("first handshake");
        let error = client.connect().expect_err("replayed nonce is refused");
        assert!(matches!(error, AgentError::ReplayNonce), "got {error:?}");
        let auth_frames = home
            .frames()
            .into_iter()
            .filter(|frame| matches!(frame, AgentFrame::AgentAuth { .. }))
            .count();
        assert_eq!(auth_frames, 1, "the replay must not be signed");
    }

    #[test]
    fn agent_reconnects_with_backoff_after_a_drop() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clock = TestClock::new();
        let config = FakeConfig {
            drop_connections: 2,
            revoke_after_heartbeats: Some(1),
            ..FakeConfig::default()
        };
        let home = FakeHome::new(config, clock.clone());
        let mut client = enrolled_client(dir.path(), &home, &clock).with_backoff(Backoff::seeded(7));

        let outcome = client
            .run(&RunOptions::new(AgentRole::Both))
            .expect("run stops on revoke");
        assert!(matches!(outcome, RunOutcome::Revoked { .. }), "got {outcome:?}");
        assert_eq!(home.connections(), 3, "two drops, then the revoking connection");

        let sleeps = clock.sleeps();
        assert_eq!(sleeps.len(), 2, "one delay per drop");
        assert!(
            sleeps[0] >= Duration::from_secs(1) && sleeps[0] <= Duration::from_millis(1250),
            "first delay {sleeps:?}"
        );
        assert!(
            sleeps[1] >= Duration::from_secs(2) && sleeps[1] <= Duration::from_millis(2500),
            "second delay {sleeps:?}"
        );
        assert!(sleeps[1] > sleeps[0], "backoff must grow: {sleeps:?}");
    }

    #[test]
    fn agent_heartbeats_on_the_interval() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clock = TestClock::new();
        let config = FakeConfig {
            revoke_after_heartbeats: Some(3),
            ..FakeConfig::default()
        };
        let home = FakeHome::new(config, clock.clone());
        let mut client = enrolled_client(dir.path(), &home, &clock);

        let outcome = client
            .run(&RunOptions::new(AgentRole::Executor))
            .expect("run stops on revoke");
        assert!(matches!(outcome, RunOutcome::Revoked { .. }), "got {outcome:?}");

        let heartbeats: Vec<Timestamp> = home
            .frames()
            .into_iter()
            .filter_map(|frame| match frame {
                AgentFrame::Heartbeat { at, .. } => Some(at),
                _ => None,
            })
            .collect();
        assert_eq!(heartbeats.len(), 3, "one immediate and two interval heartbeats");
        assert_eq!(heartbeats[1].since(heartbeats[0]), Duration::from_secs(10));
        assert_eq!(heartbeats[2].since(heartbeats[1]), Duration::from_secs(10));
        assert!(clock.sleeps().is_empty(), "a healthy connection does not back off");

        let status = AgentStatus::load(dir.path()).expect("status").expect("status written");
        assert_eq!(status.connection, ConnectionState::Revoked);
        assert_eq!(status.last_heartbeat, Some(heartbeats[2]));
        assert_eq!(status.node, Some(NodeName::parse("mac-studio").expect("node slug")));
    }

    #[test]
    fn agent_stops_on_revoked() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clock = TestClock::new();
        let config = FakeConfig {
            revoke_after_heartbeats: Some(1),
            ..FakeConfig::default()
        };
        let home = FakeHome::new(config, clock.clone());
        let mut client = enrolled_client(dir.path(), &home, &clock);

        let outcome = client.run(&RunOptions::new(AgentRole::Both)).expect("run");
        match outcome {
            RunOutcome::Revoked { reason } => assert_eq!(reason, "revoked by owner"),
            other => panic!("expected Revoked, got {other:?}"),
        }
        let status = AgentStatus::load(dir.path()).expect("status").expect("status written");
        assert_eq!(status.connection, ConnectionState::Revoked);
        assert!(clock.sleeps().is_empty());
    }

    #[test]
    fn agent_stops_on_superseded() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clock = TestClock::new();
        let config = FakeConfig {
            supersede_after_welcome: true,
            ..FakeConfig::default()
        };
        let home = FakeHome::new(config, clock.clone());
        let mut client = enrolled_client(dir.path(), &home, &clock);

        let outcome = client.run(&RunOptions::new(AgentRole::Both)).expect("run");
        match outcome {
            RunOutcome::Superseded { connection_id } => assert_eq!(connection_id, "newer-connection"),
            other => panic!("expected Superseded, got {other:?}"),
        }
        let status = AgentStatus::load(dir.path()).expect("status").expect("status written");
        assert_eq!(status.connection, ConnectionState::Superseded);
        assert!(clock.sleeps().is_empty());
    }
}
