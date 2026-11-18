# StackSpirit — Engineering Plan (Desktop First)

Status: planning baseline for `ss-0-plan`, 2026-11-14.
Audience: the `ss-1-core`, `ss-2-desktop-shell`, `ss-3-fleet-canvas`, and
`ss-4-engine-onboarding` workers, and the human owner.
Companion document: [`VOWP-CONTRACT.md`](VOWP-CONTRACT.md) pins the exact JSON read from
`/home/dev/vowp` and the cross-project dependency gates.

This document freezes the build order, module boundaries, data flow, UI specification,
and test strategy for the Desktop MVP. It writes no application code. It deliberately
does not add a Web app, a CLI, a second VOWP implementation, or any live feature that
VOWP has not shipped yet.

---

## 1. Product and principles

**StackSpirit is the desktop cockpit for VOWP.** A user adds one VPS, clicks
**Connect**, and sees the whole agent fleet without learning a CLI: the VPS node, worker
slots, running workers, job lanes, disk and memory headroom, the worker target VOWP
advises, provider limits, and the local engine that will run models.

Five principles decide the architecture:

1. **Desktop first.** The Tauri 2 desktop application and `crates/spirit-core` are the
   product. Web (`apps/spirit-web`) and CLI (`crates/spirit-cli`) are later adapters over
   the same UI boundary and the same Rust core.
2. **One bridge.** Every UI read and write goes through `SpiritBridge`. The desktop
   implementation is `TauriBridge`; tests and preview use `MockBridge`; a future Web app
   can use an HTTP/SSE implementation without rewriting views.
3. **Core owns host and VOWP logic.** SSH, VOWP command construction, parsing, host
   profiles, host-key pinning, engine supervision, keychain access, and gate probing live
   in `crates/spirit-core`. The Tauri host is a thin adapter; the UI never shells out.
4. **VOWP is the source of truth.** StackSpirit reads VOWP's own JSON and calls VOWP's
   own commands. It never opens a VOWP database, never writes a VOWP file, never
   reimplements dispatch, capacity, or merge logic, and never edits `/home/dev/vowp`.
5. **Honest gates.** Local-orchestrator execution (Phase D6) and live `.asif.zip`
   export/import (Phase D5) are gated on VOWP milestones. The app probes, displays, and
   disables those features with a reason. It does not fake completion, simulate a live
   session, or infer a capability from a version number alone.

### 1.1 Scope of the Desktop MVP (D1–D4)

- Host Manager: add one VPS profile, verify its host key, connect, disconnect, and show
  live latency and supervisor state.
- Fleet canvas: VPS root node, worker-slot ring, running worker cards, job lanes by state,
  the open inbox, and the current provider limits.
- Capacity and storage rail: disk free versus the VOWP reserve floor, RAM available
  versus the reserve and the p90 per-worker estimate, CPU load and pressure, current
  worker target versus VOWP's advised target, brake state, and waiting reasons.
- StackSpirit Engine onboarding: the bundled `opencode` sidecar presented as the
  StackSpirit Engine, included models, BYOK keys in the local OS keychain, and optional
  detection of an existing `opencode` or `claude-code` install.
- VOWP gate badges for ASIF (D5) and local orchestrators (D6).

### 1.2 Non-goals for the Desktop MVP

- No Web app, no CLI, no mobile surface.
- No multi-user accounts, no remote access to the desktop app, no public listener.
- No second VOWP home, database replica, scheduler, or supervisor.
- No automatic VPS provisioning, SSH key generation, VOWP installation, or VOWP
  supervisor start/stop. The app connects to an existing VOWP home.
- No automatic merge, deploy, release, or provider-key transfer to the VPS.
- No live local orchestrator or ASIF action before its gate is `ready`.
- No light theme, no plugin system, no extension marketplace.

---

## 2. Surface order and why it does not cause rework

```text
Now:      crates/spirit-core  ->  apps/spirit-desktop (Tauri 2 + React/TS)
Later:                         ->  apps/spirit-web   (same UI, HTTP/SSE bridge)
Later:    crates/spirit-cli    ->  same core
```

The reusable seams exist from day one:

| Seam | Desktop implementation | Later implementation | Why it survives |
|---|---|---|---|
| UI ↔ host | `TauriBridge` over Tauri IPC | `HttpSseBridge` over `vowp-daemon` or a small sidecar | Views only know `SpiritBridge`; no Tauri imports outside `src/bridge`. |
| Host/VOWP logic | `spirit-core` linked into `src-tauri` | `spirit-cli` links the same crate | No Tauri types in core; core is `async` Rust with trait-based I/O. |
| Wire model | `spirit-core::wire` parses VOWP JSON | same | One parser, one fixture suite, one compatibility policy. |
| Local engine | `EngineSupervisor` in core, launched by Tauri | CLI can supervise the same sidecar | Process management is core, not UI. |
| Gates | `LocalOrchestratorGate`, `AsifGate` in core | same states over HTTP | Gate logic is data, not a UI-only flag. |

The Web app is not scaffolded in D1–D4. The only preparation is keeping
`apps/spirit-desktop/src/bridge` free of Tauri-specific imports outside
`TauriBridge.ts`, and keeping `spirit-core` free of `tauri` dependencies.

---

## 3. Architecture

### 3.1 Process and data flow

```text
┌────────────────────────────── StackSpirit desktop ──────────────────────────────┐
│                                                                                 │
│  React 19 + TypeScript (Vite)                                                   │
│    views ──► SpiritBridge ──► TauriBridge ──invoke──┐                           │
│      ▲                                              │                           │
│      └──────── spirit://state event ────────────────┘                           │
│                                                     │                           │
│  src-tauri (Tauri 2 host)                           ▼                           │
│    commands ──► SpiritCore facade ──► HostStore                                 │
│                        │                ConnectionManager                       │
│                        │                VowpClient (trait)                       │
│                        │                EngineSupervisor                         │
│                        │                                                        │
│                        ├── SshRunner ──► system ssh ──► VPS ──► vowp --json ...  │
│                        └── Keychain ──► OS keychain (local only)                 │
│                                                                                 │
│  bundled sidecar: opencode ("StackSpirit Engine"), loopback only                │
└─────────────────────────────────────────────────────────────────────────────────┘
```

Trust boundaries:

- The **VPS** is the only VOWP home. StackSpirit is a read/write *client* through the
  VOWP CLI. It never copies a VOWP database or secret.
- The **desktop** holds host profiles, provider keys, and the local engine. Provider
  keys never leave the machine.
- **SSH** is outbound only. There is no externally reachable listener on the desktop;
  the local engine listens on `127.0.0.1` only. The app uses the system OpenSSH client,
  `BatchMode=yes`, strict host-key checking, and an app-private `known_hosts` file.
- **VOWP CLI JSON** is the wire. HTTPS/daemon APIs are not required for D1–D4.

### 3.2 Decisions frozen by this plan

| Decision | Choice | Consequence |
|---|---|---|
| Desktop framework | Tauri 2 (Rust host + React 19/TypeScript/Vite) | Small binary, native process/keychain/SSH access, one IPC boundary. |
| UI state | Zustand store fed by bridge snapshots/events | Small, testable, no server-state framework required. |
| Core wire model | Local serde types in `spirit-core::wire`, not a dependency on the VOWP crate | StackSpirit builds without `/home/dev/vowp`; golden fixtures enforce parity. |
| VOWP transport | System `ssh` running `vowp ... --json` | Reuses the owner's existing key-only SSH setup; no new VOWP listener. |
| SSH backend | `CommandRunner` trait with a system-OpenSSH implementation | Swappable; tests use a fake runner; a future `russh` backend can replace it. |
| Active host | One connected host at a time in D1–D4 | Simple state machine; multi-host is a later, explicit feature. |
| Polling | `fleet status --json` is the primary poll; capacity/queue/inbox are lower cadence or on demand | One snapshot drives the canvas; detail commands avoid over-polling. |
| Snapshot model | Parse the `snapshot` object inside `FleetStatus`; never read the remote file directly in normal operation | Gets VOWP's `source`, `fetched_at`, and `stale` fields. |
| Engine | Bundled `opencode` sidecar presented as "StackSpirit Engine"; local loopback only | Zero-install UX, no VPS dependency, no branding prerequisite. |
| Secret storage | OS keychain through a `Keychain` trait | No plaintext keys in profiles, logs, fixtures, or reports. |
| Gate probing | Capability probes with explicit `Unknown`/`Probing`/`Waiting`/`Ready`/`Incompatible` states | No version-only claims, no silent enablement. |
| Visual theme | Dark-first cockpit, tokenized | Fast to build, fits an operational tool; light theme is later. |

### 3.3 Dependency rules

1. `spirit-core` must not depend on `tauri`, `react`, or any UI crate.
2. `src-tauri` must not contain VOWP parsing, SSH command construction, or keychain
   policy. It converts IPC payloads to core calls and emits core events.
3. React components must not call `@tauri-apps/api` directly. Only `TauriBridge.ts` may
   import Tauri APIs.
4. `spirit-core` must not depend on `/home/dev/vowp` as a path dependency. Fixtures and
   docs copy the contract; the VOWP repository stays read-only.
5. No UI code may infer capacity, gate, or job state. The core derives view models from
   VOWP JSON; the UI renders them.
6. No secret may cross the SSH boundary. Commands contain provider/model *names* only.
7. Every write is a VOWP command. StackSpirit never writes `snapshot.json`, a project
   database, a job row, or a lease.

---

## 4. Repository and module layout

### 4.1 Target tree (D1–D4 plus reserved later surfaces)

```text
stackspirit/
├── Cargo.toml                       # Rust workspace: spirit-core, src-tauri
├── Cargo.lock
├── rust-toolchain.toml
├── package.json                     # UI workspace scripts (npm)
├── package-lock.json
├── docs/
│   ├── PLAN.md                      # this document
│   └── VOWP-CONTRACT.md             # exact VOWP JSON and gates
├── crates/
│   └── spirit-core/
│       ├── Cargo.toml
│       ├── fixtures/
│       │   └── vowp/                # golden JSON copied from the contract
│       ├── src/
│       │   ├── lib.rs
│       │   ├── error.rs
│       │   ├── time.rs
│       │   ├── wire/                # VOWP wire types (see §5)
│       │   ├── vowp/                # VowpClient trait + SSH/fixture/mock clients
│       │   ├── ssh/                 # command runner, target, host-key helpers
│       │   ├── host/                # profiles, store, fingerprint, probe
│       │   ├── connection/          # state machine, poll loop, latency
│       │   ├── view/                # FleetView, CapacityView, lane mapping
│       │   ├── engine/              # sidecar, catalog, keychain, gates
│       │   ├── service.rs           # SpiritCore facade
│       │   └── fixtures.rs          # feature-gated fixture loader
│       └── tests/                   # integration tests over fixtures
├── apps/
│   └── spirit-desktop/
│       ├── package.json
│       ├── vite.config.ts
│       ├── tsconfig.json
│       ├── index.html
│       ├── src/
│       │   ├── main.tsx
│       │   ├── app/                 # AppShell, TopBar, view routing
│       │   ├── bridge/              # SpiritBridge + TauriBridge + MockBridge
│       │   ├── state/               # Zustand store, selectors
│       │   ├── domain/              # pure formatting and lane/gauge helpers
│       │   ├── components/          # design-system primitives
│       │   ├── views/
│       │   │   ├── hosts/           # Host Manager
│       │   │   ├── fleet/           # canvas, ring, worker cards, job lanes
│       │   │   ├── capacity/        # storage/capacity rail
│       │   │   ├── engine/          # StackSpirit Engine onboarding
│       │   │   └── inbox/           # inbox drawer
│       │   └── test/                # render helpers and fixture scenarios
│       └── src-tauri/
│           ├── Cargo.toml
│           ├── build.rs
│           ├── tauri.conf.json
│           ├── capabilities/default.json
│           ├── binaries/            # fetched opencode sidecar (gitignored)
│           └── src/
│               ├── main.rs
│               ├── lib.rs
│               ├── state.rs
│               ├── commands/        # hosts, connection, fleet, engine, gates
│               ├── events.rs
│               └── error.rs
└── (later)
    ├── apps/spirit-web/
    └── crates/spirit-cli/
```

### 4.2 `crates/spirit-core` modules

| Module | Responsibility | Must not do |
|---|---|---|
| `error` | `SpiritError`, stable codes, redaction-safe display | Include secret values or raw provider keys |
| `time` | canonical timestamp newtype and clock trait | Depend on system time in pure view code |
| `wire` | serde types mirroring VOWP JSON; enum tolerance; parse helpers | Perform I/O or call VOWP |
| `vowp` | `VowpClient` trait and SSH/fixture/mock clients | Put Tauri or UI types in the client |
| `ssh` | `CommandRunner`, `SshTarget`, remote command quoting, timeouts, output caps, host-key/fingerprint helpers | Interpolate unquoted user text or build shell pipelines |
| `host` | `HostProfile`, `HostStore`, atomic JSON persistence, probe | Store keys, passwords, or tokens |
| `connection` | state machine, poll scheduling, backoff, staleness, latency stats, cancellation | Render or cache derived UI colors |
| `view` | deterministic projection of VOWP documents into `FleetView`/`CapacityView`/lanes | Invent missing data; guess unknown enums |
| `engine` | sidecar discovery/launch/health, model catalog, keychain trait, external harness scan, gate probes | Send a key to the VPS; expose a key in logs |
| `service` | `SpiritCore` facade used by Tauri/CLI | Depend on Tauri |
| `fixtures` (feature) | compile-time fixture loading for preview/tests | Be enabled in a release build |

### 4.3 `apps/spirit-desktop` modules

| Path | Responsibility |
|---|---|
| `src/app/AppShell.tsx` | three-column cockpit, top bar, view switching, global banners |
| `src/app/TopBar.tsx` | host chip, Connect/Disconnect, latency, supervisor status, inbox, gate badges |
| `src/bridge/SpiritBridge.ts` | the typed interface; the only contract the UI knows |
| `src/bridge/wire.ts` | TypeScript mirror of the core view models and VOWP wire types |
| `src/bridge/TauriBridge.ts` | `invoke` wrappers and `listen` subscription; the only Tauri import |
| `src/bridge/MockBridge.ts` | scenario-driven bridge for Vitest, Playwright, and preview |
| `src/state/fleetStore.ts` | revisioned store; applies state events; disables stale actions |
| `src/views/hosts/**` | host list, add wizard, fingerprint confirmation, connection controls |
| `src/views/fleet/**` | VPS node, slot ring, running worker cards, job lanes, drawer |
| `src/views/capacity/**` | target advisor, disk, RAM, CPU, brake, limits, waiting reasons |
| `src/views/engine/**` | engine status, included models, BYOK, external harness, gate cards |
| `src/components/**` | buttons, badges, gauges, sheets, tooltips, empty/error states |
| `src-tauri/src/commands/**` | thin async commands; no business logic |
| `src-tauri/src/events.rs` | forwards `SpiritEvent` to `spirit://state` |
| `src-tauri/src/state.rs` | `AppState { core: Arc<SpiritCore> }`, lifecycle hooks |

### 4.4 Build and packaging

- Rust: edition 2024, one workspace, `Cargo.lock` committed.
- UI: npm, Vite, React 19, TypeScript strict, ESLint + Prettier, Vitest + React Testing
  Library, Playwright for browser-mode flows.
- Tauri 2: `src-tauri` as a workspace member; narrow capabilities; no shell plugin
  permission for the renderer; file dialog permission only for choosing an identity file
  path; the sidecar is launched only by Rust.
- Sidecar: `src-tauri/binaries/opencode-<target-triple>` fetched by
  `scripts/fetch-engine.mjs` from a pinned manifest (version + SHA-256 + license),
  never edited by hand. CI verifies the checksum before `tauri build`.
- Platform matrix: macOS arm64 (primary), Linux x86_64, Windows x86_64. System `ssh`
  must be present; the app detects and explains if it is missing (Windows optional
  feature). A future `russh` backend removes that dependency.

---

## 5. Frozen contracts and domain model

### 5.1 Wire model rule

`spirit-core::wire` is a **local, dependency-free mirror** of the VOWP documents in
[`VOWP-CONTRACT.md`](VOWP-CONTRACT.md). It does not import `vowp-protocol`, does not
read `/home/dev/vowp`, and is not generated at runtime. The parity tests in
`crates/spirit-core/tests/contract.rs` parse golden fixtures copied from the contract and
assert every field the UI relies on.

Compatibility policy:

- Unknown object fields are ignored (serde default), so VOWP may add fields.
- Known enum values are exhaustive; any new value becomes `Unknown` and is displayed as
  an explicit **Unknown** chip, never guessed as a known state.
- Missing required fields fail the whole document with a `parse` error and a document
  name; StackSpirit keeps the last good snapshot and shows a red freshness banner.
- Timestamps are canonicalized to `YYYY-MM-DDTHH:MM:SS.mmmZ` (UTC, exactly three
  fractional digits), matching VOWP.
- IDs are strings at the wire boundary and typed newtypes in core:
  ULID-backed `SessionId`, `EventId`, `RunId`, `InboxId`, `CapsuleId`; slug-backed
  `JobId`, `NodeName`, `ProjectName`.
- `Option` fields accept both `null` and absence where VOWP uses defaults; the core
  serializes them as `null`.

### 5.2 Core public types (frozen names and shapes)

```rust
// crates/spirit-core/src/host/profile.rs
pub struct HostId(pub String); // ULID

pub struct SshTarget {
    pub user: String,
    pub host: String,
    pub port: u16,
}

pub struct HostKeyPin {
    pub algorithm: String,       // e.g. "ssh-ed25519"
    pub sha256: String,          // "SHA256:..." exactly as ssh-keygen prints it
    pub pinned_at: Timestamp,
}

pub struct HostProfile {
    pub id: HostId,
    pub label: String,
    pub ssh: SshTarget,
    pub identity_file: Option<PathBuf>, // path only, never key material
    pub vowp_home: Option<String>,      // remote --home, optional
    pub default_project: Option<String>, // remote --project for scoped actions, optional
    pub host_key: HostKeyPin,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub last_connected_at: Option<Timestamp>,
}
```

```rust
// crates/spirit-core/src/vowp/client.rs
#[async_trait]
pub trait VowpClient: Send + Sync {
    async fn probe(&self) -> Result<VowpProbe, SpiritError>;
    async fn fleet_status(&self) -> Result<FleetStatus, SpiritError>;
    async fn capacity_report(&self) -> Result<CapacityReportDoc, SpiritError>;
    async fn fleet_queue(&self) -> Result<Vec<FleetQueueRow>, SpiritError>;
    async fn fleet_inbox(&self, all: bool) -> Result<Vec<InboxItem>, SpiritError>;
    async fn job_action(&self, project: &str, job: &str, action: JobAction) -> Result<(), SpiritError>;
    async fn answer_inbox(&self, project: Option<&str>, id: &str, answer: &str) -> Result<InboxItem, SpiritError>;
    async fn set_target(&self, target: u32, mode: TargetMode, override_: bool, reason: &str)
        -> Result<CapacityTarget, SpiritError>;
}

pub struct VowpProbe {
    pub version: String,
    pub latency_ms: u64,
    pub protocol_compatible: bool,
}

pub struct CapacityReportDoc {
    #[serde(flatten)]
    pub report: CapacityReport,          // flat VOWP document, not a nested "report"
    #[serde(default)]
    pub provider_caps: BTreeMap<String, u32>,
    #[serde(default)]
    pub reload_pending: bool,
}
```

```rust
// crates/spirit-core/src/connection/state.rs
pub enum ConnectionState {
    Disconnected,
    Connecting { host_id: HostId, stage: ConnectStage },
    Connected { host_id: HostId, since: Timestamp, latency: LatencyStats },
    Stale { host_id: HostId, last_ok_at: Timestamp, error: String },
    Disconnecting { host_id: HostId },
    Failed { host_id: HostId, error: SpiritError, retryable: bool },
}

pub enum ConnectStage {
    ResolvingHostKey,
    OpeningSsh,
    CheckingVowp,
    ReadingSnapshot,
}

pub struct LatencyStats {
    pub last_ms: u64,
    pub ewma_ms: f64,
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub samples: u32,
}
```

```rust
// crates/spirit-core/src/view/fleet.rs
pub struct FleetView {
    pub revision: u64,
    pub host_id: HostId,
    pub fetched_at: Timestamp,
    pub generated_at: Timestamp,
    pub source: String,          // "local" | "remote" | "cache", from FleetStatus
    pub stale: bool,
    pub home: String,
    pub supervisor: SupervisorView,
    pub projects: Vec<ProjectView>,
    pub capacity: CapacityView,
    pub running: Vec<WorkerCardView>,
    pub lanes: JobLanes,
    pub inbox: Vec<InboxItemView>,
    pub sessions: Vec<SessionCardView>,
    pub orchestrators: Vec<OrchestratorView>,
    pub latency: LatencyStats,
}

pub struct SupervisorView {
    pub running: bool,
    pub pid: Option<u32>,
    pub paused: bool,
    pub last_tick_at: Option<Timestamp>,
    pub last_tick_age_secs: Option<u64>,
    pub tick_secs: u64,
    pub fail_streak: u32,
    pub version: String,
}

pub struct ProjectView {
    pub name: String,
    pub state: String,           // active | paused | archived | unknown
    pub paused_reason: String,
    pub jobs_by_state: BTreeMap<String, u32>,
    pub cost_today_usd: Option<f64>,
}

pub struct WorkerCardView {
    pub job_id: String,
    pub kind: String,
    pub role: String,
    pub state: String,           // active | stalled | unknown
    pub stalled: bool,
    pub executor: Option<ExecutorView>,
    pub node: String,
    pub age_secs: u64,
    pub last_event_age_secs: Option<u64>,
    pub session_id: Option<String>,
}

pub struct ExecutorView {
    pub raw: String,             // exact VOWP executor string
    pub harness: String,         // claude-code | opencode | cli | acp | vowp | unknown
    pub provider: String,        // parsed provider key, e.g. "deepseek"
    pub model: String,
    pub effort: Option<String>,
    pub variant: Option<String>,
    pub display: String,         // friendly label when known, else raw
}

pub struct JobLanes {
    pub running: Vec<JobCardView>,
    pub ready: Vec<JobCardView>,
    pub review_fix: Vec<JobCardView>,
    pub parked_blocked: Vec<JobCardView>,
    pub done: Vec<JobCardView>,
    pub counts: BTreeMap<String, u32>,
}

pub struct JobCardView {
    pub job_id: String,
    pub project: String,
    pub title: Option<String>,
    pub kind: String,
    pub role: String,
    pub state: String,
    pub priority: i32,
    pub age_secs: u64,
    pub reason: Option<String>,
    pub detail: Option<String>,
    pub gate: Option<String>,
    pub gate_at: Option<Timestamp>,
    pub gate_evidence: Option<String>,
    pub executor: Option<ExecutorView>,
    pub session_id: Option<String>,
}
```

```rust
// crates/spirit-core/src/view/capacity.rs
pub struct CapacityView {
    pub generated_at: Timestamp,
    pub measured_at: Option<Timestamp>,
    pub report_loaded: bool,          // false until capacity report succeeds
    pub host: Option<HostSampleView>,
    pub running: RunningCapacityView,
    pub target: TargetView,
    pub advised_workers: Option<u32>,
    pub brake: BrakeView,
    pub reserve_ram_bytes: u64,
    pub min_disk_bytes: u64,
    pub disk: DiskView,
    pub ram: RamView,
    pub cpu: CpuView,
    pub estimates: Vec<EstimateView>,
    pub waiting: Vec<WaitingJobView>,
    pub limits: Vec<LimitView>,
    pub provider_caps: BTreeMap<String, u32>,
    pub reload_pending: bool,
}

pub struct DiskView {
    pub free_home_bytes: u64,
    pub free_worktrees_bytes: Option<u64>,
    pub floor_bytes: u64,
    pub source: String,          // "snapshot" | "capacity_report"
    pub state: ResourceState,    // ok | warn | critical | unknown
}

pub struct RamView {
    pub available_bytes: u64,
    pub reserve_bytes: u64,
    pub running_p90_bytes: u64,
    pub per_worker_p90_bytes: Option<u64>,
    pub per_worker_source: Option<String>, // measured | pooled | default
    pub per_worker_samples: Option<u32>,
    pub estimated_extra_slots: Option<u32>,
    pub state: ResourceState,
}

pub struct CpuView {
    pub load1: f64,
    pub cpu_count: u32,
    pub load_ratio: f64,
    pub pressure_avg10: Option<f64>,
    pub state: ResourceState,
}

pub enum ResourceState { Ok, Warn, Critical, Unknown }
```

```rust
// crates/spirit-core/src/engine/mod.rs
pub enum EngineChoice {
    Bundled,
    External { path: PathBuf, kind: ExternalHarnessKind },
}

pub enum ExternalHarnessKind { Opencode, ClaudeCode }

pub enum EngineState {
    NotConfigured,
    Stopped,
    Starting,
    Ready,
    Degraded { detail: String },
    Failed { detail: String },
}

pub struct EngineStatus {
    pub choice: EngineChoice,
    pub state: EngineState,
    pub version: Option<String>,
    pub binary_path: Option<PathBuf>, // local path; never sent to VPS
    pub port: Option<u16>,            // loopback only
    pub selected_model: Option<ModelChoice>,
    pub included_models_available: bool,
    pub started_at: Option<Timestamp>,
    pub last_error: Option<String>,
}

pub enum ModelAccess { Included, Byok }

pub struct ModelChoice {
    pub id: String,               // engine-native id
    pub label: String,
    pub provider: String,
    pub access: ModelAccess,
    pub context_tokens: Option<u64>,
    pub notes: String,
    pub verified_at: Option<Timestamp>,
}
```

```rust
// crates/spirit-core/src/engine/keychain.rs
pub trait Keychain: Send + Sync {
    fn set(&self, provider: &str, secret: &SecretString) -> Result<(), KeychainError>;
    fn get(&self, provider: &str) -> Result<Option<SecretString>, KeychainError>;
    fn delete(&self, provider: &str) -> Result<(), KeychainError>;
    fn list(&self) -> Result<Vec<String>, KeychainError>;
}
```

`SecretString` is a zeroizing wrapper with no `Debug`, `Display`, or `Serialize`
implementation. `KeychainError` carries the provider name and an OS error class, never a
value.

```rust
// crates/spirit-core/src/engine/gate.rs
#[derive(Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LocalOrchestratorGate {
    Unknown,
    Probing,
    Ready {
        agent_protocol: String,
        node: Option<String>,
        capabilities: Vec<String>,
        evidence: Vec<String>,
    },
    WaitingForVowpPhase3 {
        missing: Vec<String>,          // milestone/capability labels
        detected_version: Option<String>,
        evidence: Vec<String>,
    },
    Incompatible {
        detected: String,
        required: String,
        evidence: Vec<String>,
    },
    Unreachable { detail: String },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AsifGate {
    Unknown,
    Probing,
    Ready {
        export: bool,
        import: bool,
        detected_version: Option<String>,
        evidence: Vec<String>,
    },
    WaitingForVowpAsif {
        missing: Vec<String>,          // exact v4-asif-* milestone names
        detected_version: Option<String>,
        evidence: Vec<String>,
    },
    Incompatible {
        detected: String,
        required: String,
        evidence: Vec<String>,
    },
    Unreachable { detail: String },
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalAgentState {
    Unknown,
    NotInstalled,
    NotEnrolled,
    Connecting,
    Connected,
    Reconnecting,
    Revoked,
    Superseded,
    Stopped,
    Incompatible,
}

#[derive(Serialize, Deserialize)]
pub struct LocalAgentStatus {
    pub state: LocalAgentState,
    pub node: Option<String>,
    pub protocol: Option<String>,
    pub last_heartbeat: Option<Timestamp>,
    pub last_error: Option<String>,
}

pub struct GateFeatureFlags {
    pub asif_actions: bool,               // false in D4; true only in D5
    pub local_orchestrator_actions: bool, // false in D4; true only in D6
}

pub struct GateSnapshot {
    pub probed_at: Timestamp,
    pub local_orchestrator: LocalOrchestratorGate,
    pub asif: AsifGate,
    pub local_agent: LocalAgentStatus,
    pub features: GateFeatureFlags,
}
```

```rust
// crates/spirit-core/src/error.rs
pub struct SpiritError {
    pub code: SpiritErrorCode,
    pub message: String,       // safe to show; no secret, no raw key
    pub hint: Option<String>,
    pub retryable: bool,
}

pub enum SpiritErrorCode {
    InvalidInput,
    NotFound,
    Conflict,
    Refused,
    Limit,
    Auth,
    HostNotFound,
    HostKeyMismatch,
    SshAuth,
    SshUnavailable,
    Remote,
    RemoteTimeout,
    VowpMissing,
    VowpIncompatible,
    VowpCommand,
    Parse,
    Keychain,
    Engine,
    Gate,
    Io,
    Internal,
}
```

### 5.3 `SpiritBridge` TypeScript interface (frozen)

```ts
// apps/spirit-desktop/src/bridge/SpiritBridge.ts
export interface SpiritBridge {
  hosts: {
    list(): Promise<HostProfile[]>;
    add(input: NewHostProfile): Promise<HostProfile>; // includes the confirmed pin
    update(id: string, patch: HostProfilePatch): Promise<HostProfile>;
    remove(id: string): Promise<void>;
    scanFingerprint(input: HostDraft): Promise<HostKeyScan>;
    trustFingerprint(id: string, pin: HostKeyPin): Promise<HostProfile>; // re-pin only
    probe(input: HostDraft): Promise<HostProbe>;
  };
  connection: {
    connect(hostId: string): Promise<ConnectionSnapshot>;
    disconnect(hostId: string): Promise<void>;
    status(): Promise<ConnectionSnapshot>;
    subscribe(listener: (event: StateEvent) => void): () => void;
  };
  fleet: {
    get(): Promise<FleetView | null>;
    refresh(): Promise<FleetView>;
    jobAction(jobId: string, action: JobAction): Promise<ActionResult>;
    inbox(all: boolean): Promise<InboxItemView[]>;
    answerInbox(id: string, answer: string): Promise<InboxItemView>;
    setTarget(input: SetTargetInput): Promise<CapacityTargetView>;
  };
  engine: {
    status(): Promise<EngineStatus>;
    models(): Promise<ModelChoice[]>;
    selectModel(id: string): Promise<EngineStatus>;
    storeKey(provider: string, secret: string): Promise<KeyStatus>;
    verifyKey(provider: string): Promise<KeyStatus>;
    deleteKey(provider: string): Promise<KeyStatus>;
    keyStatus(): Promise<KeyStatus[]>;
    scanExternal(): Promise<ExternalHarness[]>;
    selectExternal(kind: ExternalHarnessKind, path: string): Promise<EngineStatus>;
    useBundled(): Promise<EngineStatus>;
    restart(): Promise<EngineStatus>;
  };
  gates: {
    get(): Promise<GateSnapshot>;
    probe(): Promise<GateSnapshot>;
  };
}
```

`NewHostProfile` is the pre-save form plus the confirmed `HostKeyPin`; `HostDraft` is the
same form before the pin is confirmed. This keeps the first save atomic: StackSpirit never
stores a profile without a pin, and never connects before the user confirms it.

The remaining DTOs are concrete:

```ts
export type ResourceState = "ok" | "warn" | "critical" | "unknown";

export interface LatencyStats {
  last_ms: number;
  ewma_ms: number;
  p50_ms: number;
  p95_ms: number;
  samples: number;
}
export interface SpiritError {
  code: string;
  message: string;
  hint: string | null;
  retryable: boolean;
}

export interface HostDraft {
  label: string;
  ssh: { user: string; host: string; port: number };
  identity_file: string | null;
  vowp_home: string | null;
  default_project: string | null;
}
export interface HostKeyPin {
  algorithm: string;
  sha256: string;
  pinned_at: string;
}
export interface HostProfile extends HostDraft {
  id: string;
  host_key: HostKeyPin;
  created_at: string;
  updated_at: string;
  last_connected_at: string | null;
}
export type JobAction = "retry" | "park" | "cancel";
export interface NewHostProfile extends HostDraft { host_key: HostKeyPin }
export interface HostProfilePatch extends Partial<HostDraft> {}
export interface HostKeyScan {
  keys: { algorithm: string; sha256: string; preferred: boolean }[];
  scanned_at: string;
}
export interface HostProbe {
  reachable: boolean;
  auth_ok: boolean;
  host_key_ok: boolean;
  vowp_version: string | null;
  latency_ms: number;
  stages: { stage: string; ok: boolean; detail: string }[];
}
export interface ConnectionSnapshot {
  state: string;
  host_id: string | null;
  since: string | null;
  latency: LatencyStats | null;
  last_ok_at: string | null;
  error: SpiritError | null;
}
export interface SetTargetInput {
  target: number;
  mode: "auto" | "fixed";
  override: boolean;
  reason: string;
}
export interface ActionResult {
  job_id: string;
  action: JobAction;
  request: string | null;
  refreshed: boolean;
}
export interface KeyStatus {
  provider: string;
  state: "missing" | "saved" | "invalid" | "unverified";
  last_checked_at: string | null;
}
export interface ExternalHarness {
  kind: "opencode" | "claude-code";
  path: string;
  version: string | null;
  usable: boolean;
}
```

`InboxItemView`, `SessionCardView`, `OrchestratorView`, `WaitingJobView`, `LimitView`,
`EstimateView`, `HostSampleView`, `RunningCapacityView`, `TargetView`, `BrakeView`, and
`CapacityTargetView` are the TypeScript mirrors of the Rust structs in §5.2 and
`VOWP-CONTRACT.md` §4–§5, with `Option` mapped to `| null`. The Rust struct is the source
of truth; `wire.ts` is checked against it in the UI type tests.

`StateEvent` is a discriminated union with a monotonically increasing `revision`:

```ts
export type StateEvent =
  | { kind: "connection"; revision: number; connection: ConnectionSnapshot }
  | { kind: "fleet"; revision: number; fleet: FleetView }
  | { kind: "engine"; revision: number; engine: EngineStatus }
  | { kind: "gates"; revision: number; gates: GateSnapshot }
  | { kind: "error"; revision: number; error: SpiritError };
```

The bridge returns **view models**, not raw VOWP JSON. Raw documents stay in core; the
UI cannot accidentally depend on a field VOWP may rename.

### 5.4 Deterministic view derivation

All derivations are pure functions in `spirit-core::view` and are unit-tested.

**Job lane mapping** (exact):

| Lane | VOWP `JobState` values | Card source |
|---|---|---|
| Running | Any lock-holding state: `running`, `stalled`, `gating`, `reviewing`, `fixing`, `merging` | `ProjectSnapshot.running`; the card has no state, so its chip is **Active** unless a matching queue row says `stalled`. |
| Ready | `pending`, `ready` | `ProjectSnapshot.queue` with those states |
| Review & Fix | `gating`, `reviewing`, `fixing` | `ProjectSnapshot.gate_failures` cards + `jobs_by_state` counts |
| Parked & Blocked | `parked`, `failed`, `cancelled` | `ProjectSnapshot.blocked` + queue rows in those states |
| Done | `done`, `replanned` | `jobs_by_state` counts; cards are summarized, not live |

- `jobs_by_state` is authoritative for lane counts. Card lists are only what the
  snapshot includes; a lane may show `N more` without loading more.
- Cards are de-duplicated by `(project, job id)`. A running card wins over a queue or
  blocked row for the same job; a blocked card wins over a queue row. VOWP may list a
  job in more than one card list during a transition, and the UI must not show it twice.
- `queue` is capped at 200 by VOWP. At exactly 200, the lane footer says
  "showing the first 200 jobs" — never "200 total".
- `BlockedJobCard` has no title. The card shows the job id, state, reason, and the
  redacted detail. A title is filled only if the same job appears in the queue list. Full
  job detail is a later read and is not part of D1–D4's frozen contract.
- `JobCardView.priority` is the queue row's priority; running and blocked cards have no
  priority field, so they use `0` and the UI hides the priority dot for them.
- `WorkerCardView.state` is `active` by default because `RunningJobCard` carries no
  state. If the same job appears in `ProjectSnapshot.queue` with state `stalled`, the card
  is marked `stalled`; no other state is inferred from age or last-event age. The tooltip
  says the card may be running, gating, reviewing, fixing, or merging.
- `RunningJobCard.executor` may be `null` for shell, gate, or merge runs. The badge shows
  **shell** and no provider/model chip.
- Unknown `JobState` values go to **Parked & Blocked** with an **Unknown** chip and are
  not silently treated as ready or running.

**Executor parsing** follows VOWP's canonical grammar
`harness:model[@effort][#variant]`:

- `@effort` only for `claude-code`; `#variant` only for `opencode`.
- Provider is `claude` for `claude-code`, the segment before `/` for `opencode`, and the
  model itself for `cli`/`acp`/`vowp`, matching VOWP's own `Executor::provider()`.
- A malformed string is not normalized. `ExecutorView` keeps `raw` and sets
  `harness: "unknown"`, `display: raw`.
- Friendly names are a small lookup (`deepseek/deepseek-flash#max` →
  "DeepSeek V4.1 Flash · max"); unknown models always display the raw model id.

**Target advice precedence**: use `capacity.target.advised_workers` when non-null, else
`capacity.advised_target`, else `null` ("VOWP has no advice yet"). `target.is_override`
is shown whenever true, even if the advised value is missing.

**Per-worker RAM estimate selection**, in order:

1. An exact `UsageEstimate` matching the visible job's project, kind, harness, model, and
   build mode.
2. A pooled estimate matching kind + harness (null project/model/build mode).
3. A `default` estimate for the same kind/harness, when present.
4. The largest `ram_p90_bytes` among estimates with `samples > 0`.
5. No estimate: show the reserve only and label the p90 as "not measured yet".

The rail always shows the estimate source (`measured`, `pooled`, `default`) and sample
count. It never presents a `default` estimate as measured. If `report_loaded` is false,
the estimates/waiting/caps panels say **capacity detail unavailable**, not "none" and not
"zero".

**Capacity threshold policy** (UI guidance, not a VOWP decision):

| Gauge | Ok | Warn | Critical |
|---|---|---|---|
| Disk free vs floor | `free >= 1.25 * floor` | `floor <= free < 1.25 * floor` | `free < floor` |
| RAM available vs reserve | `available >= 2 * reserve` | `reserve <= available < 2 * reserve` | `available < reserve` |
| CPU load ratio (`load1 / cpu_count`) | `< 0.70` | `0.70 .. 1.00` | `> 1.00` |
| CPU pressure `avg10` | `< 10` | `10 .. 25` | `> 25` |

These thresholds only color the gauges. The authoritative admission decision is VOWP's
`brake` and `waiting` data, which is displayed verbatim.

**Freshness**:

- `stale = FleetStatus.stale || (fetched_at is older than max(3 * supervisor.tick_secs, 180s))`.
- A stale snapshot is dimmed, labeled "as of HH:MM:SS", and all mutations require a
  successful refresh first.
- A parse failure keeps the last good view but marks it stale and shows the parse error.
  It never renders partial data as complete.

### 5.5 Error presentation

Every error crossing the bridge has a stable `code` that maps to UI copy. VOWP exit
codes map explicitly: 2 → `invalid_input`, 3 → `not_found`, 4 → `conflict`, 5 →
`refused`, 6 → `limit`/`auth`, 7 → `remote`; 1 and unexpected failures map to
`vowp_command` or `internal`, never silently to success.

| Code | Where it appears | Retry? |
|---|---|---|
| `host_not_found` | Host Manager | No; refresh list |
| `not_found` | job/session/inbox action | Yes after refresh |
| `conflict` | action toast | Yes after refresh; never auto-retry |
| `refused` | action toast + details | No until the user changes the input |
| `limit` / `auth` | limits panel / engine | Provider reset or key fix |
| `remote` | connection banner | Yes with backoff |
| `host_key_mismatch` | blocking dialog with old/new SHA-256 | No; explicit re-pin only |
| `ssh_auth` | Host Manager + topbar | Yes after the user fixes the key/agent |
| `ssh_unavailable` | first-run setup guidance | Yes |
| `remote_timeout` | topbar amber banner | Yes with backoff |
| `vowp_missing` | Host Manager probe | Yes after VOWP is installed |
| `vowp_incompatible` | topbar + gate card | No until VOWP is updated |
| `vowp_command` | action toast + drawer detail | Depends on VOWP exit code |
| `parse` | freshness banner | Yes; keeps last good data |
| `keychain` | engine key form | Yes |
| `engine` | engine card | Yes; restart |
| `gate` | gate detail | Manual re-probe |

Raw VOWP stderr is shown only after redaction and only in an expandable "Details" area.
It is never used as the primary message.

---

## 6. Runtime data flow

### 6.1 Connect sequence

1. The user saves a host profile. `hosts.scanFingerprint` runs `ssh-keyscan` against the
   host and port, computes SHA-256 fingerprints with `ssh-keygen -lf`, and returns all
   offered key types. Nothing is trusted yet.
2. The user compares the fingerprint with the server console and confirms. Core writes
   the selected key lines into the app-private `known_hosts` and stores the SHA-256 pin
   in the profile. Connection uses `StrictHostKeyChecking=yes` and that file.
3. `connection.connect` probes `vowp --version`, then `vowp --json fleet status`.
4. On success, core starts the poll loop and emits `connection` + `fleet` events. The
   topbar changes from **Connect** to **Disconnect**.
5. On failure, the state is `Failed` with a stage (`CheckingVowp`, `ReadingSnapshot`) and
   a retryable flag. The profile remains saved.

No password authentication is supported in D1–D4. The app explains that it uses the
user's SSH agent or an identity file path and never stores a passphrase.

### 6.2 Poll loop

| Data | Command | Connected | Idle/backoff | Notes |
|---|---|---|---|---|
| Fleet snapshot | `vowp --json fleet status` | every 4 s (±10% jitter) | 15 s | Primary poll; also gives `source`, `fetched_at`, `stale`. |
| Capacity detail | `vowp --json capacity report` | every 15 s | 60 s | Needed for estimates, waiting reasons, `provider_caps`. |
| Fleet queue | `vowp --json fleet queue` | every 10 s while a lane is visible | 30 s | Optional; snapshot queue is the default source. |
| Fleet inbox | `vowp --json fleet inbox --all` | every 15 s while the inbox is open | 60 s | Full bodies; snapshot inbox cards omit `body`. |
| Host probe | `vowp --version` | on connect and manual retry | — | Version compatibility check. |
| Gate probe | see `VOWP-CONTRACT.md` §11 | on connect and manual re-probe | — | Cached per connection. |

Rules:

- Polls pause when the window is hidden and refresh immediately on focus.
- Three consecutive failures degrade to `Stale`, then back off to 30 s. A successful
  command restores `Connected` and resets the backoff.
- One poll runs at a time per document. A slow `fleet status` does not stack up; the
  next tick is skipped and the latency sample is recorded.
- Capacity, queue, and inbox polls share the SSH runner; each has an independent
  cancellation token.
- Every snapshot gets a monotonic `revision`. The UI drops events with a lower revision.
- The last good `FleetView` is kept in memory and displayed dimmed when disconnected. It
  is not persisted across app restarts in D1–D4.

### 6.3 Write flow

All mutations are **command → refresh → render**. No optimistic mutation of VOWP state.

```text
button intent
  -> SpiritBridge.fleet.setTarget({target, mode, override, reason})
  -> Tauri command
  -> core validates input + freshness + advised override rule
  -> ssh: vowp --json capacity set --target N --mode M [--override] --reason "..." --by stackspirit
  -> parse CapacityTarget or VOWP error
  -> refresh fleet status
  -> emit fleet event; toast success or VOWP error
```

Details:

- `capacity set` above the advised target requires an explicit confirmation and
  `--override`. The UI copies VOWP's own rule; it does not relax it.
- Job actions are `vowp --project <job-project> --json job retry|park|cancel <id>`. The
  button is enabled only for states VOWP accepts; a VOWP conflict is shown as a conflict,
  not a bug.
- Inbox answers are `vowp --project <item-project> --json inbox answer <id> <answer>`
  when the item has a project; node-inbox items run without `--project`. Empty and
  over-long answers are rejected locally.
- All local processes are spawned with an argv array. The remote VOWP command is encoded
  with POSIX single-quoting for every argument, so spaces and quotes in a reason or an
  inbox answer cannot split or inject a shell command.
- After a successful write, the core schedules an immediate fleet refresh rather than
  waiting for the next tick.

### 6.4 SSH command construction

```text
ssh
  -o BatchMode=yes
  -o StrictHostKeyChecking=yes
  -o UserKnownHostsFile=<app-data>/known_hosts
  -o ConnectTimeout=8
  -o ServerAliveInterval=15
  -o ServerAliveCountMax=3
  [-i <identity_file> -o IdentitiesOnly=yes]
  -p <port>
  -- <user>@<host>
  <remote-command>
```

- `<remote-command>` is built from a fixed VOWP argv array by
  `remote_command(argv)`: every argument is POSIX single-quoted (`'` becomes `'\''`) and
  joined with spaces. SSH necessarily hands the remote command to the remote login shell;
  StackSpirit never interpolates unquoted user text and never uses a pipeline, `;`, `&&`,
  or redirection. The local `ssh` process itself is spawned with an argv array, so there
  is no local shell.
- `IdentitiesOnly=yes` is set **only** when `-i` is supplied. Without an identity file the
  user's SSH agent is allowed to offer its normal keys.
- `--home` comes from the host profile. `--project` is added only for project-scoped
  commands (ASIF export/import, job actions, inbox answers), using the project named by
  the selected card or item. Fleet-wide reads (`fleet status`, `capacity report`,
  `fleet queue`, `fleet inbox`) run without `--project` so **All projects** is truly all
  projects.
- `--` terminates local ssh options and is placed **before** the destination; it is not
  sent as part of the remote command.
- Output is capped (16 MiB default for JSON documents). Exceeding the cap is a typed
  `output_too_large` parse guard, not a silent truncation.
- Timeouts: 8 s to connect, 20 s for reads, 30 s for writes, 60 s for the initial
  snapshot. The child process is killed on timeout; on Unix the process group is killed
  so no orphan `ssh` survives.
- Stderr is captured separately, redacted, bounded (64 KiB), and shown only in Details.
- The app-private `known_hosts` is mode 0600. A host-key mismatch is a hard block; there
  is no "ignore" button.

### 6.5 Performance budgets

- Cold app start to first paint: < 2 s on the primary Mac; no network in the critical
  path.
- Snapshot parse of a 1 MiB document: < 50 ms; view derivation < 10 ms.
- Poll and render at 4 s cadence must not drop frames; ring animation uses CSS/SVG
  transforms, not a React re-render per frame.
- Idle memory < 200 MiB; the app must not retain more than the last two snapshots.
- A `fleet status` round trip over a 40 ms RTT link should complete in < 500 ms including
  SSH startup. If it does not, add a persistent ControlMaster socket in D2 (Unix only)
  behind the `CommandRunner` trait; do not change the contract.

---

## 7. Visual design system

The Desktop MVP is a **dark-first operational cockpit**. It should feel calm and
precise: dark neutral surfaces, a single teal accent for action, semantic colors only for
state. No gradients on data surfaces, no glassmorphism, no decorative charts.

### 7.1 Tokens

```css
:root {
  --bg: #0a0e14;
  --surface: #111722;
  --surface-raised: #17202d;
  --surface-hover: #1d2837;
  --border: #263243;
  --border-strong: #34445a;
  --text: #e8eef7;
  --text-muted: #94a3b8;
  --text-faint: #64748b;

  --accent: #5be3c2;
  --accent-strong: #2dd4bf;
  --accent-ink: #06221d;

  --state-ok: #4ade80;
  --state-info: #38bdf8;
  --state-warn: #f5b84b;
  --state-danger: #f87171;
  --state-verifier: #c084fc;
  --state-unknown: #94a3b8;

  --radius-sm: 8px;
  --radius-md: 12px;
  --radius-lg: 16px;
  --shadow-panel: 0 12px 32px rgb(0 0 0 / 0.32);
  --focus-ring: 0 0 0 2px var(--bg), 0 0 0 4px var(--accent);

  --font-ui: -apple-system, BlinkMacSystemFont, "Segoe UI", Inter, sans-serif;
  --font-mono: "SF Mono", "JetBrains Mono", Menlo, Consolas, monospace;
  --text-xs: 11px;
  --text-sm: 12px;
  --text-md: 14px;
  --text-lg: 16px;
  --text-xl: 20px;
  --text-2xl: 28px;
  --space-1: 4px;
  --space-2: 8px;
  --space-3: 12px;
  --space-4: 16px;
  --space-5: 20px;
  --space-6: 24px;
  --space-8: 32px;
}
```

- Default dark theme only. A `data-theme` attribute is reserved for a later light theme;
  no component may hardcode a color outside the tokens.
- Numbers, job ids, executor strings, timestamps, and byte counts use `--font-mono`.
- Status is never communicated by color alone: every color has an icon, a label, or both.
- Motion: 150 ms for hover/focus, 220 ms for panel transitions; `prefers-reduced-motion`
  disables pulsing and transforms.

### 7.2 Layout

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│ Top bar 60px: brand · host chip · connect · latency · supervisor · inbox · ⚙ │
├───────────────┬──────────────────────────────────────────┬───────────────────┤
│ Host / summary│ Fleet & Worker Canvas                    │ Capacity & Storage│
│ 280px         │ fluid, min 520px                         │ 320px             │
│               │                                          │                   │
│               │  VPS card · slot ring · running cards    │  Target advisor   │
│               │  job lanes (horizontal scroll)           │  Disk / RAM / CPU │
│               │                                          │  Brake / limits   │
├───────────────┴──────────────────────────────────────────┴───────────────────┤
│ Status strip 28px: source · generated_at · stale · last error (expandable)    │
└──────────────────────────────────────────────────────────────────────────────┘
```

- Default window 1440 × 900; minimum 1120 × 720. Below 1200 px, the right rail becomes a
  slide-over opened by a **Capacity** button; the fleet canvas keeps priority.
- The primary views are **Fleet**, **Engine**, and **Hosts**. The left rail switches
  views; the top bar stays constant. No router is needed in D2–D4.
- Drawers: job detail and inbox open from the right, 420 px wide, overlay with a scrim,
  close on Esc. One drawer at a time.
- Empty states always include one primary action and a short explanation, never a blank
  panel.

### 7.3 Components

| Component | Spec |
|---|---|
| Button | 32 px height (sm 28, lg 40); radius 8; primary teal with dark ink; secondary transparent with border; danger red text/border; disabled 40% opacity and no hover. |
| Toggle | 40 × 22 track, 18 px thumb, teal when on; label + state text always adjacent. |
| Badge | 20 px height, 6 px horizontal padding, radius 999; state tint 12% background + state text; uppercase 10 px label for state, normal case for model/provider. |
| Gauge | Horizontal 8 px bar, rounded; track `--border`; fill semantic; marker line for reserve/floor; numeric value right-aligned mono. |
| Sparkline | 64 × 18 SVG, last 20 latency samples, no axis labels; tooltip lists p50/p95. |
| Panel | `--surface`, 1 px `--border`, radius 12, 16 px padding, optional header row. |
| Modal | 520 px wide, centered, 24 px padding, title 20 px, actions right-aligned. |
| Sheet/Drawer | 420 px, full height, header 56 px, body scroll, footer actions. |
| Toast | bottom-right stack, 360 px, auto-dismiss 6 s for success, persistent for errors. |
| Empty/Error | icon 32 px, title 16 px, body 14 px muted, one primary action. |

Accessibility requirements:

- WCAG 2.2 AA contrast on text and interactive controls.
- Every gauge has `role="meter"` with `aria-valuemin`, `aria-valuemax`, `aria-valuenow`,
  and `aria-label` (for example "VPS home disk free, 42.1 gigabytes, floor 5
  gigabytes").
- Connection changes are announced through a polite `aria-live` region.
- Full keyboard operation: Tab order follows visual order; Esc closes drawers/modals;
  Enter activates the focused control; no keyboard trap except modal focus management.
- Focus ring is always visible, including on dark panels.
- `prefers-reduced-motion` removes the running pulse and ring transitions.

---

## 8. Surface specifications

### 8.1 One-click VPS Host Manager

**Purpose:** add a VPS once, verify it, and connect/disconnect without a terminal.

#### 8.1.1 Empty state

```text
        Add your VPS
  StackSpirit connects to an existing VOWP home over SSH.
  You need: SSH key access · VOWP installed · nothing else.
        [ Add your VPS ]
```

Three inline checks with neutral icons: "SSH key or agent", "VOWP on the host",
"Host key verified". They become green/red during the wizard.

#### 8.1.2 Add wizard (3 steps)

**Step 1 — Address**

| Field | Required | Default | Validation |
|---|---|---|---|
| Label | yes | hostname | 1–64 chars, trimmed |
| Host | yes | — | DNS name or IP; no spaces, no `@`, no scheme |
| User | yes | `$USER` if known, else `owner` | SSH user token |
| Port | yes | 22 | 1–65535 |
| Identity file | no | — | existing file path; path only, never read into the app |
| VOWP home | no | remote default | absolute path or empty; passed as `--home` |
| Default project | no | empty (all) | project slug or empty; passed as `--project` only to project-scoped actions |

Helper under Identity file: "Leave empty to use your SSH agent. StackSpirit never stores
a passphrase or a private key."

**Step 2 — Verify host key**

- Core runs `ssh-keyscan -p <port> <host>` and shows each returned key:
  `ssh-ed25519  SHA256:AbCd...  (preferred)`, `ecdsa-sha2-nistp256 ...`,
  `ssh-rsa ...`.
- The fingerprint is monospace, selectable, and copyable.
- Copy: "Compare this with `ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub` on your
  server. StackSpirit will refuse to connect if this key changes."
- A checkbox **I verified this fingerprint on my server** gates **Trust and continue**.
- If the host is unreachable: show the SSH error, keep the form values, offer **Retry**.
- Multiple A/AAAA records returning different keys: show a warning and require choosing
  the expected key set; never merge silently.

**Step 3 — Test and save**

A checklist with live states:

1. `SSH reachable` — TCP/SSH handshake succeeded.
2. `Host key matches` — `StrictHostKeyChecking=yes` passed.
3. `VOWP found` — `vowp --version` succeeded.
4. `Snapshot readable` — `vowp --json fleet status` parsed.
5. `Supervisor running` — informational, not a save blocker; a stopped supervisor still
   allows connecting to a readable snapshot.

**Save** is enabled when checks 1–4 pass. The profile is written atomically with mode
0600. If check 5 fails, the host card shows **Connected · supervisor stopped** rather
than hiding the problem.

#### 8.1.3 Host list and connection control

- Host cards in the left rail: label, `user@host:port`, status dot, last connected,
  **Connect** or **Disconnect** button, overflow (**Edit**, **Re-pin key**, **Remove**).
- Only one host may be connected at a time. Connecting another prompts "Disconnect from
  <label> and connect to <label>?".
- **Remove** deletes the local profile and its pin; it never changes the VPS. A
  confirmation names the host.
- The top bar is the primary Connect/Disconnect control; the host card has a secondary
  one for discoverability.

#### 8.1.4 Connection states

| State | Top bar | Actions |
|---|---|---|
| Disconnected | grey dot, "Not connected", **Connect** | Connect; edit hosts |
| Connecting | spinner, stage text ("Checking VOWP…") | Cancel |
| Connected (fresh) | green dot, host, `42 ms`, "supervisor up · 12 s" | Disconnect; all fleet actions |
| Connected (stale) | amber dot, host, "stale · 2 m", last data dimmed | Refresh; writes require refresh |
| Failed | red dot, one-line error, **Retry** | Retry; edit host; details |
| Disconnecting | spinner, "Disconnecting…" | — |

- Latency is the `fleet status` round trip in ms, with a sparkline tooltip showing
  `last / p50 / p95` and the last update time.
- Supervisor status comes from `snapshot.supervisor`: running/stopped/paused, PID,
  `last_tick` age, `fail_streak`, VOWP version. If `last_tick` is older than
  `max(3 * tick_secs, 180s)`, the chip turns amber even if `running` is true.
- Disconnect stops polling and closes SSH. The last `FleetView` stays visible, dimmed,
  with "Disconnected — showing data from 12:04:32". All mutation buttons are disabled.

### 8.2 Top bar

Left to right:

1. **StackSpirit** wordmark (no engine branding).
2. Host chip: label + `user@host`, status dot, latency.
3. **Connect / Disconnect** primary button.
4. Supervisor chip: `supervisor up · tick 12s` or `supervisor stopped`.
5. Project switcher: **All projects** plus one entry per `snapshot.projects[].name`.
   `All projects` is the default. A selected project filters lanes and worker cards but
   not capacity, which is node-wide.
6. Inbox bell with open count; opens the inbox drawer.
7. **Engine** pill: `Engine ready`, `Engine stopped`, `Engine needs a key`, or
   `External: opencode`. Clicking opens the Engine view.
8. Gate badges: compact state chips for **Unknown**, **Probing**, **Waiting**,
   **Incompatible**, **Unreachable**, and **Ready**. The top bar shows the state and the
   gate name; details live in the Engine view and a gate drawer.

The top bar never shows a green state it cannot prove. If the gate probe has not run,
the badge says **Unknown**, not **Ready**.

### 8.3 Fleet & Worker Canvas

**Purpose:** show the VPS as the root, the worker slots up to VOWP's target, the workers
that are actually running, and every job lane at a glance.

#### 8.3.1 Canvas composition

```text
┌──────────────────────────── Fleet canvas ─────────────────────────────┐
│                                                                       │
│   ┌──────────────┐        ╭──────────────────────╮                    │
│   │  VPS ROOT    │        │   WORKER SLOTS       │    RUNNING WORKERS │
│   │  vps         │───────▶│   3 / 8 busy         │    [card] [card]   │
│   │  supervisor  │        │   ● ● ● ○ ○ ○ ○ ○    │    [card]          │
│   │  up · 12 s   │        ╰──────────────────────╯                    │
│   └──────────────┘                                                     │
│                                                                       │
│   RUNNING      READY       REVIEW & FIX   PARKED & BLOCKED   DONE      │
│   ┌────────┐   ┌────────┐  ┌────────┐     ┌────────┐         ┌──────┐  │
│   │ job    │   │ job    │  │ job    │     │ job    │         │ 37   │  │
│   └────────┘   └────────┘  └────────┘     └────────┘         └──────┘  │
└───────────────────────────────────────────────────────────────────────┘
```

- The VPS root card is fixed at the left of the canvas, 280 × 140 px. It shows:
  `snapshot.home`, supervisor state and last-tick age, VOWP version, project count,
  running/queue totals, and provider-limit count.
- The slot ring is centered in the upper canvas. It is a 220° arc of `target_workers`
  slots. Each slot is an 18 px circle:
  - filled teal = a running job card with role `worker`;
  - outlined = idle slot;
  - violet ring = a running verifier job card; verifiers have no exposed target in the
    snapshot, so this is an inner **verifier runs** arc, not a "verifier target" ring;
  - red pulse = a stalled worker run;
  - grey = unknown.
- Center label: `3 / 8` and **workers**. Under it: `target 8 · advised 8`.
- Target 0: the ring is replaced by an empty ring and the text "No workers allowed — VOWP
  target is 0".
- More than 12 slots: draw 12 arcs and a `+N` segment; the tooltip lists the target and
  busy count. Never shrink slots below 12 px.
- Busy workers come from `ProjectSnapshot.running` cards with `role == "worker"`. If
  `capacity.running.runs` is larger than the worker + verifier cards, show a small
  `+N other runs` chip (planner, keeper, gate, merge, or a run without a project card).
  Do not silently map every open run onto a worker slot, and do not hide the difference.
- If worker cards exceed the target (a target change or an inconsistent snapshot), show
  `8 / 8 +2` with an amber **over target** chip. StackSpirit does not hide the
  inconsistency.
- A stalled worker run pulses red and gets a **Stalled** chip. The pulse stops under
  reduced motion; the chip remains.
- The ring is decorative for screen readers; the accessible summary is a text line:
  "8 worker slots, 3 busy, 5 idle, target advised by VOWP is 8, 1 other run."

#### 8.3.2 Running worker cards

A responsive grid under the ring, up to six cards visible, vertical scroll for more.
Each card is 240 × 104 px:

```text
JOB-142  build                    2m 14s
Split the desktop shell bridge
opencode · deepseek/deepseek-flash · max     ● live
```

- Line 1: job id (mono), kind badge, age (right).
- Line 2: job title if known; otherwise the job id repeated once and a muted "title
  unavailable in snapshot" tooltip. No invented title.
- Line 3: harness badge + provider/model badge + variant/effort chip. The raw executor
  string is in the tooltip and the drawer.
- Footer: last-event age (`live` if < 30 s, `quiet 4m` if older, `stalled` if state is
  stalled) and session id short form if present.
- Shell/gate/merge runs with `executor: null` show a **shell** badge and no model chip.
- Cards are sorted by `age_secs` descending; stalled first, then running, then merging.

#### 8.3.3 Job lanes

Five columns, each min 240 px, horizontal scroll on narrow windows.

| Lane | Header color | Cards | Footer |
|---|---|---|---|
| Running | info blue | lock-holding cards; chip **Active** unless a queue row says **Stalled** | `N running` from `jobs_by_state` |
| Ready | neutral | pending/ready queue cards | `N waiting` |
| Review & Fix | verifier violet | gate-failure cards; counts from `jobs_by_state` | `N in review` |
| Parked & Blocked | warn/danger | parked/failed/cancelled + blocked cards | `N need attention` |
| Done | ok green | done/replanned summary | `N done` |

`RunningJobCard` does not carry a state, so the Running lane never claims a card is
specifically `running`; it says **Active** and marks **Stalled** only when the queue row
for the same job says `stalled`. The Review & Fix lane does not invent cards for
gating/reviewing/fixing jobs that VOWP did not include; its header count is exact from
`jobs_by_state`, and its cards are the gate failures that VOWP did include.

Card content:

- Job id (mono), kind, priority dot for `priority >= 80`.
- Title when available; otherwise job id.
- State chip; reason/detail one line for blocked/parked.
- Age; project chip when **All projects** is selected.
- A gate-failure card shows the gate name and evidence line (max 200 chars, truncated
  with a tooltip). Clicking opens the job drawer.
- Lane header counts come from `jobs_by_state`, not from the number of visible cards.
- If a list is truncated by VOWP (queue cap 200), the footer says "showing first 200".
- Empty lane: a quiet "No jobs" line, not a large illustration.

#### 8.3.4 Job drawer (D3; actions are VOWP commands)

Clicking a card opens a 420 px drawer with:

- Job id, kind, role, state, project, priority, age.
- Blocked/parked reason and detail if present.
- Gate failures (name, time, evidence) if present.
- Session id and executor if present.
- Action buttons **Retry**, **Park**, **Cancel**, enabled only for states VOWP accepts.
  Each asks for confirmation; **Cancel** is danger-styled. No optimistic state.
- A note that full run logs and terminals are not part of the Desktop MVP.

The drawer never claims to show a live terminal. Session streaming stays out of D1–D4.

### 8.4 Capacity & VPS Storage Rail

**Purpose:** answer "can this VPS safely run more workers, and what is stopping it?"
without hiding VOWP's own decision.

The rail is a vertical stack of panels, 320 px wide, scrollable. Order is deliberate:
brake first if active, then target, disk, RAM, CPU, waiting, limits. The snapshot's
capacity card drives disk/RAM/CPU immediately; panels that require
`vowp capacity report` (estimates, waiting, provider caps) show **Capacity detail
unavailable** until `report_loaded` is true. They never show an empty list as if it were
proof that nothing is waiting.

#### 8.4.1 Brake banner

If `capacity.brake.active`:

```text
⚠ VOWP brake active since 12:02
Memory pressure stayed above the safe threshold.
New runs wait; running runs continue.
```

- Red for `disk_low` or memory pressure; amber for provider/swap warnings.
- Every `reasons[]` item is listed with its `detail`.
- The banner does not disable the target editor; the brake is VOWP's decision, not a UI
  lock.
- If no brake: a slim green/neutral line "No brake active".

#### 8.4.2 Target advisor

```text
Worker target                          [ Auto ▾ ]
   8 workers          VOWP advises 8
   [−]  8  [+]                 [ Apply advised ]
```

- Current `target.target_workers`; mode chip `Auto` or `Fixed`; `set_by` and `set_at` in
  the tooltip.
- Advised value from §5.4 precedence. If null: "VOWP has no advice yet" and **Apply
  advised** is disabled.
- Current > advised: amber "above advice" badge and the button becomes **Match advised**
  (lowering is always allowed). Raising above advice requires the override dialog.
- Current < advised: teal "headroom available" and **Apply advised**.
- `is_override` true: persistent **Owner override** badge, even after the advice changes.
- The stepper commits on blur/Enter, not on every keystroke. The mutation is
  `capacity set`; a VOWP `guard`/`refused` response is shown verbatim.
- A one-line explanation: "VOWP still applies brakes and provider limits. A higher target
  is not a guarantee that runs start."

#### 8.4.3 Disk

```text
Home disk                                    42.1 GiB free
[████████████████████░░░░░░░░░░░░]  floor 5 GiB
Worktrees (if separate)                      38.7 GiB free
```

- `disk_free_home_bytes` is the primary value; `disk_free_worktrees_bytes` is a second
  bar when non-null and different.
- The floor marker is `min_disk_bytes` from the capacity card/report. If the two documents
  disagree, show the report value and a small "capacity report" source label; log the
  mismatch. Never average them.
- State copy:
  - ok: "Above the reserve floor."
  - warn: "Close to the floor. VOWP may stop admitting runs soon."
  - critical: "Below the floor. New runs will wait (disk)."
- Exact bytes are shown in a tooltip; the top-line value is binary GiB with one decimal.
- If `disk_free_home_bytes` is 0 because the sample is missing, show **Unknown**, not
  "0 bytes free".

#### 8.4.4 RAM

```text
Memory                          12.4 GiB available
Reserve 2.0 GiB · running p90 1.1 GiB
Per-worker p90 1.2 GiB (measured, 12 samples)
~8 more by RAM · target-limited
[███████████████░░░░░░░]  reserve marker
```

- `mem_available_bytes` vs `reserve_ram_bytes`; a reserve marker sits on the bar.
- `running.ram_p90_bytes` is the p90 across *open runs*, not a per-worker estimate; label
  it exactly "running p90".
- The per-worker estimate comes from §5.4. Show source and samples. A `default` estimate
  gets a muted **default estimate** chip.
- `estimated_extra_slots = floor(max(0, available - reserve) / per_worker_p90)`, capped at
  99 and shown as `~N more by RAM`. It is advisory; the target and brake remain
  authoritative. If no estimate: "Not enough samples for a per-worker estimate."
- If `available < reserve`: red "Below the RAM reserve. VOWP will brake new runs."

#### 8.4.5 CPU

```text
CPU load                    2.10 / 8 cores (26%)
Pressure CPU avg10          4.2%
```

- Load ratio = `load1 / cpu_count`, clamped for display only.
- Pressure is `pressure_cpu_avg10` when present; else "not reported".
- If `cpu_quota_cores` is present, show "quota 4.0 cores" next to the core count.
- The bar colors follow §5.4; the text is the source of truth.

#### 8.4.6 Waiting reasons

- Group `capacity.waiting` by `reason.code`; show counts and the top five rows:
  `vowp:graph-engine · RAM 1.2 GB short · waiting 3m`.
- Codes map to labels: **RAM**, **CPU**, **Disk**, **Provider limit**, **Brake**, **Target**.
- If the list is empty and `report_loaded` is true: "No jobs are waiting for capacity."
  If the report is not loaded: "Capacity detail unavailable — retrying." Never show an
  empty list as proof that nothing is waiting.
- This panel is the explanation for why the target may not produce more running workers.

#### 8.4.7 Provider limits and caps

- `limits[]`: provider, harness, kind, `resets_at` local time, detail. A limit with
  `resets_at: null` says "needs a human".
- `provider_caps`: `provider → concurrency` from the capacity report; show as compact
  chips. If the report is not loaded, hide the caps rather than guessing from the
  snapshot.
- A provider limit is not a disk/capacity problem; keep it visually separate.

#### 8.4.8 Freshness footer

`Capacity data from VOWP · 12:04:32 · fresh` or `· stale · retrying in 20s`.
Clicking opens the raw-document drawer (read-only, redacted) with `generated_at`,
`fetched_at`, and parse warnings.

### 8.5 StackSpirit Engine onboarding

**Purpose:** let a new user run models immediately, without learning that `opencode`
exists, while giving power users their own harness and keys.

#### 8.5.1 Language rule

- User-facing name: **StackSpirit Engine**.
- "opencode" appears only in Advanced details ("Engine implementation: opencode
  <version>"), license notices, and the external-harness picker.
- Never show "Install opencode", "Configure opencode", or "opencode is required".
- The app works out of the box with the bundled engine. If the bundle is missing or
  corrupt, the error is "StackSpirit Engine is unavailable — repair the installation",
  not an instruction to install a third-party tool.

#### 8.5.2 First-run flow

```text
Step 1  Engine check        Bundled engine found · version ...   [Continue]
Step 2  Choose models       ( ) Included models (recommended)
                            ( ) Use my own provider key
                            ( ) Use an existing harness
Step 3  Finish              Model selected · Engine ready        [Open fleet]
```

- Step 1 is automatic. It verifies the binary, starts it on a loopback port, and waits
  for health (10 s). Failure shows a **Retry** and **Repair** path, plus a details log.
- Step 2 is a single choice; the included option is selected by default when at least one
  included model is healthy.
- Step 3 writes the selection locally. Selecting a model does **not** change the VPS;
  provider/model names only cross the SSH boundary later, when a VOWP profile or
  orchestrator is configured (D6).
- The flow can be reopened from the Engine view; it is not a blocking modal after first
  run.

#### 8.5.3 Engine status card

- State: `Ready`, `Starting`, `Stopped`, `Degraded`, `Failed`, `External`.
- Version, loopback port, uptime, selected model, last error.
- Buttons: **Restart engine**, **Change model**, **Use bundled**, **View logs**.
- Logs are redacted and capped (last 200 lines); no provider key or full prompt appears.
- "Engine runs on this Mac only. Your VPS never receives a provider key."

#### 8.5.4 Included models

- Cards/rows: label, provider, context window, `Included · no key`, availability dot, and
  `verified <time>`.
- The list is generated from the bundled engine's model catalog at build time and
  re-verified at runtime. If the engine drops a model, the app shows it as unavailable
  and selects nothing automatically.
- Empty state: "This engine build has no included models available. Add your own provider
  key to continue." The BYOK path is one click away.
- No claim of "free forever" or "unlimited". Copy: "Included with StackSpirit Engine; no
  key required. Availability is checked when the engine starts."

#### 8.5.5 BYOK

```text
Provider            [ DeepSeek ▾ ]
API key             [ •••••••••••••••• ]  [ Save ]
                    Stored in this Mac's keychain. Never sent to your VPS.
```

- Provider list is derived from the bundled engine's supported providers; display names
  are a small catalog with a safe fallback to the provider id.
- The input is `type="password"`, never echoed, never put in a URL, never logged, never
  stored in React state longer than the submit call. On submit, the value is passed to
  `engine.storeKey`, written to the OS keychain, and cleared from the form.
- The key is verified by a local engine call. The UI shows `Saved`, `Invalid`, or
  `Unverified`; it never displays the key or a prefix longer than the last four
  characters (and only if the provider supports that safely; default is no display).
- **Delete key** asks for confirmation and removes only the keychain entry.
- `keyStatus()` returns provider names and states, never values.
- No export/import of keys. No key in a crash report, log, fixture, or report.

#### 8.5.6 External harness picker

- **Scan** looks on `PATH` for `opencode` and `claude`/`claude-code` only. It does not
  walk the disk.
- Each result shows kind, absolute path, version, and **Use this installation**.
- Copy: "Use an existing harness if you already have one. Its login and model access stay
  with that installation; StackSpirit's included models apply only to the bundled
  engine."
- Selecting an external harness never edits its config. The engine supervisor stops the
  bundled sidecar and reports `External`.
- A missing/broken external harness falls back only after an explicit user action; no
  silent switch.

#### 8.5.7 Gate cards (D5/D6)

Two cards at the bottom of the Engine view:

```text
Local orchestrator                     Waiting for VOWP Phase 2/3/4
Run global and project orchestrators on this Mac.
Missing: surface rotation · orchestrator materialization · local agent connection
[ Details ]                            [ Run locally (disabled) ]

.asif.zip sessions                     Waiting for VOWP ASIF
Download and import portable session packages.
Missing: v4-asif-core · v4-asif-cli
[ Details ]                            [ Import (disabled) ]
```

- Status chip colors: `Ready` green, `Waiting` amber, `Incompatible` red, `Unknown` grey,
  `Probing` blue with a spinner, `Unreachable` red with a retry hint.
- A gate can be `Ready` while the StackSpirit action is not implemented yet. In D4 both
  `features.asif_actions` and `features.local_orchestrator_actions` are false, so the
  action says **Not implemented in this build** with the VOWP-ready evidence beside it.
  D5 and D6 flip their flag when the action ships. A green gate never implies a dead
  button is live.
- **Details** opens a drawer with the probe evidence (commands run, exit codes, parsed
  version) and the exact missing milestones/capabilities.
- The action button is disabled unless the corresponding capability is `Ready` **and**
  the matching `features` flag is true. A disabled button has a tooltip and an adjacent
  reason; it is never a dead control.
- No countdown, fake progress, or "coming soon" without the milestone names.
- The gate state is refreshed on connect, after a VOWP version change, and on **Re-probe**.
  It is not inferred from the app version.

### 8.6 Inbox and sessions drawers

- Inbox: list of open items (kind, project, job, title, age). Selecting one shows the full
  `body`, `options`, and an answer field. Answering calls VOWP and refreshes. Answered
  items appear under **All**.
- Sessions: a read-only list from `snapshot.sessions` (id, title, project, state,
  executor, last event). The D5 ASIF export action lives here but stays disabled until
  the D5 build sets `features.asif_actions` and `AsifGate` exposes `export`. No live
  terminal in D1–D4.

### 8.7 Copy and voice

- Short, direct, operational. No exclamation marks. No "magic".
- Explain the consequence, not the implementation: "VOWP will stop admitting new runs"
  rather than "brake code disk_low".
- Use **VPS**, **worker**, **job**, **engine**, **provider key**. Use **opencode** only in
  advanced/external contexts.
- Every disabled control says why. Every error says what to try next.
- Gate copy names the exact VOWP milestone; it never blames the user.

---

## 9. Build order and job graph

### 9.1 Phase graph

| Phase | Job | Depends on | Primary output | VOWP dependency |
|---|---|---|---|---|
| D0 | `ss-0-plan` | — | this plan + `VOWP-CONTRACT.md` | Read-only docs/protocol |
| D1 | `ss-1-core` | D0 | `crates/spirit-core` with wire model, SSH runner, host store, fixture client, engine skeleton, gates | Existing VOWP CLI JSON (D0–D4) |
| D2 | `ss-2-desktop-shell` | D1 | Tauri 2 shell, `SpiritBridge`, Host Manager, top bar, MockBridge | Same |
| D3 | `ss-3-fleet-canvas` | D2 | Fleet canvas, worker ring, job lanes, capacity/storage rail, inbox | Same |
| D4 | `ss-4-engine-onboarding` | D2, D3 | Bundled StackSpirit Engine, included models, BYOK keychain, external harness picker, gate cards | Same; gate probes are read-only |
| D5 | ASIF wiring | D4 + VOWP `v4-asif-*` | Live `.asif.zip` export/import | **Gated** |
| D6 | Local orchestrators | D5 optional + VOWP Phase 2/3/4 `v4-local-3*`/`v4-local-4f-cli-surface` | Live local orchestrator sessions; core/bridge/UI landed, probes reconciled with VOWP `0.4.0` | **Gated** (VOWP `0.4.0` installed; `ss-32`, `ss-37`) |
| W1 | Web app | D1–D4 | `apps/spirit-web` over HTTP/SSE | Same, later |
| C1 | CLI | D1–D4 | `crates/spirit-cli` over `spirit-core` | Same, later |

D1–D4 are the Desktop MVP. D5 and D6 are explicitly gated and must not be implemented
against an unmerged VOWP contract. W1 and C1 are explicitly after the Desktop MVP.

D6's core, bridge and Engine panel landed in `ss-20`/`ss-21`; `ss-32` and `ss-37`
reconciled the gate probes and live-home evidence with VOWP `0.4.0`. The published action
flag still requires `LocalOrchestratorGate = Ready` and a `connected` local agent, so D6
execution remains gated.

**Chat status (HANDOFF §5 step 4; updated 2026-11-19):**

| Surface | Status | Evidence |
|---|---|---|
| Local setup assistant chat | Landed on the bundled engine; non-streaming (`streaming_supported: false`) until the owner turn pins the event cadence | `ss-42`, `cc83371`; `ss-45`, `fe6d6e5`; `ss-46`, `c1e2402`; `scripts/verify-orchestrator-chat.mjs` (`ss-47`, `76b48ac`) |
| Server `global`/`project` chat | Gated on VOWP `0.5` `vowp orchestrator endpoint` or the owner-approved SSH forward; no fake reply | `docs/ORCHESTRATOR-CHAT.md` §3.6, §7.2 |
| Local/Server switch | Gated on VOWP `0.5` `vowp orchestrator move` (HANDOFF §5 step 6); the chip is read-only | `docs/ORCHESTRATOR-CHAT.md` §3.5 |

The Server/VOWP `0.5` and pooled-compute cluster ordering in HANDOFF §5 steps 6–7 is
unchanged; the local chat landing does not reorder those steps.

### 9.2 Ownership boundaries

Sequential jobs may extend a file created by an earlier job only where listed. A job
must never touch `/home/dev/vowp`.

| Job | Owns | May read, must not edit |
|---|---|---|
| `ss-1-core` | root `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates/spirit-core/**` | `/home/dev/vowp` docs/protocol; `docs/*` |
| `ss-2-desktop-shell` | root `Cargo.toml`/`Cargo.lock` (only to add the `src-tauri` member), root `package.json`, `package-lock.json`, `apps/spirit-desktop/**` including the shell, `views/*/Placeholder.tsx`, and the `src-tauri` command stubs | `crates/spirit-core/**` (consume, do not edit) |
| `ss-3-fleet-canvas` | `apps/spirit-desktop/src/views/fleet/**`, `src/views/capacity/**`, `src/views/inbox/**`, `src/domain/**` (fleet/capacity/format/executor), small `src/app/TopBar.tsx`/`AppShell.tsx` wiring for the inbox, related tests; `crates/spirit-core/src/view/**` only if D1 leaves a stub | `crates/spirit-core/src/wire/**`; `src-tauri/**` |
| `ss-4-engine-onboarding` | `apps/spirit-desktop/src/views/engine/**`, `src/domain/engine.ts`, `src-tauri/src/commands/engine.rs`, `src-tauri/src/commands/gates.rs`, `src-tauri/src/state.rs` wiring, `crates/spirit-core/src/engine/**`, engine packaging scripts/manifests, related tests | `crates/spirit-core/src/wire/**` |

Ownership is **sequential**: the table names the primary owner after each handoff. A later
job may replace an earlier job's placeholder or stub at the exact path listed above; it
does not own the rest of the earlier job's tree. If two jobs run in parallel, the manifest
and command-module edits must move into a dedicated integration job. The supervisor's
`owns_respected` gate is evaluated against each job's own brief, so every implementation
job must transcribe the exact paths it will touch.

### 9.3 Phase D1 — `ss-1-core`

**Goal:** a headless, fully tested Rust core that can connect to a VOWP home, parse its
documents, persist host profiles, supervise the local engine, and report gate states.
No UI.

#### D1 file plan

```text
crates/spirit-core/
├── Cargo.toml                 # serde, serde_json, thiserror, time, tokio, async-trait,
│                              # keyring (desktop feature), zeroize, tracing, ulid
├── fixtures/vowp/*.json
├── src/
│   ├── lib.rs                 # re-exports; feature flags
│   ├── error.rs
│   ├── time.rs
│   ├── wire/
│   │   ├── mod.rs
│   │   ├── ids.rs
│   │   ├── enums.rs
│   │   ├── snapshot.rs
│   │   ├── capacity.rs
│   │   ├── ops.rs
│   │   ├── inbox.rs
│   │   ├── session.rs
│   │   ├── surface.rs
│   │   ├── limit.rs
│   │   ├── executor.rs
│   │   └── error_doc.rs
│   ├── vowp/
│   │   ├── mod.rs
│   │   └── client.rs          # VowpClient trait + SSH/fixture/mock clients
│   ├── ssh/
│   │   ├── mod.rs
│   │   ├── command.rs
│   │   ├── runner.rs
│   │   ├── process.rs
│   │   ├── known_hosts.rs
│   │   └── fingerprint.rs
│   ├── host/
│   │   ├── mod.rs
│   │   ├── profile.rs
│   │   ├── store.rs
│   │   └── probe.rs
│   ├── connection/
│   │   ├── mod.rs
│   │   ├── state.rs
│   │   ├── manager.rs
│   │   ├── poll.rs
│   │   └── latency.rs
│   ├── view/
│   │   ├── mod.rs
│   │   ├── fleet.rs
│   │   ├── capacity.rs
│   │   ├── lanes.rs
│   │   ├── executor.rs
│   │   └── format.rs
│   ├── engine/
│   │   ├── mod.rs
│   │   ├── catalog.rs
│   │   ├── keychain.rs
│   │   ├── runtime.rs
│   │   ├── supervisor.rs
│   │   ├── external.rs
│   │   └── gate.rs
│   ├── service.rs
│   └── fixtures.rs            # feature = "fixtures"
└── tests/
    ├── contract.rs
    ├── ssh.rs
    ├── host_store.rs
    ├── connection.rs
    ├── view.rs
    ├── engine.rs
    ├── gates.rs
    └── security.rs
```

#### D1 deliverables

1. **Wire model** for every document in `VOWP-CONTRACT.md`: `FleetStatus`, `Snapshot`,
   `ProjectSnapshot`, job cards, `CapacitySnapshot`, `HostSample`, `CapacityTarget`,
   `BrakeState`, `Limit`, `InboxSummary`, `SessionSurfaceCard`, `SurfaceRef`,
   `OrchestratorCard`, `CapacityReport` (plus CLI-only `provider_caps`/`reload_pending`),
   `FleetQueueRow`, `InboxItem`, and the `--json` error document.
2. **Tolerant enum policy** and a `WireWarning` list returned with each parse (unknown
   enum values, missing optional defaults). Warnings are shown in the UI's raw-document
   drawer, never silently discarded.
3. **`CommandRunner`** trait and a system-`ssh` implementation. Unit tests assert the
   local ssh argv, conditional `IdentitiesOnly`, remote POSIX quoting of every dynamic
   argument, strict host-key options, timeouts, output caps, and the absence of shell
   pipelines/operators. A `FakeRunner` is the only runner used by tests.
4. **Host profiles**: `HostStore` trait, JSON implementation with atomic write, mode
   0600, schema version, and a migration from unversioned files. No secrets.
5. **Host-key helpers**: scan (`ssh-keyscan`), fingerprint (`ssh-keygen -lf`), app-private
   `known_hosts` write, and mismatch detection. The core never accepts a key on the
   user's behalf without a `HostKeyPin`.
6. **Connection manager**: state machine, 4 s/15 s polling with jitter, cancellation,
   three-failure staleness, backoff, latency stats, and event fan-out.
7. **View derivation**: `FleetView`, `CapacityView`, lane mapping, executor parsing,
   target-advice precedence, estimate selection, threshold states. Pure functions.
8. **`FixtureVowpClient`** behind the `fixtures` feature, plus a `MockVowpClient` for
   tests. Both implement the same `VowpClient` trait as `SshVowpClient`.
9. **Engine skeleton**: `EngineRuntime` trait, `EngineSupervisor` with start/stop/health/
   restart/backoff, model catalog types, `Keychain` trait with a fake, external harness
   scan, and gate probe types. D1 does not ship the real sidecar; it ships the state
   machine and tests.
10. **Gate probes**: read-only, conservative algorithms from `VOWP-CONTRACT.md` §11.
    D1 can return `WaitingForVowpPhase3`/`WaitingForVowpAsif` against current VOWP and
    `Ready` against synthetic fixtures.
11. **`SpiritCore` facade** with async methods used by Tauri/CLI. No Tauri imports.

#### D1 data flow

```text
HostProfile -> SshRunner -> `vowp --json` -> wire parser -> FleetView/CapacityView
                                   |                              |
                                   +-> WireWarning[] -------------+
                                                                  v
                                              broadcast SpiritEvent(revision)
```

- `SshVowpClient` is the live implementation; `FixtureVowpClient` and `MockVowpClient`
  implement the same trait for tests/preview.
- `ConnectionManager` owns the poll tokens and the monotonic revision; it never lets an
  older response overwrite a newer one.
- `spirit-core::view` is pure: the same wire fixture produces the same view bytes. This is
  what the Tauri command and the later CLI reuse.

#### D1 test strategy

Contract and parsing:

- `snapshot_fixture_parses_every_documented_field`
- `fleet_status_wrapper_keeps_source_fetched_at_and_stale`
- `capacity_report_accepts_cli_only_provider_caps_and_reload_pending`
- `fleet_queue_is_an_array_of_rows`
- `fleet_inbox_full_body_and_options`
- `timestamp_is_canonical_utc_millis`
- `unknown_job_state_becomes_unknown_not_ready`
- `missing_required_field_is_a_parse_error_with_document_name`
- `no_fixture_contains_a_secret_shape` (see security tests)

SSH and host:

- `ssh_local_process_uses_an_argv_array`
- `ssh_remote_command_quotes_dynamic_arguments`
- `ssh_remote_command_has_no_pipeline_or_operators`
- `ssh_identities_only_is_set_only_with_an_identity_file`
- `ssh_argv_uses_strict_host_key_checking_and_app_known_hosts`
- `ssh_timeout_kills_the_child_and_returns_remote_timeout`
- `ssh_output_over_cap_is_a_typed_error`
- `host_store_writes_mode_0600_and_atomic_rename`
- `host_store_migrates_unversioned_profile_file`
- `host_key_mismatch_blocks_connect`
- `first_connect_requires_a_pinned_fingerprint`
- `identity_file_path_is_stored_but_never_read_as_key_material`

Connection and view:

- `connect_probe_sequence_reports_the_failing_stage`
- `three_failures_mark_the_snapshot_stale_and_back_off`
- `disconnect_cancels_in_flight_polls`
- `revision_increases_monotonically_and_old_events_are_dropped`
- `lanes_match_the_contract_mapping`
- `running_cards_keep_a_null_executor_as_shell`
- `queue_cap_200_is_labeled_as_truncated`
- `blocked_card_does_not_invent_a_title`
- `target_advice_uses_target_then_snapshot_value`
- `ram_estimate_prefers_exact_then_pooled_then_largest`
- `disk_ram_cpu_thresholds_are_deterministic`
- `stale_view_disables_mutations` (core returns `stale`; UI test covers disablement)

Engine and gates:

- `included_catalog_entries_are_marked_included`
- `fake_keychain_never_writes_plaintext`
- `secret_string_has_no_debug_or_serialize`
- `supervisor_restarts_then_enters_degraded`
- `external_scan_finds_only_path_opencode_and_claude`
- `phase3_gate_waits_when_surface_commands_are_absent`
- `phase3_gate_does_not_accept_agent_status_and_node_model_alone`
- `phase3_gate_is_ready_only_with_offload_and_surface_capabilities`
- `phase3_gate_requires_orchestrator_new_node`
- `asif_gate_is_ready_only_when_export_probe_exits_not_found`
- `gate_never_ready_from_version_alone`
- `gate_probe_evidence_contains_no_paths_or_secrets`

Gates:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p spirit-core
cargo test -p spirit-core --features fixtures
```

No live VOWP is required. A manual live smoke can set `SPIRIT_LIVE_HOST` in the local
shell only; it must never be committed or read from a `.env` file.

#### D1 done criteria

- All core types compile with `#![forbid(unsafe_code)]` and `missing_docs` warnings.
- Every document in `VOWP-CONTRACT.md` has a golden fixture and a parser test.
- The SSH runner is fully fakeable; no test opens a network connection.
- Host-key mismatch is a hard failure.
- Gate probes never return `Ready` without capability evidence.
- No secret is present in a fixture, error, log, or report.

### 9.4 Phase D2 — `ss-2-desktop-shell`

**Goal:** a runnable Tauri 2 window with the design system, `SpiritBridge`, MockBridge,
Host Manager, top bar, and a fleet placeholder that proves the core data flow.

#### D2 file plan

```text
apps/spirit-desktop/
├── package.json               # react, react-dom, zustand, @tauri-apps/api, vite, vitest
├── vite.config.ts
├── tsconfig.json
├── index.html
├── src/
│   ├── main.tsx
│   ├── app/AppShell.tsx
│   ├── app/TopBar.tsx
│   ├── app/StatusStrip.tsx
│   ├── bridge/{SpiritBridge,wire,TauriBridge,MockBridge,errors}.ts
│   ├── state/fleetStore.ts
│   ├── domain/{format,executor}.ts
│   ├── components/*
│   ├── views/hosts/{HostManager,AddHostWizard,FingerprintStep,HostCard,ConnectionControl}.tsx
│   ├── views/fleet/Placeholder.tsx
│   ├── views/capacity/Placeholder.tsx
│   ├── views/engine/Placeholder.tsx
│   └── test/{render,scenarios}.tsx
└── src-tauri/
    ├── Cargo.toml
    ├── tauri.conf.json
    ├── capabilities/default.json
    ├── build.rs
    └── src/{main,lib,state,events,error}.rs
        commands/{mod,hosts,connection,fleet,engine,gates}.rs
```

#### D2 deliverables

1. **App shell** with the three-column layout, top bar, status strip, left rail, and
   view switching. Fleet/engine placeholders may be simple, but the shell is final.
2. **Design system primitives** from §7: Button, Toggle, Badge, Panel, Modal, Drawer,
   Tooltip, Toast, EmptyState, ErrorState, Gauge shell, Sparkline.
3. **`SpiritBridge`** + `TauriBridge` + `MockBridge`. `TauriBridge` is the only file that
   imports `@tauri-apps/api`. MockBridge supports named scenarios and a fake clock.
4. **Zustand store** with revision handling, connection/fleet/engine/gates slices, and
   selector hooks.
5. **Host Manager**: empty state, add wizard, fingerprint confirmation, probe checklist,
   host list, connect/disconnect, failure states, latency sparkline, supervisor chip.
6. **Tauri commands**: host CRUD, fingerprint scan/trust, probe, connect/disconnect,
   status, fleet get/refresh, engine status stub, gates stub. Thin adapters only.
7. **Events**: `spirit://state` with revision and kind; frontend subscription cleanup on
   unmount.
8. **Tauri config**: window defaults, minimum size, identifier, no shell permission,
   file-dialog permission only where needed, CSP that allows only bundled assets and
   loopback engine connections.
9. **Preview mode**: `npm run dev` uses `MockBridge` when `VITE_SPIRIT_MOCK=1`; the UI shows
   a persistent **Preview data** badge. It must be impossible to mistake preview data for
   a live VPS.

#### D2 data flow acceptance

- Add a fixture host, confirm its fingerprint, click Connect, and see the top bar move to
  Connected with latency and supervisor status.
- Disconnect and verify the last fleet view is dimmed and mutation buttons are disabled.
- Kill the mock SSH backend and verify three failures produce Stale, then a backoff
  countdown.
- No component imports `@tauri-apps/api` except `TauriBridge.ts` (lint rule).

#### D2 test strategy

- `add_host_requires_a_label_host_user_and_valid_port`
- `fingerprint_must_be_confirmed_before_trust`
- `probe_checklist_shows_the_failing_stage`
- `connect_toggle_shows_connected_then_disconnected`
- `stale_connection_dims_data_and_disables_writes`
- `host_key_mismatch_dialog_lists_old_and_new_fingerprints`
- `topbar_never_shows_green_without_a_successful_probe`
- `preview_badge_is_visible_in_mock_mode`
- `spirit_bridge_is_the_only_tauri_import`
- `state_events_with_older_revision_are_ignored`
- `escape_closes_a_modal_and_restores_focus`
- `axe_has_no_critical_violations_on_shell_and_host_manager`

Gates:

```sh
npm ci
npm run typecheck
npm run lint
npm test
npm run build
cargo fmt --all -- --check
cargo clippy -p spirit-desktop --all-targets -- -D warnings
cargo test -p spirit-desktop
npx tauri build --debug
```

#### D2 done criteria

- A user can add a VPS, verify a fingerprint, connect, disconnect, and recover from a
  failed probe without a terminal.
- The app builds and launches on the primary platform.
- Mock mode is fully hermetic and visually labeled.
- No secret, private key, or passphrase is stored by the desktop.

### 9.5 Phase D3 — `ss-3-fleet-canvas`

**Goal:** the signature visual cockpit from §8.3 and §8.4, driven by real core view
models and fixture scenarios.

#### D3 file plan

```text
apps/spirit-desktop/src/
├── views/fleet/
│   ├── FleetCanvas.tsx
│   ├── VpsRootCard.tsx
│   ├── WorkerSlotRing.tsx
│   ├── RunningWorkerGrid.tsx
│   ├── WorkerCard.tsx
│   ├── JobLane.tsx
│   ├── JobCard.tsx
│   ├── JobDrawer.tsx
│   └── ProjectSwitcher.tsx
├── views/capacity/
│   ├── CapacityRail.tsx
│   ├── BrakeBanner.tsx
│   ├── TargetAdvisor.tsx
│   ├── DiskPanel.tsx
│   ├── RamPanel.tsx
│   ├── CpuPanel.tsx
│   ├── WaitingPanel.tsx
│   └── LimitsPanel.tsx
├── views/inbox/{InboxDrawer,InboxItem}.tsx
├── domain/{fleet,capacity,executor,format}.ts   # pure, unit-tested
└── test/scenarios/*.json                        # or imports from core fixtures
```

#### D3 deliverables

1. **VPS root card** using `snapshot.home`, supervisor, version, project count, and
   running/queue totals.
2. **Worker slot ring**: target slots, busy/idle/verifier/stalled states, >12 aggregate,
   target 0 state, accessible text summary.
3. **Running worker cards** with executor badges, null-executor shell state, age and
   last-event age, stalled emphasis.
4. **Job lanes** with the exact mapping, counts from `jobs_by_state`, queue truncation
   labels, blocked-card honesty, gate failures, and project chips.
5. **Job drawer** with state-aware Retry/Park/Cancel and no optimistic updates.
6. **Capacity rail**: brake, target advisor, disk, RAM, CPU, waiting reasons, limits,
   provider caps, freshness footer.
7. **Inbox drawer** with full bodies and answer flow.
8. **Scenario suite**: at least the fixtures in §10.2, rendered in tests and Storybook-like
   preview routes (no Storybook dependency required; `MockBridge` scenarios are enough).

#### D3 data flow

```text
spirit://state {revision, fleet}
  -> fleetStore.apply (drop older revision)
  -> selectors (project filter, lanes, gauges)
  -> FleetCanvas / CapacityRail / InboxDrawer
  -> user action -> SpiritBridge.fleet.* -> Tauri command -> VOWP -> refresh
```

- The canvas and rail render only `FleetView`/`CapacityView`; they do not parse raw VOWP
  JSON and do not call `invoke` directly.
- The store keeps the last good view and the current `report_loaded`/stale flags; a
  failed capacity-report poll does not blank the gauges.
- A write triggers a refresh; the renderer never mutates local job or capacity state.

#### D3 test strategy

- `vps_root_shows_supervisor_and_version`
- `ring_renders_three_busy_of_eight`
- `ring_aggregates_above_twelve_slots`
- `ring_target_zero_explains_why`
- `busy_greater_than_target_is_shown_as_an_inconsistency`
- `running_card_shell_has_no_model_badge`
- `executor_badge_splits_harness_provider_model_variant`
- `unknown_executor_keeps_raw_string`
- `lane_counts_come_from_jobs_by_state`
- `queue_cap_200_says_showing_first_200`
- `blocked_card_does_not_invent_title`
- `gate_failure_card_links_to_review_lane`
- `job_actions_disabled_for_ineligible_states`
- `disk_below_floor_is_critical_and_says_new_runs_wait`
- `disk_disagreement_prefers_capacity_report_and_marks_source`
- `ram_estimate_shows_source_and_samples`
- `ram_without_estimate_does_not_invent_slots`
- `capacity_report_unloaded_shows_unavailable_not_empty`
- `worker_card_state_marks_stalled`
- `gate_badge_shows_all_six_states`
- `cpu_pressure_absent_says_not_reported`
- `target_above_advice_requires_override_confirmation`
- `apply_advised_calls_capacity_set_and_refreshes`
- `brake_banner_lists_each_reason`
- `waiting_reasons_group_by_code`
- `provider_limits_do_not_look_like_disk_problems`
- `stale_capacity_disables_target_edit`
- `job_drawer_escape_and_focus_restore`
- `canvas_has_an_accessible_text_summary`
- `axe_has_no_critical_violations_on_fleet_and_capacity`
- `playwright_first_run_to_fleet_scenario`

Visual regression:

- SVG geometry snapshots for the ring at 0, 3/8, 12+, and busy > target.
- Panel snapshots for disk/RAM/CPU at ok/warn/critical/unknown.
- No full-page pixel snapshots (they are noisy and hide missing data).

Performance:

- Render 8 running cards + 200 queue rows in a lane within 100 ms in the test
  environment; virtualization is not required for the MVP because the snapshot caps the
  queue, but the lane must not render an unbounded DOM if the cap changes.

Gates: the D2 gates plus Playwright/axe.

#### D3 done criteria

- A user can read the entire fleet and capacity story without opening a terminal.
- Every value displayed has a documented source in `VOWP-CONTRACT.md`.
- No lane or gauge invents data when VOWP omits it.
- The canvas is keyboard accessible and passes the axe checks.

### 9.6 Phase D4 — `ss-4-engine-onboarding`

**Goal:** the zero-harness first-run experience: a bundled StackSpirit Engine, included
models, BYOK keychain, external harness detection, and the two gate cards.

#### D4 file plan

```text
crates/spirit-core/src/engine/
├── mod.rs
├── catalog.rs             # bundled included-model catalog + runtime verification
├── keychain.rs            # trait, OS implementation, fake
├── runtime.rs             # EngineRuntime trait; opencode adapter
├── supervisor.rs          # launch, health, restart, stop, logs
├── external.rs            # PATH scan, version probes
└── gate.rs                # LocalOrchestratorGate / AsifGate probes

apps/spirit-desktop/src/
├── views/engine/{EngineOnboarding,EngineStatusCard,ModelPicker,ByokForm,ExternalHarnessPicker,GateCard,GateDrawer}.tsx
├── domain/engine.ts
└── test/engine/*

apps/spirit-desktop/src-tauri/
├── commands/engine.rs
├── commands/gates.rs
└── binaries/opencode-<target-triple>   # fetched, not hand-written
```

#### D4 deliverables

1. **Sidecar packaging**: `engine/manifest.json` with pinned version, SHA-256, source,
   license; `scripts/fetch-engine.mjs`; `bundle.externalBin` in `tauri.conf.json`; CI
   checksum verification. The app resolves the sidecar from Tauri's resource/binary path,
   never from `PATH` for the bundled choice.
2. **`EngineSupervisor`**: start on a loopback ephemeral port, health check, restart with
   capped backoff, stop on app exit, no orphan process. Logs are bounded and redacted.
3. **Included model catalog**: generated from the bundled engine's model list at build
   time; runtime verification; `Included` access only when no user key is required.
   Never label a model included unless the verification says so.
4. **BYOK**: OS keychain implementation (`keyring` crate or platform APIs) behind the
   `Keychain` trait. Save/verify/delete/list; provider names only in status; zeroize
   secrets; no key in logs, errors, fixtures, or reports.
5. **External harness picker**: PATH scan for `opencode`, `claude`, `claude-code`;
   version probe; explicit selection; no config edits; no silent fallback.
6. **Gate cards**: `LocalOrchestratorGate` and `AsifGate` probes from
   `VOWP-CONTRACT.md` §11; evidence drawer; disabled actions with reasons; manual
   re-probe. `GateSnapshot.features` is false for both actions in D4, so a `Ready` gate
   still says **not implemented in this build**; D5/D6 flip the corresponding flag when
   the action ships.
7. **First-run flow** and persistence of the user's engine choice/model (local config
   only; no VPS write).
8. **No `opencode` branding** in the main onboarding path. A test asserts the first-run
   screen contains "StackSpirit Engine" and does not contain "opencode" outside an
   advanced details expander.

#### D4 data flow

```text
engine choice -> EngineSupervisor -> local loopback engine
provider key  -> OS keychain (never disk/log/VPS)
model choice  -> local config only (no VOWP write in D4)
gate probe    -> LocalOrchestratorGate/AsifGate -> GateSnapshot -> Engine view
```

- The supervisor starts/stops the bundled sidecar and never lets the renderer spawn a
  process.
- Included-model verification and keychain status are local reads; the VPS sees neither.
- Gate probes run on connect/re-probe and update the same `GateSnapshot` store slice.

#### D4 test strategy

- `bundled_engine_is_found_from_the_resource_dir_not_path`
- `engine_starts_on_loopback_and_never_on_a_public_interface`
- `engine_health_timeout_enters_degraded`
- `engine_restart_backoff_caps_and_does_not_fork_bombs`
- `app_exit_stops_the_bundled_engine`
- `included_model_catalog_requires_runtime_verification`
- `no_included_models_shows_byok_empty_state`
- `byok_save_writes_keychain_and_never_a_file`
- `byok_key_is_absent_from_logs_errors_and_ipc_events`
- `byok_delete_removes_only_the_keychain_entry`
- `byok_verify_failure_keeps_the_key_with_invalid_status`
- `external_scan_matches_exact_binary_names_only`
- `external_selection_never_edits_the_external_config`
- `external_failure_requires_an_explicit_fallback`
- `first_run_does_not_require_opencode_branding`
- `gate_actions_disabled_when_waiting`
- `gate_details_list_exact_missing_milestones`
- `gate_ready_fixture_still_disables_unimplemented_action`
- `asif_export_and_import_enable_independently_after_d5_flag`
- `keychain_implementation_round_trips_on_the_platform_ci` (macOS/Linux/Windows)
- `sidecar_checksum_mismatch_fails_the_build`

Gates: the D3 gates plus:

```sh
cargo test -p spirit-core --features fixtures engine
cargo clippy -p spirit-core --all-targets -- -D warnings
npm test -- engine
npx tauri build --debug
scripts/verify-engine-checksum.sh
```

#### D4 done criteria

- A first-run user can select an included model and reach **Engine ready** without
  installing anything.
- BYOK stores a key in the OS keychain, verifies it locally, and never exposes it.
- The app detects an existing `opencode`/`claude-code` only when asked.
- The gate cards are accurate against current VOWP and switch to `Ready` only when the
  probes succeed.
- No engine branding is a user prerequisite.

### 9.7 Phase D5 — live ASIF (gated)

Not implemented until VOWP `v4-asif-core` and `v4-asif-cli` are merged (the daemon
`v4-asif-web` route is optional for the desktop's SSH path). When the gate is `Ready`
**and** `features.asif_actions` is enabled:

- Sessions drawer gets **Export .asif.zip** and **Import .asif.zip**.
- Export runs the public CLI on the VPS to a private remote temp file, copies it over
  SSH with a size check, then deletes the remote temp file. The summary and declared
  losses are shown before/after. The UI never claims lossless.
- Import streams the local package to
  `vowp --project <project> --json session import - --format asif`, shows `loss_count`,
  `losses`, `unsupported`, and the new session id; the imported session is idle and is
  never auto-run.
- Strict/allow-loss are exposed only as advanced options with VOWP's exact semantics.
- The gate card and buttons are the only entry points; no hidden "force" switch.

The detailed contract is in `VOWP-CONTRACT.md` §11.2.

### 9.8 Phase D6 — local orchestrators (gated)

VOWP Phase 2 (`v4-local-2e/2f/2g`), Phase 3 (`v4-local-3*`), and the Phase 4 CLI surface
(`v4-local-4f-cli-surface`) are merged, and the installed `vowp 0.4.0` advertises them
(`ss-32`, `cd6a975`). D6's core, bridge and Engine panel landed in `ss-20`/`ss-21`, and the
live home read is recorded (`ss-37`, `39d8538`). The Mac-side local agent must still be
enrolled and `connected`. When `LocalOrchestratorGate` is `Ready` **and**
`features.local_orchestrator_actions` is enabled:

- The Engine view creates a local orchestrator through VOWP's own home commands:
  `vowp surface pair` then the Mac-side `vowp agent pair`, and
  `vowp orchestrator new --node <mac> --app opencode` for the app host. `surface ls`,
  `surface rotate`, and `surface revoke` own the surface token lifecycle;
  `orchestrator ls|stop|resume` own the read model and lifecycle.
- The Mac runs one `vowp agent run` (or the optional `vowp agent service install`), and
  exposes MCP through `vowp mcp --agent-socket`; a chat surface uses
  `vowp chat --agent --session <id>`. There is no `vowp agent surface start|attach|detach`
  command in the merged CLI, so D6 does not call or require one.
- A local orchestrator surface uses the StackSpirit Engine or the selected external
  harness; VOWP owns the lease, log, session mirror, and backfill.
- The app displays the node, protocol, and agent status; disconnects and degraded states
  are honest. A local agent that is not enrolled or not `connected` leaves the gate
  `Waiting`/`NotEnrolled`, never `Ready`.
- The app never starts a second supervisor, never opens a VOWP database, and never sends
  a home secret to the desktop.

The detailed contract, command mapping, and probe are in `VOWP-CONTRACT.md` §11.3.

**Status (2026-11-18):** the live home reports an online `macbook` agent card with protocol
`vowp-agent/1`, but the Mac-side `vowp agent status` (`connection: connected`) is unverified
and the live agent reports `agent_version: 0.3.0`. The gate therefore remains `Waiting`, D6
actions are disabled, and no live desktop smoke has been recorded.

### 9.9 W1 — Web app (after Desktop MVP)

- Extract the UI into `packages/spirit-ui` only when the Web job starts; do not
  pre-extract in D1–D4.
- Add `HttpSseBridge` implementing `SpiritBridge` against `vowp-daemon` (read routes) and
  a narrow write proxy. The daemon is loopback-only, so a Web deployment needs an
  explicit, documented reverse proxy; that is not part of this plan.
- Reuse `spirit-core` view types; no second parser.

### 9.10 C1 — CLI (after Desktop MVP)

- `crates/spirit-cli` (`spirit`) links `spirit-core`.
- Commands mirror the bridge: `host add/ls/connect/disconnect`, `fleet`, `capacity`,
  `engine`, `gates`.
- JSON output reuses the core view models. No Tauri dependency.
- The CLI is not a prerequisite for the desktop app.

---

## 10. Test strategy

### 10.1 Test pyramid

| Layer | Tool | Scope | Runs in CI |
|---|---|---|---|
| Wire/contract | `cargo test` | Golden fixtures, field parity, enum tolerance, timestamps | Yes |
| Core units | `cargo test` | SSH argv, store, state machine, view derivation, engine state, gates | Yes |
| Core integration | `cargo test` | Fake SSH runner + fixture client + connection manager end to end | Yes |
| UI units | Vitest + RTL | Components, store, formatting, accessibility roles | Yes |
| UI flows | Playwright (browser mode, MockBridge) | First run, host add/connect, stale, capacity, engine, gates | Yes |
| Tauri host | `cargo test -p spirit-desktop` | Command routing, error mapping, event emission | Yes |
| Build smoke | `npx tauri build --debug` | Bundle, sidecar path, CSP | Primary platform |
| Live smoke | Manual, local only | Real VPS + real VOWP | Not CI |

Every test is hermetic: no real `~/.vowp`, no network, no real keychain in unit tests,
no real provider call, no real SSH. The live smoke is opt-in and uses a host the owner
configures locally; it is never committed and never reads a `.env` file.

### 10.2 Fixture scenarios

Fixtures live in `crates/spirit-core/fixtures/vowp/` and are loaded by the `fixtures`
feature. Each is a complete document, not a hand-written UI object.

| Scenario | What it proves |
|---|---|
| `fresh-single` | 3/8 worker slots, 3 running, 3 ready, 1 reviewing, disk 42 GiB, RAM 12 GiB, no brake |
| `idle` | supervisor up, 0 running, 0 queue, target 4, advised 4 |
| `busy` | 8/8 running, 12 queue, 3 blocked, provider limit |
| `storage-low` | `disk_free_home_bytes < min_disk_bytes`, `disk_low` brake, disk waiting jobs |
| `memory-pressure` | available < reserve, memory-pressure brake, high swap |
| `provider-limit` | quota limit with reset, provider waiting reason |
| `supervisor-down` | `running: false`, old last tick, stale snapshot |
| `stale-cache` | `source: "cache"`, `stale: true` |
| `multi-project` | vowp + northnoir + vowp-asif, project filtering |
| `queue-cap` | queue length 200, truncation label |
| `unknown-states` | a new job state / harness value becomes **Unknown**, not misclassified |
| `no-advice` | target advised null, editor disabled advice action |
| `disk-disagreement` | snapshot vs capacity report disk values; report wins with source label |
| `engine-included` | included models available |
| `engine-no-included` | BYOK empty state |
| `engine-external` | external opencode detected |
| `gate-phase3-waiting` | local orchestrator card waiting with exact missing capabilities |
| `gate-phase3-ready` | synthetic capability evidence; gate Ready, action disabled by the D4 feature flag |
| `gate-asif-waiting` | ASIF card waiting with `v4-asif-*` names |
| `gate-asif-ready` | synthetic export/import evidence; buttons disabled by the D4 feature flag |

Fixtures must use documentation addresses (`192.0.2.0/24`, `2001:db8::/32`) and fake
values only. No real host, key, token, or provider credential.

### 10.3 Contract tests

`crates/spirit-core/tests/contract.rs` is the gate against VOWP drift:

- Parse every fixture into the exact wire type.
- Re-serialize and compare the fields the view consumes (not byte-for-byte; VOWP may
  reorder object keys).
- Assert the documented enum strings for every enum used by the UI.
- Assert timestamp shape with a regex.
- Assert `Option` fields serialize as `null` where the view expects them.
- Assert the `capacity report` parser accepts `provider_caps` and `reload_pending`.
- Assert unknown fields and unknown enum values do not crash.
- Assert `snapshot.schema == "vowp-snapshot/1"` and `version == 1`; other versions are
  `vowp_incompatible`, not silently parsed.

When VOWP changes, the contract test fails first. The fix is to update
`VOWP-CONTRACT.md`, add/update fixtures, and update the view in the same change.

### 10.4 UI test rules

- No test asserts a raw VOWP JSON field in a component; it asserts the view model.
- Every view with a disabled action has a test for the reason text.
- Every gauge has a test for `unknown` and for a missing sample.
- Every drawer/modal has an Esc/focus test.
- Every lane has a test for queue truncation and empty state.
- Snapshot tests are limited to deterministic SVG geometry and small panels.
- MockBridge scenarios are selected by name; no test mutates global state.

### 10.5 Security tests

- Grep-based fixture test: no PEM private-key block, no common API-token prefix,
  no private-key header, and no environment-token assignment. The test asserts on
  patterns in code, so it never needs a real credential in the repository.
- Keychain fake writes to an in-memory map; a test asserts no file is created in the
  profile directory.
- `SecretString` has no `Debug`/`Serialize`; a compile-fail test (trybuild) or a static
  check enforces it.
- Error formatting redacts anything passed as a secret; tests cover keychain, engine, and
  SSH auth errors.
- The raw-document drawer shows only parsed fields and warnings; it never dumps the raw
  command line or stderr without redaction.
- Host-key mismatch is tested at both the core and UI layers.

### 10.6 CI gates

Per job, in order:

```sh
# Core
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p spirit-core
cargo test -p spirit-core --features fixtures

# Desktop UI
npm ci
npm run typecheck
npm run lint
npm test
npm run build
cargo test -p spirit-desktop
cargo clippy -p spirit-desktop --all-targets -- -D warnings
npx tauri build --debug
```

The final merge queue also runs the repository `secret_scan`, `owns_respected`, and
`review` gates. A job report must list the exact commands and results; "docs only" jobs
write `tests: "n/a - ..."` with a reason.

---

## 11. Security and privacy

### 11.1 Secrets

- Provider API keys exist only in the local OS keychain. They are never written to a
  profile, a JSON file, a log, a crash report, a fixture, a test snapshot, or a VOWP
  command.
- The VPS receives provider/model **names** only, and only through VOWP configuration
  that the user explicitly requests (D6). D4 model selection stays local.
- SSH private keys and passphrases are never read by the app. The app stores an identity
  file path and relies on the user's SSH agent or key file.
- No `.env` file is read by the app, tests, or build scripts.
- Logs redact provider keys, lease tokens, and any value typed into a password field.
  Logs are bounded and local.

### 11.2 Host identity

- First connection requires a fingerprint the user confirms out of band.
- The app writes its own `known_hosts` (0600) and uses
  `StrictHostKeyChecking=yes`.
- A changed host key is a hard block with old and new fingerprints. There is no
  auto-accept, no `accept-new`, and no "continue anyway" hidden in a details menu.
- Removing a host removes its pin and known-hosts entry.

### 11.3 Command safety

- `CommandRunner` takes a local argv array; the remote command is built with POSIX
  single-quoting for every argument, with no shell pipeline or operators.
- Host, user, port, paths, job ids, inbox ids, and answer text are validated before use;
  remote text is always quoted, never interpolated.
- Remote stderr is bounded and shown as untrusted text.
- SSH connects only to the configured host; no arbitrary URL fetches, no telemetry, no
  auto-update in D1–D4.

### 11.4 Local engine

- The engine listens on `127.0.0.1` with an ephemeral port; it is never exposed to the
  LAN.
- The bundled binary is checksum-verified at build time and resolved from the app
  resource directory.
- The engine is a child of the app and is stopped on exit. It does not install a
  service in D1–D4.
- External harnesses are the user's own installation; StackSpirit never edits their
  config.

### 11.5 Honesty

- A gate is `Ready` only from a successful capability probe, never from a version string
  alone.
- A preview mode is labeled **Preview data** at all times.
- Missing snapshot fields are **Unknown**, not zero.
- A queue cap is labeled as truncated.
- ASIF exports are never described as lossless; the loss list is shown.

---

## 12. Risks, assumptions, and open questions

### 12.1 Risks

| Risk | Mitigation |
|---|---|
| VOWP snapshot is large and poll latency hurts the UI | Parse off-thread; budget 1 MiB < 50 ms; skip overlapping polls; consider ControlMaster if RTT > 500 ms. |
| `vowp fleet status --json` changes shape | Contract tests fail loudly; keep last good view; update fixtures + contract in one change. |
| `Executor` strings are free-form or unknown | Keep raw string; unknown harness chip; never guess a provider. |
| `BlockedJobCard` lacks a title | Show job id + reason; hydrate only on demand; never invent. |
| Queue is capped at 200 | Label "showing first 200"; counts come from `jobs_by_state`. |
| Capacity and snapshot disagree | Prefer the capacity report for detail, label the source, log a warning. |
| Target above advice is refused | UI asks for override and passes `--override`; VOWP remains authoritative. |
| System `ssh` missing on Windows | Detect and explain; document the optional feature; keep `russh` as a future backend. |
| Sidecar binary size/licensing | Pinned manifest, checksum, license notice, CI check; no commit of the binary. |
| Included models change/disappear | Runtime verification; BYOK empty state; no silent model switch. |
| Gate probe is brittle against help text | Use exit-code probes where possible; mark `Unknown`/`Waiting` on ambiguity; add a `vowp capabilities --json` contract ask. |
| Multiple hosts requested | Out of scope; one active host, multiple saved profiles; revisit after D4. |

### 12.2 Assumptions

- The owner has SSH key access to the VPS and VOWP is installed there.
- VOWP's CLI JSON is stable enough for D1–D4 (snapshot v1, protocol 1).
- The VPS is a single home; the desktop is a control surface, not a second home.
- The primary desktop is macOS; Linux/Windows builds are required but secondary.
- The bundled `opencode` binary can be redistributed under its license; the packaging
  job must verify this and record it.

### 12.3 Open questions (owner)

1. Should the app ever start/stop the VOWP supervisor, or only report it? The plan says
   report only in D1–D4.
2. Is one active host enough for the first release, or is simultaneous multi-host a
   requirement? The plan assumes one.
3. Should the desktop offer a redaction list for ASIF exports in D5, or point users to the
   CLI `--redact-file`? The plan points to the CLI and uses safe defaults.
4. Which included models are contractual for the first release? The catalog is generated
   from the bundled engine at build time; the owner should pin a minimum set.
5. Should external `claude-code` be supported in D4 or only detected? The plan supports
   selection; model access depends on that installation.
6. Is a light theme required for the MVP? The plan is dark-first only.

---

## 13. Definition of done for the Desktop MVP

A release candidate is done when:

1. A new user can add a VPS, verify its host key, connect, and see the fleet without a
   terminal.
2. Disconnect is one click and clearly preserves/dims the last data.
3. The fleet canvas shows the VPS root, the ring up to the target, running worker cards
   with harness + provider/model badges, and all five job lanes.
4. The capacity rail shows disk free vs floor, RAM available vs reserve and p90, CPU
   load/pressure, target vs advised, brake state, waiting reasons, and provider limits.
5. The StackSpirit Engine is bundled, starts locally, offers included models, stores BYOK
   keys in the OS keychain, and detects external harnesses only on request.
6. `LocalOrchestratorGate` and `AsifGate` are visible, honest, and never enable an
   unshipped VOWP feature.
7. All fixtures and contract tests pass; no secret is present; host-key mismatch is a
   hard block.
8. The desktop builds on macOS, Linux, and Windows, and the primary platform passes the
   manual smoke script.
9. The report for each job lists the exact gates run and any platform-specific caveats.

---

## 14. References

- `/home/dev/vowp/docs/design/v0.4/local-orchestrator.md` — Phase 3 local surfaces,
  offload, gates, and commands.
- `/home/dev/vowp/docs/design/v0.4/asif-integration.md` — ASIF CLI/web contract and
  milestone split.
- `/home/dev/vowp/crates/vowp-protocol/src/` — exact Rust wire types.
- [`VOWP-CONTRACT.md`](VOWP-CONTRACT.md) — the frozen read contract and gate probes.
