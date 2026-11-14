# crate `vowp-agent` — Mac-side v0.4 agent client

Phase 0/1 client for a local node: local config, the ed25519 node key, the SSH stdio transport,
surface pairing, the `vowp-protocol` frame client and the run-assignment transport loop. It depends on
`vowp-protocol` only; it never opens a store, a database or `vowp-core`, and it never dials a home except
through the injected transport.
Spec: [`../v0.4/local-orchestrator.md`](../v0.4/local-orchestrator.md) §4.2, §4.6, §6.3–§6.5,
§8.2–§8.5, §9 and §12.2.

## 1. Module layout

```
crates/vowp-agent/
  Cargo.toml
  src/lib.rs        re-exports; #![forbid(unsafe_code)], #![warn(missing_docs)]
  src/config.rs     AgentConfig, TransportConfig, default state dir, surface paths, 0600/0644 file helpers
  src/key.rs        NodeKey, handshake_payload, fingerprints
  src/transport.rs  Transport + FrameStream traits, SshTransport, 1 MiB JSON-line framing
  src/client.rs     AgentClient, AgentClock, Backoff, RunOptions, RunOutcome, SurfacePairRequest
  src/executor.rs   RunHandler, RunSink, AcceptedRun, HandlerReject, RunSummary, spool/ack/replay loop
  src/chat.rs       ChatClient, ChatTurnRequest, ChatTurnStatus, lease/turn/replay loop
  src/spool.rs      Spool, SpoolAppend, acked.json and the 2 GiB cap
  src/status.rs     AgentStatus, ConnectionState, spool and surface-expiry readers
  src/worktree.rs   PathMap, WorktreeManager, Worktree, push/cleanup and the disk guard
  tests/surface_pair.rs  hermetic in-memory home for `AgentClient::pair`
  tests/config.rs   path-map persistence, validation, enrollment and update frames
  tests/chat.rs     fake home and fake adapter for local chat spool/replay/takeover
  tests/executor.rs fake home and fake handler for assignments, acks, replay and fencing
  tests/worktree.rs temporary origin/clone plus real git for create/reuse/cleanup/push
```

`~/.vowp-agent` is the default state directory. `VOWP_AGENT_HOME` overrides it; tests always set
that variable or a temporary `HOME`, so the real directory is never touched.

## 2. Files and permissions

| File | Mode | Contents |
|---|---|---|
| `agent.json` | 0600 | `home`, `node`, `state_dir`, `transport = { kind = "ssh", target }`, and the `path_maps` array |
| `node.key` | 0600 | the 32-byte ed25519 secret as lowercase hex |
| `node.pub` | 0644 | the 32-byte public key as lowercase hex |
| `status.json` | 0600 | the last `AgentStatus` document |
| `spool/` | — | pending bytes counted by `status`, never opened as a database |
| `surfaces/` | 0700 | one directory per paired surface |
| `surfaces/<id>/token` | 0600 | the `vowp_surf_` token returned by `SurfaceIssued`; never printed |
| `surfaces/<id>/session` | 0600 | the session id the surface was paired for |
| `surfaces/<id>/expires_at` | 0600 | RFC 3339 surface-token expiry, read by `agent status` |
| `spool/<session>/<run>/events.jsonl` | 0600 | every sent event and the terminal summary, fsynced before send |
| `spool/<session>/<run>/acked.json` | 0600 | event id to home sequence, plus `summary` after the terminal ack |
| `spool/<session>/<run>/quarantine.json` | 0600 | reason a conflicting event id stopped the run's stream |

The state directory is created 0700. Private files are created with `OpenOptionsExt::mode(0o600)`
and their mode is set again after writing, so an existing wider file is tightened. The private key
and the enrollment code are never logged, echoed, put in argv or written to `status.json`.
`NodeKey`'s `Debug` implementation prints only the public key.

## 3. Public API

```rust
pub struct AgentConfig { pub home: String, pub node: NodeName, pub state_dir: PathBuf,
                         pub transport: TransportConfig, pub path_maps: Vec<AgentPathMap> }
pub enum TransportConfig { Ssh { target: String } }
pub const DEFAULT_WORKTREE_DIR: &str;                               // ".vowp/worktrees"

impl AgentConfig {
    pub fn new(home: impl Into<String>, node: NodeName, state_dir: PathBuf, target: impl Into<String>) -> Self;
    pub fn with_path_maps(self, path_maps: Vec<AgentPathMap>) -> Self;
    pub fn default_state_dir() -> Result<PathBuf, AgentError>;          // $VOWP_AGENT_HOME, else $HOME/.vowp-agent
    pub fn file_path(state_dir: &Path) -> PathBuf;
    pub fn surfaces_dir(state_dir: &Path) -> PathBuf;                   // state_dir/surfaces
    pub fn surface_dir(state_dir: &Path, surface: &SurfaceId) -> PathBuf;
    pub fn surface_token_path(state_dir: &Path, surface: &SurfaceId) -> PathBuf;
    pub fn surface_expiry_path(state_dir: &Path, surface: &SurfaceId) -> PathBuf;
    pub fn default_worktree_root(local_root: &Path) -> PathBuf;         // <local_root>/.vowp/worktrees
    pub fn validate_path_maps(state_dir: &Path, path_maps: &[AgentPathMap]) -> Result<(), AgentError>;
    pub fn path_maps(&self) -> &[AgentPathMap];
    pub fn path_map(&self, project: &ProjectName) -> Option<&AgentPathMap>;
    pub fn set_path_map(&mut self, map: AgentPathMap) -> Result<(), AgentError>;
    pub fn remove_path_map(&mut self, project: &ProjectName) -> bool;
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
    pub fn with_path_maps(self, path_maps: Vec<AgentPathMap>) -> Self;
    pub fn path_maps(&self) -> &[AgentPathMap];
    pub fn enroll(&mut self, target: &str, code: &str) -> Result<Enrollment, AgentError>;
    pub fn pair(&mut self, request: &SurfacePairRequest) -> Result<PairedSurface, AgentError>;
    pub fn connect(&mut self) -> Result<(), AgentError>;
    pub fn refresh_path_maps(&mut self) -> Result<usize, AgentError>;
    pub fn heartbeat(&mut self) -> Result<Timestamp, AgentError>;
    pub fn run(&mut self, options: &RunOptions) -> Result<RunOutcome, AgentError>;
    pub fn run_with_handler(&mut self, options: &RunOptions, handler: Arc<dyn RunHandler>)
        -> Result<RunOutcome, AgentError>;
    pub fn status(&self) -> AgentStatus;
}

pub struct SurfacePairRequest { pub code: String, pub surface_id: SurfaceId, pub session: SessionId, pub app: String }
pub struct PairedSurface { pub surface_id: SurfaceId, pub scopes: Vec<SurfaceScope>, pub expires_at: Timestamp, pub token_path: PathBuf }
pub fn validate_pairing_code(code: &str) -> Result<(), AgentError>;
pub fn validate_surface_id(surface: &SurfaceId) -> Result<(), AgentError>;

pub struct Enrollment { pub node: NodeName, pub home: String, pub protocol: String, pub heartbeat_secs: u64 }
pub enum RunOutcome { Revoked { reason: String }, Superseded { connection_id: String } }
pub struct RunOptions { pub role: AgentRole, pub foreground: bool }

pub struct PairedToken { pub surface_id: SurfaceId, pub token: String }   // Debug redacts token
pub fn paired_token_for_session(state_dir: &Path, session: &SessionId) -> Result<Option<PairedToken>, AgentError>;

pub struct ChatClient { /* client, surface, session, lease, spool cap */ }
impl ChatClient {
    pub fn new(client: AgentClient, surface: PairedToken, session: SessionId) -> Self;
    pub fn from_state_dir(state_dir: PathBuf, session: SessionId, clock: Arc<dyn AgentClock>) -> Result<Self, AgentError>;
    pub fn with_spool_cap(self, cap: u64) -> Self;
    pub fn surface_id(&self) -> &SurfaceId;
    pub fn lease_token(&self) -> Option<&str>;
    pub fn turn(&mut self, request: &ChatTurnRequest, adapter: &dyn Adapter,
                on_event: &mut dyn FnMut(&HarnessEvent), cancel: Option<&CancelToken>)
        -> Result<ChatTurnStatus, AgentError>;
    pub fn release(&mut self) -> Result<(), AgentError>;
}
pub struct ChatTurnRequest { pub executor: Executor, pub prompt: String, pub cwd: PathBuf,
                             pub cwd_ref: PathRef, pub job: Option<JobId>, pub purpose: RunPurpose }
pub enum ChatTurnStatus { Done, ReadOnly { reason: String }, SpoolFull { bytes: u64, cap: u64 }, Interrupted }

pub trait RunHandler: Send + Sync {
    fn accept(&self, assignment: &AssignmentPayload) -> Result<AcceptedRun, HandlerReject>;
    fn run(&self, accepted: AcceptedRun, sink: &mut dyn RunSink);
    fn cancel(&self, run: &RunId);
    fn cleanup(&self, run: &RunId);
}
pub trait RunSink: Send {
    fn line(&mut self, line: RemoteLine) -> Result<(), RunSinkError>;
    fn state(&mut self, state: RemoteRunState, detail: &str) -> Result<(), RunSinkError>;
    fn finish(&mut self, summary: RunSummary) -> Result<(), RunSinkError>;
    fn is_stopped(&self) -> bool;
    fn spool_bytes(&self) -> u64;
}
pub struct AcceptedRun { pub assignment: AssignmentPayload, pub epoch: u64,
                          pub cwd: PathBuf, pub worktree: PathBuf, pub project_root: PathBuf,
                          pub reserve_bytes: u64 }                 // Debug redacts local paths
pub struct HandlerReject { pub reason: RunRejectReason, pub detail: String }
pub struct RunSummary { pub exit_code: Option<i32>, pub outcome: RunOutcome,
                        pub native_session_id: Option<String>, pub cost: Cost, pub limit: Option<LimitEvent>,
                        pub usage: Option<RemoteUsage>, pub head_commit: Option<String>,
                        pub push_status: Option<PushStatus>, pub result_text: Option<String>,
                        pub stop_reason: StopReason }
pub enum PushStatus { NotRequested, Pushed { branch: String, remote: String, head_commit: String },
                      Failed { detail: String } }
pub enum RunSinkError { Stopped { reason: String }, SpoolFull { bytes: u64, cap: u64 },
                        Unsupported { kind: &'static str } }
pub const EVENT_WINDOW: u64;                 // 64
pub const QUARANTINE_FILE: &str;             // "quarantine.json"

pub struct Spool { /* dir, cap, events file, acked map */ }
impl Spool {
    pub fn open(state_dir: &Path, session: &SessionId, run: &RunId) -> Result<Self, AgentError>;
    pub fn with_cap(state_dir: &Path, session: &SessionId, run: &RunId, cap: u64) -> Result<Self, AgentError>;
    pub fn append(&mut self, frame: &AgentFrame) -> Result<SpoolAppend, AgentError>;
    pub fn mark_acked(&mut self, event_id: &str, seq: u64) -> Result<(), AgentError>;
    pub fn mark_summary_acked(&mut self) -> Result<(), AgentError>;
    pub fn summary_acked(&self) -> bool;
    pub fn unacked(&self) -> Result<Vec<AgentFrame>, AgentError>;
    pub fn unacked_events(&self) -> Result<usize, AgentError>;
}
pub enum SpoolAppend { Written, Full { bytes: u64, cap: u64 } }

pub enum ConnectionState { Unknown, Connecting, Connected, Reconnecting, Revoked, Superseded, Stopped }
pub struct AgentStatus {
    pub connection: ConnectionState, pub node: Option<NodeName>, pub home: Option<String>,
    pub protocol: Option<String>, pub last_heartbeat: Option<Timestamp>, pub last_error: Option<String>,
    pub pending_spool_bytes: u64, pub key_fingerprint: Option<String>,
    pub surface_token_expires_at: Option<Timestamp>,
}

pub struct PathMap { /* validated canonical roots and allow list */ }
impl PathMap {
    pub fn new(map: &AgentPathMap) -> Result<Self, WorktreeError>;
    pub fn project(&self) -> &ProjectName;
    pub fn local_root(&self) -> &Path;
    pub fn worktree_root(&self) -> &Path;
    pub fn resolve(&self, project: &ProjectName, path_ref: &PathRef) -> Result<PathBuf, WorktreeError>;
    pub fn worktree_path(&self, job: &JobId) -> Result<PathBuf, WorktreeError>;
}

pub struct WorktreeSpec { pub job: JobId, pub base_commit: String, pub remote: String, pub reserve_bytes: u64 }
pub struct WorktreeManager { /* path map + injectable GitRunner */ }
impl WorktreeManager {
    pub fn new(map: PathMap) -> Self;
    pub fn with_git(map: PathMap, git: Arc<dyn GitRunner>) -> Self;
    pub fn map(&self) -> &PathMap;
    pub fn create(&self, spec: &WorktreeSpec) -> Result<Worktree, WorktreeError>;
    pub fn cleanup(&self, job: &JobId) -> Result<(), WorktreeError>;
}

pub struct Worktree { /* project, job, path, branch, remote, git runner */ }
impl Worktree {
    pub fn path(&self) -> &Path;
    pub fn branch(&self) -> &str;
    pub fn head_commit(&self) -> Result<String, WorktreeError>;
    pub fn is_clean(&self) -> Result<bool, WorktreeError>;
    pub fn push_job(&self, expected_remote_url: &str) -> Result<PushResult, WorktreeError>;
    pub fn cleanup(self) -> Result<(), WorktreeError>;
}
pub struct PushResult { pub branch: String, pub remote: String, pub head_commit: String }

pub trait GitRunner: Send + Sync { fn run(&self, cwd: &Path, args: &[&str]) -> Result<GitOutput, WorktreeError>; }
pub struct SystemGit { /* git binary name */ }
pub fn strip_remote_credentials(url: &str) -> String;
pub const DEFAULT_DISK_RESERVE_BYTES: u64;   // 5 GiB
```

## 4. Behaviour

### Enrollment

`AgentClient::enroll` validates that the code is 52 ASCII alphanumeric characters, generates or
loads the node key, opens the transport and sends one `Enroll` frame with the public key (lowercase
hex), agent version, OS, arch, empty labels, names-only capabilities and the configured path map.
`capabilities.projects` lists each mapped project name; the roots travel only in `paths`, the
owner-only `node_paths` payload. `Enrolled` supplies `home_id`, `node`, `protocol` and
`heartbeat_secs`. The client closes that stream, reconnects and performs the handshake; only after
the handshake succeeds does it write `agent.json` (including `path_maps`). The code is consumed by
the home and is never written locally.

### Path map

`AgentConfig.path_maps` is a `Vec<AgentPathMap>` persisted in the same 0600 `agent.json` as the
enrollment. `serde(default)` keeps an older file without the array loadable; it starts empty.
`validate_path_maps` requires unique project names, an absolute UTF-8 `local_root` without `..`
or NUL that exists and is a directory, a `worktree_root` that exists or is created only when it
is below the agent state directory, and allow entries that are already normalised relative paths
(no leading `/`, empty, `.` or `..` component, NUL, backslash or control character). A map with
no `worktree_root` is validated against `<local_root>/.vowp/worktrees`. `AgentConfig::write`
validates every map, `load` validates again, and `set_path_map`/`remove_path_map` edit one entry
in memory before the caller writes the file. The default worktree root for a checkout is
`<local_root>/.vowp/worktrees` (`DEFAULT_WORKTREE_DIR`).

`vowp agent run` reloads `agent.json` on every heartbeat. A fresh process reports the saved map
after its first handshake, so a project added while the agent was stopped reaches the home
without re-enrollment. `AgentClient::refresh_path_maps` sends one `PathMapUpdate { map }` for each
project added or changed since the last report and remembers the sent set, so an unchanged map is
not sent twice. A removal changes only local eligibility;
the frame vocabulary has no removal message. A home that predates `PathMapUpdate` answers `Error`;
the client disables further updates for that process rather than reconnecting in a loop. Absolute
roots appear only in `agent.json`, the `Enroll.paths`/`PathMapUpdate` frames and the owner-only
`vowp agent status --json`/human output, never in `status.json`, a log line or a run frame.

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
free bytes, zero busy slots) and the current `spool/` byte count. Each heartbeat also reloads
`agent.json` and sends any changed `PathMapUpdate` frames. Every heartbeat rewrites
`status.json` 0600; that file holds names and counts, never a path map.

`Ping` is answered with `Pong`. `Revoked` and `Superseded` end the loop with the matching
`RunOutcome`. A dropped transport, EOF or retryable home error is reported in `status.json` and
retried with [`Backoff`]: 1, 2, 4 … 60 seconds with up to 25 % jitter. A connection that stayed up
for 60 seconds resets the backoff. Fatal authentication, config, key, protocol and non-retryable
home errors end the loop.

The CLI loads `status.json` through `AgentStatus::load`, fills missing node/home/protocol/
fingerprint fields from `agent.json` and `node.pub`, and recounts `spool/` so the displayed byte
count is current even when the agent is stopped. A missing enrollment is exit 3 with the
enrollment hint.

### Local chat

`ChatClient::from_state_dir` loads `agent.json`, `node.key` and the surface token bound to the
session. `turn` first sends `LeaseAcquire { mode: acquire }`; a live lease held by another surface
is a conflict and never a silent takeover. It then sends `TurnBegin { surface, session,
lease_token, prompt, executor, cwd_ref, job, purpose }` and waits for `TurnGrant { run_id,
session, lease_token, expires_at, tail_seq }`.

The working directory stays local: the CLI builds a `PathRef` (project or subdir) for the home and
passes the absolute path only to the adapter on this machine. Events are normalized to
`RemoteLine::Event`, appended to `spool/<session>/<run>/events.jsonl` and fsynced **before** the
`RemoteLine` envelope is sent. The home assigns `seq` in `IngestAck`; the agent records the id and
seq in `acked.json`. The home admits at most 64 event lines per stream window; it sends `Credit`
before the last acknowledgement and opens the next window only after the agent confirms the
grant with a lease heartbeat. A refused event is retried after a reconnect, never appended out
of order. The terminal `TurnEnd { run_id, summary }` is spooled the same way and the summary ack
sets the reserved `summary` key.

On a transport error the client reconnects with `connect`, sends `LeaseHeartbeat`, sends a fresh
`RemoteLine::Hello`, and replays every frame not in `acked.json` in order. The home resumes the
run's ingest state from its open run row, paired surface and live lease, then dedupes by event
id, so an event whose ack was lost becomes a duplicate rather than a second append. A takeover or
lease loss (`LeaseLost`, `LeaseRevoked`, `IngestAck.lease_revoked`, `RunLost`) stops the adapter,
returns [`ChatTurnStatus::ReadOnly`] and keeps the spool; that includes a loss reported by the
reconnect after the first `Hello` send fails. A frame that would push `events.jsonl` past 2 GiB
returns [`ChatTurnStatus::SpoolFull`]; it is never sent. Unacknowledged events are never deleted.

### Run assignment loop

`AgentClient::run` keeps the Phase 0 behaviour: it has no executor, so an `AssignRun` is refused
with `RunRejected { reason: other, detail: "this agent has no run handler" }` and never starts
anything. `AgentClient::run_with_handler(options, handler)` installs an injected [`RunHandler`] and
handles assignments on the same frame loop.

On `AssignRun` the driver:

1. Compares the assignment's `(run_id, epoch)` with the open runs. The same `(run_id, epoch)` is
   a duplicate: the client re-sends `RunAccepted` and updates the in-memory lease token without a
   second handler; a different assignment id for that run and epoch is refused with `other`. A
   lower epoch is refused with `other` (`stale assignment epoch`). A higher epoch, or a repeat of
   a stopped run, supersedes the old dispatch: the handler is cancelled, the spool is kept and
   replay skips lines from the old epoch.
2. Refuses a second live run for the same job with `worktree_conflict`, an assignment whose
   deadline has passed with `other`, and a `cwd_ref` naming another project with `other`.
3. Calls `RunHandler::accept`, which performs the node-specific checks the transport cannot:
   path map and writable roots, harness binary, environment and secret names, disk reserve and
   worktree conflict. A [`HandlerReject`] is sent as `RunRejected` with its documented
   `RunRejectReason` and never calls `RunHandler::run`. The mapping is the protocol vocabulary:
   `node_inactive`, `project_unmapped`, `path_missing`, `path_not_writable`, `cwd_outside_allow`,
   `harness_missing`, `env_missing`, `disk_low`, `worktree_conflict`, `origin_mismatch` and
   `other`.
4. Sends `RunAccepted { assignment_id, run_id, epoch }`, `RunState { running, spool_bytes }` and
   one `RemoteLine::Hello` carrying that run's lease token, then starts `RunHandler::run` on its
   own thread. The hello is the first line of the run's stream; it is never spooled because its
   lease token is a frame-only secret. The handler gets an [`AcceptedRun`] with the token-free
   assignment, the epoch and the resolved `cwd`, `worktree`, project root and reserve; its `Debug`
   prints the three paths as `[local path]`.

`RunSink::line` accepts `RemoteLine::Event` and `RemoteLine::Heartbeat`: an event is wrapped with
the run id and epoch, appended to `events.jsonl` and fsynced **before** the frame loop sends it,
and a heartbeat is sent without being spooled (the spool never replays liveness lines). While the
connection is down the spool is the queue; an offline event is not also copied into an in-memory
buffer, and non-terminal state and heartbeat notifications are dropped rather than queued. The
reconnect replays the durable lines. `RunSink::finish` spools a `TurnEnd` with the classified
summary and blocks until the home acknowledges it, the run is stopped, or the frame does not fit.
If the handler returns without a summary, the driver spools and sends an error summary instead of
leaving the run open.

Every event line consumes one of 64 window credits. `Credit { count }` replenishes the window; a
reconnect starts a fresh stream from the home and resets each resumed run to a full window. The
driver sends one line per run and waits for its `IngestAck`, so it never has more than one line
in flight even though the home's window allows 64. The home assigns `seq`; `acked.json` records the
event id and sequence. A duplicate acknowledgement (the same id and content) is recorded and not
re-sent. `IngestAck.error`, an `IngestAck.lease_revoked` and a `RunLost` stop the stream; a
`Conflict` acknowledgement also writes `quarantine.json` with the run id, epoch and reason and
stops the handler. A full spool stops the handler, sends a path-free
`Error { kind: guard, message: "spool_full: …" }` and `RunState { disconnected, detail:
"spool_full" }`; the terminal frame is not sent because it cannot be spooled. The reason is never
silently dropped.

`RunState` is sent at start (`running`), on a dropped connection (`disconnected`, best effort)
and before the terminal frame (`finishing`, with the head commit when the summary has one). The
terminal `TurnEnd` carries exit code, outcome, native session id, cost, limit, usage, result text
and stop reason. Result-return metadata (`head_commit`, `push_status`) stays in the local summary
and the `finishing` state; `push_status` is `not_requested`, `pushed { branch, remote, head_commit }`
or `failed { detail }`.

`CancelRun` stops the handler with `RunHandler::cancel`, reports `finishing` with a
`cancelled: …` detail, keeps the spool and worktree and leaves removal to the handler and a later
`Cleanup`. `RunLost` fences the run: it stops the handler, marks the state fenced, keeps the spool
and excludes the run from later `Resume` frames. `Cleanup` releases the run state and calls
`RunHandler::cleanup`, which removes the handler's own worktree; a stale `CancelRun` after
`Cleanup` is ignored.

After a reconnect the driver sends `Resume { runs: [{run_id, epoch, last_acked}], surfaces: [] }`.
It consumes `ResumeOk`, fences every run the home did not accept, and drains an immediately
following `AssignRun` so the replay uses the freshly issued lease token. It then sends a fresh
`RemoteLine::Hello` and replays the first unacknowledged line in order; the spool is the replay
queue, so an acknowledged line is never sent twice. Only lines whose envelope epoch matches the
current entry (plus a summary once the handler produced one) are replayed.

The invariant is enforced on every path: the lease token is held only in memory and appears only
in the `Hello` frame; it is never written to `events.jsonl`, `acked.json`, `quarantine.json`,
`status.json`, an error frame or a log line. Secret values never reach this crate (the assignment
carries names only), and the driver never puts an absolute local path in a frame, status or error:
`AcceptedRun`'s `Debug` redacts paths, `RunState` details are static, and spool failures are
reported as path-free strings. A handler must redact its own event payloads and result text; the
home's redaction and `secret_scan` gate remain the backstop.

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

`AgentError` is the crate's transport and client error type: `Io`, `Config`, `Protocol`,
`Transport`, `Auth`, `ReplayNonce`, `FrameTooLarge`, `NotEnrolled`, `Key`,
`Remote { kind, message, retryable }`, `Revoked`, `Superseded` and `Json`. `is_fatal` names the
errors that must not be retried. The worktree primitives use their own `WorktreeError`, mapped
to the protocol's refusal vocabulary by `reject_reason`; §8 lists its variants.

## 7. Named tests

`agent_key_is_created_0600`, `agent_enroll_sends_a_public_key_and_stores_the_node`,
`agent_handshake_rejects_a_bad_signature`, `agent_handshake_rejects_a_replayed_nonce`,
`agent_reconnects_with_backoff_after_a_drop`, `agent_heartbeats_on_the_interval`,
`agent_stops_on_revoked`, `agent_stops_on_superseded`. The path-map slice adds, in
`tests/config.rs`, `agent_enroll_sends_the_configured_path_map`,
`agent_path_map_persists_0600_and_reloads`,
`agent_path_map_rejects_a_relative_root_a_traversal_and_a_missing_checkout`,
`agent_paths_add_update_and_remove_one_project` and
`agent_path_map_update_is_sent_when_the_map_changes`; the CLI adds
`agent_status_redacts_absolute_paths_from_the_snapshot_cache`. The transport tests
`ssh_argv_matches_the_documented_command` and `read_line_limited_rejects_an_oversized_frame` cover
the exact SSH argv and the frame cap. The surface-pairing slice adds the in-memory home tests
`agent_pair_writes_0600_token_and_prints_no_secret`, `agent_pair_refuses_a_replayed_code` and
`agent_pair_surfaces_path_is_private` in `tests/surface_pair.rs`. The local-chat slice adds
`agent_chat_spools_before_sending`, `agent_chat_replays_from_the_first_unacked`,
`agent_chat_stops_on_lease_revoked` and `agent_chat_stops_on_spool_full` in `tests/chat.rs`, plus
`chat_requires_a_paired_surface_for_the_session` and `spool_keeps_unacked_events_and_records_acks`.
The worktree slice adds, in `tests/worktree.rs`:
`worktree_is_created_from_the_base_commit_without_touching_the_main_checkout`,
`worktree_leaves_a_dirty_main_checkout_untouched`,
`worktree_reuses_a_clean_branch_at_the_expected_base_and_recreates_a_dirty_one`,
`worktree_cleanup_removes_only_the_vowp_worktree_and_branch`,
`worktree_path_ref_stays_inside_the_worktree_root`,
`path_map_rejects_traversal_a_symlink_escape_and_a_case_only_escape`,
`worktree_refuses_a_remote_that_does_not_match_the_project`,
`worktree_refuses_a_fork_push_url`,
`worktree_refuses_a_push_instead_of_rewrite`,
`worktree_remote_mismatch_strips_credentials_from_the_actual_url`,
`worktree_push_fails_cleanly_and_keeps_the_worktree` and
`worktree_refuses_a_create_below_the_disk_reserve`, plus the fetch, push-only-job-branch, allow
list and missing/unwritable-root cases. The case-only test runs the real wrong-case directory on
a case-insensitive filesystem and a case-only symlink alias on a case-sensitive one; it never
passes silently.

The run-assignment slice adds, in `tests/executor.rs`, with a fake home and fake handler:
`assign_run_rejects_with_the_documented_reason_and_does_not_start`,
`accepted_run_spools_every_line_before_sending`,
`duplicate_ack_is_not_resent_and_conflict_stops_the_stream`,
`run_state_and_turn_end_carry_the_summary`,
`cancel_run_stops_the_handler_and_keeps_the_spool`, `run_lost_fences_the_handler`,
`resume_replays_from_the_first_unacked_line`, `cleanup_releases_the_run_state`,
`lease_token_and_secret_values_never_reach_the_spool_or_status` and
`spool_full_stops_the_run_and_records_the_reason`. Each test drives the real frame loop through
`AgentClient::run_with_handler`; the fake home implements `Transport`/`FrameStream` in memory and
the fake handler implements `RunHandler`/`RunSink`.

## 8. Worktrees, path maps and result return

`src/worktree.rs` is the §6.4–§6.5 foundation the executor calls after `AssignRun`. It adds no
frame loop, no harness start and no policy decision. Git runs through the small `GitRunner`
trait: the default `SystemGit` uses `std::process::Command`, and tests pass a recording wrapper
around the real binary. Absolute local paths stay in this module; the only frames that carry a
root are `Enroll.paths` and `PathMapUpdate`, which the home stores in its owner-only `node_paths`.

### Path map

`PathMap::new` canonicalises `local_root` and `worktree_root` (the latter defaults to
`<local_root>/.vowp/worktrees`), requires both to exist, be directories and carry a write bit,
and returns `path_missing` for a missing root or `path_not_writable` for a root without write
permission. It never panics. The allow list is normalised once; an existing allow entry is
canonicalised under the local root, so its on-disk case is what later requests are compared with.

`PathMap::resolve(project, path_ref)` returns an absolute local path and refuses:

- a `PathRef` for another project (`ProjectMismatch`);
- an absolute path, `..`, NUL, a backslash, a control character or an empty, `.` or repeated
  component (`InvalidPathRef`);
- a requested subdirectory outside the canonical local root, or a worktree leaf outside the
  canonical worktree root (`PathOutsideRoot`);
- a symlink that resolves outside the root (`SymlinkEscape`); a worktree leaf that is any
  symlink is refused (`SymlinkEscape` when it leaves the root, `WorktreeConflict` otherwise);
- a component that matches the on-disk entry only by case (`CaseOnlyEscape`); the requested and
  canonical components are compared exactly, so a case-insensitive filesystem cannot alias
  another directory through a differently-cased spelling;
- a subdirectory outside a declared `allow` entry (`OutsideAllow`).

`PathRef::Project` returns the canonical project root. `PathRef::Worktree` returns
`<worktree_root>/<job>` without requiring the leaf to exist yet, because the agent resolves the
ref before it creates the worktree. A `Subdir` must exist and be a directory. `Worktree` and
`Subdir` refs keep the project name; the caller's project must match both the map and the ref.

### Worktree lifecycle

`WorktreeSpec` carries the job, the full base commit, the remote (default `origin`) and the
free-space reserve (default `DEFAULT_DISK_RESERVE_BYTES`, 5 GiB). `WorktreeManager::create`:

1. Checks free space with `df -Pk` **before** looking at an existing worktree and refuses with
   `disk_low` when free bytes are below the reserve. It never deletes a worktree to make room.
2. Resolves the base commit. A commit missing from the main checkout is fetched with
   `git fetch --no-tags <remote> <sha>`; if that fails, a full `git fetch --no-tags <remote>` is
   tried before the typed `BaseCommitMissing` error.
3. Reuses an existing worktree only when all of the following hold: the ownership record and
   worktree marker name this job, the path is registered with the main checkout as a worktree,
   `HEAD` is on `job/<id>`, the branch head is exactly the expected base, and
   `git status --porcelain --untracked-files=normal` is empty. Any other **owned** worktree is
   removed with `git worktree remove --force` and recreated, so a stale branch or a dirty tree
   cannot leak into a new run. A directory at the job path without VOWP's ownership record is
   `worktree_conflict` and is never removed.
4. Creates the worktree with
   `git worktree add --detach <worktree_root>/<job> <base>`, writes the ownership marker and
   record, then runs `git checkout -B job/<id> <base>` inside the new worktree.

The main checkout is only fetched. No command checks out, resets, cleans or otherwise changes
its index or working tree; a dirty main checkout is ignored and left untouched. The ownership
record is `<git-common-dir>/vowp/worktrees/<job>.json` (0600) and the marker is
`<worktree-git-dir>/vowp-job` (0600); both are written by VOWP and must match before a worktree
is reused or removed.

`WorktreeManager::cleanup(job)` is idempotent. It removes only the agent's own worktree path and
`job/<id>` branch. It refuses a worktree directory without the matching ownership record and
marker, and a branch without the matching ownership record (`Refused`); refused paths are left
in place. When both the worktree and branch are already gone it only clears a stale record.
`Worktree::cleanup` applies the same checks to the worktree it was created for.

`head_commit` runs `git rev-parse HEAD`; `is_clean` runs
`git status --porcelain --untracked-files=normal` and treats any output, including an untracked
file, as dirty. Both are the summary helpers the run reports use.

### Result return

`Worktree::push_job(expected_remote_url)` first reads every URL `git push <remote>` will use with
`git remote get-url --push --all <remote>`, so `remote.<name>.pushurl`,
`url.<base>.pushInsteadOf` and `url.<base>.insteadOf` rewrites are part of the check. It strips
credentials with `strip_remote_credentials` (URL userinfo and the scp-like `user@` prefix). If any
push URL does not equal the home-supplied `projects.remote_url`, it returns `remote_mismatch` with
the mismatching URL after credential stripping, and does not push.

On a match it pushes exactly one refspec:

```text
git -c push.followTags=false push <remote> refs/heads/job/<id>:refs/heads/job/<id>
```

The branch is always `job/<id>`; the default branch, other branches, tags and releases are never
pushed. A successful push returns `PushResult { branch, remote, head_commit }`. A non-zero push
exit is `git_push_failed` carrying the command and the last 2 KiB of stderr; the worktree stays
in place for inspection and no success is reported.

### Errors

`WorktreeError` is the module's typed error: `Io`, `Json`, `ProjectMismatch`, `InvalidPathRef`,
`InvalidPathMapEntry`, `PathMissing`, `PathNotWritable`, `NotADirectory`, `PathOutsideRoot`,
`SymlinkEscape`, `CaseOnlyEscape`, `OutsideAllow`, `NotAGitCheckout`, `InvalidCommit`,
`InvalidRemote`, `BaseCommitMissing`, `DiskLow`, `WorktreeConflict`, `RemoteMismatch`,
`GitFailed`, `GitPushFailed` and `Refused`. `reject_reason` maps the path, disk, worktree and
remote cases to the protocol's `RunRejectReason` vocabulary. There is no `unwrap`, `expect` or
`unsafe` in the module outside tests.

## 9. Out of scope

The home-side gateway, store methods, node enrollment codes, node commands, runs, leases,
offload, home-side path-map storage and `vowp node paths`, launchd and WSS ingress are separate
jobs. Phase 0 `vowp agent run`
stays in the foreground; `--role` is accepted and carried in `RunOptions`, and the run-assignment
frame loop is `run_with_handler` with an injected handler. `vowp agent surface
start|attach|detach`, the MCP proxy and token rotation are later v0.4 slices. The local chat
bridge is this slice; the opencode app bridge and MCP proxy remain later. The real harness runner
(adapter selection, worktree creation, environment resolution and result push) that implements
`RunHandler` is the next executor job; this slice supplies the transport, spool and replay half
and tests it with a fake handler.
