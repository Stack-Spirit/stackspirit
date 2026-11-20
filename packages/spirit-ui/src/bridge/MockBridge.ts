import type { ChatBridgeSurface, SpiritBridge } from "./SpiritBridge";
import type {
  ActionResult,
  AgentPairInput,
  AgentPairing,
  AgentPathAddInput,
  AgentPathView,
  AppKeyView,
  AsifExportOptions,
  AsifExportResult,
  AsifGate,
  AsifImportOptions,
  AsifImportResult,
  AsifLoss,
  BackupPolicyView,
  BrakeView,
  CapacityTargetView,
  CapacityView,
  ChatEvent,
  ChatLogPage,
  ChatLogQuery,
  ChatMessage,
  ChatModelView,
  ChatPanelTarget,
  ChatPermissionDecision,
  ChatPermissionPrompt,
  ChatToolActivity,
  ChatSendInput,
  ChatSendReceipt,
  ChatSessionView,
  ChatStartInput,
  ChatUsageView,
  ClusterConfigView,
  ClusterFormInput,
  ClusterFormedView,
  ClusterPromoteInput,
  ClusterPromoteOutcome,
  ClusterStateView,
  ConnectionSnapshot,
  DiskView,
  EngineStatus,
  ExternalHarness,
  ExternalHarnessKind,
  FleetView,
  GateSnapshot,
  HostDraft,
  HostKeyPin,
  HostKeyScan,
  HostProfile,
  HostProfilePatch,
  HostProbe,
  HostProbeStage,
  InboxItemView,
  JobAction,
  JobDetailView,
  JobGateView,
  JobLanes,
  JobRunView,
  KeyStatus,
  LatencyStats,
  LocalAgentSurfaceView,
  LocalAgentView,
  LocalOrchestratorGate,
  LocalOrchestratorOverview,
  LocalOrchestratorView,
  LocalSurfaceView,
  ModelChoice,
  NewHostProfile,
  NodeCardView,
  OnboardingInstallInput,
  OnboardingInstallOutcome,
  OrchestratorNewInput,
  OrchestratorView,
  ProjectView,
  RawDocument,
  SecretListView,
  SecretRemoveOutcome,
  SecretSetInput,
  SecretSetOutcome,
  SessionCardView,
  SessionId,
  SetTargetInput,
  SpiritErrorDto,
  StateEvent,
  SurfaceEntryView,
  SurfacePairInput,
  SurfacePairResult,
  SurfaceRevokeResult,
  SurfaceRotateInput,
  SurfaceRotateResult,
  TargetView,
  WireWarning,
  WorkerCardView,
} from "./wire";
import { gateActionEnabled, gateActionReason } from "../domain/engine";
import { parseExecutor } from "../domain/executor";

export const MOCK_SCENARIOS = [
  "fresh",
  "empty",
  "idle",
  "busy",
  "multi-project",
  "queue-cap",
  "storage-low",
  "supervisor-down",
  "stale-cache",
  "warnings",
  "unknown-states",
  "host-key-mismatch",
  "probe-failure",
  "connect-failure",
  "stale-after-3",
  "engine-ready",
  "engine-no-key",
  "gates-ready",
  "gate-asif-waiting",
  "gate-asif-ready-export-only",
  "gate-asif-ready",
  "asif-export-losses",
  "asif-import-warnings",
  "asif-strict-refusal",
  "asif-conflict",
  "job-detail-error",
  "gate-d6-waiting",
  "gate-d6-incompatible",
  "gate-d6-ready",
  "gate-d6-revoked",
  "chat-preview",
  "chat-streaming",
  "chat-slow",
  "chat-stop",
  "chat-error",
  "chat-reconnect",
  "chat-engine-stopped",
  "chat-external-engine",
  "chat-server-gated",
  "chat-session-lost",
  "chat-tools",
  "chat-permission",
] as const;

export type MockScenario = (typeof MOCK_SCENARIOS)[number];

const EPOCH = Date.parse("2026-11-14T12:00:00.000Z");
const GIB = 1024 ** 3;
const OLD_SHA = "SHA256:oldPin0000000000000000000000000000000000000";
const NEW_SHA = "SHA256:newPin11111111111111111111111111111111111111";
const PIN_SHA = "SHA256:mockEd25519Fingerprint000000000000000000000";

function iso(ms: number): string {
  return new Date(ms).toISOString();
}

function clone<T>(value: T): T {
  return structuredClone(value);
}

function errorDto(
  code: string,
  message: string,
  hint: string | null = null,
  retryable = false,
): SpiritErrorDto {
  return { code, message, hint, retryable };
}

/**
 * Preview mode has no host process: onboarding must fail honestly instead of
 * pretending that a key was created or that a server was changed.
 */
function previewUnavailable(): SpiritErrorDto {
  return errorDto(
    "preview_unavailable",
    "One-time-password onboarding needs the desktop host; preview mode does not create or install keys.",
    "Run the desktop app without VITE_SPIRIT_MOCK to use onboarding.",
  );
}

/**
 * Preview mode has no host and no server vault: every server-secret call must fail
 * honestly instead of faking a list or storing a key in the renderer.
 */
function previewSecretsUnavailable(): SpiritErrorDto {
  return errorDto(
    "preview_unavailable",
    "The server secret vault needs the desktop host; preview mode has no host and does not store keys.",
    "Run the desktop app without VITE_SPIRIT_MOCK to manage server secrets.",
  );
}

/**
 * Preview mode has no SSH connection and no saved cluster, so every cluster action
 * must fail honestly instead of inventing a topology, a pooled total or a promotion.
 */
function previewClusterUnavailable(): SpiritErrorDto {
  return errorDto(
    "preview_unavailable",
    "Clusters need the desktop host; preview mode has no SSH connection and does not form, read or promote one.",
    "Run the desktop app without VITE_SPIRIT_MOCK to work with clusters.",
  );
}

interface MockOptions {
  scenario?: MockScenario;
  now?: string;
  hosts?: HostProfile[];
}

interface MockLocalState {
  agent: LocalAgentView;
  node: NodeCardView | null;
  surfaces: LocalSurfaceView[];
  orchestrators: LocalOrchestratorView[];
  paths: AgentPathView[];
}

/** One recorded job action including the park reason the UI supplied. */
export interface MockJobAction extends ActionResult {
  reason: string | null;
}

/** One recorded ASIF call. Preview-only; it never touches a package or a host. */
export interface MockAsifCall {
  kind: "export" | "import";
  session_id: string | null;
  strict: boolean;
  allow_loss: boolean;
  path: string | null;
}

const ASIF_IMPORTED_SESSION = "01JCF5WQ8ZK3M2V7N4P6R8T0AF";

/** The D6 preview session, surface and node names. None of these is a credential. */
const LOCAL_SESSION_ID = "01JCF5WQ8ZK3M2V7N4P6R8T0AE";
const LOCAL_SURFACE_ID = "01J9Z00000000000000000000A";
const LOCAL_REVOKED_SURFACE_ID = "01J9Z00000000000000000000B";
const LOCAL_NODE = "mac-studio";
const LOCAL_PAIRING_CODE = "pairing-code-placeholder";

/** One recorded D6 call. Preview-only; it never touches a real host or agent. */
export interface MockLocalCall {
  action:
    | "agent_pair"
    | "agent_path_add"
    | "agent_path_remove"
    | "surface_pair"
    | "surface_list"
    | "surface_rotate"
    | "surface_revoke"
    | "orchestrator_new"
    | "orchestrator_list"
    | "orchestrator_stop"
    | "orchestrator_resume";
  name: string | null;
}

type MockLocalGateMode = "unknown" | "waiting" | "incompatible" | "ready" | "revoked";

type MockGateMode = "unknown" | "waiting" | "ready" | "ready-export-only";

const ASIF_EXPORT_LOSSES: AsifLoss[] = [
  {
    kind: "cwd-omitted",
    stage: "translation",
    scope: "environment",
    message: "The working directory was omitted from the portable document.",
  },
  {
    kind: "contexts-reconstructed",
    stage: "normalization",
    scope: "contexts",
    message: "Context references were reconstructed from the event log.",
  },
];

const ASIF_IMPORT_LOSSES: AsifLoss[] = [
  {
    kind: "capture-id-not-preserved",
    stage: "translation",
    scope: "native",
    message: "The source capture identifier was not preserved.",
  },
  {
    kind: "cwd-rebound",
    stage: "translation",
    scope: "environment",
    message: "The working directory was rebound to the target project.",
  },
  {
    kind: "session-id-changed",
    stage: "translation",
    scope: "native",
    message: "The imported session received a new VOWP session id.",
  },
];

export class MockBridge implements SpiritBridge {
  readonly kind = "mock" as const;
  readonly preview = true;

  private readonly scenario: MockScenario;
  private nowMs: number;
  private revision = 0;
  private hostList: HostProfile[];
  private connectionState: ConnectionSnapshot;
  private fleetView: FleetView | null = null;
  private raw: RawDocument | null = null;
  private engineStatus: EngineStatus;
  private gateSnapshot: GateSnapshot;
  private inboxItems: InboxItemView[];
  private listeners = new Set<(event: StateEvent) => void>();
  private latencySamples: number[] = [];
  private failures = 0;
  private backendDown: boolean;
  private actions: MockJobAction[] = [];
  private asifCalls: MockAsifCall[] = [];
  private localCalls: MockLocalCall[] = [];
  private localState: MockLocalState;
  private keys = new Map<string, KeyStatus>();
  private modelChoices: ModelChoice[];
  /**
   * Preview-only chat models. The first is a verified included OpenCode free model, so
   * the setup composer has an honest default; the BYOK entry demonstrates a configured
   * provider without pretending a key exists in the real path.
   */
  private chatModels: ChatModelView[] = [
    {
      id: "opencode/deepseek-v4.1-flash-free",
      label: "DeepSeek V4.1 Flash Free",
      provider: "opencode",
      tier: "included",
      context_tokens: 128_000,
    },
    {
      id: "opencode/big-pickle",
      label: "Big Pickle",
      provider: "opencode",
      tier: "included",
      context_tokens: null,
    },
    {
      id: "deepseek/deepseek-flash",
      label: "DeepSeek V4.1 Flash",
      provider: "deepseek",
      tier: "byok",
      context_tokens: 128_000,
    },
  ];
  /** Preview-only chat state. Nothing here talks to an engine, host or model. */
  private chatSessions = new Map<SessionId, ChatSessionView>();
  private chatLogs = new Map<SessionId, ChatMessage[]>();
  private chatTimers = new Map<SessionId, ReturnType<typeof setTimeout>[]>();
  private chatListeners = new Set<(event: ChatEvent) => void>();
  /** Preview-only: every permission answer, so tests can assert what was sent. */
  readonly permissionAnswers: { id: string; decision: ChatPermissionDecision }[] = [];
  private chatSeq = 0;

  constructor(options: MockOptions | MockScenario = "fresh") {
    const resolved: MockOptions = typeof options === "string" ? { scenario: options } : options;
    this.scenario = resolved.scenario ?? "fresh";
    this.backendDown = this.scenario === "stale-after-3";
    this.nowMs = resolved.now ? Date.parse(resolved.now) : EPOCH;
    const now = iso(this.nowMs);
    this.hostList =
      resolved.hosts ??
      (this.scenario === "empty" || this.scenario === "probe-failure"
        ? []
        : [this.makeHost("staging-vps", "203.0.113.10", now)]);
    this.connectionState = {
      state: "disconnected",
      host_id: null,
      since: null,
      latency: null,
      last_ok_at: null,
      error: null,
    };
    this.modelChoices = [
      {
        id: "deepseek/deepseek-flash",
        label: "DeepSeek V4.1 Flash",
        provider: "deepseek",
        access: "included",
        context_tokens: 128000,
        notes: "Included with StackSpirit Engine; no key required.",
        verified_at: now,
      },
      {
        id: "openai/gpt-5-mini",
        label: "GPT-5 mini",
        provider: "openai",
        access: "included",
        context_tokens: 400000,
        notes: "Included while the engine reports it.",
        verified_at: now,
      },
    ];
    const readyEngine =
      this.scenario === "engine-ready" ||
      this.scenario === "fresh" ||
      this.scenario.startsWith("gate-d6-");
    this.engineStatus = {
      choice: { type: "bundled" },
      state: readyEngine ? { state: "ready" } : { state: "stopped" },
      version: readyEngine ? "0.1.0-mock" : null,
      binary_path: readyEngine ? "/Applications/StackSpirit Engine/mock" : null,
      port: readyEngine ? 43117 : null,
      selected_model: readyEngine ? (this.modelChoices[0] ?? null) : null,
      included_models_available: readyEngine,
      started_at: readyEngine ? now : null,
      last_error: null,
    };
    this.gateSnapshot = this.makeGates(this.gateMode(false));
    this.localState = this.makeLocalState();
    this.inboxItems = this.makeInbox();
  }

  /** Test hook: advances the deterministic clock used by every mock timestamp. */
  advanceClock(ms: number): void {
    this.nowMs += ms;
  }

  /**
   * Test hook: brings the simulated SSH backend back after `stale-after-3`.
   * The next refresh is then a successful command that restores `Connected`.
   */
  restoreBackend(): void {
    this.backendDown = false;
  }

  now(): string {
    return iso(this.nowMs);
  }

  hostsApi = {
    list: async (): Promise<HostProfile[]> => clone(this.hostList),
    add: async (input: NewHostProfile): Promise<HostProfile> => {
      this.validateDraft(input);
      this.validatePin(input.host_key);
      if (this.hostList.some((host) => host.ssh.host === input.ssh.host && host.ssh.port === input.ssh.port)) {
        throw errorDto(
          "conflict",
          `A host for ${input.ssh.host}:${input.ssh.port} already exists.`,
          "Edit the existing profile instead.",
        );
      }
      const now = this.now();
      const profile: HostProfile = {
        ...clone(input),
        label: input.label.trim(),
        id: `01MOCKHOST${String(this.hostList.length + 1).padStart(18, "0")}`,
        created_at: now,
        updated_at: now,
        last_connected_at: null,
      };
      this.hostList = [...this.hostList, profile];
      return clone(profile);
    },
    update: async (id: string, patch: HostProfilePatch): Promise<HostProfile> => {
      const index = this.hostList.findIndex((host) => host.id === id);
      const current = this.hostList[index];
      if (!current) throw errorDto("host_not_found", `Host ${id} was not found.`);
      const candidate: HostProfile = {
        ...current,
        ...(patch.label === undefined ? {} : { label: patch.label.trim() }),
        ...(patch.ssh === undefined ? {} : { ssh: patch.ssh }),
        ...(patch.identity_file === undefined ? {} : { identity_file: patch.identity_file }),
        ...(patch.vowp_home === undefined ? {} : { vowp_home: patch.vowp_home }),
        ...(patch.default_project === undefined
          ? {}
          : { default_project: patch.default_project }),
        updated_at: this.now(),
      };
      this.validateDraft(candidate);
      this.hostList = this.hostList.map((host) => (host.id === id ? candidate : host));
      return clone(candidate);
    },
    remove: async (id: string): Promise<void> => {
      if (!this.hostList.some((host) => host.id === id)) {
        throw errorDto("host_not_found", `Host ${id} was not found.`);
      }
      this.hostList = this.hostList.filter((host) => host.id !== id);
    },
    scanFingerprint: async (input: HostDraft): Promise<HostKeyScan> => {
      this.validateDraft(input);
      return {
        keys: [
          {
            algorithm: "ssh-ed25519",
            sha256: this.scenario === "host-key-mismatch" ? NEW_SHA : PIN_SHA,
            preferred: true,
          },
          {
            algorithm: "ecdsa-sha2-nistp256",
            sha256: "SHA256:mockEcdsaFingerprint00000000000000000000000",
            preferred: false,
          },
        ],
        scanned_at: this.now(),
      };
    },
    trustFingerprint: async (id: string, pin: HostKeyPin): Promise<HostProfile> => {
      this.validatePin(pin);
      const index = this.hostList.findIndex((host) => host.id === id);
      const current = this.hostList[index];
      if (!current) throw errorDto("host_not_found", `Host ${id} was not found.`);
      const updated: HostProfile = { ...current, host_key: clone(pin), updated_at: this.now() };
      this.hostList = this.hostList.map((host) => (host.id === id ? updated : host));
      return clone(updated);
    },
    probe: async (input: HostDraft, pin?: HostKeyPin | null): Promise<HostProbe> => {
      this.validateDraft(input);
      const failing = this.scenario === "probe-failure";
      const supervisorDown = this.scenario === "supervisor-down";
      const stages: HostProbeStage[] = [
        {
          stage: "host_key",
          ok: Boolean(pin),
          detail: pin ? `pinned ${pin.sha256}` : "no pinned fingerprint; scan and confirm one first",
        },
        {
          stage: "ssh",
          ok: !failing,
          detail: failing ? "SSH authentication failed: Permission denied (publickey)." : "SSH reachable",
        },
        {
          stage: "vowp",
          ok: !failing,
          detail: failing ? "not checked: SSH failed first" : "vowp 0.4.0-mock",
        },
        {
          stage: "snapshot",
          ok: !failing,
          detail: failing ? "not checked: SSH failed first" : "snapshot v1 from mock",
        },
        {
          stage: "supervisor",
          ok: !failing && !supervisorDown,
          detail: failing
            ? "not checked: SSH failed first"
            : supervisorDown
              ? "supervisor stopped"
              : "supervisor up, tick 60s",
        },
      ];
      return {
        reachable: !failing,
        auth_ok: !failing,
        host_key_ok: Boolean(pin),
        vowp_version: failing ? null : "0.4.0-mock",
        latency_ms: 42,
        stages,
      };
    },
  };

  onboardingApi = {
    keyStatus: async (): Promise<AppKeyView> => {
      throw previewUnavailable();
    },
    createKey: async (): Promise<AppKeyView> => {
      throw previewUnavailable();
    },
    installKey: async (input: OnboardingInstallInput): Promise<OnboardingInstallOutcome> => {
      // Preview mode has no host, so it never reads the one-time password.
      void input;
      throw previewUnavailable();
    },
  };

  secretsApi = {
    list: async (): Promise<SecretListView> => {
      throw previewSecretsUnavailable();
    },
    set: async (input: SecretSetInput): Promise<SecretSetOutcome> => {
      // Preview mode has no host, so it never reads the write-only value.
      void input;
      throw previewSecretsUnavailable();
    },
    remove: async (name: string): Promise<SecretRemoveOutcome> => {
      void name;
      throw previewSecretsUnavailable();
    },
  };

  clusterApi = {
    form: async (input: ClusterFormInput): Promise<ClusterFormedView> => {
      // Preview mode has no host, so it never reads the confirmation or forms anything.
      void input;
      throw previewClusterUnavailable();
    },
    health: async (): Promise<ClusterStateView> => {
      throw previewClusterUnavailable();
    },
    promote: async (input: ClusterPromoteInput): Promise<ClusterPromoteOutcome> => {
      // The confirmation phrase never unlocks a fake promotion.
      void input;
      throw previewClusterUnavailable();
    },
    backupPolicy: async (): Promise<BackupPolicyView | null> => {
      throw previewClusterUnavailable();
    },
    removeWorker: async (hostId: string): Promise<ClusterConfigView> => {
      void hostId;
      throw previewClusterUnavailable();
    },
  };

  connectionApi = {
    connect: async (hostId: string): Promise<ConnectionSnapshot> => {
      const host = this.hostList.find((candidate) => candidate.id === hostId);
      if (!host) throw errorDto("host_not_found", `Host ${hostId} was not found.`);
      this.setConnection({ state: "connecting", host_id: hostId, error: null });
      await Promise.resolve();
      if (this.scenario === "host-key-mismatch") {
        const error = errorDto(
          "host_key_mismatch",
          `host key changed: pinned ${OLD_SHA}, server offered ${NEW_SHA}`,
          "Compare the fingerprint out of band and explicitly re-pin the host.",
        );
        this.setConnection({ state: "failed", host_id: hostId, error });
        this.emitError(error);
        throw error;
      }
      if (this.scenario === "connect-failure") {
        const error = errorDto(
          "ssh_auth",
          "SSH authentication failed",
          "Check the SSH agent or the identity file path.",
          true,
        );
        this.setConnection({ state: "failed", host_id: hostId, error });
        this.emitError(error);
        throw error;
      }
      this.failures = 0;
      this.recordLatency(42);
      const revision = this.nextRevision();
      this.fleetView = this.makeFleet(hostId, revision);
      this.raw = this.makeRawDocument();
      this.emit({ kind: "fleet", revision, fleet: clone(this.fleetView) });
      this.setConnection({
        state: "connected",
        host_id: hostId,
        since: this.now(),
        latency: this.latency(),
        last_ok_at: this.now(),
        error: null,
      });
      this.hostList = this.hostList.map((candidate) =>
        candidate.id === hostId
          ? { ...candidate, last_connected_at: this.now(), updated_at: this.now() }
          : candidate,
      );
      return clone(this.connectionState);
    },
    disconnect: async (hostId: string): Promise<void> => {
      if (this.connectionState.host_id && this.connectionState.host_id !== hostId) {
        throw errorDto("conflict", "A different host is connected.");
      }
      this.failures = 0;
      this.setConnection({
        state: "disconnected",
        host_id: null,
        since: null,
        latency: null,
        last_ok_at: null,
        error: null,
      });
    },
    status: async (): Promise<ConnectionSnapshot> => clone(this.connectionState),
    subscribe: (listener: (event: StateEvent) => void): (() => void) => {
      this.listeners.add(listener);
      return () => this.listeners.delete(listener);
    },
  };

  fleetApi = {
    get: async (): Promise<FleetView | null> => (this.fleetView ? clone(this.fleetView) : null),
    refresh: async (): Promise<FleetView> => {
      if (!this.fleetView || !this.connectionState.host_id) {
        throw errorDto("conflict", "Connect to a host before refreshing.", "Connect first.");
      }
      if (this.scenario === "stale-after-3" && this.backendDown) {
        this.failures += 1;
        const stale = this.failures >= 3;
        const error = errorDto(
          "remote_timeout",
          "the remote command did not finish within 20s",
          stale
            ? "Three polls failed. Backing off for 30s; Refresh retries immediately."
            : "Check the SSH connection and retry.",
          true,
        );
        if (stale) {
          this.connectionState = {
            ...this.connectionState,
            state: "stale",
            error,
          };
          this.fleetView = { ...this.fleetView, stale: true };
          this.emit({ kind: "connection", revision: this.nextRevision(), connection: clone(this.connectionState) });
        }
        this.emitError(error);
        throw error;
      }
      this.failures = 0;
      this.recordLatency(38);
      const revision = this.nextRevision();
      if (this.scenario !== "stale-cache" && this.scenario !== "supervisor-down") {
        this.fleetView = {
          ...this.fleetView,
          revision,
          fetched_at: this.now(),
          generated_at: this.now(),
          stale: false,
          latency: this.latency(),
        };
      } else {
        this.fleetView = { ...this.fleetView, revision, latency: this.latency() };
      }
      // A successful command restores Connected and resets the backoff (PLAN §6.2).
      this.connectionState = {
        ...this.connectionState,
        state: "connected",
        latency: this.latency(),
        last_ok_at: this.now(),
        error: null,
      };
      this.emit({ kind: "fleet", revision, fleet: clone(this.fleetView) });
      this.emit({ kind: "connection", revision, connection: clone(this.connectionState) });
      return clone(this.fleetView);
    },
    capacity: async (): Promise<CapacityView | null> =>
      this.fleetView ? clone(this.fleetView.capacity) : null,
    job: async (id: string): Promise<JobDetailView> => {
      if (this.scenario === "job-detail-error") {
        throw errorDto(
          "daemon_unreachable",
          "The daemon at mock.invalid did not answer.",
          "Start the daemon and retry.",
          true,
        );
      }
      const detail = this.makeJobDetail(id);
      if (!detail) {
        throw errorDto(
          "not_found",
          `Job ${id} was not found.`,
          "Refresh the job list.",
          false,
        );
      }
      return clone(detail);
    },
    jobAction: async (
      jobId: string,
      action: JobAction,
      reason?: string | null,
    ): Promise<ActionResult> => {
      const known = this.fleetView
        ? [
            ...this.fleetView.running.map((card) => card.job_id),
            ...Object.values(this.fleetView.lanes).flatMap((lane) =>
              Array.isArray(lane) ? lane.map((card) => card.job_id) : [],
            ),
          ].includes(jobId)
        : false;
      if (!known) throw errorDto("not_found", `Job ${jobId} was not found.`);
      const trimmedReason = reason?.trim() ?? "";
      if (action === "park" && trimmedReason.length === 0) {
        throw errorDto("invalid_input", "reason: must not be empty when parking a job");
      }
      if (trimmedReason.length > 200) {
        throw errorDto("invalid_input", "reason: must be at most 200 characters");
      }
      const result: MockJobAction = {
        job_id: jobId,
        action,
        request: null,
        refreshed: false,
        reason: action === "park" ? trimmedReason : null,
      };
      this.actions.push(result);
      return clone(result);
    },
    inbox: async (all: boolean): Promise<InboxItemView[]> => {
      if (all) return clone(this.inboxItems);
      return clone(
        this.inboxItems.map((item) => ({
          ...item,
          body: null,
          options: [],
          state: "open",
          answer: null,
          answered_by: null,
          answered_at: null,
        })),
      );
    },
    answerInbox: async (id: string, answer: string): Promise<InboxItemView> => {
      if (!answer.trim()) {
        throw errorDto("invalid_input", "answer: must be 1-10,000 bytes and contain no NUL");
      }
      const item = this.inboxItems.find((candidate) => candidate.id === id);
      if (!item) throw errorDto("not_found", `Inbox item ${id} was not found.`);
      const answered: InboxItemView = {
        ...item,
        state: "answered",
        answer,
        answered_by: "stackspirit",
        answered_at: this.now(),
      };
      this.inboxItems = this.inboxItems.map((candidate) => (candidate.id === id ? answered : candidate));
      return clone(answered);
    },
    setTarget: async (input: SetTargetInput): Promise<CapacityTargetView> => {
      if (!Number.isInteger(input.target) || input.target < 0 || input.target > 64) {
        throw errorDto("invalid_input", "target: must be an integer between 0 and 64");
      }
      if (!input.reason.trim()) {
        throw errorDto("invalid_input", "reason: must not be empty");
      }
      const target: TargetView = {
        mode: input.mode,
        target_workers: input.target,
        advised_workers: this.fleetView?.capacity.advised_workers ?? null,
        is_override: input.override,
        set_by: "stackspirit",
        set_at: this.now(),
        reason: input.reason,
      };
      if (this.fleetView) {
        const revision = this.nextRevision();
        this.fleetView = {
          ...this.fleetView,
          revision,
          capacity: { ...this.fleetView.capacity, target },
        };
        this.emit({ kind: "fleet", revision, fleet: clone(this.fleetView) });
      }
      return clone(target);
    },
    rawDocument: async (): Promise<RawDocument | null> => (this.raw ? clone(this.raw) : null),
  };

  engineApi = {
    status: async (): Promise<EngineStatus> => clone(this.engineStatus),
    models: async (): Promise<ModelChoice[]> => clone(this.modelChoices),
    selectModel: async (id: string): Promise<EngineStatus> => {
      const model = this.modelChoices.find((candidate) => candidate.id === id);
      if (!model) throw errorDto("not_found", `Model ${id} was not found.`);
      this.engineStatus = { ...this.engineStatus, selected_model: clone(model) };
      this.emit({ kind: "engine", revision: this.nextRevision(), engine: clone(this.engineStatus) });
      return clone(this.engineStatus);
    },
    storeKey: async (provider: string, secret: string): Promise<KeyStatus> => {
      if (!provider.trim() || !secret) {
        throw errorDto("keychain", "keychain: provider and key are required");
      }
      const status: KeyStatus = { provider, state: "saved", last_checked_at: this.now() };
      this.keys.set(provider, status);
      return clone(status);
    },
    verifyKey: async (provider: string): Promise<KeyStatus> => {
      const status: KeyStatus = {
        provider,
        state: "unverified",
        last_checked_at: this.now(),
      };
      this.keys.set(provider, status);
      return clone(status);
    },
    deleteKey: async (provider: string): Promise<KeyStatus> => {
      const status: KeyStatus = { provider, state: "missing", last_checked_at: this.now() };
      this.keys.set(provider, status);
      return clone(status);
    },
    keyStatus: async (): Promise<KeyStatus[]> => clone([...this.keys.values()]),
    scanExternal: async (): Promise<ExternalHarness[]> => [],
    selectExternal: async (kind: ExternalHarnessKind, path: string): Promise<EngineStatus> => {
      this.engineStatus = {
        ...this.engineStatus,
        choice: { type: "external", kind, path },
        state: { state: "stopped" },
      };
      this.emit({ kind: "engine", revision: this.nextRevision(), engine: clone(this.engineStatus) });
      return clone(this.engineStatus);
    },
    useBundled: async (): Promise<EngineStatus> => {
      this.engineStatus = { ...this.engineStatus, choice: { type: "bundled" } };
      this.emit({ kind: "engine", revision: this.nextRevision(), engine: clone(this.engineStatus) });
      return clone(this.engineStatus);
    },
    restart: async (): Promise<EngineStatus> => {
      this.engineStatus = {
        ...this.engineStatus,
        state: { state: "ready" },
        included_models_available: true,
        last_error: null,
        started_at: this.now(),
      };
      this.emit({ kind: "engine", revision: this.nextRevision(), engine: clone(this.engineStatus) });
      return clone(this.engineStatus);
    },
  };

  gatesApi = {
    get: async (): Promise<GateSnapshot> => clone(this.gateSnapshot),
    probe: async (): Promise<GateSnapshot> => {
      this.gateSnapshot = this.makeGates(this.gateMode(true), this.now());
      this.localState = this.makeLocalState();
      this.emit({ kind: "gates", revision: this.nextRevision(), gates: clone(this.gateSnapshot) });
      return clone(this.gateSnapshot);
    },
  };

  sessionsApi = {
    list: async (): Promise<SessionCardView[]> =>
      clone(this.fleetView?.sessions ?? []),
    exportAsif: async (
      sessionId: string,
      options: AsifExportOptions = {},
    ): Promise<AsifExportResult | null> => {
      this.requireAsifAction("asif_export");
      this.validateAsifOptions(options, ["strict", "allowLoss", "path"]);
      this.requireSessionId(sessionId);
      const strict = options.strict === true;
      const allowLoss = options.allowLoss === true;
      this.recordAsifCall({
        kind: "export",
        session_id: sessionId,
        strict,
        allow_loss: allowLoss,
        path: options.path ?? null,
      });
      if (this.scenario === "asif-strict-refusal" && strict) {
        throw errorDto(
          "refused",
          "VOWP refused the ASIF export: a blocking or integrity loss was found.",
          "Review the declared losses, or choose allow-loss explicitly. StackSpirit has no force switch.",
        );
      }
      const losses = ASIF_EXPORT_LOSSES;
      return {
        session_id: sessionId,
        file_name: this.localFileName(sessionId, options.path),
        bytes: 18342,
        loss_count: losses.length,
        losses: clone(losses),
        unsupported: ["vowp.jobs", "vowp.gates", "vowp.leases", "vowp.project-db"],
      };
    },
    importAsif: async (options: AsifImportOptions = {}): Promise<AsifImportResult | null> => {
      this.requireAsifAction("asif_import");
      this.validateAsifOptions(options, ["branch", "strict", "allowLoss", "path"]);
      if (options.branch !== undefined && options.branch !== null) {
        const branch = options.branch;
        if (
          typeof branch !== "string" ||
          branch.length === 0 ||
          branch.length > 200 ||
          branch.startsWith("-")
        ) {
          throw errorDto("invalid_input", "ASIF import branch: must be 1-200 non-whitespace characters");
        }
        const invalidChar = [...branch].some((char) => {
          const code = char.codePointAt(0) ?? 0;
          return char.trim() === "" || code < 0x20 || code === 0x7f;
        });
        if (invalidChar) {
          throw errorDto("invalid_input", "ASIF import branch: must be 1-200 non-whitespace characters");
        }
      }
      this.recordAsifCall({
        kind: "import",
        session_id: null,
        strict: options.strict === true,
        allow_loss: options.allowLoss === true,
        path: options.path ?? null,
      });
      if (this.scenario === "asif-conflict") {
        throw errorDto(
          "conflict",
          "The ASIF import conflicts with an existing session's event ids.",
          "StackSpirit does not retry with a mutated package. Inspect the source session and choose a clean package.",
        );
      }
      const withWarnings = this.scenario === "asif-import-warnings";
      return {
        session_id: ASIF_IMPORTED_SESSION,
        loss_count: ASIF_IMPORT_LOSSES.length,
        losses: clone(ASIF_IMPORT_LOSSES),
        warnings: withWarnings ? ["project example-source left null"] : [],
        unsupported: [],
        state: "idle",
      };
    },
  };

  localOrchestratorsApi = {
    overview: async (): Promise<LocalOrchestratorOverview> => {
      this.requireConnectedHost();
      return clone({
        agent: this.localState.agent,
        node: this.localState.node,
        surfaces: this.localState.surfaces,
        orchestrators: this.localState.orchestrators,
        paths: this.localState.paths,
        paths_error: null,
      });
    },
    agentStatus: async (): Promise<LocalAgentView> => clone(this.localState.agent),
    agentPair: async (input: AgentPairInput): Promise<AgentPairing> => {
      this.validateAgentPair(input);
      this.requireLocalAction("agent_pair");
      this.recordLocal("agent_pair", input.surface);
      const surface = this.localState.surfaces.find(
        (candidate) => candidate.surface_id === input.surface,
      );
      if (!surface) {
        throw errorDto("not_found", `Surface ${input.surface} was not found.`);
      }
      const paired: LocalSurfaceView = {
        ...surface,
        state: "attached",
        agent_status: "connected",
        session_state: "active",
        can_write: true,
      };
      const agentSurfaces: LocalAgentSurfaceView[] = [
        ...this.localState.agent.surfaces.filter((row) => row.surface !== input.surface),
        {
          surface: input.surface,
          prefix: "vowp_surf_ab",
          expires_at: iso(this.nowMs + 30 * 24 * 3_600_000),
          state: "paired",
        },
      ];
      this.localState = {
        ...this.localState,
        agent: { ...this.localState.agent, surfaces: agentSurfaces },
        surfaces: this.localState.surfaces.map((row) =>
          row.surface_id === input.surface ? paired : row,
        ),
      };
      return {
        surface_id: input.surface,
        scopes: [`session.read:${input.session}`, `session.write:${input.session}`],
        expires_at: iso(this.nowMs + 30 * 24 * 3_600_000),
      };
    },
    agentPaths: async (): Promise<AgentPathView[]> => clone(this.localState.paths),
    agentPathAdd: async (input: AgentPathAddInput): Promise<AgentPathView> => {
      this.validatePathAdd(input);
      this.recordLocal("agent_path_add", input.name);
      const view: AgentPathView = {
        project: input.name,
        path_ref: `project:${input.name}`,
        worktree_root_configured: Boolean(input.worktreeRoot),
        allow: [...(input.allow ?? [])],
        valid: true,
        invalid_reason: null,
      };
      this.localState = {
        ...this.localState,
        paths: [...this.localState.paths.filter((row) => row.project !== input.name), view],
      };
      return clone(view);
    },
    agentPathRemove: async (project: string): Promise<void> => {
      this.validateProject(project);
      this.recordLocal("agent_path_remove", project);
      this.localState = {
        ...this.localState,
        paths: this.localState.paths.filter((row) => row.project !== project),
      };
    },
    surfacePair: async (input: SurfacePairInput): Promise<SurfacePairResult> => {
      this.validateSurfacePair(input);
      this.requireLocalAction("surface_pair");
      this.recordLocal("surface_pair", input.node);
      const surfaceId = `01J9Z${String(100 + this.localState.surfaces.length).padStart(21, "0")}`;
      const surface: LocalSurfaceView = {
        surface_id: surfaceId,
        node: input.node,
        protocol: "vowp-agent/1",
        agent_status: this.localState.agent.state,
        lease_holder: null,
        session_state: "pending",
        kind: input.kind,
        state: "pending",
        expires_at: iso(this.nowMs + 10 * 60_000),
        expired: false,
        rotation_pending: false,
        can_write: false,
      };
      this.localState = {
        ...this.localState,
        surfaces: [...this.localState.surfaces, surface],
      };
      const command = [
        "vowp agent pair --code-file -",
        `--surface ${surfaceId}`,
        `--session ${input.session}`,
        input.app ? `--app ${input.app}` : "",
      ]
        .filter(Boolean)
        .join(" ");
      return {
        pairing: {
          surface: surfaceId,
          node: input.node,
          session: input.session,
          kind: input.kind,
          app: input.app ?? null,
          expires_at: iso(this.nowMs + 10 * 60_000),
          command,
          replaced: input.replace ?? null,
        },
        code: LOCAL_PAIRING_CODE,
      };
    },
    surfaceList: async (node?: string | null): Promise<SurfaceEntryView[]> => {
      this.requireConnectedHost();
      this.recordLocal("surface_list", node ?? null);
      return clone(
        this.localState.surfaces
          .filter((surface) => !node || surface.node === node)
          .map((surface) => this.toSurfaceEntry(surface)),
      );
    },
    surfaceRotate: async (input: SurfaceRotateInput): Promise<SurfaceRotateResult> => {
      this.validateSurfaceRotate(input);
      this.requireConnectedHost();
      this.requireLocalAction("surface_rotate");
      this.recordLocal("surface_rotate", input.surface);
      const surface = this.localState.surfaces.find(
        (candidate) => candidate.surface_id === input.surface,
      );
      if (!surface) {
        throw errorDto("not_found", `Surface ${input.surface} was not found.`);
      }
      this.localState = {
        ...this.localState,
        surfaces: this.localState.surfaces.map((row) =>
          row.surface_id === input.surface ? { ...row, rotation_pending: true } : row,
        ),
      };
      return {
        surface: input.surface,
        node: surface.node,
        prefix: "vowp_surf_cd",
        expires_at: iso(this.nowMs + 30 * 24 * 3_600_000),
        old_expires_at: iso(this.nowMs + 10 * 60_000),
        pending: true,
        queued_at: this.now(),
      };
    },
    surfaceRevoke: async (surface: string): Promise<SurfaceRevokeResult> => {
      this.validateSurfaceId(surface);
      this.requireConnectedHost();
      this.recordLocal("surface_revoke", surface);
      const known = this.localState.surfaces.some((row) => row.surface_id === surface);
      if (!known) {
        throw errorDto("not_found", `Surface ${surface} was not found.`);
      }
      this.localState = {
        ...this.localState,
        surfaces: this.localState.surfaces.map((row) =>
          row.surface_id === surface
            ? {
                ...row,
                state: "revoked",
                session_state: "revoked",
                can_write: false,
                expired: true,
              }
            : row,
        ),
      };
      return { surface, status: "revoked", tokens_revoked: 1 };
    },
    orchestratorNew: async (input: OrchestratorNewInput): Promise<OrchestratorView> => {
      this.validateOrchestratorNew(input);
      this.requireLocalAction("orchestrator_new");
      this.recordLocal("orchestrator_new", input.name ?? input.node);
      const name = input.name ?? `local-${this.localState.orchestrators.length + 1}`;
      const existing = this.localState.orchestrators.find((row) => row.name === name);
      if (existing && !input.force) {
        throw errorDto("conflict", `Orchestrator ${name} already exists.`);
      }
      const view: LocalOrchestratorView = {
        name,
        session_id: "01JCF5WQ8ZK3M2V7N4P6R8T0AG",
        project: input.project ?? null,
        app: input.app,
        state: "running",
        node: input.node,
        protocol: "vowp-agent/1",
        agent_status: this.localState.agent.state,
        lease_holder: "StackSpirit Desktop",
        session_state: "running",
        join: this.localJoin("01JCF5WQ8ZK3M2V7N4P6R8T0AG"),
      };
      this.localState = {
        ...this.localState,
        orchestrators: [
          ...this.localState.orchestrators.filter((row) => row.name !== name),
          view,
        ],
      };
      return clone(this.toOrchestrator(view));
    },
    orchestratorList: async (): Promise<OrchestratorView[]> => {
      this.requireConnectedHost();
      this.recordLocal("orchestrator_list", null);
      return clone(this.localState.orchestrators.map((row) => this.toOrchestrator(row)));
    },
    orchestratorStop: async (name: string): Promise<OrchestratorView> => {
      this.requireConnectedHost();
      this.validateOrchestratorName(name);
      this.recordLocal("orchestrator_stop", name);
      const orchestrator = this.localState.orchestrators.find((row) => row.name === name);
      if (!orchestrator) {
        throw errorDto("not_found", `Orchestrator ${name} was not found.`);
      }
      const stopped: LocalOrchestratorView = {
        ...orchestrator,
        state: "stopped",
        session_state: "stopped",
        lease_holder: null,
      };
      this.localState = {
        ...this.localState,
        orchestrators: this.localState.orchestrators.map((row) =>
          row.name === name ? stopped : row,
        ),
      };
      return clone(this.toOrchestrator(stopped));
    },
    orchestratorResume: async (
      name: string,
      project?: string | null,
      node?: string | null,
    ): Promise<OrchestratorView> => {
      this.validateOrchestratorName(name);
      this.requireConnectedHost();
      if (project) this.validateProject(project);
      if (node) this.validateNode(node);
      this.requireLocalAction("orchestrator_resume");
      this.recordLocal("orchestrator_resume", name);
      const orchestrator = this.localState.orchestrators.find((row) => row.name === name);
      if (!orchestrator) {
        throw errorDto("not_found", `Orchestrator ${name} was not found.`);
      }
      const running: LocalOrchestratorView = {
        ...orchestrator,
        state: "running",
        session_state: "running",
        lease_holder: "StackSpirit Desktop",
      };
      this.localState = {
        ...this.localState,
        orchestrators: this.localState.orchestrators.map((row) =>
          row.name === name ? running : row,
        ),
      };
      return clone(this.toOrchestrator(running));
    },
  };

  /**
   * Preview-only orchestrator chat (`docs/ORCHESTRATOR-CHAT.md` §4.3 and §4.7).
   *
   * It never talks to an engine, host or model. Assistant messages are always
   * `origin: "preview"` and say so in their first paragraph; the real path never
   * constructs this origin.
   */
  chatApi: ChatBridgeSurface = {
    start: async (input: ChatStartInput): Promise<ChatSessionView> => {
      this.validateChatStart(input);
      this.requireChatScenario(input.session);
      if (input.resume === false && input.session !== "setup") {
        throw errorDto(
          "chat_not_mirrored",
          "A new global or project session must be created in VOWP; preview does not create an unmirrored session.",
          "Create or recreate the orchestrator in VOWP, then attach.",
        );
      }
      const existing = this.chatSessions.get(input.session);
      if (existing && input.resume !== false) {
        if (existing.state === "stopped" || existing.state === "error") {
          return clone(
            this.updateChatView(input.session, {
              state: "ready",
              attached: true,
              error: null,
            }),
          );
        }
        return clone(existing);
      }
      if (input.resume === false) {
        this.clearChatTimers(input.session);
        this.chatLogs.set(input.session, []);
      } else if (!this.chatLogs.has(input.session)) {
        this.chatLogs.set(input.session, []);
      }
      const view = this.makeChatView(
        input.session,
        input.session === "setup" ? "local" : input.location,
      );
      this.chatSessions.set(input.session, view);
      this.emitChat({
        kind: "session",
        revision: this.nextRevision(),
        session: clone(view),
      });
      return clone(view);
    },
    attach: async (session: SessionId): Promise<ChatSessionView> => {
      this.validateChatSession(session);
      if (this.scenario === "chat-session-lost" && session !== "setup") {
        throw errorDto(
          "session_lost",
          "The stored native session no longer exists.",
          "Recreate the orchestrator session in VOWP, then attach.",
        );
      }
      if (!this.chatSessions.has(session)) {
        throw errorDto(
          "chat_not_started",
          "No session has been started in this app run.",
          "Call chat.start first.",
        );
      }
      return clone(
        this.updateChatView(session, { state: "ready", attached: true, error: null }),
      );
    },
    send: async (input: ChatSendInput): Promise<ChatSendReceipt> => {
      this.validateChatSession(input.session);
      const text = typeof input.text === "string" ? input.text.trim() : "";
      if (text.length === 0) {
        throw errorDto("invalid_input", "message: must not be empty");
      }
      if (text.length > 32_000) {
        throw errorDto(
          "invalid_input",
          "message: must be at most 32,000 characters",
        );
      }
      const view = this.chatSessions.get(input.session);
      if (!view || !view.attached) {
        throw errorDto(
          "chat_not_started",
          "Start or attach the session before sending.",
          "Call chat.start or chat.attach.",
        );
      }
      if (view.state === "streaming") {
        throw errorDto(
          "chat_busy",
          "A turn is already running; there is no queue.",
          "Wait for it to finish or stop it.",
        );
      }
      if (view.state !== "ready") {
        throw errorDto(
          "chat_not_started",
          "The session is not ready for another turn.",
          "Start or attach the session, then send.",
        );
      }
      const message: ChatMessage = {
        id: this.nextChatId("local"),
        role: "user",
        text: [text],
        created_at: this.now(),
        origin: "user",
      };
      this.chatLogs.set(input.session, [
        ...(this.chatLogs.get(input.session) ?? []),
        message,
      ]);
      this.updateChatView(input.session, {
        state: "streaming",
        attached: true,
        error: null,
      });
      this.emitChat({
        kind: "message",
        revision: this.nextRevision(),
        session: input.session,
        message: clone(message),
      });
      this.scheduleChatReply(input.session, text);
      return { session: input.session, message: clone(message), accepted: true };
    },
    stop: async (session: SessionId): Promise<ChatSessionView> => {
      this.validateChatSession(session);
      if (!this.chatSessions.has(session)) {
        throw errorDto(
          "chat_not_started",
          "No session has been started in this app run.",
          "Call chat.start first.",
        );
      }
      this.clearChatTimers(session);
      const stopped = this.updateChatView(session, {
        state: "stopped",
        attached: false,
        error: null,
        pending_permissions: undefined,
      });
      this.emitChat({
        kind: "closed",
        revision: this.nextRevision(),
        session,
        reason: "stopped",
      });
      return clone(stopped);
    },
    replyPermission: async (
      session: SessionId,
      permissionId: string,
      decision: ChatPermissionDecision,
    ): Promise<ChatSessionView> => {
      this.validateChatSession(session);
      if (decision !== "once" && decision !== "always" && decision !== "reject") {
        throw errorDto("invalid_input", "decision: must be once, always or reject");
      }
      const view = this.chatSessions.get(session);
      if (!view) {
        throw errorDto(
          "chat_not_started",
          "No session has been started in this app run.",
          "Call chat.start first.",
        );
      }
      const pending = view.pending_permissions ?? [];
      if (!pending.some((prompt) => prompt.id === permissionId)) {
        // Already answered elsewhere: a harmless refresh, nothing is invented.
        return clone(view);
      }
      this.permissionAnswers.push({ id: permissionId, decision });
      const rest = pending.filter((prompt) => prompt.id !== permissionId);
      const next = this.updateChatView(session, {
        pending_permissions: rest.length > 0 ? rest : undefined,
      });
      if (rest.length === 0) {
        this.scheduleChat(session, 5, () =>
          this.finishPermissionTurn(session, decision),
        );
      }
      return clone(next);
    },
    setModel: async (session: SessionId, modelId: string): Promise<ChatSessionView> => {
      this.validateChatSession(session);
      if (!this.chatSessions.has(session)) {
        throw errorDto(
          "chat_not_started",
          "No session has been started in this app run.",
          "Call chat.start first.",
        );
      }
      if (session !== "setup") {
        throw errorDto(
          "chat_model_gated",
          "An orchestrator owns its own model choice.",
          "Change the model in the orchestrator's OpenCode config.",
        );
      }
      const model = this.chatModels.find((candidate) => candidate.id === modelId);
      if (!model) {
        throw errorDto(
          "not_found",
          "That model is not available in this preview build.",
          "Choose a model from the inline list.",
        );
      }
      return clone(this.updateChatView(session, { model: clone(model), error: null }));
    },
    log: async (
      session: SessionId,
      query: ChatLogQuery = {},
    ): Promise<ChatLogPage> => {
      this.validateChatSession(session);
      if (!this.chatSessions.has(session)) {
        throw errorDto(
          "chat_not_started",
          "No session has been started in this app run.",
          "Call chat.start first.",
        );
      }
      const limit = query.limit ?? 50;
      if (!Number.isInteger(limit) || limit < 1 || limit > 200) {
        throw errorDto("invalid_input", "limit: must be an integer between 1 and 200");
      }
      const all = this.chatLogs.get(session) ?? [];
      let end = all.length;
      if (query.cursor !== undefined && query.cursor !== null) {
        if (typeof query.cursor !== "string" || !/^\d{1,9}$/.test(query.cursor)) {
          throw errorDto("invalid_input", "cursor: is not a valid page cursor");
        }
        end = Math.min(Number(query.cursor), all.length);
      }
      const start = Math.max(0, end - limit);
      return {
        messages: all.slice(start, end).map((message) => clone(message)),
        next: start > 0 ? String(start) : null,
        truncated: false,
      };
    },
    subscribe: (listener: (event: ChatEvent) => void): (() => void) => {
      this.chatListeners.add(listener);
      return () => {
        this.chatListeners.delete(listener);
      };
    },
  };

  hosts = this.hostsApi;
  onboarding = this.onboardingApi;
  secrets = this.secretsApi;
  cluster = this.clusterApi;
  connection = this.connectionApi;
  fleet = this.fleetApi;
  engine = this.engineApi;
  gates = this.gatesApi;
  chat = this.chatApi;
  sessions = this.sessionsApi;
  localOrchestrators = this.localOrchestratorsApi;

  /** Releases preview chat timers and listeners. Safe to call more than once. */
  dispose(): void {
    for (const timers of this.chatTimers.values()) {
      for (const timer of timers) clearTimeout(timer);
    }
    this.chatTimers.clear();
    this.chatListeners.clear();
  }

  /** Test hook: recorded job actions. */
  recordedActions(): MockJobAction[] {
    return clone(this.actions);
  }

  /** Test hook: recorded ASIF calls in order. */
  recordedAsifCalls(): MockAsifCall[] {
    return clone(this.asifCalls);
  }

  private recordAsifCall(call: MockAsifCall): void {
    this.asifCalls = [...this.asifCalls, call];
  }

  private makeChatView(
    session: SessionId,
    location: ChatSessionView["location"],
  ): ChatSessionView {
    const setup = session === "setup";
    const selected = setup ? (this.chatModels[0] ?? null) : null;
    return {
      session,
      location,
      state: "ready",
      attached: true,
      streaming_supported: true,
      model: selected ? clone(selected) : null,
      available_models: setup ? this.chatModels.map((model) => clone(model)) : undefined,
      usage: setup && selected ? this.sampleChatUsage(selected) : null,
      started_at: this.now(),
      last_event_at: this.now(),
      error: null,
    };
  }

  /** Deterministic preview usage; every value is sample data, never a real receipt. */
  private sampleChatUsage(model: ChatModelView): ChatUsageView {
    const limit = model.context_tokens ?? null;
    return {
      context_tokens_used: limit === null ? null : Math.min(limit, 23_600),
      context_limit: limit,
      input_tokens: 12_800,
      output_tokens: 2_400,
      cache_read_tokens: 8_400,
      turns: 3,
      cost_usd: model.tier === "included" ? 0 : 0.0042,
    };
  }

  /** Advances the preview usage after one turn; never a real model receipt. */
  private advanceChatUsage(
    view: ChatSessionView,
    reply: ChatMessage,
  ): ChatUsageView | null {
    const usage = view.usage;
    if (!usage) return null;
    const output = Math.max(1, Math.ceil(reply.text.join(" ").length / 4));
    const input = Math.max(1, output);
    const used =
      usage.context_tokens_used === null
        ? null
        : usage.context_limit === null
          ? usage.context_tokens_used + input + output
          : Math.min(usage.context_limit, usage.context_tokens_used + input + output);
    return {
      ...usage,
      context_tokens_used: used,
      input_tokens:
        usage.input_tokens === null ? null : usage.input_tokens + input,
      output_tokens:
        usage.output_tokens === null ? null : usage.output_tokens + output,
      turns: usage.turns + 1,
    };
  }

  private updateChatView(
    session: SessionId,
    patch: Partial<ChatSessionView>,
  ): ChatSessionView {
    const current = this.chatSessions.get(session);
    if (!current) {
      throw errorDto(
        "chat_not_started",
        "No session has been started in this app run.",
        "Call chat.start first.",
      );
    }
    const next: ChatSessionView = {
      ...current,
      ...patch,
      last_event_at: this.now(),
    };
    this.chatSessions.set(session, next);
    this.emitChat({
      kind: "session",
      revision: this.nextRevision(),
      session: clone(next),
    });
    return next;
  }

  private emitChat(event: ChatEvent): void {
    for (const listener of this.chatListeners) listener(clone(event));
  }

  private scheduleChat(session: SessionId, delay: number, run: () => void): void {
    const timer = setTimeout(() => {
      const timers = this.chatTimers.get(session);
      if (timers) {
        this.chatTimers.set(
          session,
          timers.filter((candidate) => candidate !== timer),
        );
      }
      const view = this.chatSessions.get(session);
      if (!view || view.state === "stopped") return;
      run();
    }, delay);
    const timers = this.chatTimers.get(session) ?? [];
    this.chatTimers.set(session, [...timers, timer]);
  }

  private clearChatTimers(session: SessionId): void {
    for (const timer of this.chatTimers.get(session) ?? []) {
      clearTimeout(timer);
    }
    this.chatTimers.delete(session);
  }

  private previewTools(
    statuses: ChatToolActivity["status"][],
  ): ChatToolActivity[] {
    const rows: Omit<ChatToolActivity, "status">[] = [
      { id: "tool-1", name: "read", summary: "docs/PLAN.md" },
      { id: "tool-2", name: "bash", summary: "git status --short" },
      { id: "tool-3", name: "write", summary: "/srv/shared/report.md" },
    ];
    return statuses.map((status, index) => ({
      ...rows[index % rows.length]!,
      status,
    }));
  }

  private permissionPrompt(): ChatPermissionPrompt {
    return {
      id: "per_preview_1",
      permission: "external_directory",
      patterns: ["/srv/shared/*"],
      created_at: this.now(),
    };
  }

  private permissionMessageId = "preview:permission-turn";

  private finishPermissionTurn(
    session: SessionId,
    decision: ChatPermissionDecision,
  ): void {
    const view = this.chatSessions.get(session);
    if (!view || view.state === "stopped") return;
    const rejected = decision === "reject";
    const reply: ChatMessage = {
      id: this.permissionMessageId,
      role: "assistant",
      text: [
        rejected
          ? "Preview reply. You rejected the request, so nothing was written."
          : "Preview reply. You allowed the request and the file was written.",
      ],
      created_at: this.now(),
      origin: "preview",
      streaming: false,
      show: null,
      tools: this.previewTools(["completed", rejected ? "error" : "completed", rejected ? "error" : "completed"]),
    };
    this.finishChatTurn(session, reply);
  }

  private scheduleChatReply(session: SessionId, text: string): void {
    if (this.scenario === "chat-tools") {
      this.scheduleChat(session, 5, () => {
        this.emitChat({
          kind: "message",
          revision: this.nextRevision(),
          session,
          message: {
            id: this.permissionMessageId,
            role: "assistant",
            text: [],
            created_at: this.now(),
            origin: "preview",
            streaming: true,
            tools: this.previewTools(["completed", "running"]),
          },
        });
      });
      this.scheduleChat(session, 15, () => {
        const reply = this.previewChatReply(session, text);
        this.finishChatTurn(session, {
          ...reply,
          id: this.permissionMessageId,
          tools: this.previewTools(["completed", "completed", "error"]),
        });
      });
      return;
    }
    if (this.scenario === "chat-permission") {
      this.scheduleChat(session, 5, () => {
        this.emitChat({
          kind: "message",
          revision: this.nextRevision(),
          session,
          message: {
            id: this.permissionMessageId,
            role: "assistant",
            text: [],
            created_at: this.now(),
            origin: "preview",
            streaming: true,
            tools: this.previewTools(["completed", "completed", "running"]),
          },
        });
        this.updateChatView(session, {
          pending_permissions: [this.permissionPrompt()],
        });
      });
      return;
    }
    // These scenarios keep a turn open so the Stop action has something to abort.
    if (this.scenario === "chat-slow" || this.scenario === "chat-stop") {
      return;
    }
    if (this.scenario === "chat-error") {
      this.scheduleChat(session, 5, () => this.failChatTurn(session));
      return;
    }
    if (this.scenario === "chat-reconnect") {
      this.scheduleChat(session, 5, () => {
        this.updateChatView(session, {
          state: "connecting",
          attached: true,
          error: null,
        });
      });
      this.scheduleChat(session, 15, () => this.completeChatTurn(session, text));
      return;
    }
    if (this.scenario === "chat-streaming") {
      const reply = this.previewChatReply(session, text);
      const fullText = reply.text.join("\n\n");
      const first = reply.text[0] ?? "Preview reply.";
      const remainder = fullText.slice(first.length);
      const shell: ChatMessage = {
        ...reply,
        text: [first],
        streaming: true,
        show: null,
      };
      this.emitChat({
        kind: "message",
        revision: this.nextRevision(),
        session,
        message: clone(shell),
      });
      let offset = 0;
      const chunkSize = Math.max(1, Math.ceil(remainder.length / 4));
      const emitNext = (): void => {
        if (offset < remainder.length) {
          const chunk = remainder.slice(offset, offset + chunkSize);
          offset += chunk.length;
          this.emitChat({
            kind: "delta",
            revision: this.nextRevision(),
            session,
            message_id: reply.id,
            part_id: "preview-part",
            text: chunk,
          });
          this.scheduleChat(session, 5, emitNext);
          return;
        }
        this.finishChatTurn(session, reply);
      };
      this.scheduleChat(session, 5, emitNext);
      return;
    }
    this.scheduleChat(session, 5, () => this.completeChatTurn(session, text));
  }

  private completeChatTurn(session: SessionId, text: string): void {
    this.finishChatTurn(session, this.previewChatReply(session, text));
  }

  private finishChatTurn(session: SessionId, reply: ChatMessage): void {
    const view = this.chatSessions.get(session);
    if (!view || view.state === "stopped") return;
    this.chatLogs.set(session, [
      ...(this.chatLogs.get(session) ?? []),
      reply,
    ]);
    this.emitChat({
      kind: "message",
      revision: this.nextRevision(),
      session,
      message: clone(reply),
    });
    this.emitChat({
      kind: "done",
      revision: this.nextRevision(),
      session,
      message_id: reply.id,
      truncated: false,
    });
    this.updateChatView(session, {
      state: "ready",
      attached: true,
      error: null,
      usage: this.advanceChatUsage(view, reply),
      pending_permissions: undefined,
    });
  }

  private failChatTurn(session: SessionId): void {
    const error = errorDto(
      "chat_failed",
      "The preview orchestrator reported a safe error.",
      "In the real app this is a typed error; no reply is invented.",
      true,
    );
    this.emitChat({
      kind: "error",
      revision: this.nextRevision(),
      session,
      error: clone(error),
    });
    this.updateChatView(session, {
      state: "error",
      attached: true,
      error,
    });
  }

  private previewChatReply(session: SessionId, text: string): ChatMessage {
    const where =
      session === "global"
        ? "the global orchestrator"
        : session === "setup"
          ? "the setup assistant"
          : "this project's orchestrator";
    const preview = `Preview reply. In the real app ${where} answers here.`;
    let hint: string | null = null;
    let show: ChatPanelTarget | null = null;
    if (/running|working|workers|progress/i.test(text)) {
      hint = "Here's what is running right now. I opened it on the right.";
      show = { kind: "overview" };
    } else if (/busy|server|capacity|memory|ram|disk|cpu/i.test(text)) {
      hint = "Here's the server right now. I opened it on the right.";
      show = { kind: "server" };
    } else if (/attention|inbox|question|blocked|waiting/i.test(text)) {
      hint = "These are waiting for you. I opened your inbox on the right.";
      show = { kind: "inbox" };
    }
    return {
      id: this.nextChatId("preview"),
      role: "assistant",
      text: hint ? [preview, hint] : [preview],
      created_at: this.now(),
      origin: "preview",
      streaming: false,
      show,
    };
  }

  private nextChatId(prefix: "local" | "preview"): string {
    this.chatSeq += 1;
    const alphabet = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let value = this.chatSeq;
    let suffix = "";
    for (let index = 0; index < 26; index += 1) {
      suffix = alphabet.charAt(value % 32) + suffix;
      value = Math.floor(value / 32);
    }
    return `${prefix}:${suffix}`;
  }

  private validateChatStart(input: ChatStartInput): void {
    this.validateChatSession(input.session);
    if (input.location !== "local" && input.location !== "server") {
      throw errorDto("invalid_input", "location: use `local` or `server`");
    }
    if (input.session === "setup" || input.session === "global") {
      if (input.project !== null) {
        throw errorDto(
          "invalid_input",
          "project: must be null for setup and global sessions",
        );
      }
      return;
    }
    if (input.project !== input.session.slice("project:".length)) {
      throw errorDto(
        "invalid_input",
        "project: must match the project:<name> session",
      );
    }
  }

  private validateChatSession(session: SessionId): void {
    if (session === "setup" || session === "global") return;
    if (typeof session !== "string" || !session.startsWith("project:")) {
      throw errorDto(
        "invalid_input",
        "session: use `setup`, `global` or `project:<name>`",
      );
    }
    const project = session.slice("project:".length);
    if (
      project.length === 0 ||
      project.length > 64 ||
      project.startsWith("-") ||
      project === "." ||
      project === ".." ||
      !/^[A-Za-z0-9._-]+$/.test(project)
    ) {
      throw errorDto(
        "invalid_input",
        "session: project names use 1-64 letters, digits, '.', '-' or '_'",
      );
    }
  }

  private requireChatScenario(session: SessionId): void {
    if (this.scenario === "chat-engine-stopped") {
      throw errorDto(
        "engine_not_ready",
        "The bundled StackSpirit Engine is stopped.",
        "Start the engine in Settings → Engine, then retry.",
        true,
      );
    }
    if (this.scenario === "chat-external-engine") {
      throw errorDto(
        "engine_external",
        "The setup assistant needs the bundled StackSpirit Engine; the external harness is model-only.",
        "Switch to the bundled engine in Settings → Advanced.",
      );
    }
    if (this.scenario === "chat-server-gated" && session !== "setup") {
      throw errorDto(
        "chat_gated",
        "Server chat needs VOWP 0.5's `vowp orchestrator endpoint`.",
        "Update to VOWP 0.5, then retry; the app-owned SSH forward is not approved.",
      );
    }
    if (this.scenario === "chat-session-lost" && session !== "setup") {
      throw errorDto(
        "session_lost",
        "The stored native session no longer exists.",
        "Recreate the orchestrator session in VOWP, then attach.",
      );
    }
  }

  private requireSessionId(sessionId: string): void {
    if (typeof sessionId !== "string" || !/^[0-9A-HJKMNP-TV-Z]{26}$/.test(sessionId)) {
      throw errorDto("invalid_input", "session id: must be a 26-character ULID");
    }
  }

  private localFileName(sessionId: string, path: string | null | undefined): string {
    if (!path) return `${sessionId}.asif.zip`;
    const name = path.split(/[\\/]/).filter(Boolean).pop();
    if (!name) throw errorDto("invalid_input", "local ASIF package path: must name a file");
    if (!name.toLowerCase().endsWith(".zip")) {
      throw errorDto("invalid_input", "local ASIF package path: must end in .zip");
    }
    return name;
  }

  private requireAsifAction(action: "asif_export" | "asif_import"): void {
    if (gateActionEnabled(this.gateSnapshot, action)) return;
    throw errorDto(
      "gate",
      gateActionReason(this.gateSnapshot, action) || "The ASIF action is unavailable.",
      "Probe the VOWP home gates, then retry.",
    );
  }

  private validateAsifOptions(
    options: AsifExportOptions | AsifImportOptions,
    allowed: readonly string[],
  ): void {
    for (const key of Object.keys(options)) {
      if (!allowed.includes(key)) {
        throw errorDto("invalid_input", `ASIF options: ${key} is not a supported option`);
      }
    }
    for (const key of ["strict", "allowLoss"] as const) {
      const value = options[key];
      if (value !== undefined && typeof value !== "boolean") {
        throw errorDto("invalid_input", `ASIF options: ${key} must be a boolean`);
      }
    }
    if (options.strict === true && options.allowLoss === true) {
      throw errorDto(
        "invalid_input",
        "ASIF options: strict and allowLoss are mutually exclusive",
      );
    }
    const path = options.path;
    if (path !== undefined && path !== null) {
      if (typeof path !== "string" || path.length === 0 || path.includes("\u0000")) {
        throw errorDto("invalid_input", "local ASIF package path: must be a non-empty string");
      }
    }
  }

  /** Test hook: recorded D6 calls in order. */
  recordedLocalCalls(): MockLocalCall[] {
    return clone(this.localCalls);
  }

  private recordLocal(action: MockLocalCall["action"], name: string | null): void {
    this.localCalls = [...this.localCalls, { action, name }];
  }

  private requireConnectedHost(): void {
    if (this.connectionState.state !== "connected") {
      throw errorDto(
        "conflict",
        "Connect to a host before using local orchestrator actions.",
        "Connect a host first.",
      );
    }
  }

  /**
   * Mirrors the backend three-part guard: the gate must be Ready, the local agent must
   * be connected and the build must advertise the D6 action flag. A stale renderer is
   * never authority, so every mutation re-checks this method.
   */
  private requireLocalAction(action: string): void {
    if (gateActionEnabled(this.gateSnapshot, "local_orchestrator")) return;
    throw errorDto(
      "gate",
      gateActionReason(this.gateSnapshot, "local_orchestrator") ||
        `Local orchestrator action ${action} is unavailable.`,
      "Re-probe the local orchestrator gate and resolve the named prerequisite.",
    );
  }

  private validateUlid(value: string, field: string): void {
    if (!/^[0-9A-HJKMNP-TV-Z]{26}$/.test(value)) {
      throw errorDto("invalid_input", `${field}: must be a 26-character ULID`);
    }
  }

  private validateNode(node: string): void {
    if (!/^[A-Za-z0-9._-]{1,64}$/.test(node) || node.startsWith("-")) {
      throw errorDto("invalid_input", "node: use 1-64 letters, digits, '.', '-' or '_'");
    }
  }

  private validateProject(project: string): void {
    if (!/^[A-Za-z0-9._-]{1,64}$/.test(project) || project.startsWith("-")) {
      throw errorDto("invalid_input", "project: use 1-64 letters, digits, '.', '-' or '_'");
    }
  }

  private validateApp(app: string, field: string): void {
    if (!/^[A-Za-z0-9._-]{1,64}$/.test(app) || app.startsWith("-")) {
      throw errorDto(
        "invalid_input",
        `${field}: use 1-64 letters, digits, '.', '-' or '_'`,
      );
    }
  }

  private validateSurfaceId(surface: string): void {
    if (!/^[0-9A-Za-z]{26}$/.test(surface)) {
      throw errorDto("invalid_input", "surface id: expected a 26-character ULID");
    }
  }

  private validateDuration(value: string, field: string): void {
    if (!/^\d{1,8}[smhdw]$/.test(value)) {
      throw errorDto(
        "invalid_input",
        `${field}: use a duration such as 30m, 10h or 7d`,
      );
    }
  }

  private validateOrchestratorName(name: string): void {
    if (
      name.length === 0 ||
      name.length > 64 ||
      name.startsWith("-") ||
      name === "." ||
      name === ".." ||
      !/^[A-Za-z0-9._-]+$/.test(name)
    ) {
      throw errorDto(
        "invalid_input",
        "orchestrator name: use 1-64 letters, digits, '.', '-' or '_'",
      );
    }
  }

  private validateAgentPair(input: AgentPairInput): void {
    if (
      typeof input.code !== "string" ||
      input.code.trim().length === 0 ||
      input.code.length > 4096 ||
      /[\r\n\0]/.test(input.code)
    ) {
      throw errorDto("invalid_input", "pairing code: must be a non-empty single line");
    }
    this.validateSurfaceId(input.surface);
    this.validateUlid(input.session, "session id");
    if (input.app !== undefined && input.app !== null) {
      this.validateApp(input.app, "surface app");
    }
  }

  private validateSurfacePair(input: SurfacePairInput): void {
    this.requireConnectedHost();
    this.validateUlid(input.session, "session id");
    this.validateNode(input.node);
    if (input.kind !== "opencode" && input.kind !== "chat") {
      throw errorDto("invalid_input", "surface kind: use `opencode` or `chat`");
    }
    if (input.app !== undefined && input.app !== null) {
      this.validateApp(input.app, "surface app");
    }
    if (input.replace !== undefined && input.replace !== null) {
      this.validateSurfaceId(input.replace);
    }
  }

  private validateSurfaceRotate(input: SurfaceRotateInput): void {
    this.validateSurfaceId(input.surface);
    if (input.node !== undefined && input.node !== null) this.validateNode(input.node);
    if (input.wait !== undefined && input.wait !== null) {
      this.validateDuration(input.wait, "--wait");
    }
    if (input.grace !== undefined && input.grace !== null) {
      this.validateDuration(input.grace, "--grace");
    }
  }

  private validateOrchestratorNew(input: OrchestratorNewInput): void {
    this.requireConnectedHost();
    this.validateNode(input.node);
    if (!["opencode", "claude_code", "chat"].includes(input.app)) {
      throw errorDto(
        "invalid_input",
        "orchestrator app: use `opencode`, `claude_code` or `chat`",
      );
    }
    if (input.project !== undefined && input.project !== null) {
      this.validateProject(input.project);
    }
    if (input.name !== undefined && input.name !== null) {
      this.validateOrchestratorName(input.name);
    }
  }

  private validatePathAdd(input: AgentPathAddInput): void {
    if (
      typeof input.projectRoot !== "string" ||
      !input.projectRoot.startsWith("/") ||
      input.projectRoot.length < 2 ||
      input.projectRoot.includes("\0")
    ) {
      throw errorDto("invalid_input", "project root: must be an absolute local path");
    }
    this.validateProject(input.name);
    if (input.worktreeRoot !== undefined && input.worktreeRoot !== null) {
      if (!input.worktreeRoot.startsWith("/") || input.worktreeRoot.length < 2) {
        throw errorDto("invalid_input", "worktree root: must be an absolute local path");
      }
    }
    for (const allowed of input.allow ?? []) {
      if (
        allowed.length === 0 ||
        allowed.startsWith("/") ||
        allowed.includes("\\") ||
        allowed.split("/").includes("..")
      ) {
        throw errorDto(
          "invalid_input",
          "allow: must be a relative path below the project root",
        );
      }
    }
  }

  private localJoin(sessionId: string) {
    return {
      desktop: "Open the local opencode app",
      phone: "Join from the phone surface",
      chat: `vowp chat --agent --session ${sessionId}`,
    };
  }

  private toSurfaceEntry(surface: LocalSurfaceView): SurfaceEntryView {
    return {
      id: surface.surface_id,
      node: surface.node,
      kind: surface.kind,
      state: surface.state,
      scopes: [],
      prefix: surface.can_write ? "vowp_surf_ab" : null,
      expires_at: surface.expires_at,
      overlap_until: null,
      expired: surface.expired,
      pending: surface.rotation_pending,
      queued_at: null,
      delivery_deadline: null,
    };
  }

  private toOrchestrator(view: LocalOrchestratorView): OrchestratorView {
    return {
      name: view.name,
      session_id: view.session_id,
      project: view.project,
      app: view.app,
      state: view.state,
      node: view.node,
      join: view.join,
    };
  }

  private localGateMode(): MockLocalGateMode {
    switch (this.scenario) {
      case "gate-d6-waiting":
        return "waiting";
      case "gate-d6-incompatible":
        return "incompatible";
      case "gate-d6-ready":
        return "ready";
      case "gate-d6-revoked":
        return "revoked";
      case "gates-ready":
      case "gate-asif-ready":
      case "gate-asif-ready-export-only":
      case "asif-export-losses":
      case "asif-import-warnings":
      case "asif-strict-refusal":
      case "asif-conflict":
        return "ready";
      case "gate-asif-waiting":
        return "waiting";
      default:
        return "unknown";
    }
  }

  private makeLocalState(): MockLocalState {
    const gate = this.gateSnapshot.local_orchestrator;
    const agentState = this.gateSnapshot.local_agent.state;
    const emptyAgent: LocalAgentView = {
      state: "not_installed",
      node: null,
      protocol: null,
      last_heartbeat: null,
      last_error: null,
      cache_entries: 0,
      cache_bytes: 0,
      cache_oldest_at: null,
      surfaces: [],
    };
    if (gate.state !== "ready" && agentState !== "revoked") {
      return {
        agent:
          agentState === "not_enrolled"
            ? { ...emptyAgent, state: "not_enrolled" }
            : agentState === "incompatible"
              ? {
                  ...emptyAgent,
                  state: "incompatible",
                  node: LOCAL_NODE,
                  protocol: "vowp-agent/0",
                  last_error: "protocol incompatible: detected vowp-agent/0, required vowp-agent/1",
                }
              : emptyAgent,
        node: null,
        surfaces: [],
        orchestrators: [],
        paths: [],
      };
    }

    const now = this.now();
    const expiresAt = iso(this.nowMs + 30 * 24 * 3_600_000);
    const agent: LocalAgentView = {
      state: gate.state === "ready" ? "connected" : "revoked",
      node: LOCAL_NODE,
      protocol: "vowp-agent/1",
      last_heartbeat: now,
      last_error:
        gate.state === "ready" ? null : "the node credential was revoked; reconnect after enrolling it again",
      cache_entries: 12,
      cache_bytes: 4096,
      cache_oldest_at: iso(this.nowMs - 2 * 3_600_000),
      surfaces: [
        {
          surface: LOCAL_SURFACE_ID,
          prefix: "vowp_surf_ab",
          expires_at: expiresAt,
          state: "paired",
        },
        {
          surface: LOCAL_REVOKED_SURFACE_ID,
          prefix: null,
          expires_at: null,
          state: "revoked",
        },
      ],
    };
    const node: NodeCardView = {
      name: LOCAL_NODE,
      kind: "agent",
      status: gate.state === "ready" ? "active" : "revoked",
      presence: gate.state === "ready" ? "online" : "revoked",
      labels: ["local"],
      runs: 1,
      mapped: ["vowp"],
      agent_version: "0.4.0-mock",
      protocol: "vowp-agent/1",
      last_error: gate.state === "ready" ? null : "node credential revoked",
    };
    const usable = gate.state === "ready";
    const surfaces: LocalSurfaceView[] = [
      {
        surface_id: LOCAL_SURFACE_ID,
        node: LOCAL_NODE,
        protocol: "vowp-agent/1",
        agent_status: agent.state,
        lease_holder: usable ? "StackSpirit Desktop" : null,
        session_state: usable ? "active" : "revoked",
        kind: "opencode",
        state: usable ? "attached" : "revoked",
        expires_at: expiresAt,
        expired: !usable,
        rotation_pending: false,
        can_write: usable,
      },
      {
        surface_id: LOCAL_REVOKED_SURFACE_ID,
        node: LOCAL_NODE,
        protocol: "vowp-agent/1",
        agent_status: agent.state,
        lease_holder: null,
        session_state: "revoked",
        kind: "opencode",
        state: "revoked",
        expires_at: null,
        expired: true,
        rotation_pending: false,
        can_write: false,
      },
      {
        surface_id: "01J9Z00000000000000000000D",
        node: LOCAL_NODE,
        protocol: "vowp-agent/1",
        agent_status: agent.state,
        lease_holder: null,
        session_state: usable ? "superseded" : "revoked",
        kind: "chat",
        state: usable ? "attached" : "revoked",
        expires_at: expiresAt,
        expired: !usable,
        rotation_pending: false,
        can_write: false,
      },
      {
        surface_id: "01J9Z00000000000000000000E",
        node: LOCAL_NODE,
        protocol: "vowp-agent/1",
        agent_status: agent.state,
        lease_holder: null,
        session_state: usable ? "degraded" : "revoked",
        kind: "opencode",
        state: usable ? "attached" : "revoked",
        expires_at: expiresAt,
        expired: !usable,
        rotation_pending: false,
        can_write: false,
      },
    ];
    const orchestrators: LocalOrchestratorView[] = [
      {
        name: "vowp-local",
        session_id: LOCAL_SESSION_ID,
        project: "vowp",
        app: "opencode",
        state: gate.state === "ready" ? "running" : "node_offline",
        node: LOCAL_NODE,
        protocol: "vowp-agent/1",
        agent_status: agent.state,
        lease_holder: gate.state === "ready" ? "StackSpirit Desktop" : null,
        session_state: gate.state === "ready" ? "running" : "revoked",
        join: this.localJoin(LOCAL_SESSION_ID),      },
      {
        name: "chat-local",
        session_id: "01JCF5WQ8ZK3M2V7N4P6R8T0AH",
        project: null,
        app: "chat",
        state: "stopped",
        node: LOCAL_NODE,
        protocol: "vowp-agent/1",
        agent_status: agent.state,
        lease_holder: null,
        session_state: "stopped",
        join: this.localJoin("01JCF5WQ8ZK3M2V7N4P6R8T0AH"),
      },
      {
        name: "docs-local",
        session_id: "01JCF5WQ8ZK3M2V7N4P6R8T0AJ",
        project: "vowp",
        app: "opencode",
        state: "degraded",
        node: LOCAL_NODE,
        protocol: "vowp-agent/1",
        agent_status: agent.state,
        lease_holder: null,
        session_state: "degraded",
        join: this.localJoin("01JCF5WQ8ZK3M2V7N4P6R8T0AJ"),
      },
      {
        name: "phone-local",
        session_id: "01JCF5WQ8ZK3M2V7N4P6R8T0AK",
        project: "vowp",
        app: "chat",
        state: "reconnecting",
        node: LOCAL_NODE,
        protocol: "vowp-agent/1",
        agent_status: agent.state,
        lease_holder: null,
        session_state: "reconnecting",
        join: this.localJoin("01JCF5WQ8ZK3M2V7N4P6R8T0AK"),
      },
    ];
    return {
      agent,
      node,
      surfaces,
      orchestrators,
      paths: [
        {
          project: "vowp",
          path_ref: "project:vowp",
          worktree_root_configured: true,
          allow: ["src"],
          valid: true,
          invalid_reason: null,
        },
      ],
    };
  }

  private makeHost(label: string, host: string, now: string): HostProfile {
    return {
      id: "01MOCKHOST000000000000000001",
      label,
      ssh: { user: "owner", host, port: 22 },
      identity_file: null,
      vowp_home: null,
      default_project: null,
      host_key: {
        algorithm: "ssh-ed25519",
        sha256: this.scenario === "host-key-mismatch" ? OLD_SHA : PIN_SHA,
        pinned_at: now,
      },
      created_at: now,
      updated_at: now,
      last_connected_at: null,
    };
  }

  private validateDraft(draft: HostDraft): void {
    if (!draft.label.trim() || draft.label.trim().length > 64) {
      throw errorDto("invalid_input", "label: must be 1-64 characters");
    }
    if (
      !draft.ssh.user ||
      !/^[A-Za-z0-9._-]+$/.test(draft.ssh.user) ||
      draft.ssh.user.startsWith("-")
    ) {
      throw errorDto("invalid_input", "ssh.user: use letters, digits, '.', '_' and '-' only");
    }
    if (
      !draft.ssh.host ||
      draft.ssh.host.startsWith("-") ||
      draft.ssh.host.length > 253 ||
      /[\s@/]/.test(draft.ssh.host)
    ) {
      throw errorDto(
        "invalid_input",
        "ssh.host: use a DNS name or IP address without a scheme, '@', or whitespace",
      );
    }
    if (!Number.isInteger(draft.ssh.port) || draft.ssh.port < 1 || draft.ssh.port > 65535) {
      throw errorDto("invalid_input", "ssh.port: must be 1-65535");
    }
    if (draft.vowp_home && draft.vowp_home !== "" && !draft.vowp_home.startsWith("/")) {
      throw errorDto("invalid_input", "vowp_home: must be an absolute path or empty");
    }
  }

  private validatePin(pin: HostKeyPin): void {
    if (!/^(ssh-|ecdsa-)/.test(pin.algorithm)) {
      throw errorDto("invalid_input", "host_key.algorithm: must be an SSH public-key algorithm");
    }
    if (!pin.sha256.startsWith("SHA256:") || pin.sha256.length < 20) {
      throw errorDto("invalid_input", "host_key.sha256: must be a SHA256 fingerprint from ssh-keygen");
    }
  }

  private nextRevision(): number {
    this.revision += 1;
    return this.revision;
  }

  private emit(event: StateEvent): void {
    for (const listener of this.listeners) listener(clone(event));
  }

  private emitError(error: SpiritErrorDto): void {
    this.emit({ kind: "error", revision: this.nextRevision(), error: clone(error) });
  }

  private setConnection(patch: Partial<ConnectionSnapshot>): void {
    this.connectionState = { ...this.connectionState, ...patch };
    this.emit({
      kind: "connection",
      revision: this.nextRevision(),
      connection: clone(this.connectionState),
    });
  }

  private recordLatency(ms: number): void {
    this.latencySamples = [...this.latencySamples.slice(-19), ms];
  }

  private latency(): LatencyStats {
    const sorted = [...this.latencySamples].sort((left, right) => left - right);
    const pick = (percent: number): number => {
      if (sorted.length === 0) return 0;
      const index = Math.min(sorted.length - 1, Math.ceil((sorted.length * percent) / 100) - 1);
      return sorted[Math.max(0, index)] ?? 0;
    };
    const last = this.latencySamples[this.latencySamples.length - 1] ?? 0;
    return {
      last_ms: last,
      ewma_ms: last,
      p50_ms: pick(50),
      p95_ms: pick(95),
      samples: this.latencySamples.length,
    };
  }

  private makeFleet(hostId: string, revision: number): FleetView {
    const now = this.now();
    const runningCount =
      this.scenario === "idle" || this.scenario === "empty"
        ? 0
        : this.scenario === "busy"
          ? 4
          : this.scenario === "multi-project"
            ? 2
            : 1;
    const running = Array.from({ length: runningCount }, (_, index) =>
      this.makeWorker(index),
    );
    const projects = this.makeProjects(running);
    const lanes = this.makeLanes(projects, running);
    const capacity = this.makeCapacity(runningCount);
    return {
      revision,
      host_id: hostId,
      fetched_at: now,
      generated_at: now,
      source: this.scenario === "stale-cache" ? "cache" : "remote",
      stale: this.scenario === "stale-cache",
      home: "vps",
      supervisor: {
        running: this.scenario !== "supervisor-down",
        pid: this.scenario === "supervisor-down" ? null : 4812,
        paused: false,
        last_tick_at: this.scenario === "supervisor-down" ? iso(this.nowMs - 900_000) : now,
        last_tick_age_secs: this.scenario === "supervisor-down" ? 900 : 2,
        tick_secs: 60,
        fail_streak: this.scenario === "supervisor-down" ? 7 : 0,
        version: "0.4.0-mock",
      },
      projects,
      capacity,
      running,
      lanes,
      inbox: this.inboxItems.map((item) => ({
        ...item,
        body: null,
        options: [],
      })),
      sessions: this.makeSessions(),
      orchestrators: [],
      latency: this.latency(),
    };
  }

  private makeWorker(index: number): WorkerCardView {
    const executors = [
      {
        raw: "opencode:deepseek/deepseek-flash#max",
        harness: "opencode",
        provider: "deepseek",
        model: "deepseek/deepseek-flash",
        effort: null,
        variant: "max",
        display: "DeepSeek V4.1 Flash · max",
      },
      {
        raw: "claude-code:model-a@medium",
        harness: "claude-code",
        provider: "claude",
        model: "model-a",
        effort: "medium",
        variant: null,
        display: "model-a · medium",
      },
      null,
    ];
    const executor = executors[index % executors.length] ?? null;
    const jobIds = [
      "ss-2-desktop-shell",
      "ss-3-fleet-canvas",
      "ss-4-engine-onboarding",
      "ss-5-fleet-inbox",
    ];
    return {
      job_id: jobIds[index % jobIds.length] ?? "ss-2-desktop-shell",
      project: index % 2 === 0 ? "vowp" : "annotator",
      kind: "build",
      role: index === 2 ? "verifier" : "worker",
      state: "active",
      stalled: false,
      executor,
      node: "vps",
      age_secs: 134 + index * 60,
      last_event_age_secs: 3 + index * 8,
      session_id: `01MOCKSESSION00000000000000${index}`,
    };
  }

  /**
   * Deterministic job-detail fixtures for preview and tests.
   *
   * `ss-9-gate-check` has runs and two failed gates (the review/fix case);
   * `ss-5-mock` is ready and has no runs (the true empty case); any unknown id is a 404.
   */
  private makeJobDetail(id: string): JobDetailView | null {
    const now = this.nowMs;
    const queued = /^ss-(\d+)-mock$/.exec(id);
    const specs: Record<
      string,
      {
        title: string;
        kind: string;
        role: string;
        state: string;
        priority: number;
        needs: string[];
        attempts: number;
        note: string;
        report: string;
        parked_reason: string | null;
      }
    > = {
      "ss-2-desktop-shell": {
        title: "Build the desktop shell",
        kind: "build",
        role: "worker",
        state: "running",
        priority: 80,
        needs: [],
        attempts: 0,
        note: "",
        report: ".vowp/jobs/ss-2-desktop-shell/report.md",
        parked_reason: null,
      },
      "ss-3-fleet-canvas": {
        title: "Build the fleet canvas",
        kind: "build",
        role: "worker",
        state: "running",
        priority: 60,
        needs: ["ss-2-desktop-shell"],
        attempts: 1,
        note: "First attempt produced the layout.",
        report: ".vowp/jobs/ss-3-fleet-canvas/report.md",
        parked_reason: null,
      },
      "ss-4-engine-onboarding": {
        title: "Engine onboarding",
        kind: "build",
        role: "worker",
        state: "running",
        priority: 50,
        needs: [],
        attempts: 0,
        note: "",
        report: ".vowp/jobs/ss-4-engine-onboarding/report.md",
        parked_reason: null,
      },
      "ss-5-fleet-inbox": {
        title: "Fleet inbox",
        kind: "build",
        role: "worker",
        state: "running",
        priority: 50,
        needs: [],
        attempts: 0,
        note: "",
        report: ".vowp/jobs/ss-5-fleet-inbox/report.md",
        parked_reason: null,
      },
      "ss-9-gate-check": {
        title: "Gate check",
        kind: "build",
        role: "worker",
        state: "reviewing",
        priority: 50,
        needs: ["ss-5-mock"],
        attempts: 1,
        note: "Verifier reported a failing report gate.",
        report: ".vowp/jobs/ss-9-gate-check/report.md",
        parked_reason: null,
      },
      "ss-8-blocked-review": {
        title: "Blocked review",
        kind: "verify",
        role: "verifier",
        state: "parked",
        priority: 20,
        needs: ["ss-9-gate-check"],
        attempts: 2,
        note: "Waiting for the owner to choose a merge policy.",
        report: ".vowp/jobs/ss-8-blocked-review/report.md",
        parked_reason: "question",
      },
      "ss-5-mock": {
        title: "Build the visual fleet canvas",
        kind: "build",
        role: "worker",
        state: "ready",
        priority: 50,
        needs: [],
        attempts: 0,
        note: "",
        report: ".vowp/jobs/ss-5-mock/report.md",
        parked_reason: null,
      },
      "ss-6-mock": {
        title: "Capacity and storage rail",
        kind: "build",
        role: "worker",
        state: "ready",
        priority: 50,
        needs: [],
        attempts: 0,
        note: "",
        report: ".vowp/jobs/ss-6-mock/report.md",
        parked_reason: null,
      },
      "ss-7-mock": {
        title: "Engine model catalog",
        kind: "build",
        role: "worker",
        state: "pending",
        priority: 50,
        needs: ["ss-6-mock"],
        attempts: 0,
        note: "",
        report: ".vowp/jobs/ss-7-mock/report.md",
        parked_reason: null,
      },
    };
    if (queued && !specs[id]) {
      const index = Number(queued[1]);
      specs[id] = {
        title: `Queued mock job ${index}`,
        kind: "build",
        role: "worker",
        state: index === 7 ? "pending" : "ready",
        priority: 50,
        needs: [],
        attempts: 0,
        note: "",
        report: `.vowp/jobs/${id}/report.md`,
        parked_reason: null,
      };
    }
    const spec = specs[id];
    if (!spec) return null;
    const knownCard = this.fleetView
      ? [
          ...this.fleetView.running,
          ...this.fleetView.lanes.running,
          ...this.fleetView.lanes.ready,
          ...this.fleetView.lanes.review_fix,
          ...this.fleetView.lanes.parked_blocked,
          ...this.fleetView.lanes.done,
        ].find((card) => card.job_id === id)
      : undefined;

    const base: JobDetailView = {
      id,
      project: knownCard?.project ?? "vowp",
      title: spec.title,
      kind: spec.kind,
      role: spec.role,
      state: spec.state,
      priority: spec.priority,
      needs: spec.needs,
      attempts: spec.attempts,
      note: spec.note,
      report_path: spec.report,
      parked_reason: spec.parked_reason,
      created_at: iso(now - 7_200_000),
      state_changed_at: iso(now - 2_400_000),
      updated_at: iso(now - 600_000),
      last_progress_at: iso(now - 1_800_000),
      fetched_at: this.now(),
      runs: [],
      runs_truncated: false,
      gates: [],
      gates_truncated: false,
    };

    if (id === "ss-9-gate-check") {
      const runs: JobRunView[] = [
        {
          id: "01MOCKRUN00000000000000001",
          role: "worker",
          purpose: "start",
          executor: parseExecutor("opencode:deepseek/deepseek-flash#max"),
          started_at: iso(now - 7_200_000),
          ended_at: iso(now - 5_400_000),
          outcome: "ok",
          exit_code: 0,
          detail: "Implemented the gate check and wrote the report.",
        },
        {
          id: "01MOCKRUN00000000000000002",
          role: "verifier",
          purpose: "verify",
          executor: parseExecutor("claude-code:model-a@medium"),
          started_at: iso(now - 5_000_000),
          ended_at: iso(now - 4_200_000),
          outcome: "error",
          exit_code: 1,
          detail: "Report gate failed: the tests field is missing.",
        },
      ];
      const gates: JobGateView[] = [
        {
          attempt: 1,
          gate_index: 0,
          kind: "report",
          passed: false,
          evidence_summary: "missing: tests · 1 problem",
          duration_ms: 1_200,
          checked_at: iso(now - 4_500_000),
        },
        {
          attempt: 1,
          gate_index: 1,
          kind: "owns_respected",
          passed: true,
          evidence_summary: "2 changed · 0 outside owns",
          duration_ms: 45,
          checked_at: iso(now - 4_500_000),
        },
        {
          attempt: 1,
          gate_index: 2,
          kind: "secret_scan",
          passed: false,
          evidence_summary: "12 files scanned · 1 hit",
          duration_ms: 800,
          checked_at: iso(now - 4_500_000),
        },
      ];
      return { ...base, runs, gates };
    }

    if (id === "ss-8-blocked-review") {
      return {
        ...base,
        runs: [
          {
            id: "01MOCKRUN00000000000000003",
            role: "verifier",
            purpose: "verify",
            executor: parseExecutor("claude-code:model-a@medium"),
            started_at: iso(now - 12_000_000),
            ended_at: iso(now - 11_000_000),
            outcome: "error",
            exit_code: 1,
            detail: "Review found an unanswered merge-policy question.",
          },
        ],
        gates: [
          {
            attempt: 2,
            gate_index: 0,
            kind: "review",
            passed: false,
            evidence_summary: "verdict: request_changes · 1 finding",
            duration_ms: 1_500,
            checked_at: iso(now - 11_500_000),
          },
        ],
      };
    }

    if (id === "ss-3-fleet-canvas") {
      return {
        ...base,
        runs: [
          {
            id: "01MOCKRUN00000000000000004",
            role: "worker",
            purpose: "start",
            executor: parseExecutor("opencode:deepseek/deepseek-flash#max"),
            started_at: iso(now - 3_600_000),
            ended_at: iso(now - 2_400_000),
            outcome: "ok",
            exit_code: 0,
            detail: "Built the visual fleet canvas.",
          },
        ],
      };
    }

    if (id === "ss-2-desktop-shell" || id === "ss-4-engine-onboarding" || id === "ss-5-fleet-inbox") {
      return {
        ...base,
        runs: [
          {
            id: `01MOCKRUN0000000000000${id === "ss-2-desktop-shell" ? "5" : id === "ss-4-engine-onboarding" ? "6" : "7"}`,
            role: "worker",
            purpose: "start",
            executor: parseExecutor("opencode:deepseek/deepseek-flash#max"),
            started_at: iso(now - 1_200_000),
            ended_at: null,
            outcome: null,
            exit_code: null,
            detail: "Run is still open.",
          },
        ],
      };
    }

    return base;
  }

  private makeProjects(running: WorkerCardView[]): ProjectView[] {
    const base: ProjectView = {
      name: "vowp",
      state: "active",
      paused_reason: "",
      jobs_by_state: {
        running: Math.max(1, running.length),
        ready: this.scenario === "queue-cap" ? 200 : 3,
        reviewing: 1,
        parked: 1,
        done: 37,
      },
      cost_today_usd: 1.37,
    };
    if (this.scenario !== "multi-project" && this.scenario !== "busy") return [base];
    return [
      base,
      {
        name: "annotator",
        state: "active",
        paused_reason: "",
        jobs_by_state: { running: 1, ready: 2, done: 12 },
        cost_today_usd: 0.42,
      },
      {
        name: "platform",
        state: "paused",
        paused_reason: "Owner paused the project.",
        jobs_by_state: { ready: 1 },
        cost_today_usd: null,
      },
    ];
  }

  private makeLanes(projects: ProjectView[], running: WorkerCardView[]): JobLanes {
    const counts: Record<string, number> = {
      running: 0,
      ready: 0,
      review_fix: 0,
      parked_blocked: 0,
      done: 0,
    };
    for (const project of projects) {
      for (const [state, count] of Object.entries(project.jobs_by_state)) {
        const lane =
          state === "running" || state === "stalled" || state === "merging"
            ? "running"
            : state === "pending" || state === "ready"
              ? "ready"
              : state === "gating" || state === "reviewing" || state === "fixing"
                ? "review_fix"
                : state === "done" || state === "replanned"
                  ? "done"
                  : "parked_blocked";
        counts[lane] = (counts[lane] ?? 0) + count;
      }
    }
    const runningCards = running.map((card) => ({
      job_id: card.job_id,
      project: card.project,
      title: null,
      kind: card.kind,
      role: card.role,
      state: "active",
      priority: 0,
      age_secs: card.age_secs,
      reason: null,
      detail: null,
      gate: null,
      gate_at: null,
      gate_evidence: null,
      executor: card.executor,
      session_id: card.session_id,
    }));
    const readyCards = Array.from({ length: this.scenario === "queue-cap" ? 200 : 3 }, (_, index) => ({
      job_id: `ss-${5 + index}-mock`,
      project: "vowp",
      title: ["Build the visual fleet canvas", "Capacity and storage rail", "Engine model catalog"][
        index % 3
      ] ?? "Queued job",
      kind: "build",
      role: "worker",
      state: index === 2 ? "pending" : "ready",
      priority: 50,
      age_secs: 300 + index,
      reason: index === 1 ? "RAM 1.2 GB short" : null,
      detail: null,
      gate: null,
      gate_at: null,
      gate_evidence: null,
      executor: null,
      session_id: null,
    }));
    return {
      running: runningCards,
      ready: readyCards,
      review_fix: [
        {
          job_id: "ss-9-gate-check",
          project: "vowp",
          title: "Gate check",
          kind: "build",
          role: "worker",
          state: "reviewing",
          priority: 0,
          age_secs: 420,
          reason: null,
          detail: null,
          gate: "secret_scan",
          gate_at: iso(this.nowMs - 2_400_000),
          gate_evidence: "example evidence line, redacted",
          executor: null,
          session_id: null,
        },
      ],
      parked_blocked: [
        {
          job_id: "ss-8-blocked-review",
          project: "vowp",
          title: null,
          kind: "unknown",
          role: "unknown",
          state:
            this.scenario === "unknown-states"
              ? "quarantined"
              : "parked",
          priority: 0,
          age_secs: 0,
          reason: "question",
          detail: "Waiting for the owner to choose.",
          gate: null,
          gate_at: null,
          gate_evidence: null,
          executor: null,
          session_id: null,
        },
      ],
      done: [],
      counts,
      queue_truncated: this.scenario === "queue-cap",
      queue_shown: this.scenario === "queue-cap" ? 200 : 3,
      queue_cap: 200,
    };
  }

  private makeCapacity(runningCount: number): CapacityView {
    const storageLow = this.scenario === "storage-low";
    const pressure = this.scenario === "supervisor-down";
    const freeHome = storageLow ? 3 * GIB : 42.1 * GIB;
    const floor = 5 * GIB;
    const disk: DiskView = {
      free_home_bytes: freeHome,
      free_worktrees_bytes: storageLow ? null : 38.7 * GIB,
      floor_bytes: floor,
      source: "capacity_report",
      state: storageLow ? "critical" : freeHome < floor * 1.25 ? "warn" : "ok",
    };
    const brake: BrakeView = storageLow
      ? {
          active: true,
          reasons: [
            {
              code: "disk_low",
              detail: "Home disk is below the reserve floor.",
            },
          ],
          since: iso(this.nowMs - 120_000),
        }
      : {
          active: false,
          reasons: [],
          since: null,
        };
    const advised = this.scenario === "idle" ? 4 : 8;
    return {
      generated_at: this.now(),
      measured_at: this.now(),
      report_loaded: true,
      host: {
        at: this.now(),
        interval_secs: 15,
        mem_total_bytes: 16 * GIB,
        mem_available_bytes: pressure ? 1.4 * GIB : 12.4 * GIB,
        swap_used_bytes: 0,
        load1: 2.1,
        load5: 1.8,
        load15: 1.6,
        cpu_count: 8,
        cpu_quota_cores: null,
        disk_free_home_bytes: freeHome,
        disk_free_worktrees_bytes: storageLow ? null : 38.7 * GIB,
        pressure_cpu_avg10: 4.2,
        pressure_mem_avg10: pressure ? 28.4 : 1.1,
        pressure_io_avg10: 0.8,
        source: "proc",
      },
      running: {
        runs: runningCount,
        ram_p90_bytes: 1.1 * GIB,
        cpu_p90_cores: 2.4,
      },
      target: {
        mode: "fixed",
        target_workers: advised,
        advised_workers: advised,
        is_override: false,
        set_by: "owner",
        set_at: iso(this.nowMs - 9_000_000),
        reason: "steady state",
      },
      advised_workers: advised,
      brake,
      reserve_ram_bytes: 2 * GIB,
      min_disk_bytes: floor,
      disk,
      ram: {
        available_bytes: pressure ? 1.4 * GIB : 12.4 * GIB,
        reserve_bytes: 2 * GIB,
        running_p90_bytes: 1.1 * GIB,
        per_worker_p90_bytes: 1.2 * GIB,
        per_worker_source: "pooled",
        per_worker_samples: 12,
        estimated_extra_slots: pressure ? 0 : 8,
        state: pressure ? "critical" : "ok",
      },
      cpu: {
        load1: 2.1,
        cpu_count: 8,
        load_ratio: 0.2625,
        pressure_avg10: 4.2,
        state: "ok",
      },
      estimates: [
        {
          project: null,
          job_kind: "build",
          harness: "opencode",
          model: null,
          build_mode: null,
          samples: 12,
          ram_p50_bytes: 1 * GIB,
          ram_p90_bytes: 1.2 * GIB,
          cpu_p50_cores: 1.8,
          cpu_p90_cores: 2.2,
          duration_p50_ms: 240000,
          duration_p90_ms: 420000,
          source: "pooled",
        },
      ],
      waiting:
        this.scenario === "busy" || this.scenario === "warnings"
          ? [
              {
                project: "vowp",
                job: "ss-5-mock",
                code: "ram",
                detail: "RAM 1.2 GB short",
                since: iso(this.nowMs - 180_000),
              },
            ]
          : [],
      limits:
        this.scenario === "busy"
          ? [
              {
                provider: "openai",
                harness: "opencode",
                kind: "quota",
                resets_at: iso(this.nowMs + 3_600_000),
                detail: "Provider quota resets at the top of the hour.",
                since: iso(this.nowMs - 600_000),
                last_probe_at: null,
                probe_fail_streak: 0,
                concurrency_cap: 2,
                cap_until: iso(this.nowMs + 3_600_000),
              },
            ]
          : [],
      provider_caps: {},
      reload_pending: false,
    };
  }

  private makeInbox(): InboxItemView[] {
    return [
      {
        id: "01MOCKINBOX000000000000000001",
        project: "vowp",
        job_id: "ss-8-blocked-review",
        kind: "question",
        title: "Choose a merge queue policy",
        body: "Should the supervisor merge one job at a time or batch ready jobs?",
        options: ["One at a time", "Batch ready jobs"],
        state: "open",
        answer: null,
        answered_by: null,
        created_at: iso(this.nowMs - 600_000),
        answered_at: null,
        age_secs: 600,
      },
    ];
  }

  private makeSessions(): SessionCardView[] {
    return [
      {
        id: "01MOCKSESSION00000000000000",
        project: "vowp",
        job_id: "ss-2-desktop-shell",
        role: "worker",
        title: "Desktop shell",
        state: "active",
        executor: {
          raw: "opencode:deepseek/deepseek-flash#max",
          harness: "opencode",
          provider: "deepseek",
          model: "deepseek/deepseek-flash",
          effort: null,
          variant: "max",
          display: "DeepSeek V4.1 Flash · max",
        },
        node: "vps",
        active_surface: null,
        last_event_at: this.now(),
      },
      {
        id: "01MOCKSESSION00000000000001",
        project: "vowp",
        job_id: null,
        role: null,
        title: "Imported session",
        state: "idle",
        executor: null,
        node: "vps",
        active_surface: null,
        last_event_at: iso(this.nowMs - 120_000),
      },
    ];
  }

  private gateMode(probed: boolean): MockGateMode {
    switch (this.scenario) {
      case "gate-asif-waiting":
        return "waiting";
      case "gate-asif-ready-export-only":
        return "ready-export-only";
      case "gate-asif-ready":
      case "gates-ready":
      case "asif-export-losses":
      case "asif-import-warnings":
      case "asif-strict-refusal":
      case "asif-conflict":
        return "ready";
      default:
        return probed ? "waiting" : "unknown";
    }
  }

  private makeGates(mode: MockGateMode, now = iso(this.nowMs)): GateSnapshot {
    const d6 = this.localGateMode();
    const localMode: MockLocalGateMode =
      mode === "unknown"
        ? "unknown"
        : d6 !== "unknown"
          ? d6
          : mode === "ready" || mode === "ready-export-only"
            ? "ready"
            : "waiting";
    const localReady = localMode === "ready";
    const localOrchestrator: LocalOrchestratorGate =
      localReady
        ? {
            state: "ready",
            agent_protocol: "vowp-agent/1",
            node: LOCAL_NODE,
            capabilities: [
              "node_card",
              "offload_status",
              "surface_pair",
              "surface_list",
              "surface_revoke",
              "surface_rotate",
              "mcp_agent_socket",
              "orchestrator_new_node",
              "orchestrator_lifecycle",
              "orchestrator_read",
              "local_agent_connected",
              "local_surface_pair",
              "local_agent_run",
              "backfill",
              "local_path_map",
              "local_cache",
            ],
            evidence: ["synthetic preview capability evidence"],
          }
        : localMode === "incompatible"
          ? {
              state: "incompatible",
              detected: "vowp-agent/0",
              required: "vowp-agent/1",
              evidence: ["vowp agent status --json: protocol vowp-agent/0"],
            }
          : localMode === "waiting" || localMode === "revoked"
            ? {
                state: "waiting_for_vowp_phase3",
                missing:
                  localMode === "revoked"
                    ? ["local agent connection"]
                    : [
                        "no active local agent node",
                        "local agent connection",
                        "surface rotation",
                        "orchestrator materialization",
                      ],
                detected_version: "0.4.0-mock",
                evidence:
                  localMode === "revoked"
                    ? ["vowp agent status --json: revoked"]
                    : [
                        "vowp node show --json: kind=home status=active presence=offline",
                        "vowp agent status --json: not_enrolled",
                      ],
              }
            : { state: "unknown" };
    const agentState =
      localReady
        ? "connected"
        : localMode === "incompatible"
          ? "incompatible"
          : localMode === "revoked"
            ? "revoked"
            : localMode === "waiting"
              ? "not_enrolled"
              : "not_installed";
    const asif: AsifGate =
      mode === "ready" || mode === "ready-export-only"
        ? {
            state: "ready",
            export: true,
            import: mode === "ready",
            detected_version: "0.4.0-mock",
            evidence: ["synthetic preview export/import evidence"],
          }
        : mode === "waiting"
          ? {
              state: "waiting_for_vowp_asif",
              missing: ["v4-asif-core", "v4-asif-cli"],
              detected_version: "0.4.0-mock",
              evidence: ["session export --format asif: exit 2 (unsupported)"],
            }
          : { state: "unknown" };
    return {
      probed_at: mode === "unknown" ? "1970-01-01T00:00:00.000Z" : now,
      local_orchestrator: localOrchestrator,
      asif,
      local_agent: {
        state: agentState,
        node: localReady ? LOCAL_NODE : agentState === "not_installed" ? null : LOCAL_NODE,
        protocol:
          localReady || agentState === "revoked"
            ? "vowp-agent/1"
            : agentState === "incompatible"
              ? "vowp-agent/0"
              : null,
        last_heartbeat: localReady ? now : null,
        last_error:
          agentState === "revoked"
            ? "the node credential was revoked"
            : agentState === "incompatible"
              ? "protocol incompatible: detected vowp-agent/0, required vowp-agent/1"
              : null,
      },
      // The D6 build advertises the action, but the flag is on exactly for Ready plus a
      // connected local agent. A waiting or incompatible gate keeps the action disabled.
      features: {
        asif_actions: true,
        local_orchestrator_actions: localReady && agentState === "connected",
      },
    };
  }


  private makeRawDocument(): RawDocument {
    const warnings: WireWarning[] =
      this.scenario === "warnings"
        ? [
            {
              path: "snapshot.projects[0].state",
              code: "unknown_enum",
              detail: 'unknown value "paused_forever"; shown as Unknown',
            },
            {
              path: "snapshot.projects[0].queue",
              code: "missing_optional",
              detail: "optional field absent; using the documented default",
            },
          ]
        : this.scenario === "unknown-states"
          ? [
              {
                path: "snapshot.projects[0].queue[0].state",
                code: "unknown_enum",
                detail: 'unknown value "quarantined"; shown as Unknown',
              },
            ]
          : [];
    const capacityWarnings: WireWarning[] =
      this.scenario === "warnings"
        ? [
            {
              path: "report.estimates",
              code: "truncated",
              detail: "VOWP capped the estimate list; the view labels the truncation.",
            },
          ]
        : [];
    return {
      status: {
        snapshot: {
          schema: "vowp-snapshot/1",
          version: 1,
          sequence: 4412,
          generated_at: this.now(),
          home: "vps",
          supervisor: {
            running: this.scenario !== "supervisor-down",
            pid: 4812,
            last_tick: this.now(),
            tick_secs: 60,
            paused: false,
            fail_streak: 0,
            version: "0.4.0-mock",
          },
          projects: [],
        },
        source: this.scenario === "stale-cache" ? "cache" : "remote",
        fetched_at: this.now(),
        stale: this.scenario === "stale-cache",
      },
      capacity: {
        report: {
          generated_at: this.now(),
          target: { mode: "fixed", target_workers: 8 },
          waiting: [],
        },
      },
      status_warnings: warnings,
      capacity_warnings: capacityWarnings,
      fetched_at: this.now(),
      generated_at: this.now(),
    };
  }
}

