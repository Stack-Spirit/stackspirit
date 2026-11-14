import type { SpiritBridge } from "./SpiritBridge";
import type {
  ActionResult,
  AsifGate,
  BrakeView,
  CapacityTargetView,
  CapacityView,
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
  JobLanes,
  KeyStatus,
  LatencyStats,
  LocalOrchestratorGate,
  ModelChoice,
  NewHostProfile,
  ProjectView,
  RawDocument,
  SetTargetInput,
  SpiritErrorDto,
  StateEvent,
  TargetView,
  WireWarning,
  WorkerCardView,
} from "./wire";

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

interface MockOptions {
  scenario?: MockScenario;
  now?: string;
  hosts?: HostProfile[];
}

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
  private actions: ActionResult[] = [];
  private keys = new Map<string, KeyStatus>();
  private modelChoices: ModelChoice[];

  constructor(options: MockOptions | MockScenario = "fresh") {
    const resolved: MockOptions = typeof options === "string" ? { scenario: options } : options;
    this.scenario = resolved.scenario ?? "fresh";
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
    const readyEngine = this.scenario === "engine-ready" || this.scenario === "fresh";
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
    this.gateSnapshot = this.makeGates("unknown");
    this.inboxItems = this.makeInbox();
  }

  /** Test hook: advances the deterministic clock used by every mock timestamp. */
  advanceClock(ms: number): void {
    this.nowMs += ms;
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
      if (this.scenario === "stale-after-3") {
        this.failures += 1;
        const error = errorDto(
          "remote_timeout",
          "the remote command did not finish within 20s",
          "Check the SSH connection and retry.",
          true,
        );
        if (this.failures >= 3) {
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
      this.recordLatency(38 + this.failures);
      const revision = this.nextRevision();
      if (this.scenario !== "stale-cache" && this.scenario !== "supervisor-down") {
        this.fleetView = {
          ...this.fleetView,
          revision,
          fetched_at: this.now(),
          generated_at: this.now(),
          latency: this.latency(),
        };
      } else {
        this.fleetView = { ...this.fleetView, revision, latency: this.latency() };
      }
      this.connectionState = { ...this.connectionState, latency: this.latency() };
      this.emit({ kind: "fleet", revision, fleet: clone(this.fleetView) });
      this.emit({ kind: "connection", revision, connection: clone(this.connectionState) });
      return clone(this.fleetView);
    },
    capacity: async (): Promise<CapacityView | null> =>
      this.fleetView ? clone(this.fleetView.capacity) : null,
    jobAction: async (jobId: string, action: JobAction): Promise<ActionResult> => {
      const known = this.fleetView
        ? [
            ...this.fleetView.running.map((card) => card.job_id),
            ...Object.values(this.fleetView.lanes).flatMap((lane) =>
              Array.isArray(lane) ? lane.map((card) => card.job_id) : [],
            ),
          ].includes(jobId)
        : false;
      if (!known) throw errorDto("not_found", `Job ${jobId} was not found.`);
      const result: ActionResult = {
        job_id: jobId,
        action,
        request: null,
        refreshed: false,
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
      this.gateSnapshot = this.makeGates(
        this.scenario === "gates-ready" ? "ready" : "waiting",
        this.now(),
      );
      this.emit({ kind: "gates", revision: this.nextRevision(), gates: clone(this.gateSnapshot) });
      return clone(this.gateSnapshot);
    },
  };

  hosts = this.hostsApi;
  connection = this.connectionApi;
  fleet = this.fleetApi;
  engine = this.engineApi;
  gates = this.gatesApi;

  /** Test hook: recorded job actions. */
  recordedActions(): ActionResult[] {
    return clone(this.actions);
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
      sessions: [],
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

  private makeGates(mode: "unknown" | "waiting" | "ready", now = iso(this.nowMs)): GateSnapshot {
    const localOrchestrator: LocalOrchestratorGate =
      mode === "ready"
        ? {
            state: "ready",
            agent_protocol: "vowp-agent/1",
            node: "macbook",
            capabilities: [
              "node_card",
              "offload_status",
              "surface_start",
              "surface_attach",
              "mcp_agent_socket",
              "orchestrator_new_node",
              "orchestrator_read",
            ],
            evidence: ["synthetic preview evidence"],
          }
        : mode === "waiting"
          ? {
              state: "waiting_for_vowp_phase3",
              missing: [
                "local opencode app host",
                "surface bridge",
                "MCP proxy",
                "orchestrator materialization",
              ],
              detected_version: "0.4.0-mock",
              evidence: ["vowp agent surface start --help: exit 2"],
            }
          : { state: "unknown" };
    const asif: AsifGate =
      mode === "ready"
        ? {
            state: "ready",
            export: true,
            import: true,
            detected_version: "0.4.0-mock",
            evidence: ["synthetic preview evidence"],
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
        state: mode === "ready" ? "connected" : "not_installed",
        node: mode === "ready" ? "macbook" : null,
        protocol: mode === "ready" ? "vowp-agent/1" : null,
        last_heartbeat: null,
        last_error: null,
      },
      features: { asif_actions: false, local_orchestrator_actions: false },
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

