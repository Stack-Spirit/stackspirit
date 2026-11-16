/**
 * TypeScript mirrors of `spirit-core` view models and bridge DTOs.
 *
 * The Rust structs in `crates/spirit-core` are the source of truth. Every `Option<T>`
 * maps to `T | null`; timestamps are canonical RFC 3339 UTC strings with millis.
 * Components must consume these view models, never raw VOWP JSON.
 */

export type Timestamp = string;
export type ResourceState = "ok" | "warn" | "critical" | "unknown";
export type JobAction = "retry" | "park" | "cancel";
export type TargetMode = "auto" | "fixed";
export type ExternalHarnessKind = "opencode" | "claude-code";
export type ModelAccess = "included" | "byok";

export interface LatencyStats {
  last_ms: number;
  ewma_ms: number;
  p50_ms: number;
  p95_ms: number;
  samples: number;
}

export interface SpiritErrorDto {
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
  pinned_at: Timestamp;
}

export interface HostProfile extends HostDraft {
  id: string;
  host_key: HostKeyPin;
  created_at: Timestamp;
  updated_at: Timestamp;
  last_connected_at: Timestamp | null;
}

export interface NewHostProfile extends HostDraft {
  host_key: HostKeyPin;
}

/** `undefined` leaves a field unchanged; `null` clears a nullable field. */
export interface HostProfilePatch {
  label?: string;
  ssh?: { user: string; host: string; port: number };
  identity_file?: string | null;
  vowp_home?: string | null;
  default_project?: string | null;
}

export interface ScannedHostKey {
  algorithm: string;
  sha256: string;
  preferred: boolean;
}

export interface HostKeyScan {
  keys: ScannedHostKey[];
  scanned_at: Timestamp;
}

export interface HostProbeStage {
  stage: string;
  ok: boolean;
  detail: string;
}

export interface HostProbe {
  reachable: boolean;
  auth_ok: boolean;
  host_key_ok: boolean;
  vowp_version: string | null;
  latency_ms: number;
  stages: HostProbeStage[];
}

export interface ConnectionSnapshot {
  state:
    | "disconnected"
    | "connecting"
    | "connected"
    | "stale"
    | "disconnecting"
    | "failed"
    | string;
  host_id: string | null;
  since: Timestamp | null;
  latency: LatencyStats | null;
  last_ok_at: Timestamp | null;
  error: SpiritErrorDto | null;
}

export interface SetTargetInput {
  target: number;
  mode: TargetMode;
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
  last_checked_at: Timestamp | null;
}

export interface ExternalHarness {
  kind: ExternalHarnessKind;
  path: string;
  version: string | null;
  usable: boolean;
}

export interface ModelChoice {
  id: string;
  label: string;
  provider: string;
  access: ModelAccess;
  context_tokens: number | null;
  notes: string;
  verified_at: Timestamp | null;
}

export type EngineChoice =
  | { type: "bundled" }
  | { type: "external"; path: string; kind: ExternalHarnessKind };

export type EngineState =
  | { state: "not_configured" }
  | { state: "stopped" }
  | { state: "starting" }
  | { state: "ready" }
  | { state: "degraded"; detail: string }
  | { state: "failed"; detail: string };

export interface EngineStatus {
  choice: EngineChoice;
  state: EngineState;
  version: string | null;
  binary_path: string | null;
  port: number | null;
  selected_model: ModelChoice | null;
  included_models_available: boolean;
  started_at: Timestamp | null;
  last_error: string | null;
}

export type LocalAgentState =
  | "unknown"
  | "not_installed"
  | "not_enrolled"
  | "connecting"
  | "connected"
  | "reconnecting"
  | "revoked"
  | "superseded"
  | "stopped"
  | "incompatible";

export interface LocalAgentStatus {
  state: LocalAgentState;
  node: string | null;
  protocol: string | null;
  last_heartbeat: Timestamp | null;
  last_error: string | null;
}

export type LocalOrchestratorGate =
  | { state: "unknown" }
  | { state: "probing" }
  | {
      state: "ready";
      agent_protocol: string;
      node: string | null;
      capabilities: string[];
      evidence: string[];
    }
  | {
      state: "waiting_for_vowp_phase3";
      missing: string[];
      detected_version: string | null;
      evidence: string[];
    }
  | {
      state: "incompatible";
      detected: string;
      required: string;
      evidence: string[];
    }
  | { state: "unreachable"; detail: string };

export type AsifGate =
  | { state: "unknown" }
  | { state: "probing" }
  | {
      state: "ready";
      export: boolean;
      import: boolean;
      detected_version: string | null;
      evidence: string[];
    }
  | {
      state: "waiting_for_vowp_asif";
      missing: string[];
      detected_version: string | null;
      evidence: string[];
    }
  | {
      state: "incompatible";
      detected: string;
      required: string;
      evidence: string[];
    }
  | { state: "unreachable"; detail: string };

export interface GateSnapshot {
  probed_at: Timestamp;
  local_orchestrator: LocalOrchestratorGate;
  asif: AsifGate;
  local_agent: LocalAgentStatus;
  features: { asif_actions: boolean; local_orchestrator_actions: boolean };
}

/** The honest local session/surface state shown by the D6 Engine panel. */
export type LocalSessionState =
  | "active"
  | "idle"
  | "running"
  | "blocked"
  | "switching"
  | "closed"
  | "pending"
  | "disconnected"
  | "degraded"
  | "reconnecting"
  | "revoked"
  | "superseded"
  | "stopped"
  | "unknown";

/** One local surface stored by the Mac agent; never the token or its file. */
export interface LocalAgentSurfaceView {
  surface: string;
  prefix: string | null;
  expires_at: Timestamp | null;
  state: string;
}

/** The safe Mac agent status view, including local cache counters. */
export interface LocalAgentView {
  state: string;
  node: string | null;
  protocol: string | null;
  last_heartbeat: Timestamp | null;
  last_error: string | null;
  cache_entries: number;
  cache_bytes: number;
  cache_oldest_at: Timestamp | null;
  surfaces: LocalAgentSurfaceView[];
}

/** One home surface with its honest node/agent/lease/session state. */
export interface LocalSurfaceView {
  surface_id: string;
  node: string;
  protocol: string | null;
  agent_status: string;
  lease_holder: string | null;
  session_state: LocalSessionState;
  kind: string;
  state: string;
  expires_at: Timestamp | null;
  expired: boolean;
  rotation_pending: boolean;
  can_write: boolean;
}

/** One local orchestrator with the node, protocol, agent and lease state. */
export interface LocalOrchestratorView {
  name: string;
  session_id: string;
  project: string | null;
  app: string;
  state: string;
  node: string | null;
  protocol: string | null;
  agent_status: string;
  lease_holder: string | null;
  session_state: LocalSessionState;
  join: JoinView;
}

/** A safe Mac path-map summary. The absolute Mac root is deliberately absent. */
export interface AgentPathView {
  project: string;
  path_ref: string;
  worktree_root_configured: boolean;
  allow: string[];
  valid: boolean;
  invalid_reason: string | null;
}

/** The redacted `vowp node show --json` fields the D6 UI displays. */
export interface NodeCardView {
  name: string;
  kind: string;
  status: string;
  presence: string;
  labels: string[];
  runs: number;
  mapped: string[];
  agent_version: string | null;
  protocol: string | null;
  last_error: string | null;
}

/** The safe local read model behind the Engine local-orchestrator panel. */
export interface LocalOrchestratorOverview {
  agent: LocalAgentView;
  node: NodeCardView | null;
  surfaces: LocalSurfaceView[];
  orchestrators: LocalOrchestratorView[];
  paths: AgentPathView[];
  paths_error: string | null;
}

/** One home surface row from `vowp surface ls --json`. */
export interface SurfaceEntryView {
  id: string;
  node: string;
  kind: string;
  state: string;
  scopes: string[];
  prefix: string | null;
  expires_at: Timestamp | null;
  overlap_until: Timestamp | null;
  expired: boolean;
  pending: boolean;
  queued_at: Timestamp | null;
  delivery_deadline: Timestamp | null;
}

/** A requested home surface pairing. */
export interface SurfacePairInput {
  session: string;
  node: string;
  kind: "opencode" | "chat";
  app?: string | null;
  replace?: string | null;
}

/** Safe pairing fields; the one-time code is returned separately. */
export interface SurfacePairingView {
  surface: string;
  node: string;
  session: string;
  kind: string;
  app: string | null;
  expires_at: Timestamp;
  command: string;
  replaced: string | null;
}

/**
 * The direct result of a home pairing. `code` is a one-time credential: the renderer
 * may show it once for the owner to copy and must never log or persist it.
 */
export interface SurfacePairResult {
  pairing: SurfacePairingView;
  code: string;
}

/** A requested Mac-side surface pairing. */
export interface AgentPairInput {
  code: string;
  surface: string;
  session: string;
  app?: string | null;
}

/** A paired Mac surface, without the token or its file path. */
export interface AgentPairing {
  surface_id: string;
  scopes: string[];
  expires_at: Timestamp;
}

/** A requested surface token rotation. */
export interface SurfaceRotateInput {
  surface: string;
  node?: string | null;
  wait?: string | null;
  grace?: string | null;
}

/** The `vowp surface rotate --json` result; the token itself is never on the wire. */
export interface SurfaceRotateResult {
  surface: string;
  node: string;
  prefix: string;
  expires_at: Timestamp;
  old_expires_at: Timestamp;
  pending: boolean;
  queued_at: Timestamp | null;
}

/** The `vowp surface revoke --json` result. */
export interface SurfaceRevokeResult {
  surface: string;
  status: string;
  tokens_revoked: number;
}

/** A requested `vowp orchestrator new` local app host. */
export interface OrchestratorNewInput {
  node: string;
  app: "opencode" | "claude_code" | "chat";
  project?: string | null;
  name?: string | null;
  force?: boolean;
}

/** A requested Mac path-map row. `projectRoot` stays on the Mac. */
export interface AgentPathAddInput {
  projectRoot: string;
  name: string;
  worktreeRoot?: string | null;
  allow?: string[];
}

export type WireWarningCode =
  | "unknown_enum"
  | "missing_optional"
  | "truncated"
  | "schema";

export interface WireWarning {
  path: string;
  code: WireWarningCode;
  detail: string;
}

/**
 * Parsed wire documents kept by core for the raw-document drawer. `status` and
 * `capacity` are the complete parsed DTOs (unknown here on purpose: the UI must not
 * depend on VOWP field names), while the warnings are always preserved even when
 * tracing is off.
 */
export interface RawDocument {
  status: unknown | null;
  capacity: unknown | null;
  status_warnings: WireWarning[];
  capacity_warnings: WireWarning[];
  fetched_at: Timestamp | null;
  generated_at: Timestamp | null;
}

export interface SupervisorView {
  running: boolean;
  pid: number | null;
  paused: boolean;
  last_tick_at: Timestamp | null;
  last_tick_age_secs: number | null;
  tick_secs: number;
  fail_streak: number;
  version: string;
}

export interface ProjectView {
  name: string;
  state: string;
  paused_reason: string;
  jobs_by_state: Record<string, number>;
  cost_today_usd: number | null;
}

export interface ExecutorView {
  raw: string;
  harness: string;
  model: string;
  effort: string | null;
  variant: string | null;
  provider: string;
  display: string;
}

export interface WorkerCardView {
  job_id: string;
  project: string;
  kind: string;
  role: string;
  state: string;
  stalled: boolean;
  executor: ExecutorView | null;
  /** The joined session's node, or `null` when no session was matched. */
  node: string | null;
  age_secs: number;
  last_event_age_secs: number | null;
  session_id: string | null;
}

export interface InboxItemView {
  id: string;
  project: string | null;
  job_id: string | null;
  kind: string;
  title: string;
  body: string | null;
  options: string[];
  state: string;
  answer: string | null;
  answered_by: string | null;
  created_at: Timestamp;
  answered_at: Timestamp | null;
  age_secs: number;
}

export interface SurfaceRefView {
  surface_id: string;
  kind: string;
  holder: string;
  node: string;
  acquired_at: Timestamp;
  heartbeat_at: Timestamp;
}

export interface SessionCardView {
  id: string;
  project: string | null;
  job_id: string | null;
  role: string | null;
  title: string;
  state: string;
  executor: ExecutorView | null;
  node: string;
  active_surface: SurfaceRefView | null;
  last_event_at: Timestamp;
}

/** One declared ASIF information change or omission. It is path-free by contract. */
export interface AsifLoss {
  kind: string;
  stage: string;
  scope: string | null;
  message: string;
}

/**
 * Strict/allow-loss is mutually exclusive. `strict` and `allowLoss` are the only
 * loss-policy switches; there is no `allowUnsupported` force switch.
 */
export interface AsifExportOptions {
  strict?: boolean;
  allowLoss?: boolean;
  /**
   * An optional absolute local save path. Desktop callers normally omit it so the
   * native save dialog chooses the location; a supplied path is validated locally.
   */
  path?: string | null;
}

/** Import has no remote `cwd` selector: the renderer never sends or receives a remote path. */
export interface AsifImportOptions {
  branch?: string | null;
  strict?: boolean;
  allowLoss?: boolean;
  /**
   * An optional absolute local package path. Desktop callers normally omit it so the
   * native open dialog chooses the file; a supplied path is validated locally.
   */
  path?: string | null;
}

/**
 * The safe DTO an export returns to the renderer.
 *
 * `file_name` is a local basename only. There is deliberately no remote path,
 * `report_path`, package bytes, token, or provider key.
 */
export interface AsifExportResult {
  session_id: string;
  file_name: string;
  bytes: number;
  loss_count: number;
  losses: AsifLoss[];
  unsupported: string[];
}

/**
 * The safe DTO an import returns. The imported session is always idle; no turn,
 * capsule, lease, or job is created.
 */
export interface AsifImportResult {
  session_id: string;
  loss_count: number;
  losses: AsifLoss[];
  /** VOWP reindex warnings (for example a project left null), not declared losses. */
  warnings: string[];
  unsupported: string[];
  state: "idle";
}

export interface JoinView {
  desktop: string;
  phone: string;
  chat: string;
}

export interface OrchestratorView {
  name: string;
  session_id: string;
  project: string | null;
  app: string;
  state: string;
  join: JoinView;
  node: string | null;
}

export interface HostSampleView {
  at: Timestamp;
  interval_secs: number;
  mem_total_bytes: number;
  mem_available_bytes: number;
  swap_used_bytes: number;
  load1: number;
  load5: number;
  load15: number;
  cpu_count: number;
  cpu_quota_cores: number | null;
  disk_free_home_bytes: number;
  disk_free_worktrees_bytes: number | null;
  pressure_cpu_avg10: number | null;
  pressure_mem_avg10: number | null;
  pressure_io_avg10: number | null;
  source: string;
}

export interface RunningCapacityView {
  runs: number;
  ram_p90_bytes: number;
  cpu_p90_cores: number;
}

export interface TargetView {
  mode: string;
  target_workers: number;
  advised_workers: number | null;
  is_override: boolean;
  set_by: string;
  set_at: Timestamp;
  reason: string;
}

export type CapacityTargetView = TargetView;

export interface BrakeReasonView {
  code: string;
  detail: string;
}

export interface BrakeView {
  active: boolean;
  reasons: BrakeReasonView[];
  since: Timestamp | null;
}

export interface DiskView {
  free_home_bytes: number;
  free_worktrees_bytes: number | null;
  floor_bytes: number;
  source: string;
  state: ResourceState;
}

export interface RamView {
  available_bytes: number;
  reserve_bytes: number;
  running_p90_bytes: number;
  per_worker_p90_bytes: number | null;
  per_worker_source: string | null;
  per_worker_samples: number | null;
  estimated_extra_slots: number | null;
  state: ResourceState;
}

export interface CpuView {
  load1: number;
  cpu_count: number;
  load_ratio: number;
  pressure_avg10: number | null;
  state: ResourceState;
}

export interface EstimateView {
  project: string | null;
  job_kind: string | null;
  harness: string | null;
  model: string | null;
  build_mode: string | null;
  samples: number;
  ram_p50_bytes: number;
  ram_p90_bytes: number;
  cpu_p50_cores: number;
  cpu_p90_cores: number;
  duration_p50_ms: number;
  duration_p90_ms: number;
  source: string;
}

export interface WaitingJobView {
  project: string;
  job: string;
  code: string;
  detail: string;
  since: Timestamp;
}

export interface LimitView {
  provider: string;
  harness: string | null;
  kind: string;
  resets_at: Timestamp | null;
  detail: string;
  since: Timestamp;
  last_probe_at: Timestamp | null;
  probe_fail_streak: number;
  concurrency_cap: number | null;
  cap_until: Timestamp | null;
}

export interface CapacityView {
  generated_at: Timestamp;
  measured_at: Timestamp | null;
  report_loaded: boolean;
  host: HostSampleView | null;
  running: RunningCapacityView;
  target: TargetView;
  advised_workers: number | null;
  brake: BrakeView;
  reserve_ram_bytes: number;
  min_disk_bytes: number;
  disk: DiskView;
  ram: RamView;
  cpu: CpuView;
  estimates: EstimateView[];
  waiting: WaitingJobView[];
  limits: LimitView[];
  provider_caps: Record<string, number>;
  reload_pending: boolean;
}

export interface JobCardView {
  job_id: string;
  project: string;
  title: string | null;
  kind: string;
  role: string;
  state: string;
  priority: number;
  age_secs: number;
  reason: string | null;
  detail: string | null;
  gate: string | null;
  gate_at: Timestamp | null;
  gate_evidence: string | null;
  executor: ExecutorView | null;
  session_id: string | null;
}

export interface JobLanes {
  running: JobCardView[];
  ready: JobCardView[];
  review_fix: JobCardView[];
  parked_blocked: JobCardView[];
  done: JobCardView[];
  counts: Record<string, number>;
  queue_truncated: boolean;
  queue_shown: number;
  queue_cap: number;
}

/** One stored run of a job. `JobDetailView.runs` is oldest first. */
export interface JobRunView {
  id: string;
  role: string;
  purpose: string;
  /** `null` for shell, gate, and merge runs. */
  executor: ExecutorView | null;
  started_at: Timestamp;
  ended_at: Timestamp | null;
  /** `RunOutcome` string, or `null` while the run is still open. */
  outcome: string | null;
  exit_code: number | null;
  /** Redacted by VOWP; StackSpirit also strips control characters and local paths. */
  detail: string;
}

/** One gate result of a job's newest attempt. */
export interface JobGateView {
  attempt: number;
  /** Gate index in the job's gate list; -1 for repo-wide merge checks. */
  gate_index: number;
  /** The `Gate` variant name, for example `report`, `cmd`, or `secret_scan`. */
  kind: string;
  passed: boolean;
  /** Safe summary only: no raw output tail, report body, token, or local path. */
  evidence_summary: string | null;
  duration_ms: number;
  checked_at: Timestamp;
}

/**
 * The read-only detail behind one job drawer: the `Job` fields the drawer needs, its runs
 * oldest first, and the gate results of its newest attempt.
 *
 * `Job.worktree` (an absolute local path) and every home path are deliberately omitted.
 * `report_path` is present only when VOWP reports a repo-relative path.
 */
export interface JobDetailView {
  id: string;
  project: string;
  title: string;
  kind: string;
  role: string;
  state: string;
  priority: number;
  needs: string[];
  attempts: number;
  note: string;
  report_path: string | null;
  parked_reason: string | null;
  created_at: Timestamp;
  state_changed_at: Timestamp;
  updated_at: Timestamp;
  last_progress_at: Timestamp | null;
  /** Client time the three job-detail reads completed. */
  fetched_at: Timestamp;
  /** Oldest first (`started_at`, then id). */
  runs: JobRunView[];
  /** True when the daemon set a `next` cursor on the run page. */
  runs_truncated: boolean;
  /** Ordered by `gate_index`. */
  gates: JobGateView[];
  /** True when the daemon set a `next` cursor on the gate page. */
  gates_truncated: boolean;
}

export interface FleetView {
  revision: number;
  host_id: string;
  fetched_at: Timestamp;
  generated_at: Timestamp;
  source: string;
  stale: boolean;
  home: string;
  supervisor: SupervisorView;
  projects: ProjectView[];
  capacity: CapacityView;
  running: WorkerCardView[];
  lanes: JobLanes;
  inbox: InboxItemView[];
  sessions: SessionCardView[];
  orchestrators: OrchestratorView[];
  latency: LatencyStats;
}

export type StateEvent =
  | { kind: "connection"; revision: number; connection: ConnectionSnapshot }
  | { kind: "fleet"; revision: number; fleet: FleetView }
  | { kind: "engine"; revision: number; engine: EngineStatus }
  | { kind: "gates"; revision: number; gates: GateSnapshot }
  | { kind: "error"; revision: number; error: SpiritErrorDto };

/**
 * What a bridge can honestly supply. The shell reads this map instead of branching on
 * `kind`. Desktop bridges omit it (every capability they implement is assumed present);
 * the Web bridge always publishes the map so a missing daemon route is visible.
 */
export interface BridgeCapabilities {
  capacity_detail: boolean;
  supervisor_state: boolean;
  host_management: boolean;
  engine_management: boolean;
  capability_gates: boolean;
  /** The route exists and a local write proxy is configured. */
  job_actions: boolean;
  /** The route exists and a local write proxy is configured. */
  inbox_answer: boolean;
  set_target: boolean;
  raw_documents: boolean;
  session_detail: boolean;
}

/** The `/api/v1/health` document as the Web shell uses it. */
export interface DaemonHealth {
  /** Scheme + host + optional port. Never a path, query, or token. */
  origin: string;
  /** Version of the running `vowp-daemon` binary. */
  version: string;
  node: string;
  project: string;
  /** Store schema version, not the snapshot schema. */
  schema_version: number;
  heartbeat_at: Timestamp | null;
  received_at: Timestamp;
}

/** Why a desktop-shaped view could not be constructed. */
export interface CapacityUnavailable {
  code: "no_route" | "not_loaded";
  /** The route the daemon does not serve, or `null` when no route would fill it. */
  route: string | null;
  detail: string;
}

export interface SupervisorUnavailable {
  code: "no_route";
  detail: string;
}

export type UnavailableFeature =
  | "capacity_report"
  | "supervisor_state"
  | "active_surfaces"
  | "gate_failures"
  | "orchestrators"
  | "project_state"
  | "raw_documents"
  | "session_detail";

/** One feature the daemon cannot supply, with the exact reason. */
export interface UnavailableNotice {
  feature: UnavailableFeature;
  /** `no_route` means the route is absent; `read_failed` means a present route failed. */
  code: "no_route" | "read_failed";
  route: string | null;
  detail: string;
}

/** A board limit row, reduced exactly as `GET /api/v1/board` reports it. */
export interface BoardLimitView {
  provider: string;
  kind: string;
  resets_at: Timestamp | null;
}

/**
 * The board facts that exist on the current daemon. These are deliberately not a
 * `CapacityView`: no host sample, target, brake, estimate, or waiting list is implied.
 */
export interface BoardFacts {
  slots_busy: number;
  slots_total: number;
  planner: string | null;
  keeper: string | null;
  limits: BoardLimitView[];
  cost_today_usd: number | null;
  inbox_open: number;
  last_checkpoint: Timestamp | null;
}

/** `JobLanes` with the honest "no server-side cap" value for the unpaginated list. */
export interface WebJobLanes extends Omit<JobLanes, "queue_cap"> {
  queue_cap: number | null;
}

/**
 * The honest Web read model for one daemon.
 *
 * It intentionally does not reuse `FleetView`: `FleetView` requires a complete
 * `CapacityView` and `SupervisorView`, and the current daemon exposes neither. The Web
 * shell consumes this type; desktop bridges keep returning `FleetView`.
 */
export interface WebFleetView {
  /** Monotonic per applied snapshot; a local counter, never a server value. */
  revision: number;
  /** Opaque key for the one daemon. It is not a `HostProfile` id. */
  host_id: string;
  /** Time the complete snapshot was received (client clock). */
  fetched_at: Timestamp;
  /** `Board.at`, the time the changed board was collected. Not a freshness signal. */
  generated_at: Timestamp;
  source: "daemon" | "preview";
  /** True only when the last reads failed and the retained view is not current. */
  stale: boolean;
  home: string;
  project: ProjectView;
  board: BoardFacts;
  capacity_unavailable: CapacityUnavailable;
  supervisor_unavailable: SupervisorUnavailable;
  running: WorkerCardView[];
  lanes: WebJobLanes;
  inbox: InboxItemView[];
  /** Empty plus an `active_surfaces` notice; VOWP exposes no surface lease. */
  sessions: SessionCardView[];
  /** Empty plus an `orchestrators` notice. */
  orchestrators: OrchestratorView[];
  /** HTTP round-trip to the daemon. Never SSH or VOWP latency. */
  latency: LatencyStats;
  unavailable: UnavailableNotice[];
  daemon: DaemonHealth;
}

/** Transport state the Web shell shows beside the connection label. */
export interface WebBridgeStatus {
  /** `EventSource` is absent or failed permanently; reads use the 5 s poll. */
  polling_only: boolean;
  /** Epoch milliseconds of the next stale-backoff poll, or `null` when polling is off. */
  next_retry_at: number | null;
  /** Last board frame or poll success. */
  last_frame_at: Timestamp | null;
  /** Last stream heartbeat. */
  heartbeat_at: Timestamp | null;
  /** `open`, `reconnecting`, `polling`, `idle`, or `closed`. */
  stream_state: "open" | "reconnecting" | "polling" | "idle" | "closed";
}
