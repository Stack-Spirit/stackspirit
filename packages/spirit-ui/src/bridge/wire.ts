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

/**
 * The app-owned SSH key as the renderer may see it.
 *
 * Deliberately path-free: the host process resolves the key inside its own app-data
 * directory, and the private half never crosses the bridge.
 */
export interface AppKeyView {
  exists: boolean;
  fingerprint: string | null;
  public_key: string | null;
  /** Ids of saved profiles that point at the app key; never a filesystem location. */
  installed_host_ids?: string[];
}

/**
 * A one-time-password onboarding install.
 *
 * `password` is the server's one-time login password. The host command consumes it
 * once, never stores it, never returns it, and never includes it in an error. The
 * host resolves the app key path itself; the renderer never supplies one.
 */
export interface OnboardingInstallInput {
  draft: HostDraft;
  host_key: HostKeyPin;
  password: string;
  disable_password_auth: boolean;
}

/** Why `PasswordAuthentication no` was not applied. Mirrors the core enum exactly. */
export type PasswordAuthUnsupported =
  | "sudo_unavailable"
  | "no_passwordless_sudo"
  | "sshd_unavailable"
  | "drop_in_unsupported"
  | "drop_in_ignored"
  | "reload_unavailable";

/** The tagged outcome of the optional password-login step. */
export type PasswordAuthOutcome =
  | { state: "not_requested" }
  | { state: "disabled" }
  | { state: "not_supported"; reason: PasswordAuthUnsupported }
  | { state: "failed"; detail: string };

/**
 * The result of one successful install. The one-time password is already gone.
 * `profile` is the saved host profile that now points at the app key.
 */
export interface OnboardingInstallOutcome {
  installed: boolean;
  fingerprint: string;
  password_login_disabled: boolean;
  password_auth: PasswordAuthOutcome;
  profile: HostProfile;
}

/**
 * One stored server secret's metadata. The value itself is never part of this type.
 */
export interface SecretEntryView {
  /** Validated secret name (`^[A-Z][A-Z0-9_]{0,63}$`). */
  name: string;
  /** The last four characters of the value, at most four characters. */
  last4: string;
  /** When VOWP first stored the secret. */
  created_at: Timestamp;
  /** When VOWP last replaced the secret. */
  updated_at: Timestamp;
}

/**
 * The `vowp secret ls` read model.
 *
 * `storage` is the `mode` of the VOWP `0.5.1` `secret ls --json` document (serde alias
 * `mode`): `basic`, `vault_user` or `encrypted`; any other safe token is passed through
 * verbatim. It is `null` when VOWP does not report one (the `0.4.0` bare array); `null`
 * never means "basic".
 */
export interface SecretListView {
  /** Secrets in the exact order VOWP returned them. */
  secrets: SecretEntryView[];
  storage: string | null;
}

/**
 * A write-only request to store one secret on the server.
 *
 * The host consumes `value` once, moves it into zeroizing storage and sends it over
 * verified SSH stdin only. It is never returned, logged, put in argv or included in an
 * error.
 */
export interface SecretSetInput {
  /** Validated secret name (`^[A-Z][A-Z0-9_]{0,63}$`). */
  name: string;
  /** The secret value. Write-only: no read ever returns it. */
  value: string;
}

/** The safe summary of one stored secret. Never the value. */
export interface SecretSetOutcome {
  name: string;
  last4: string;
}

/** The `vowp secret rm NAME --yes` result. */
export interface SecretRemoveOutcome {
  name: string;
  removed: boolean;
}

/** What kind of node a remote directory entry is. */
export type RemoteEntryKind = "file" | "dir" | "symlink";

/** One row of a server directory listing (`spirit_core::vowp::files::RemoteEntry`). */
export interface RemoteEntry {
  name: string;
  /** Absolute path of the entry. */
  path: string;
  kind: RemoteEntryKind;
  size_bytes: number;
  /** Four octal digits, for example `0600`. */
  mode: string;
  modified_at: Timestamp | null;
  /** `.env*`, `id_*`, `*.pem`, `*.key`, `auth.json`, `credentials*`: preview is masked. */
  is_secret_like: boolean;
  /** `vowp.db*`, `vowp.toml`, `.ssh` or a worktree: changing it shows the seatbelt. */
  is_protected_hint: boolean;
}

/** A listed directory with the paths the breadcrumb needs. */
export interface RemoteDirListing {
  /** Resolved absolute path. */
  path: string;
  parent: string | null;
  /** The server user's home (what `~` means). */
  home: string;
  /** Directories first, then files. */
  entries: RemoteEntry[];
  truncated: boolean;
}

export type RemotePreviewKind = "text" | "markdown" | "image" | "binary";

/** A bounded preview of one remote file. Secret-like files are masked unless revealed. */
export interface RemotePreview {
  path: string;
  name: string;
  kind: RemotePreviewKind;
  size_bytes: number;
  /** True when the file is secret-like and was not revealed; no content was read. */
  masked: boolean;
  truncated: boolean;
  text: string | null;
  image_mime: string | null;
  image_base64: string | null;
  note: string | null;
}

export interface RemoteFileOutcome {
  path: string;
}

export interface RemoteUploadOutcome {
  path: string;
  size_bytes: number;
}

export interface RemoteDownloadOutcome {
  local_path: string;
  size_bytes: number;
}

/** Input for moving or renaming a remote path. */
export interface RemoteMoveInput {
  from: string;
  /** The exact destination path (a folder drop passes `folder/name`). */
  to: string;
  /** The user accepted the live-VOWP-path seatbelt. The host re-checks it. */
  confirm_protected?: boolean;
}

/** Input for deleting a remote path. */
export interface RemoteDeleteInput {
  path: string;
  /** Allow deleting a non-empty folder. */
  recursive?: boolean;
  confirm_protected?: boolean;
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

/**
 * One provider the running engine reports. `connected` is the engine's own fact
 * (OpenCode auth, an environment variable or a keychain key); `source` is its source
 * class. No key, endpoint or configuration is carried.
 */
export interface EngineProvider {
  id: string;
  label: string;
  connected?: boolean;
  source?: string;
  model_count?: number;
  free_model_count?: number;
}

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
  /** The running engine's provider catalog; absent while it is stopped. */
  providers?: EngineProvider[];
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
  /** Newest merge, when any. */
  last_merge?: MergeView | null;
}

export interface MergeView {
  job_id: string;
  at: Timestamp;
  commit: string;
  branch: string;
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

/** Structured session ownership (VOWP 0.5.1). Absent on older snapshots. */
export interface SessionOwnerView {
  surface_id: string | null;
  /** Display name: the saved label, else the holder. */
  label: string;
  holder: string;
  kind: string;
  node: string;
  /** `idle`, `running`, `waiting`, or an unknown raw value. */
  activity: string;
  /** `interactive` or `job`. */
  mode: string;
  /** `active`, `stale`, `expired`, or an unknown raw value. */
  state: string;
  epoch: number;
  acquired_at: Timestamp;
  heartbeat_at: Timestamp;
  expires_at: Timestamp | null;
  session_id: string;
}

/** The path-free result of a session takeover. */
export interface SessionTakeoverResult {
  session_id: string;
  previous: string | null;
}

/** One redacted event of a worker session's log (`vowp session tail`). */
export interface SessionEventView {
  seq: number;
  at: string;
  /** Event type such as `assistant.message`, `tool.call`, `turn.started`. */
  kind: string;
  actor: string;
  summary: string;
  detail: string | null;
}

/** A page of a session's event log, oldest first. */
export interface SessionTailView {
  session_id: string;
  events: SessionEventView[];
  /** Pass as `afterSeq` on the next poll. */
  next_seq: number;
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

/** Where an orchestrator moves to; matches the serde tagged enum in spirit-core. */
export type OrchestratorMoveTarget =
  | { kind: "home" }
  | { kind: "node"; node: string };

/** Where a new project comes from; matches the serde tagged enum in spirit-core. */
export type ProjectSource = { kind: "git"; url: string } | { kind: "folder"; path: string };

/**
 * A one-step `vowp project add` request. There is no branch: VOWP clones the
 * repository's default branch.
 */
export interface ProjectAddInput {
  name: string;
  source: ProjectSource;
  init?: boolean;
  orchestrator?: "chat" | "opencode" | "claude_code" | null;
}

/** The result of `project add`. The home-side path VOWP prints is never included. */
export interface ProjectAddOutcome {
  project: string;
  initialized: boolean;
  orchestrator: OrchestratorView | null;
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

/* ---------------------------------------------------------------------------
 * Pooled-compute and failover clusters (`crates/spirit-core/src/cluster`).
 *
 * These are safe mirrors: no SSH target, absolute path, backup destination,
 * pairing code or token is ever part of a cluster view. The core's
 * `ClusterService` owns every remote command; these shapes only describe the
 * saved configuration and the read-only health card.
 * ------------------------------------------------------------------------- */

/** The role of one worker server. */
export type ClusterWorkerRole = "worker" | "backup";

/** Last-known enrollment state of one worker. */
export type ClusterAgentState =
  | "enrolled"
  | "pending"
  | "offline"
  | "revoked"
  | "missing"
  | "unknown";

/** Which host a health row describes. */
export type ClusterHostRole = "home" | "worker" | "backup";

/** Which source produced the pooled totals. */
export type ClusterTotalsSource = "vowp_cluster" | "computed";

/** A backup interval and the owner's retention expectation. */
export interface BackupPolicyInput {
  /** Interval between backup runs; VOWP's schedule minimum is 60 seconds. */
  every_secs: number;
  keep: number;
}

/** A saved backup policy plus VOWP's schedule specification. */
export interface BackupPolicyView extends BackupPolicyInput {
  /** For example `every 1d`, as `BackupPolicy::schedule_spec` renders it. */
  schedule: string;
}

/** One requested worker server. */
export interface ClusterWorkerInput {
  /** Saved host profile id. */
  host: string;
  /** VOWP node name; a lowercase slug unique inside the cluster. */
  node_name: string;
  role: ClusterWorkerRole;
}

/** One configured worker server. */
export interface ClusterWorkerView extends ClusterWorkerInput {
  agent_state: ClusterAgentState;
}

/** The Home project whose `vowp.toml` receives the `[offload]` settings. */
export interface ClusterProjectInput {
  /** Project slug; it is the `--project` selector for the backup schedule. */
  name: string;
  /** Absolute checkout path on Home. It stays in the request and is never stored. */
  root: string;
}

/** One optional worker path map added with `vowp agent paths add`. */
export interface ClusterWorkerProjectInput {
  /** Worker host profile the mapping belongs to. */
  worker: string;
  /** Project slug; the form reuses the Home project's name. */
  project: string;
  /** Absolute checkout on the worker. Never stored after the form. */
  root: string;
  /** Optional absolute worktree root on the worker. */
  worktree_root: string | null;
  /** Allowed relative paths below the root. */
  allow: string[];
}

/** Everything the renderer sends to form a cluster. */
export interface ClusterFormInput {
  /** Exactly one Home host profile. */
  home: string;
  /** One or more worker profiles; none may be Home. */
  workers: ClusterWorkerInput[];
  backup: BackupPolicyInput;
  home_project: ClusterProjectInput;
  /** Optional path maps; an empty list leaves VOWP's existing mappings alone. */
  worker_projects: ClusterWorkerProjectInput[];
  /** Path or rsync target for `vowp backup --to`; never a secret. */
  backup_target: string;
  /** The owner's explicit yes. The host refuses an unconfirmed request. */
  confirmed: boolean;
}

/** The saved cluster shape. It contains no paths or credentials. */
export interface ClusterConfigView {
  home: string;
  workers: ClusterWorkerView[];
  backup: BackupPolicyView;
}

/** The result of forming a cluster, with the safe parts of `ClusterFormed`. */
export interface ClusterFormedView {
  config: ClusterConfigView;
  backup_schedule: string;
  offload_config_changed: boolean;
  formed_at: Timestamp;
}

/** One safe per-host health row. */
export interface ClusterHostHealthView {
  host: string;
  label: string;
  role: ClusterHostRole;
  node_name: string | null;
  agent_state: ClusterAgentState;
  status: string;
  presence: string;
  registered: boolean;
  slots_busy: number;
  slots_max: number;
  mapped_projects: string[];
  last_seen_at: Timestamp | null;
  last_error: string | null;
}

/** The combined cluster read model. */
export interface ClusterHealthView {
  home_node: string;
  /** Active, online worker nodes; Home is not counted. */
  online_workers: number;
  home_target_workers: number;
  /** Maximum slots contributed by the online workers only. */
  worker_slots: number;
  /** Home target plus online worker slots, or VOWP 0.5's cluster total. */
  pooled_target_workers: number;
  /** Home running runs plus online worker busy slots. */
  busy_workers: number;
  cluster_totals_present: boolean;
  totals_source: ClusterTotalsSource;
  hosts: ClusterHostHealthView[];
  warnings: string[];
  checked_at: Timestamp;
}

/** One recorded promotion. It names the restored Home but no directory. */
export interface ClusterPromotionView {
  new_home: string;
  node_name: string;
  backup_at: Timestamp;
  promoted_at: Timestamp;
}

/** The saved cluster plus its latest read. */
export interface ClusterStateView {
  configured: boolean;
  config: ClusterConfigView | null;
  health: ClusterHealthView | null;
  /** A failed health read; the saved config is still shown. */
  health_error: SpiritErrorDto | null;
  /** The latest promotion, if one was recorded on this machine. */
  promotion: ClusterPromotionView | null;
}

/** A request to promote one worker's backup to a new Home. */
export interface ClusterPromoteInput {
  /** One configured worker that holds the backup. */
  target: string;
  /** Absolute backup directory on the target. Never stored. */
  backup_dir: string;
  /** Fresh home directory on the target; null uses the profile's VOWP home. */
  new_home: string | null;
  /** Must equal `promote-backup-to-home`. */
  confirmation: string;
}

/** The restored Home identity; the target's home directory is not returned. */
export interface ClusterPromoteOutcome {
  new_home: string;
  node_name: string;
  backup_at: Timestamp;
  promoted_at: Timestamp;
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
  /** Upstream job ids; explicit `needs`, else derived for verify/fix child jobs. */
  needs?: string[];
  /** Executor a queued job will run on once dispatched; absent on older VOWP snapshots. */
  planned_executor?: ExecutorView | null;
  /** Usage of a running job; each field is absent or null when VOWP does not report it. */
  tokens_input?: number | null;
  tokens_output?: number | null;
  tokens_cache_read?: number | null;
  context_window?: number | null;
  /** API cost in USD; null is unknown, never zero. */
  cost_usd?: number | null;
  billing_mode?: BillingMode | null;
}

/** How a run is billed. Unknown values from newer VOWP builds are shown as unknown usage. */
export type BillingMode = "subscription" | "api" | "free";

export interface JobLanes {
  running: JobCardView[];
  ready: JobCardView[];
  review_fix: JobCardView[];
  parked_blocked: JobCardView[];
  done: JobCardView[];
  /** Cancelled jobs and gate failures of jobs no longer live; History tab only. */
  history?: JobCardView[];
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
  /** Structured session owners keyed by `session_id`; absent or empty on older VOWP. */
  session_owners?: SessionOwnerView[];
  orchestrators: OrchestratorView[];
  latency: LatencyStats;
}

export type StateEvent =
  | { kind: "connection"; revision: number; connection: ConnectionSnapshot }
  | { kind: "fleet"; revision: number; fleet: FleetView }
  | { kind: "engine"; revision: number; engine: EngineStatus }
  | { kind: "gates"; revision: number; gates: GateSnapshot }
  | { kind: "error"; revision: number; error: SpiritErrorDto };

/*
 * Orchestrator chat wire types (`docs/ORCHESTRATOR-CHAT.md` §4.1–§4.2).
 *
 * These types cross the bridge. The chat shapes below deliberately contain no endpoint
 * URL, port, token, authorization header, password, key path, identity file, working
 * directory, binary path, pid, native session id, SSH target or remote port. The core
 * keeps every one of those values private; the renderer receives only the safe shapes
 * below. `ProjectRequest` keeps only the add-project dialog's user input: a Git URL or
 * a folder path the owner chose.
 */

/** Where an orchestrator session runs, as reported by the transport. */
export type SessionLocation = "local" | "server";

/** "setup" is the local first-run assistant; "global" and projects are orchestrators. */
export type SessionId = "setup" | "global" | `project:${string}`;

export type ChatMessageRole = "user" | "assistant";
export type ChatMessageOrigin = "user" | "model" | "app" | "preview";

export interface ChatMessage {
  /** Stable across reconnects; derived from the native message id when one exists. */
  id: string;
  role: ChatMessageRole;
  /** Paragraphs of plain text; never HTML, markdown or a tool dump. */
  text: string[];
  /** Canonical RFC 3339 UTC string with millis. */
  created_at: Timestamp;
  /**
   * Who produced this message. The real path sets `user`, `model` or `app`; only the
   * preview bridge sets `preview`, and every preview assistant message says so in its
   * first paragraph. A real bridge never constructs `preview`.
   */
  origin: ChatMessageOrigin;
  /** True while assistant text is still arriving. */
  streaming?: boolean;
  /**
   * A details-panel hint. Only app/preview messages set it; model text never does.
   * The renderer maps it to `PanelContent`.
   */
  show?: ChatPanelTarget | null;
  /**
   * Compact, redacted tool-call rows for an assistant turn. Absent when the message has
   * no tool parts. Never carries raw tool input, output or error text.
   */
  tools?: ChatToolActivity[];
}

export type ChatToolStatus = "pending" | "running" | "completed" | "error";

/** One compact tool-call row. */
export interface ChatToolActivity {
  /** The native part id. */
  id: string;
  /** The tool name, for example `bash`. */
  name: string;
  status: ChatToolStatus;
  /** A short redacted hint such as the command or path. */
  summary?: string;
}

/** One permission request the engine is waiting on. */
export interface ChatPermissionPrompt {
  /** The native request id, needed to reply. */
  id: string;
  /** The requested action, for example `bash` or `external_directory`. */
  permission: string;
  /** The requested patterns, already redacted for display. */
  patterns: string[];
  created_at: Timestamp;
}

export type ChatPermissionDecision = "once" | "always" | "reject";

/** The details panel target, moved here so `MockBridge` need not import `next/`. */
export type ChatPanelTarget =
  | { kind: "overview" }
  | { kind: "server" }
  | { kind: "inbox" }
  | { kind: "job"; project: string; jobId: string };

/**
 * What the add-project dialog collects. The Git URL is the user's repository address
 * and the folder path is user input, not an endpoint URL or a leaked server/machine
 * path. Core validates both and never echoes them in an error or a view.
 */
export interface ProjectRequest {
  name: string;
  source:
    | { kind: "git"; url: string; branch: string }
    | { kind: "folder"; path: string };
}

export type ChatSessionState =
  | "idle"
  | "connecting"
  | "streaming"
  | "ready"
  | "stopped"
  | "error";

/**
 * Why a model is usable without visiting Settings: included free with the engine,
 * configured with a BYOK key, or owned by the orchestrator.
 */
export type ChatModelTier = "included" | "byok" | "orchestrator";

export interface ChatModelView {
  id: string;
  label: string;
  provider: string;
  /** Added in ss-72; older bridges omit it. */
  tier?: ChatModelTier;
  /** Context window when the engine reports it; otherwise null/absent. */
  context_tokens?: number | null;
  /**
   * Reasoning-effort variants the engine declares for this model (for example `low`,
   * `high`). Absent or empty means the model has no reasoning support.
   */
  variants?: string[];
}

/**
 * Token and context usage reported by the session engine. Every counter is `null`
 * when the engine did not report it; a value is never invented.
 */
export interface ChatUsageView {
  context_tokens_used: number | null;
  context_limit: number | null;
  input_tokens: number | null;
  output_tokens: number | null;
  cache_read_tokens: number | null;
  turns: number;
  cost_usd: number | null;
}

/** The provider action OpenCode attaches to a retry status. */
export interface ChatRetryAction {
  /** Stable reason such as `free_tier_limit` or `rate_limit`. */
  reason: string;
  /** Short title suitable for a notice heading. */
  title: string;
  /** One or two calm sentences from the provider. */
  message: string;
  /** Provider id such as `opencode`. */
  provider: string;
}

/**
 * One active OpenCode `session.status` retry. OpenCode can sleep for hours before the
 * next attempt (the free-tier limit is the common case); the chat shows this instead of
 * a permanent “Replying…” line. `next` is epoch milliseconds.
 */
export interface ChatRetryStatus {
  /** 0-based attempt counter from OpenCode. */
  attempt: number;
  /** Short, redacted provider message; never raw error JSON. */
  message: string;
  /** Epoch milliseconds of the next attempt, when reported. */
  next?: number | null;
  /** Provider action such as the free-tier upgrade prompt, when reported. */
  action?: ChatRetryAction | null;
}

export interface ChatSessionView {
  session: SessionId;
  /** The actual transport location, never the requested label. */
  location: SessionLocation;
  state: ChatSessionState;
  /** True when a native session is attached and usable. */
  attached: boolean;
  /** False when the server has no usable SSE; the UI says so. */
  streaming_supported: boolean;
  model: ChatModelView | null;
  /**
   * Verified models this session's engine can use right now. `undefined` means the
   * transport cannot report a list (an orchestrator owns its config); an empty array
   * is an honest "no usable model".
   */
  available_models?: ChatModelView[];
  /** Live token/context usage, or null/absent when the engine reported none. */
  usage?: ChatUsageView | null;
  started_at: Timestamp | null;
  last_event_at: Timestamp | null;
  error: SpiritErrorDto | null;
  /** Permission requests the engine is waiting on, oldest first. Absent when none. */
  pending_permissions?: ChatPermissionPrompt[];
  /** An active retry wait, absent when OpenCode is not sleeping before another try. */
  retry?: ChatRetryStatus | null;
}

export interface ChatStartInput {
  session: SessionId;
  /** The project name for project:<name>; null for setup/global. Never a path. */
  project: string | null;
  /**
   * A transport hint. Setup is always local. For global/project, VOWP's endpoint is
   * authoritative and may override this hint; the reply reports the actual location.
   */
  location: SessionLocation;
  /**
   * Reuse the stored native session when true (default). For `setup` only,
   * `false` creates a new local session and leaves the old log intact.
   * `global`/`project` never create a session here; they attach to VOWP's.
   */
  resume?: boolean;
}

/** One image/file attached to a user turn; `url` is a `data:` URI. */
export interface ChatAttachment {
  filename: string;
  mime: string;
  url: string;
}

export interface ChatSendInput {
  session: SessionId;
  text: string;
  /** Selected reasoning-effort variant, forwarded to OpenCode as `variant`. */
  variant?: string | null;
  attachments?: ChatAttachment[];
}

export interface ChatSendReceipt {
  session: SessionId;
  /** The user message as stored. Render this; do not invent another one. */
  message: ChatMessage;
  /** True once the request was accepted locally; assistant text arrives as events. */
  accepted: boolean;
}

export interface ChatLogQuery {
  /** Opaque cursor from a previous page; null/absent starts at the newest page. */
  cursor?: string | null;
  /** 1..200; defaults to 50. */
  limit?: number;
}

export interface ChatLogPage {
  messages: ChatMessage[];
  /** Opaque cursor for older messages; null at the start. */
  next: string | null;
  /** True when the local cache dropped messages and this page may have a gap. */
  truncated: boolean;
}

export type ChatCloseReason =
  | "stopped"
  | "engine_stopped"
  | "host_disconnected"
  | "session_lost"
  | "failed";

/**
 * One orchestrator chat event.
 *
 * `revision` is a monotonic per-bridge counter. A gap (`event.revision > last + 1`)
 * means the renderer must call `chat.log` for that session and replace the messages.
 * A `message` event is authoritative and replaces a message with the same `id`; a
 * `delta` only appends. Events never carry an endpoint URL, credential or path.
 */
export type ChatEvent =
  | { kind: "session"; revision: number; session: ChatSessionView }
  | { kind: "message"; revision: number; session: SessionId; message: ChatMessage }
  | {
      kind: "delta";
      revision: number;
      session: SessionId;
      message_id: string;
      part_id: string | null;
      text: string;
    }
  | {
      kind: "done";
      revision: number;
      session: SessionId;
      message_id: string;
      truncated: boolean;
    }
  | { kind: "error"; revision: number; session: SessionId; error: SpiritErrorDto }
  | { kind: "closed"; revision: number; session: SessionId; reason: ChatCloseReason };

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
