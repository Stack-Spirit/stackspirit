# crate `vowp-agent` — Mac-side v0.4 agent client

Phase 0 client for a local node: local config, the ed25519 node key, the SSH stdio transport and
the `vowp-protocol` frame client. It depends on `vowp-protocol` only; it never opens a store, a
database or `vowp-core`, and it never dials a home except through the injected transport.
Spec: [`../v0.4/local-orchestrator.md`](../v0.4/local-orchestrator.md) §4.2, §4.6, §8.2–§8.5 and
§12.2.

## 1. Module layout

```
crates/vowp-agent/
  Cargo.toml
  src/lib.rs        re-exports; #![forbid(unsafe_code)], #![warn(missing_docs)]
  src/config.rs     AgentConfig, TransportConfig, default state dir, surface paths, 0600/0644 file helpers
  src/key.rs        NodeKey, handshake_payload, fingerprints
  src/transport.rs  Transport + FrameStream traits, SshTransport, 1 MiB JSON-line framing
  src/client.rs     AgentClient, AgentClock, Backoff, RunOptions, RunOutcome, SurfacePairRequest
  src/status.rs     AgentStatus, ConnectionState, spool and surface-expiry readers
  tests/surface_pair.rs  hermetic in-memory home for `AgentClient::pair`
```

`~/.vowp-agent` is the default state directory. `VOWP_AGENT_HOME` overrides it; tests always set
that variable or a temporary `HOME`, so the real directory is never touched.

## 2. Files and permissions

| File | Mode | Contents |
|---|---|---|
| `agent.json` | 0600 | `home`, `node`, `state_dir`, `transport = { kind = "ssh", target }` |
| `node.key` | 0600 | the 32-byte ed25519 secret as lowercase hex |
| `node.pub` | 0644 | the 32-byte public key as lowercase hex |
| `status.json` | 0600 | the last `AgentStatus` document |
| `spool/` | — | pending bytes counted by `status`, never opened as a database |
| `surfaces/` | 0700 | one directory per paired surface |
| `surfaces/<id>/token` | 0600 | the `vowp_surf_` token returned by `SurfaceIssued`; never printed |
| `surfaces/<id>/expires_at` | 0600 | RFC 3339 surface-token expiry, read by `agent status` |

The state directory is created 0700. Private files are created with `OpenOptionsExt::mode(0o600)`
and their mode is set again after writing, so an existing wider file is tightened. The private key
and the enrollment code are never logged, echoed, put in argv or written to `status.json`.
`NodeKey`'s `Debug` implementation prints only the public key.

## 3. Public API

```rust
pub struct AgentConfig { pub home: String, pub node: NodeName, pub state_dir: PathBuf, pub transport: TransportConfig }
pub enum TransportConfig { Ssh { target: String } }

impl AgentConfig {
    pub fn new(home: impl Into<String>, node: NodeName, state_dir: PathBuf, target: impl Into<String>) -> Self;
    pub fn default_state_dir() -> Result<PathBuf, AgentError>;          // $VOWP_AGENT_HOME, else $HOME/.vowp-agent
    pub fn file_path(state_dir: &Path) -> PathBuf;
    pub fn surfaces_dir(state_dir: &Path) -> PathBuf;                   // state_dir/surfaces
    pub fn surface_dir(state_dir: &Path, surface: &SurfaceId) -> PathBuf;
    pub fn surface_token_path(state_dir: &Path, surface: &SurfaceId) -> PathBuf;
    pub fn surface_expiry_path(state_dir: &Path, surface: &SurfaceId) -> PathBuf;
    pub fn load(state_dir: &Path) -> Result<Self, AgentError>;          // NotEnrolled when missing
    pub fn write(&self) -> Result<(), AgentError>;                      // agent.json, 0600
    pub fn ssh_target(&self) -> Result<&str, AgentError>;
}

pub struct NodeKey { /* SigningKey; Debug prints the public key only */ }
impl NodeKey {
    pub fn generate(state_dir: &Path) -> Result<Self, AgentError>;      // OsRng, writes node.key + node.pub
    pub fn load(state_dir: &Path) -> Result<Self, AgentError>;
    pub fn load_or_generate(state_dir: &Path) -> Result<Self, AgentError>;
    pub fn verifying_key(&self) -> VerifyingKey;
    pub fn public_key_hex(&self) -> String;                             // 64 lowercase hex chars
    pub fn fingerprint(&self) -> String;                                // "sha256:" + first 8 digest bytes
    pub fn sign_handshake(&self, nonce: &str, home_id: &str, connection_id: &str) -> String;
}
pub fn handshake_payload(nonce: &str, home_id: &str, connection_id: &str) -> Vec<u8>; // nonce\nhome_id\nconnection_id
pub fn load_public(state_dir: &Path) -> Result<VerifyingKey, AgentError>;
pub fn fingerprint_of(key: &VerifyingKey) -> String;

pub enum FrameRead { Frame(AgentFrame), Timeout, Closed }
pub trait FrameStream: Send {
    fn recv(&mut self, timeout: Option<Duration>) -> Result<FrameRead, AgentError>;
    fn send(&mut self, frame: &AgentFrame) -> Result<(), AgentError>;
}
pub trait Transport: Send { fn open(&mut self) -> Result<Box<dyn FrameStream>, AgentError>; }

pub struct SshTransport { /* target, binary, max_frame_bytes */ }
impl SshTransport {
    pub fn new(target: impl Into<String>) -> Self;
    pub fn with_binary(target: impl Into<String>, binary: impl Into<String>) -> Self;
    pub fn argv(&self) -> Vec<String>;                                  // pure; tests assert it
}

pub trait AgentClock: Send + Sync { fn now(&self) -> Timestamp; fn sleep(&self, duration: Duration); }
pub struct SystemAgentClock;
pub struct Backoff { /* 1, 2, 4 … 60 s with up to 25 % jitter */ }
impl Backoff { pub fn new() -> Self; pub fn seeded(seed: u64) -> Self; pub fn next_delay(&mut self) -> Duration; pub fn reset(&mut self); }

pub struct AgentClient { /* state_dir, key, config, transport, clock, connection, status */ }
impl AgentClient {
    pub fn new(state_dir: PathBuf, key: NodeKey, config: AgentConfig, transport: Box<dyn Transport>, clock: Arc<dyn AgentClock>) -> Self;
    pub fn unenrolled(state_dir: PathBuf, key: NodeKey, transport: Box<dyn Transport>, clock: Arc<dyn AgentClock>) -> Self;
    pub fn from_state_dir(state_dir: PathBuf, clock: Arc<dyn AgentClock>) -> Result<Self, AgentError>;
    pub fn with_backoff(self, backoff: Backoff) -> Self;
    pub fn enroll(&mut self, target: &str, code: &str) -> Result<Enrollment, AgentError>;
    pub fn pair(&mut self, request: &SurfacePairRequest) -> Result<PairedSurface, AgentError>;
    pub fn connect(&mut self) -> Result<(), AgentError>;
    pub fn heartbeat(&mut self) -> Result<Timestamp, AgentError>;
    pub fn run(&mut self, options: &RunOptions) -> Result<RunOutcome, AgentError>;
    pub fn status(&self) -> AgentStatus;
}

pub struct SurfacePairRequest { pub code: String, pub surface_id: SurfaceId, pub session: SessionId, pub app: String }
pub struct PairedSurface { pub surface_id: SurfaceId, pub scopes: Vec<SurfaceScope>, pub expires_at: Timestamp, pub token_path: PathBuf }
pub fn validate_pairing_code(code: &str) -> Result<(), AgentError>;
pub fn validate_surface_id(surface: &SurfaceId) -> Result<(), AgentError>;

pub struct Enrollment { pub node: NodeName, pub home: String, pub protocol: String, pub heartbeat_secs: u64 }
pub enum RunOutcome { Revoked { reason: String }, Superseded { connection_id: String } }
pub struct RunOptions { pub role: AgentRole, pub foreground: bool }

pub enum ConnectionState { Unknown, Connecting, Connected, Reconnecting, Revoked, Superseded, Stopped }
pub struct AgentStatus {
    pub connection: ConnectionState, pub node: Option<NodeName>, pub home: Option<String>,
    pub protocol: Option<String>, pub last_heartbeat: Option<Timestamp>, pub last_error: Option<String>,
    pub pending_spool_bytes: u64, pub key_fingerprint: Option<String>,
    pub surface_token_expires_at: Option<Timestamp>,
}
```

## 4. Behaviour

### Enrollment

`AgentClient::enroll` validates that the code is 52 ASCII alphanumeric characters, generates or
loads the node key, opens the transport and sends one `Enroll` frame with the public key (lowercase
hex), agent version, OS, arch, empty labels/capabilities/path maps. `Enrolled` supplies `home_id`,
`node`, `protocol` and `heartbeat_secs`. The client closes that stream, reconnects and performs the
handshake; only after the handshake succeeds does it write `agent.json`. The code is consumed by
the home and is never written locally.

### Surface pairing

`AgentClient::pair` requires an enrolled `agent.json`. It validates the 52-character code and
the 26-character surface id, performs the normal handshake, and sends one
`SurfacePair { code, surface_id, session, app }` frame. The home answers `SurfaceIssued` with
the token, scopes and expiry. The client writes `<token>\n` to
`surfaces/<surface_id>/token` (0600) and the RFC 3339 expiry to `surfaces/<surface_id>/expires_at`
(0600), after creating the state, `surfaces/` and surface directories 0700. The token is never printed,
logged or returned: [`PairedSurface`] names only the token path, scopes and expiry, and
`SurfacePairRequest`'s `Debug` redacts the code. A malformed request is `Config`; a
`SurfaceRevoked` frame, a bad code or a replay is a non-retryable `Remote { kind: Refused }`;
a `Revoked`/`Superseded` node credential keeps its existing meaning. The client waits at most
[`PAIR_TIMEOUT`] (30 seconds) for the answer and closes the transport after pairing.

### Handshake

1. `AgentHello` carries the node name, the public key as `key_id`, the agent version, OS, arch and
   names-only capabilities.
2. `HomeChallenge` supplies `nonce`, `home_id`, `connection_id`, `server_time`, `heartbeat_secs`
   and `max_frame_bytes`.
3. The client refuses an empty nonce and any nonce it has already signed (`ReplayNonce`), then
   sends `AgentAuth { signature }`: ed25519 over the UTF-8 bytes
   `nonce + "\n" + home_id + "\n" + connection_id`, lowercase hex (64 bytes, 128 hex characters).
4. `HomeWelcome` must name the negotiated `vowp-agent/1` protocol, the same home and connection,
   and the enrolled node. A mismatch is an authentication or protocol error.
5. `Revoked` and `Superseded` stop immediately; `Error { kind: auth }` is fatal, other `Error`
   frames are retried only when `retryable` is true.

### Heartbeat and reconnect

`run` opens a connection, sends one heartbeat immediately, then sends another every
`heartbeat_secs` (10 by default). A heartbeat carries `Timestamp::now` from the injected clock, a
local capacity snapshot (CPU count, `/proc/loadavg` and `/proc/meminfo` when available, `df -Pk`
free bytes, zero busy slots) and the current `spool/` byte count. Every heartbeat rewrites
`status.json` 0600.

`Ping` is answered with `Pong`. `Revoked` and `Superseded` end the loop with the matching
`RunOutcome`. A dropped transport, EOF or retryable home error is reported in `status.json` and
retried with [`Backoff`]: 1, 2, 4 … 60 seconds with up to 25 % jitter. A connection that stayed up
for 60 seconds resets the backoff. Fatal authentication, config, key, protocol and non-retryable
home errors end the loop.

The CLI loads `status.json` through `AgentStatus::load`, fills missing node/home/protocol/
fingerprint fields from `agent.json` and `node.pub`, and recounts `spool/` so the displayed byte
count is current even when the agent is stopped. A missing enrollment is exit 3 with the
enrollment hint.

## 5. Transport

`SshTransport::argv` is exactly the documented command:

```text
ssh -T -o BatchMode=yes -o ServerAliveInterval=15 -o ServerAliveCountMax=3 <target> vowp agent gateway --stdio
```

The real stream runs that process with piped stdin/stdout/stderr, reads one JSON `AgentFrame` per
line on a reader thread, refuses a frame over 1 MiB, and sends each frame as one JSON line plus
`\n`. Stderr is drained into a 4 KiB tail for transport errors and never includes a frame or a
secret. Dropping the stream kills the ssh child. Tests implement `Transport`/`FrameStream` in
memory and never spawn ssh, a socket or the network.

## 6. Errors

`AgentError` is the crate's single error type: `Io`, `Config`, `Protocol`, `Transport`, `Auth`,
`ReplayNonce`, `FrameTooLarge`, `NotEnrolled`, `Key`, `Remote { kind, message, retryable }`,
`Revoked`, `Superseded` and `Json`. `is_fatal` names the errors that must not be retried.

## 7. Named tests

`agent_key_is_created_0600`, `agent_enroll_sends_a_public_key_and_stores_the_node`,
`agent_handshake_rejects_a_bad_signature`, `agent_handshake_rejects_a_replayed_nonce`,
`agent_reconnects_with_backoff_after_a_drop`, `agent_heartbeats_on_the_interval`,
`agent_stops_on_revoked`, `agent_stops_on_superseded`. The transport tests
`ssh_argv_matches_the_documented_command` and `read_line_limited_rejects_an_oversized_frame` cover
the exact SSH argv and the frame cap. The surface-pairing slice adds the in-memory home tests
`agent_pair_writes_0600_token_and_prints_no_secret`, `agent_pair_refuses_a_replayed_code` and
`agent_pair_surfaces_path_is_private` in `tests/surface_pair.rs`.

## 8. Out of scope

The home-side gateway, store methods, node enrollment codes, node commands, runs, leases,
offload, path-map mutation, launchd and WSS ingress are separate jobs. Phase 0 `vowp agent run`
stays in the foreground; `--role` is accepted and carried in `RunOptions`, but assignment
handling arrives with the executor job. `vowp agent surface start|attach|detach`, local chat,
the MCP proxy and token rotation are later v0.4 slices.
