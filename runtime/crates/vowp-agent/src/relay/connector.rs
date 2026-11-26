//! The machine connector: one outbound WebSocket to the relay, a challenge-signed login, channel
//! handshakes, and the loopback HTTP proxy for one `http.read` stream.
//!
//! Spec: `docs/design/relay-and-packaging.md` §6.3, §6.5, §6.6 and §6.9. The connector
//!
//! * never starts a daemon: it discovers the loopback daemon through `[relay].daemon_url` or
//!   `<home>/web-window` on every request;
//! * serves `http.read`/`http.write` streams now; an MCP, TCP or exec stream returns a typed
//!   `not_implemented` (or `scope_denied` when the scope is not enabled) until slice 6 wires them;
//! * never writes a payload, token, credential or key to its log: the log sink only ever receives
//!   fixed event lines with ids and stable codes;
//! * reconnects with exponential backoff and jitter, refreshes the credential before it expires,
//!   and stops on revocation or when another connection supersedes it.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use tungstenite::client::IntoClientRequest;
use tungstenite::http::HeaderValue;
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::protocol::{CloseFrame, Message, WebSocket, WebSocketConfig};
use vowp_protocol::{
    CLOSE_CRYPTO, CLOSE_NORMAL, CLOSE_TOO_BIG, Clock, E2E_SUITE, Jwk, MAX_CHUNK_ENCODED_BYTES, MAX_RELAY_FRAME_BYTES,
    RELAY_FRAME_VERSION, RELAY_SUBPROTOCOL, RelayConfig, RelayDataFrame, RelayFrame, RemoteFrame, Scope, SystemClock,
    Timestamp, codes, valid_stream_id,
};

use super::auth::{self, CachedJwks, CredentialClaims, Jwks, ReplayCache, TicketContext};
use super::crypto::{AcceptInput, ChannelCipher, machine_accept, public_from_jwk};
use super::key::{ConnectorState, RelayJson, RelayKey, RelayPaths, RelayStatusFile};
use super::{
    CREDENTIAL_REFRESH_MARGIN_SECS, Endpoint, RELAY_AGENT_VERSION, RelayError, RelayStream, capabilities_for,
    fetch_jwks, read_body, read_head, refresh_credential, write_request,
};

/// The environment variable the machine reads, per write request, for the daemon's bearer token.
pub const API_TOKEN_ENV: &str = "VOWP_API_TOKEN";
/// Raw bytes per `HttpBody` chunk before base64 (256 KiB encodes to under 512 KiB).
const HTTP_CHUNK_BYTES: usize = 256 * 1024;
/// Largest non-SSE HTTP response the machine forwards.
const HTTP_RESPONSE_LIMIT: usize = 8 * 1024 * 1024;
/// Longest an SSE response is forwarded.
const SSE_LIMIT: Duration = Duration::from_secs(3_600);
/// Largest proxied request body.
const HTTP_REQUEST_BODY_LIMIT: usize = 1_048_576;
/// Concurrent HTTP streams per channel.
const MAX_STREAMS_PER_CHANNEL: usize = 16;
/// How often the online loop wakes to check the stop flag, the outbox and the heartbeat.
const TICK: Duration = Duration::from_millis(50);
/// A relay JWKS refetch is rate limited to this interval.
const JWKS_REFETCH_SECS: i64 = 30;

/// How the connector ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorOutcome {
    /// The stop flag was set.
    Stopped,
    /// The owner revoked this machine.
    Revoked,
    /// Another connection for the same machine took over.
    Superseded,
}

/// What a finished [`Connector::run`] reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorReport {
    /// How the loop ended.
    pub outcome: ConnectorOutcome,
    /// Authenticated sessions established.
    pub connects: u64,
    /// The backoff delays slept, in order.
    pub delays: Vec<Duration>,
}

/// Connector settings.
pub struct ConnectorOptions {
    /// The node home.
    pub home: PathBuf,
    /// The `[relay]` table.
    pub config: RelayConfig,
    /// Set to ask the connector to stop.
    pub stop: Arc<AtomicBool>,
    /// The clock.
    pub clock: Arc<dyn Clock>,
    /// Receives fixed event lines. Never a payload, token, credential or key.
    pub log: Arc<dyn Fn(&str) + Send + Sync>,
    /// First reconnect delay.
    pub reconnect_min: Duration,
    /// Reconnect delay cap.
    pub reconnect_max: Duration,
    /// Machine status interval.
    pub heartbeat: Duration,
    /// Expected ticket audience; `None` means the host of `relay_url`.
    pub audience: Option<String>,
    /// Bearer token added to daemon write requests; `new` reads it from `VOWP_API_TOKEN`. It is
    /// never logged and never sent to the client.
    pub api_token: Option<String>,
}

impl ConnectorOptions {
    /// Options from a `[relay]` table with a system clock and a discarding log.
    pub fn new(home: PathBuf, config: RelayConfig) -> Self {
        Self {
            reconnect_min: Duration::from_secs(config.reconnect_min_secs),
            reconnect_max: Duration::from_secs(config.reconnect_max_secs),
            heartbeat: Duration::from_secs(config.heartbeat_secs),
            home,
            config,
            stop: Arc::new(AtomicBool::new(false)),
            clock: Arc::new(SystemClock),
            log: Arc::new(|_| {}),
            audience: None,
            api_token: std::env::var(API_TOKEN_ENV).ok().filter(|token| !token.is_empty()),
        }
    }
}

/// How one session ended.
enum SessionEnd {
    Stopped,
    Revoked,
    Superseded,
    /// The connection dropped; `online` says whether login had succeeded.
    Dropped {
        online: bool,
        code: &'static str,
    },
}

/// One outbound message from a worker thread to the connection loop.
struct Outbound {
    channel: [u8; 16],
    frame: RemoteFrame,
}

struct Channel {
    id: String,
    cipher: ChannelCipher,
    granted: BTreeSet<Scope>,
    hello: bool,
    streams: HashMap<u32, Arc<AtomicBool>>,
}

impl Channel {
    fn cancel_all(&self) {
        for cancel in self.streams.values() {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}

type Socket = WebSocket<RelayStream>;

/// The machine connector.
pub struct Connector {
    opts: ConnectorOptions,
}

impl Connector {
    /// Creates a connector.
    pub fn new(opts: ConnectorOptions) -> Self {
        Self { opts }
    }

    fn log(&self, line: &str) {
        (self.opts.log)(line);
    }

    fn stopping(&self) -> bool {
        self.opts.stop.load(Ordering::Relaxed)
    }

    fn now(&self) -> Timestamp {
        self.opts.clock.now()
    }

    /// Runs until stopped, revoked or superseded. Returns an error only for a problem a retry
    /// cannot fix: relay disabled, not enrolled, or an unusable key file.
    pub fn run(&self) -> Result<ConnectorReport, RelayError> {
        if !self.opts.config.enabled {
            return Err(RelayError::Config(
                "relay is disabled; set [relay] enabled = true".to_owned(),
            ));
        }
        self.opts
            .config
            .validate()
            .map_err(|error| RelayError::Config(error.to_string()))?;
        let paths = RelayPaths::new(&self.opts.home, &self.opts.config);
        let mut state = Progress::new(&paths, &self.opts.config);
        let mut report = ConnectorReport {
            outcome: ConnectorOutcome::Stopped,
            connects: 0,
            delays: Vec::new(),
        };
        let mut attempt: u32 = 0;
        let mut force_refresh = false;
        let mut jitter = JitterRng::new(self.now().unix_millis() as u64);
        loop {
            if self.stopping() {
                state.write(&self.now(), ConnectorState::Stopped, 0, None);
                return Ok(report);
            }
            state.write(&self.now(), ConnectorState::Connecting, 0, None);
            let mut session = Session::new(self, &paths, &mut state);
            let result = session.run(force_refresh);
            drop(session);
            report.connects = state.connects;
            force_refresh = false;
            let (code, online) = match result {
                Ok(SessionEnd::Stopped) => {
                    report.outcome = ConnectorOutcome::Stopped;
                    state.write(&self.now(), ConnectorState::Stopped, 0, None);
                    return Ok(report);
                }
                Ok(SessionEnd::Revoked) | Err(RelayError::Revoked) => {
                    report.outcome = ConnectorOutcome::Revoked;
                    self.log("relay: revoked; stopping");
                    state.write(&self.now(), ConnectorState::Revoked, 0, Some("revoked"));
                    return Ok(report);
                }
                Ok(SessionEnd::Superseded) => {
                    report.outcome = ConnectorOutcome::Superseded;
                    self.log("relay: superseded by another connection; stopping");
                    state.write(&self.now(), ConnectorState::Superseded, 0, Some("superseded"));
                    return Ok(report);
                }
                Ok(SessionEnd::Dropped { online, code }) => (code, online),
                Err(error @ (RelayError::NotEnrolled(_) | RelayError::Key(_) | RelayError::Config(_))) => {
                    state.write(&self.now(), ConnectorState::Stopped, 0, Some("fatal"));
                    return Err(error);
                }
                Err(RelayError::Auth(_)) => {
                    force_refresh = true;
                    ("unauthenticated", false)
                }
                Err(error) => (error_code(&error), false),
            };
            if online {
                attempt = 0;
            }
            let delay = backoff(self.opts.reconnect_min, self.opts.reconnect_max, attempt, &mut jitter);
            attempt = attempt.saturating_add(1);
            self.log(&format!("relay: dropped ({code}); retry in {}ms", delay.as_millis()));
            state.write(&self.now(), ConnectorState::Backoff, 0, Some(code));
            report.delays.push(delay);
            self.sleep(delay);
        }
    }

    fn sleep(&self, total: Duration) {
        let end = Instant::now() + total;
        while Instant::now() < end && !self.stopping() {
            std::thread::sleep(TICK.min(end.saturating_duration_since(Instant::now())));
        }
    }
}

/// `min * 2^attempt` capped at `max`, plus up to 25 % jitter.
fn backoff(min: Duration, max: Duration, attempt: u32, jitter: &mut JitterRng) -> Duration {
    let base = min.saturating_mul(1u32 << attempt.min(16)).min(max);
    let span = u64::try_from(base.as_millis() / 4).unwrap_or(0);
    let extra = if span == 0 { 0 } else { jitter.next() % (span + 1) };
    base + Duration::from_millis(extra)
}

struct JitterRng(u64);

impl JitterRng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn error_code(error: &RelayError) -> &'static str {
    match error {
        RelayError::Network(_) | RelayError::Io(_) => "network",
        RelayError::Protocol(_) => "protocol",
        RelayError::Api { .. } => "api",
        RelayError::Crypto(_) => "crypto",
        _ => "error",
    }
}

/// Writes `relay-status.json`; failures are ignored because status is best-effort.
struct Progress {
    path: PathBuf,
    machine_id: String,
    relay_url: String,
    connects: u64,
    last_connected_at: Option<String>,
}

impl Progress {
    fn new(paths: &RelayPaths, config: &RelayConfig) -> Self {
        let record = RelayJson::load(&paths.credential()).ok();
        Self {
            path: paths.status(),
            machine_id: record.as_ref().map(|r| r.machine_id.clone()).unwrap_or_default(),
            relay_url: record.map_or_else(|| config.relay_url.clone(), |r| r.relay_url),
            connects: 0,
            last_connected_at: None,
        }
    }

    fn write(&mut self, now: &Timestamp, state: ConnectorState, channels: u32, error: Option<&str>) {
        let _ = RelayStatusFile {
            state,
            pid: std::process::id(),
            machine_id: self.machine_id.clone(),
            relay_url: self.relay_url.clone(),
            channels,
            connects: self.connects,
            last_connected_at: self.last_connected_at.clone(),
            updated_at: now.as_rfc3339(),
            last_error: error.map(str::to_owned),
        }
        .save(&self.path);
    }
}

/// One connection attempt and, if login succeeds, its online loop.
struct Session<'a> {
    connector: &'a Connector,
    paths: &'a RelayPaths,
    progress: &'a mut Progress,
    authenticated: bool,
    jwks: Option<(i64, Jwks)>,
    replay: ReplayCache,
    channels: HashMap<[u8; 16], Channel>,
    outbox: Sender<Outbound>,
    inbox: Receiver<Outbound>,
    record: Option<RelayJson>,
    key: Option<RelayKey>,
    local_scopes: BTreeSet<Scope>,
}

impl<'a> Session<'a> {
    fn new(connector: &'a Connector, paths: &'a RelayPaths, progress: &'a mut Progress) -> Self {
        let (outbox, inbox) = mpsc::channel();
        Self {
            local_scopes: local_scopes(&connector.opts.config),
            connector,
            paths,
            progress,
            authenticated: false,
            jwks: None,
            replay: ReplayCache::default(),
            channels: HashMap::new(),
            outbox,
            inbox,
            record: None,
            key: None,
        }
    }

    fn config(&self) -> &RelayConfig {
        &self.connector.opts.config
    }

    fn now(&self) -> Timestamp {
        self.connector.now()
    }

    fn run(&mut self, force_refresh: bool) -> Result<SessionEnd, RelayError> {
        let key = RelayKey::load(&self.paths.home, &self.paths.key_file)?;
        let mut record = RelayJson::load(&self.paths.credential())?;
        if key.key_id() != record.key_id {
            return Err(RelayError::Key("relay.key does not match relay.json".to_owned()));
        }
        // Refresh the credential before it expires (or when the relay refused it).
        let now = self.now();
        let expiring = CredentialClaims::decode(&record.credential)
            .map(|claims| claims.expires_within(now.unix_secs(), CREDENTIAL_REFRESH_MARGIN_SECS))
            .unwrap_or(true);
        if expiring || force_refresh {
            match refresh_credential(self.paths, now) {
                Ok(updated) => {
                    self.connector.log("relay: credential refreshed");
                    record = updated;
                }
                Err(RelayError::Revoked) => return Err(RelayError::Revoked),
                Err(error) => {
                    // Keep going with a still-valid credential; otherwise this attempt fails.
                    let still_valid = CredentialClaims::decode(&record.credential)
                        .is_ok_and(|claims| !claims.expires_within(now.unix_secs(), 0));
                    if !still_valid || force_refresh {
                        return Err(error);
                    }
                }
            }
        }
        self.record = Some(record);
        self.key = Some(key);
        let mut socket = self.open_socket()?;
        let end = self.login_and_serve(&mut socket);
        for channel in self.channels.values() {
            channel.cancel_all();
        }
        if matches!(end, Ok(SessionEnd::Stopped)) {
            let _ = socket.close(Some(CloseFrame {
                code: CloseCode::Normal,
                reason: "stopping".into(),
            }));
            let _ = socket.flush();
        }
        end
    }

    fn open_socket(&self) -> Result<Socket, RelayError> {
        let record = self
            .record
            .as_ref()
            .ok_or_else(|| RelayError::Config("not loaded".to_owned()))?;
        let endpoint = Endpoint::parse(&record.relay_url)?;
        if !endpoint.tls && !endpoint.is_loopback() {
            return Err(RelayError::Config("a non-loopback relay URL must be wss".to_owned()));
        }
        let stream = endpoint.connect(Duration::from_secs(vowp_protocol::RELAY_HANDSHAKE_SECS))?;
        let mut request = record
            .relay_url
            .as_str()
            .into_client_request()
            .map_err(|_| RelayError::Config("the relay URL is invalid".to_owned()))?;
        request
            .headers_mut()
            .insert("Sec-WebSocket-Protocol", HeaderValue::from_static(RELAY_SUBPROTOCOL));
        let mut ws_config = WebSocketConfig::default();
        ws_config.max_message_size = Some(MAX_RELAY_FRAME_BYTES);
        ws_config.max_frame_size = Some(MAX_RELAY_FRAME_BYTES);
        let (socket, response) = tungstenite::client::client_with_config(request, stream, Some(ws_config))
            .map_err(|_| RelayError::Network("websocket handshake failed".to_owned()))?;
        if response
            .headers()
            .get("Sec-WebSocket-Protocol")
            .and_then(|value| value.to_str().ok())
            != Some(RELAY_SUBPROTOCOL)
        {
            return Err(RelayError::Protocol(
                "the relay did not select stackspirit-relay/1".to_owned(),
            ));
        }
        Ok(socket)
    }

    fn send_control(&self, socket: &mut Socket, frame: &RelayFrame) -> Result<(), RelayError> {
        let text = frame
            .to_json()
            .map_err(|error| RelayError::Protocol(error.to_string()))?;
        socket
            .send(Message::text(text))
            .map_err(|_| RelayError::Network("send failed".to_owned()))
    }

    /// Reads one text control frame during login; a timeout or close is an error.
    fn read_login_frame(&self, socket: &mut Socket) -> Result<RelayFrame, RelayError> {
        let deadline = Instant::now() + Duration::from_secs(vowp_protocol::RELAY_HANDSHAKE_SECS);
        loop {
            match socket.read() {
                Ok(Message::Text(text)) => {
                    return RelayFrame::from_json(text.as_str())
                        .map_err(|error| RelayError::Protocol(error.to_string()));
                }
                Ok(Message::Close(frame)) => {
                    let code = frame.map(|frame| u16::from(frame.code)).unwrap_or(0);
                    return Err(close_error(code));
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(error)) if super::is_idle(&error) => {
                    if Instant::now() >= deadline {
                        return Err(RelayError::Network("login timed out".to_owned()));
                    }
                }
                Err(_) => return Err(RelayError::Network("connection closed during login".to_owned())),
            }
        }
    }

    fn login_and_serve(&mut self, socket: &mut Socket) -> Result<SessionEnd, RelayError> {
        let (Some(record), Some(key)) = (self.record.clone(), self.key.clone()) else {
            return Err(RelayError::Config("not loaded".to_owned()));
        };
        socket
            .get_ref()
            .set_read_timeout(Some(Duration::from_millis(500)))
            .map_err(RelayError::Io)?;
        self.connector.log("relay: connected, authenticating");
        self.send_control(
            socket,
            &RelayFrame::MachineHello {
                v: RELAY_FRAME_VERSION,
                machine_id: record.machine_id.clone(),
                account_id: record.account_id.clone(),
                key_id: record.key_id.clone(),
                public_key: key.public_key_hex(),
                credential: record.credential.clone(),
                agent_version: RELAY_AGENT_VERSION.to_owned(),
                capabilities: capabilities_for(self.config()),
                sent_at: self.now(),
            },
        )?;
        let RelayFrame::RelayChallenge {
            relay_id,
            session_id,
            nonce,
            server_time,
            ..
        } = self.expect_login(socket)?
        else {
            return Err(RelayError::Protocol("expected relay.challenge".to_owned()));
        };
        let canonical = auth::machine_auth_canonical(
            &record.machine_id,
            &record.account_id,
            &record.key_id,
            &relay_id,
            &session_id,
            &nonce,
            &server_time.as_rfc3339(),
        );
        self.send_control(
            socket,
            &RelayFrame::MachineAuth {
                v: RELAY_FRAME_VERSION,
                signature: auth::sign_machine_auth(&key, &canonical),
            },
        )?;
        let RelayFrame::RelayWelcome {
            heartbeat_secs,
            session_id,
            ..
        } = self.expect_login(socket)?
        else {
            return Err(RelayError::Protocol("expected relay.welcome".to_owned()));
        };
        self.authenticated = true;
        self.progress.connects += 1;
        self.progress.last_connected_at = Some(self.now().as_rfc3339());
        self.connector.log(&format!("relay: online session={session_id}"));
        let heartbeat = if heartbeat_secs == 0 {
            self.connector.opts.heartbeat
        } else {
            self.connector.opts.heartbeat.min(Duration::from_secs(heartbeat_secs))
        };
        socket.get_ref().set_read_timeout(Some(TICK)).map_err(RelayError::Io)?;
        self.serve(socket, heartbeat)
    }

    /// Reads a login frame and turns an `error` or `superseded` frame into the right failure.
    fn expect_login(&self, socket: &mut Socket) -> Result<RelayFrame, RelayError> {
        match self.read_login_frame(socket)? {
            RelayFrame::Error { code, .. } if code == codes::REVOKED => Err(RelayError::Revoked),
            RelayFrame::Error { code, .. } if code == codes::UNAUTHENTICATED => Err(RelayError::Auth(
                "the relay refused the credential or signature".to_owned(),
            )),
            RelayFrame::Error { .. } => Err(RelayError::Protocol("the relay refused the login".to_owned())),
            other => Ok(other),
        }
    }

    fn serve(&mut self, socket: &mut Socket, heartbeat: Duration) -> Result<SessionEnd, RelayError> {
        let mut last_status = Instant::now() - heartbeat;
        let mut last_rx = Instant::now();
        loop {
            if self.connector.stopping() {
                return Ok(SessionEnd::Stopped);
            }
            self.drain_outbox(socket)?;
            if last_status.elapsed() >= heartbeat {
                self.send_status(socket)?;
                last_status = Instant::now();
                self.refresh_if_expiring();
            }
            if last_rx.elapsed() > heartbeat.saturating_mul(3).max(Duration::from_secs(3)) {
                return Ok(SessionEnd::Dropped {
                    online: true,
                    code: "heartbeat",
                });
            }
            match socket.read() {
                Ok(Message::Text(text)) => {
                    last_rx = Instant::now();
                    match RelayFrame::from_json(text.as_str()) {
                        Ok(frame) => {
                            if let Some(end) = self.on_control(socket, frame)? {
                                return Ok(end);
                            }
                        }
                        Err(_) => self.connector.log("relay: ignored a malformed control frame"),
                    }
                }
                Ok(Message::Binary(bytes)) => {
                    last_rx = Instant::now();
                    self.on_data(socket, &bytes)?;
                }
                Ok(Message::Close(frame)) => {
                    let code = frame.map(|frame| u16::from(frame.code)).unwrap_or(0);
                    return Ok(match code {
                        vowp_protocol::CLOSE_SUPERSEDED => SessionEnd::Superseded,
                        _ => SessionEnd::Dropped {
                            online: true,
                            code: "closed",
                        },
                    });
                }
                Ok(_) => last_rx = Instant::now(),
                Err(tungstenite::Error::Io(error)) if super::is_idle(&error) => {}
                Err(tungstenite::Error::Capacity(_)) => {
                    let _ = socket.close(Some(CloseFrame {
                        code: CloseCode::Size,
                        reason: "frame too large".into(),
                    }));
                    let _ = CLOSE_TOO_BIG;
                    return Ok(SessionEnd::Dropped {
                        online: true,
                        code: "too_large",
                    });
                }
                Err(_) => {
                    return Ok(SessionEnd::Dropped {
                        online: true,
                        code: "closed",
                    });
                }
            }
        }
    }

    fn send_status(&mut self, socket: &mut Socket) -> Result<(), RelayError> {
        let channels = u32::try_from(self.channels.len()).unwrap_or(u32::MAX);
        self.send_control(
            socket,
            &RelayFrame::MachineStatus {
                v: RELAY_FRAME_VERSION,
                state: vowp_protocol::MachineState::Online,
                channels,
                sent_at: self.now(),
            },
        )?;
        let now = self.now();
        self.progress.write(&now, ConnectorState::Online, channels, None);
        Ok(())
    }

    /// Refreshes the stored credential in the background of an online session so the next
    /// reconnect does not start with an expired one. A failure is retried on the next heartbeat.
    fn refresh_if_expiring(&mut self) {
        let Some(record) = &self.record else { return };
        let now = self.now();
        let expiring = CredentialClaims::decode(&record.credential)
            .map(|claims| claims.expires_within(now.unix_secs(), CREDENTIAL_REFRESH_MARGIN_SECS))
            .unwrap_or(true);
        if expiring && let Ok(updated) = refresh_credential(self.paths, now) {
            self.connector.log("relay: credential refreshed");
            self.record = Some(updated);
        }
    }

    fn on_control(&mut self, socket: &mut Socket, frame: RelayFrame) -> Result<Option<SessionEnd>, RelayError> {
        match frame {
            RelayFrame::Ping { nonce, .. } => {
                self.send_control(
                    socket,
                    &RelayFrame::Pong {
                        v: RELAY_FRAME_VERSION,
                        nonce,
                    },
                )?;
            }
            RelayFrame::Superseded { .. } => return Ok(Some(SessionEnd::Superseded)),
            RelayFrame::Error { code, .. } if code == codes::REVOKED => return Ok(Some(SessionEnd::Revoked)),
            RelayFrame::Error { code, .. } => {
                self.connector
                    .log(&format!("relay: error frame ({})", safe_code(&code)));
            }
            RelayFrame::ChannelOpen {
                channel_id,
                machine_id,
                kind,
                ticket,
                scopes,
                client_key,
                client_nonce,
                ..
            } => self.on_channel_open(
                socket,
                channel_id,
                machine_id,
                kind,
                ticket,
                scopes,
                client_key,
                client_nonce,
            )?,
            RelayFrame::ChannelClose { channel_id, .. } => {
                if let Some(bytes) = RelayDataFrame::channel_bytes(&channel_id)
                    && let Some(channel) = self.channels.remove(&bytes)
                {
                    channel.cancel_all();
                    self.connector.log(&format!("relay: channel closed {}", channel.id));
                }
            }
            _ => self.connector.log("relay: ignored an unexpected control frame"),
        }
        Ok(None)
    }

    fn reject(
        &self,
        socket: &mut Socket,
        channel_id: &str,
        code: &str,
        message: &str,
        retryable: bool,
    ) -> Result<(), RelayError> {
        self.connector
            .log(&format!("relay: channel rejected {channel_id} ({code})"));
        self.send_control(
            socket,
            &RelayFrame::ChannelReject {
                v: RELAY_FRAME_VERSION,
                channel_id: channel_id.to_owned(),
                code: code.to_owned(),
                message: message.to_owned(),
                retryable,
            },
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn on_channel_open(
        &mut self,
        socket: &mut Socket,
        channel_id: String,
        machine_id: String,
        kind: vowp_protocol::ClientKind,
        ticket: String,
        scopes: Vec<Scope>,
        client_key: Jwk,
        client_nonce: String,
    ) -> Result<(), RelayError> {
        let (Some(record), Some(key)) = (self.record.clone(), self.key.clone()) else {
            return Err(RelayError::Config("not loaded".to_owned()));
        };
        let Some(channel_bytes) = RelayDataFrame::channel_bytes(&channel_id) else {
            return self.reject(socket, &channel_id, codes::PROTOCOL, "bad channel id", false);
        };
        if self.channels.contains_key(&channel_bytes) {
            return self.reject(socket, &channel_id, codes::PROTOCOL, "channel exists", false);
        }
        if self.channels.len() >= self.config().max_channels as usize {
            return self.reject(socket, &channel_id, codes::OVERLOADED, "too many channels", true);
        }
        if machine_id != record.machine_id {
            return self.reject(socket, &channel_id, codes::TICKET_INVALID, "wrong machine", false);
        }
        if public_from_jwk(&client_key).is_err() {
            return self.reject(socket, &channel_id, codes::CRYPTO_ERROR, "invalid client key", false);
        }
        if scopes.is_empty() {
            return self.reject(socket, &channel_id, codes::BAD_REQUEST, "no scopes requested", false);
        }
        let now = self.now();
        let claims = match self.verify_ticket(&ticket, &record, &key, &client_key, now.unix_secs()) {
            Ok(claims) => claims,
            Err(error) => return self.reject(socket, &channel_id, error.code, &error.message, false),
        };
        if claims.kind != kind {
            return self.reject(
                socket,
                &channel_id,
                codes::TICKET_INVALID,
                "client kind mismatch",
                false,
            );
        }
        let ticket_scopes: BTreeSet<Scope> = claims.scopes.iter().copied().collect();
        let requested: BTreeSet<Scope> = scopes.into_iter().collect();
        if !requested.is_subset(&ticket_scopes) || !requested.is_subset(&self.local_scopes) {
            return self.reject(socket, &channel_id, codes::SCOPE_DENIED, "scope not permitted", false);
        }
        let accept = match machine_accept(
            &AcceptInput {
                channel_id: &channel_id,
                client_key: &client_key,
                client_nonce: &client_nonce,
                machine_id: &record.machine_id,
                account_id: &record.account_id,
                ticket_jti: &claims.jti,
            },
            &key,
        ) {
            Ok(accept) => accept,
            Err(_) => return self.reject(socket, &channel_id, codes::CRYPTO_ERROR, "handshake failed", false),
        };
        self.send_control(
            socket,
            &RelayFrame::ChannelAccept {
                v: RELAY_FRAME_VERSION,
                channel_id: channel_id.clone(),
                machine_key: accept.machine_key,
                machine_nonce: accept.machine_nonce,
                machine_key_id: record.key_id.clone(),
                machine_signature: accept.machine_signature,
                suite: E2E_SUITE.to_owned(),
            },
        )?;
        self.connector.log(&format!("relay: channel accepted {channel_id}"));
        self.channels.insert(
            channel_bytes,
            Channel {
                id: channel_id,
                cipher: accept.cipher,
                granted: requested,
                hello: false,
                streams: HashMap::new(),
            },
        );
        Ok(())
    }

    fn verify_ticket(
        &mut self,
        ticket: &str,
        record: &RelayJson,
        key: &RelayKey,
        client_key: &Jwk,
        now_unix: i64,
    ) -> Result<auth::TicketClaims, auth::TicketError> {
        let issuer = record.api_url.trim_end_matches('/').to_owned();
        let audience = self
            .connector
            .opts
            .audience
            .clone()
            .or_else(|| Endpoint::parse(&record.relay_url).ok().map(|endpoint| endpoint.host))
            .unwrap_or_default();
        let key_id = key.key_id();
        let mkey = key.public_key_hex();
        let no_keys = || auth::TicketError {
            code: codes::TICKET_INVALID,
            message: "no verified signing keys".to_owned(),
        };
        for force in [false, true] {
            let jwks = self.jwks(record, now_unix, force).ok_or_else(no_keys)?;
            let ctx = TicketContext {
                jwks: &jwks,
                issuer: &issuer,
                audience: &audience,
                machine_id: &record.machine_id,
                account_id: &record.account_id,
                key_id: &key_id,
                mkey: &mkey,
                client_key,
                now_unix,
            };
            match auth::verify_ticket(ticket, &ctx, &mut self.replay) {
                // A bad signature or unknown kid may mean the API rotated its keys: refetch once.
                Err(error) if error.code == codes::TICKET_INVALID && !force => continue,
                other => return other,
            }
        }
        Err(no_keys())
    }

    /// The API signing keys: memory, then the 24-hour cache file, then the API. `force` refetches
    /// (at most every 30 seconds). There is no fallback that trusts an unverified key.
    fn jwks(&mut self, record: &RelayJson, now_unix: i64, force: bool) -> Option<Jwks> {
        if !force && let Some((_, jwks)) = &self.jwks {
            return Some(jwks.clone());
        }
        if !force && let Some(cached) = CachedJwks::load_fresh(&self.paths.jwks(), now_unix) {
            self.jwks = Some((cached.fetched_at, cached.jwks.clone()));
            return Some(cached.jwks);
        }
        if force
            && let Some((fetched, jwks)) = &self.jwks
            && now_unix - *fetched < JWKS_REFETCH_SECS
        {
            return Some(jwks.clone());
        }
        let fetched = fetch_jwks(&record.api_url).ok()?;
        let _ = CachedJwks {
            fetched_at: now_unix,
            jwks: fetched.clone(),
        }
        .save(&self.paths.jwks());
        self.jwks = Some((now_unix, fetched.clone()));
        Some(fetched)
    }

    fn on_data(&mut self, socket: &mut Socket, bytes: &[u8]) -> Result<(), RelayError> {
        let frame = match RelayDataFrame::from_bytes(bytes) {
            Ok(frame) => frame,
            Err(_) => {
                // A malformed header names no trustworthy channel; there is nothing to close.
                self.connector.log("relay: dropped a malformed data frame");
                return Ok(());
            }
        };
        let Some(channel) = self.channels.get_mut(&frame.channel_id) else {
            return self.send_control(
                socket,
                &RelayFrame::ChannelClose {
                    v: RELAY_FRAME_VERSION,
                    channel_id: RelayDataFrame::channel_string(frame.channel_id),
                    code: codes::CRYPTO_ERROR.to_owned(),
                    message: "unknown channel".to_owned(),
                },
            );
        };
        let plaintext = match channel.cipher.open(&frame) {
            Ok(plaintext) => plaintext,
            Err(_) => return self.close_channel(socket, frame.channel_id, codes::CRYPTO_ERROR),
        };
        let remote = match RemoteFrame::from_json(&plaintext) {
            Ok(remote) => remote,
            Err(_) => return self.app_close(socket, frame.channel_id, codes::PROTOCOL, "malformed frame"),
        };
        self.on_remote(socket, frame.channel_id, remote)
    }

    /// Closes a channel after a crypto failure: no plaintext fallback, just an operational close.
    fn close_channel(&mut self, socket: &mut Socket, id: [u8; 16], code: &str) -> Result<(), RelayError> {
        let channel_id = RelayDataFrame::channel_string(id);
        if let Some(channel) = self.channels.remove(&id) {
            channel.cancel_all();
        }
        self.connector
            .log(&format!("relay: channel failed {channel_id} ({code})"));
        self.send_control(
            socket,
            &RelayFrame::ChannelClose {
                v: RELAY_FRAME_VERSION,
                channel_id,
                code: code.to_owned(),
                message: "channel closed".to_owned(),
            },
        )
    }

    /// Sends an authenticated application `Close` and drops the channel.
    fn app_close(&mut self, socket: &mut Socket, id: [u8; 16], code: &str, message: &str) -> Result<(), RelayError> {
        self.send_remote(
            socket,
            id,
            &RemoteFrame::Close {
                code: code.to_owned(),
                message: message.to_owned(),
            },
        )?;
        if let Some(channel) = self.channels.remove(&id) {
            channel.cancel_all();
        }
        Ok(())
    }

    fn send_remote(&mut self, socket: &mut Socket, id: [u8; 16], frame: &RemoteFrame) -> Result<(), RelayError> {
        let Some(channel) = self.channels.get_mut(&id) else {
            return Ok(());
        };
        let plaintext = frame
            .to_json()
            .map_err(|error| RelayError::Protocol(error.to_string()))?;
        let data = channel.cipher.seal(&plaintext)?;
        let bytes = data
            .to_bytes()
            .map_err(|error| RelayError::Protocol(error.to_string()))?;
        socket
            .send(Message::binary(bytes))
            .map_err(|_| RelayError::Network("send failed".to_owned()))
    }

    fn drain_outbox(&mut self, socket: &mut Socket) -> Result<(), RelayError> {
        while let Ok(out) = self.inbox.try_recv() {
            let stream = out.frame.stream();
            let ended = matches!(
                out.frame,
                RemoteFrame::HttpBody { end: true, .. } | RemoteFrame::StreamError { .. }
            );
            self.send_remote(socket, out.channel, &out.frame)?;
            if ended
                && let Some(stream) = stream
                && let Some(channel) = self.channels.get_mut(&out.channel)
            {
                channel.streams.remove(&stream);
            }
        }
        Ok(())
    }

    fn stream_error(
        &mut self,
        socket: &mut Socket,
        id: [u8; 16],
        stream: u32,
        code: &str,
        message: &str,
        retryable: bool,
    ) -> Result<(), RelayError> {
        self.send_remote(
            socket,
            id,
            &RemoteFrame::StreamError {
                stream,
                code: code.to_owned(),
                message: message.to_owned(),
                retryable,
            },
        )
    }

    fn on_remote(&mut self, socket: &mut Socket, id: [u8; 16], frame: RemoteFrame) -> Result<(), RelayError> {
        let Some(channel) = self.channels.get_mut(&id) else {
            return Ok(());
        };
        if !channel.hello {
            return match frame {
                RemoteFrame::Hello { v, machine, scopes, .. } => {
                    let machine_ok = self.record.as_ref().is_some_and(|record| record.machine_id == machine);
                    let requested: BTreeSet<Scope> = scopes.into_iter().collect();
                    let channel = self
                        .channels
                        .get_mut(&id)
                        .ok_or_else(|| RelayError::Protocol("channel".to_owned()))?;
                    if v != vowp_protocol::REMOTE_FRAME_VERSION || !machine_ok {
                        return self.app_close(socket, id, codes::PROTOCOL, "bad hello");
                    }
                    // The machine rejects a requested scope it did not enable before any stream.
                    if !requested.is_subset(&channel.granted) {
                        return self.app_close(socket, id, codes::SCOPE_DENIED, "scope not permitted");
                    }
                    if !requested.is_empty() {
                        channel.granted = requested;
                    }
                    channel.hello = true;
                    let granted: Vec<Scope> = channel.granted.iter().copied().collect();
                    let capabilities = capability_names(&channel.granted);
                    let machine = self.record.as_ref().map(|r| r.machine_id.clone()).unwrap_or_default();
                    self.send_remote(
                        socket,
                        id,
                        &RemoteFrame::HelloAck {
                            v: vowp_protocol::REMOTE_FRAME_VERSION,
                            machine,
                            scopes: granted,
                            capabilities,
                        },
                    )
                }
                _ => self.app_close(socket, id, codes::PROTOCOL, "hello required"),
            };
        }
        match frame {
            RemoteFrame::Ping { nonce } => self.send_remote(socket, id, &RemoteFrame::Pong { nonce }),
            RemoteFrame::Pong { .. }
            | RemoteFrame::McpCancel { .. }
            | RemoteFrame::TcpData { .. }
            | RemoteFrame::TcpClose { .. } => Ok(()),
            RemoteFrame::ExecData { .. } | RemoteFrame::ExecResize { .. } => Ok(()),
            RemoteFrame::Close { .. } => {
                if let Some(channel) = self.channels.remove(&id) {
                    channel.cancel_all();
                }
                Ok(())
            }
            RemoteFrame::HttpCancel { stream } => {
                if let Some(channel) = self.channels.get_mut(&id)
                    && let Some(cancel) = channel.streams.remove(&stream)
                {
                    cancel.store(true, Ordering::Relaxed);
                }
                Ok(())
            }
            RemoteFrame::HttpRequest {
                stream,
                method,
                path,
                headers,
                body,
            } => self.on_http_request(socket, id, stream, method, path, headers, body),
            RemoteFrame::McpRequest { stream, .. } => {
                self.deny_or_unimplemented(socket, id, stream, &[Scope::McpRead, Scope::McpWrite], "mcp")
            }
            RemoteFrame::TcpOpen { stream, .. } => self.deny_or_unimplemented(socket, id, stream, &[Scope::Ssh], "tcp"),
            RemoteFrame::ExecStart { stream, .. } => {
                self.deny_or_unimplemented(socket, id, stream, &[Scope::Exec], "exec")
            }
            RemoteFrame::Hello { .. } => self.app_close(socket, id, codes::PROTOCOL, "duplicate hello"),
            _ => self.app_close(socket, id, codes::PROTOCOL, "unexpected frame"),
        }
    }

    /// An MCP, TCP or exec stream: `scope_denied` unless the channel holds one of `any_of`, and
    /// then a typed `not_implemented` until slice 6 wires the channel.
    fn deny_or_unimplemented(
        &mut self,
        socket: &mut Socket,
        id: [u8; 16],
        stream: u32,
        any_of: &[Scope],
        what: &str,
    ) -> Result<(), RelayError> {
        if !valid_stream_id(stream) {
            return self.app_close(socket, id, codes::PROTOCOL, "bad stream id");
        }
        let allowed = self
            .channels
            .get(&id)
            .is_some_and(|channel| any_of.iter().any(|scope| channel.granted.contains(scope)));
        if allowed {
            self.stream_error(
                socket,
                id,
                stream,
                codes::NOT_IMPLEMENTED,
                &format!("{what} streams are not wired yet"),
                false,
            )
        } else {
            self.stream_error(socket, id, stream, codes::SCOPE_DENIED, "scope not permitted", false)
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn on_http_request(
        &mut self,
        socket: &mut Socket,
        id: [u8; 16],
        stream: u32,
        method: String,
        path: String,
        headers: BTreeMap<String, String>,
        body: String,
    ) -> Result<(), RelayError> {
        if !valid_stream_id(stream) {
            return self.app_close(socket, id, codes::PROTOCOL, "bad stream id");
        }
        let needed = if method == "POST" {
            Scope::HttpWrite
        } else {
            Scope::HttpRead
        };
        let Some(channel) = self.channels.get(&id) else {
            return Ok(());
        };
        if !channel.granted.contains(&needed) {
            return self.stream_error(socket, id, stream, codes::SCOPE_DENIED, "scope not permitted", false);
        }
        if channel.streams.contains_key(&stream) {
            return self.stream_error(socket, id, stream, codes::BAD_REQUEST, "stream in use", false);
        }
        if channel.streams.len() >= MAX_STREAMS_PER_CHANNEL {
            return self.stream_error(socket, id, stream, codes::OVERLOADED, "too many streams", true);
        }
        if let Err(message) = validate_http_request(&method, &path, &headers) {
            return self.stream_error(socket, id, stream, codes::BAD_REQUEST, &message, false);
        }
        let body = match BASE64.decode(body.as_bytes()) {
            Ok(body) if body.len() <= HTTP_REQUEST_BODY_LIMIT => body,
            _ => return self.stream_error(socket, id, stream, codes::BAD_REQUEST, "bad request body", false),
        };
        if method == "GET" && !body.is_empty() {
            return self.stream_error(socket, id, stream, codes::BAD_REQUEST, "a GET has no body", false);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        if let Some(channel) = self.channels.get_mut(&id) {
            channel.streams.insert(stream, Arc::clone(&cancel));
        }
        let job = HttpJob {
            home: self.paths.home.clone(),
            daemon_url: self.config().daemon_url.clone(),
            out: self.outbox.clone(),
            channel: id,
            stream,
            method,
            path,
            headers,
            body,
            cancel,
            stop: Arc::clone(&self.connector.opts.stop),
            api_token: self.connector.opts.api_token.clone(),
        };
        std::thread::spawn(move || job.run());
        Ok(())
    }
}

fn close_error(code: u16) -> RelayError {
    match code {
        vowp_protocol::CLOSE_UNAUTHENTICATED => RelayError::Auth("the relay closed with 4001".to_owned()),
        vowp_protocol::CLOSE_SUPERSEDED => RelayError::Network("superseded".to_owned()),
        c if c == CLOSE_CRYPTO || c == CLOSE_NORMAL => {
            RelayError::Network("the relay closed the connection".to_owned())
        }
        _ => RelayError::Network("the relay closed the connection".to_owned()),
    }
}

fn safe_code(code: &str) -> String {
    code.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
        .take(32)
        .collect()
}

/// The scopes the owner enabled in `[relay]`.
pub fn local_scopes(config: &RelayConfig) -> BTreeSet<Scope> {
    let mut scopes = BTreeSet::new();
    for (enabled, scope) in [
        (config.allow_http_read, Scope::HttpRead),
        (config.allow_http_write, Scope::HttpWrite),
        (config.allow_mcp_read, Scope::McpRead),
        (config.allow_mcp_write, Scope::McpWrite),
        (config.allow_ssh, Scope::Ssh),
        (config.allow_exec, Scope::Exec),
    ] {
        if enabled {
            scopes.insert(scope);
        }
    }
    scopes
}

fn capability_names(scopes: &BTreeSet<Scope>) -> Vec<String> {
    let mut names = Vec::new();
    for (name, any_of) in [
        ("http", &[Scope::HttpRead, Scope::HttpWrite][..]),
        ("mcp", &[Scope::McpRead, Scope::McpWrite][..]),
        ("ssh", &[Scope::Ssh][..]),
        ("exec", &[Scope::Exec][..]),
    ] {
        if any_of.iter().any(|scope| scopes.contains(scope)) {
            names.push(name.to_owned());
        }
    }
    names
}

// ----------------------------------------------------------------------------------------------
// HTTP request policy (§6.6) and the loopback proxy
// ----------------------------------------------------------------------------------------------

const ALLOWED_REQUEST_HEADERS: [&str; 4] = ["accept", "content-type", "last-event-id", "if-none-match"];
const ALLOWED_RESPONSE_HEADERS: [&str; 6] = [
    "content-type",
    "cache-control",
    "etag",
    "last-modified",
    "location",
    "retry-after",
];
const DENIED_QUERY_KEYS: [&str; 5] = ["token", "key", "secret", "auth", "code"];
const WRITE_ROUTES: [&str; 10] = [
    "/api/v1/inbox/{id}/answer",
    "/api/v1/sessions/{id}/switch",
    "/api/v1/sessions/import/asif",
    "/api/v1/jobs/{id}/retry",
    "/api/v1/jobs/{id}/park",
    "/api/v1/jobs/{id}/cancel",
    "/api/v1/jobs/{id}/nudge",
    "/api/v1/jobs/{id}/restart",
    "/api/v1/jobs/{id}/replan",
    "/api/v1/jobs/{id}/fail",
];

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(value) = text
                .get(index + 1..index + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(value);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn route_matches(pattern: &str, path: &str) -> bool {
    let want: Vec<&str> = pattern.split('/').collect();
    let have: Vec<&str> = path.split('/').collect();
    want.len() == have.len()
        && want.iter().zip(&have).all(|(want, have)| {
            if *want == "{id}" {
                !have.is_empty()
                    && have
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            } else {
                want == have
            }
        })
}

/// Checks an `HttpRequest` against §6.6 before anything reaches the daemon.
///
/// Rejects a path outside `/api/v1/`, `..`, `//`, `\`, an encoded dot or separator, NUL, CR/LF and
/// other control characters; a query over 2 KiB or with a `token`, `key`, `secret`, `auth` or
/// `code` key; a method other than `GET` and `POST`; a `POST` outside the ten write routes; and any
/// request header other than `Accept`, `Content-Type`, `Last-Event-ID` and `If-None-Match`.
/// The message is path-free.
pub fn validate_http_request(method: &str, path: &str, headers: &BTreeMap<String, String>) -> Result<(), String> {
    if method != "GET" && method != "POST" {
        return Err("method not allowed".to_owned());
    }
    if path
        .chars()
        .any(|c| c.is_control() || c.is_whitespace() || c == '\\' || c == '#')
    {
        return Err("path has a forbidden character".to_owned());
    }
    let (route, query) = match path.split_once('?') {
        Some((route, query)) => (route, Some(query)),
        None => (path, None),
    };
    let lower = route.to_ascii_lowercase();
    if !route.starts_with("/api/v1/") || route.len() == "/api/v1/".len() {
        return Err("path must be under /api/v1/".to_owned());
    }
    if route.contains("..")
        || route.contains("//")
        || ["%2f", "%5c", "%2e", "%00", "%0a", "%0d"]
            .iter()
            .any(|bad| lower.contains(bad))
    {
        return Err("path is not normalised".to_owned());
    }
    if let Some(query) = query {
        if query.len() > 2048 {
            return Err("query is too long".to_owned());
        }
        for pair in query.split('&') {
            let key = percent_decode(pair.split('=').next().unwrap_or("")).to_ascii_lowercase();
            if DENIED_QUERY_KEYS.contains(&key.as_str()) {
                return Err("query key is not allowed".to_owned());
            }
        }
    }
    if method == "POST" && (query.is_some() || !WRITE_ROUTES.iter().any(|pattern| route_matches(pattern, route))) {
        return Err("POST is only allowed on the write routes".to_owned());
    }
    for (name, value) in headers {
        if !ALLOWED_REQUEST_HEADERS.contains(&name.to_ascii_lowercase().as_str()) {
            return Err("header not allowed".to_owned());
        }
        if value.len() > 1024 || value.chars().any(char::is_control) {
            return Err("header value not allowed".to_owned());
        }
    }
    Ok(())
}

struct HttpJob {
    home: PathBuf,
    daemon_url: String,
    out: Sender<Outbound>,
    channel: [u8; 16],
    stream: u32,
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
    cancel: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    api_token: Option<String>,
}

impl HttpJob {
    fn send(&self, frame: RemoteFrame) -> bool {
        self.out
            .send(Outbound {
                channel: self.channel,
                frame,
            })
            .is_ok()
    }

    fn fail(&self, code: &str, message: &str, retryable: bool) {
        self.send(RemoteFrame::StreamError {
            stream: self.stream,
            code: code.to_owned(),
            message: message.to_owned(),
            retryable,
        });
    }

    fn halted(&self) -> bool {
        self.cancel.load(Ordering::Relaxed) || self.stop.load(Ordering::Relaxed)
    }

    /// The loopback daemon: `[relay].daemon_url`, else `<home>/web-window`. Never starts one.
    fn daemon(&self) -> Option<Endpoint> {
        let url = if self.daemon_url.is_empty() {
            std::fs::read_to_string(self.home.join("web-window"))
                .ok()?
                .trim()
                .to_owned()
        } else {
            self.daemon_url.clone()
        };
        let endpoint = Endpoint::parse(&url).ok()?;
        (!endpoint.tls && endpoint.is_loopback()).then_some(endpoint)
    }

    fn run(self) {
        let Some(endpoint) = self.daemon() else {
            return self.fail(codes::DAEMON_UNAVAILABLE, "the local daemon is not running", true);
        };
        let Ok(mut stream) = endpoint.connect(Duration::from_secs(5)) else {
            return self.fail(codes::DAEMON_UNAVAILABLE, "the local daemon is not reachable", true);
        };
        let mut headers: Vec<(String, String)> = self
            .headers
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        if self.method == "POST"
            && let Some(token) = &self.api_token
        {
            headers.push(("Authorization".to_owned(), format!("Bearer {token}")));
        }
        if write_request(
            &mut stream,
            &self.method,
            &endpoint.host_header(),
            &self.path,
            &headers,
            &self.body,
        )
        .is_err()
        {
            return self.fail(
                codes::DAEMON_UNAVAILABLE,
                "the local daemon closed the connection",
                true,
            );
        }
        let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
        let mut reader = BufReader::new(stream);
        let started = Instant::now();
        let head_wait = Duration::from_secs(30);
        let Ok(head) = read_head(&mut reader, &mut || !self.halted() && started.elapsed() < head_wait) else {
            if !self.halted() {
                self.fail(codes::DAEMON_UNAVAILABLE, "the local daemon gave no response", true);
            }
            return;
        };
        let response_headers: BTreeMap<String, String> = head
            .headers
            .iter()
            .filter(|(name, _)| ALLOWED_RESPONSE_HEADERS.contains(&name.as_str()))
            .map(|(name, value)| (canonical_header(name), value.clone()))
            .collect();
        let sse = head
            .header("content-type")
            .is_some_and(|value| value.to_ascii_lowercase().starts_with("text/event-stream"));
        if !self.send(RemoteFrame::HttpResponse {
            stream: self.stream,
            status: head.status,
            headers: response_headers,
        }) {
            return;
        }
        let limit = if sse { usize::MAX } else { HTTP_RESPONSE_LIMIT };
        let deadline = if sse { SSE_LIMIT } else { Duration::from_secs(120) };
        let mut sent_ok = true;
        let result = read_body(
            &mut reader,
            &head,
            limit,
            &mut |chunk| {
                for piece in chunk.chunks(HTTP_CHUNK_BYTES) {
                    let data = BASE64.encode(piece);
                    debug_assert!(data.len() <= MAX_CHUNK_ENCODED_BYTES);
                    if self.halted()
                        || !self.send(RemoteFrame::HttpBody {
                            stream: self.stream,
                            data,
                            end: false,
                        })
                    {
                        sent_ok = false;
                        return false;
                    }
                }
                true
            },
            &mut || !self.halted() && started.elapsed() < deadline,
        );
        if self.halted() || !sent_ok {
            return;
        }
        match result {
            Ok(()) => {
                self.send(RemoteFrame::HttpBody {
                    stream: self.stream,
                    data: String::new(),
                    end: true,
                });
            }
            Err(_) => self.fail(codes::DAEMON_UNAVAILABLE, "the response stream failed", true),
        }
    }
}

fn canonical_header(name: &str) -> String {
    const CANONICAL: [&str; 6] = [
        "Content-Type",
        "Cache-Control",
        "ETag",
        "Last-Modified",
        "Location",
        "Retry-After",
    ];
    CANONICAL
        .iter()
        .find(|canonical| canonical.eq_ignore_ascii_case(name))
        .map_or_else(|| name.to_owned(), |canonical| (*canonical).to_owned())
}
