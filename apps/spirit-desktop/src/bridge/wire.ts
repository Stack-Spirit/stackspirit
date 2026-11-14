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
  node: string;
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
