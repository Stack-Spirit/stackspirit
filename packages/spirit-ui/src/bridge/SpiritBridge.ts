import type {
  ActionResult,
  AgentPairInput,
  AgentPairing,
  AgentPathAddInput,
  AgentPathView,
  AppKeyView,
  AsifExportOptions,
  AsifExportResult,
  AsifImportOptions,
  AsifImportResult,
  BridgeCapabilities,
  CapacityTargetView,
  CapacityView,
  ConnectionSnapshot,
  DaemonHealth,
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
  InboxItemView,
  JobAction,
  JobDetailView,
  KeyStatus,
  LocalAgentView,
  LocalOrchestratorOverview,
  ModelChoice,
  NewHostProfile,
  OnboardingInstallInput,
  OnboardingInstallOutcome,
  OrchestratorNewInput,
  OrchestratorView,
  RawDocument,
  SecretListView,
  SecretRemoveOutcome,
  SecretSetInput,
  SecretSetOutcome,
  SessionCardView,
  SetTargetInput,
  StateEvent,
  SurfaceEntryView,
  SurfacePairInput,
  SurfacePairResult,
  SurfaceRevokeResult,
  SurfaceRotateInput,
  SurfaceRotateResult,
  WebBridgeStatus,
  WebFleetView,
} from "./wire";

/** The only surface the UI may call. Tauri, Mock, and the Web HTTP/SSE bridge implement it. */
export interface SpiritBridge {
  /** Which transport is behind this bridge. */
  readonly kind: "tauri" | "mock" | "http-sse";
  /** True only for preview data. The shell always shows a persistent badge. */
  readonly preview: boolean;
  /**
   * Optional capability map. Desktop bridges may omit it; the shared desktop shell keeps
   * its existing behaviour. The Web shell reads it instead of branching on `kind`.
   */
  readonly capabilities?: BridgeCapabilities;

  hosts: {
    list(): Promise<HostProfile[]>;
    add(input: NewHostProfile): Promise<HostProfile>;
    update(id: string, patch: HostProfilePatch): Promise<HostProfile>;
    remove(id: string): Promise<void>;
    scanFingerprint(input: HostDraft): Promise<HostKeyScan>;
    /** Re-pin only; the caller must have verified the fingerprint out of band. */
    trustFingerprint(id: string, pin: HostKeyPin): Promise<HostProfile>;
    /**
     * Runs the add-wizard checklist. When a confirmed pin is supplied, the host key
     * stage is included and the implementation may install the app-private
     * `known_hosts` entry after verifying the pin against a fresh scan.
     */
    probe(input: HostDraft, pin?: HostKeyPin | null): Promise<HostProbe>;
  };

  /**
   * Optional one-time-password onboarding for the app-owned SSH key. Present on the
   * desktop host and on the preview bridge, which reports a typed `preview_unavailable`
   * error instead of pretending to install anything.
   *
   * `installKey` receives the owner's one-time server password. The host command
   * consumes it once for the password bootstrap, then forgets it: it is never stored,
   * never returned, and never included in an error. The renderer never supplies the
   * app key path; the host resolves its own app-private key.
   */
  onboarding?: {
    /** Returns the existing app key, or `exists: false` when none was created yet. */
    keyStatus(): Promise<AppKeyView>;
    /** Idempotent: returns the existing app key or creates one. */
    createKey(): Promise<AppKeyView>;
    /**
     * Installs the app key with the one-time password and saves the pinned host
     * profile first, so a failed install leaves a profile that can be retried.
     */
    installKey(input: OnboardingInstallInput): Promise<OnboardingInstallOutcome>;
    /**
     * Ids of saved hosts whose profile points at the app-owned key, so Settings can
     * offer the optional password-login retry without exposing the key path. Hosts
     * saved before a failed install are included; their pin is already trusted.
     */
    installedHosts?(): Promise<string[]>;
  };

  /**
   * Optional server secret vault over VOWP's `vowp secret` commands. Present on the
   * desktop host; the preview bridge reports a typed `preview_unavailable` error because
   * preview mode has no host and never pretends to store a key.
   *
   * The value is **write-only**: `set` accepts it once, the host moves it into zeroizing
   * storage and sends it over verified SSH stdin, and no read ever returns it. The
   * renderer only receives names, the last four characters and timestamps. A secret is
   * always sent on stdin, never argv or a log, and the AI only ever sees metadata.
   */
  secrets?: {
    /** Reads `vowp --json secret ls`; metadata only, never a value. */
    list(): Promise<SecretListView>;
    /** Stores one secret and returns only `{name,last4}`. */
    set(input: SecretSetInput): Promise<SecretSetOutcome>;
    /** Removes one secret through `vowp secret rm NAME --yes`. */
    remove(name: string): Promise<SecretRemoveOutcome>;
  };

  connection: {
    connect(hostId: string): Promise<ConnectionSnapshot>;
    disconnect(hostId: string): Promise<void>;
    status(): Promise<ConnectionSnapshot>;
    /** Returns an unsubscribe function; callers must clean up on unmount. */
    subscribe(listener: (event: StateEvent) => void): () => void;
  };

  fleet: {
    get(): Promise<FleetView | null>;
    refresh(): Promise<FleetView>;
    capacity(): Promise<CapacityView | null>;
    jobAction(jobId: string, action: JobAction, reason?: string | null): Promise<ActionResult>;
    inbox(all: boolean): Promise<InboxItemView[]>;
    answerInbox(id: string, answer: string): Promise<InboxItemView>;
    setTarget(input: SetTargetInput): Promise<CapacityTargetView>;
    /**
     * Optional read-only detail for one job: the full `Job` fields the drawer needs, its
     * runs oldest first, and the gate results of its newest attempt. Present on the Web
     * and preview bridges; desktop bridges may omit it until the desktop drawer adopts it.
     * A 404 is a typed `not_found`, not a missing route.
     */
    job?(id: string): Promise<JobDetailView>;
    /** Parsed VOWP documents plus every `WireWarning` from the last parse. */
    rawDocument(): Promise<RawDocument | null>;
    /**
     * Optional honest Web read model. Desktop bridges omit it; the Web bridge returns
     * `null` only before its first successful snapshot. `refresh` forces a full read.
     */
    snapshot?(refresh?: boolean): Promise<WebFleetView | null>;
    /** Optional push for new Web snapshots. Returns an unsubscribe function. */
    watch?(listener: (snapshot: WebFleetView) => void): () => void;
    /** Optional transport/liveness detail for the stale and polling notices. */
    webStatus?(): WebBridgeStatus;
  };

  /** Optional daemon health read. Present on the Web and preview bridges. */
  daemon?: {
    health(): Promise<DaemonHealth>;
  };

  /** Releases streams, timers, and in-flight requests. Safe to call more than once. */
  dispose?(): void;

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

  /**
   * Optional read/action surface for VOWP sessions. Present on the desktop and preview
   * bridges; the read-only Web bridge omits it.
   *
   * Every action requires the ASIF gate snapshot to be `ready` for that direction and
   * `features.asif_actions` to be true. The renderer never receives a remote path,
   * package bytes, token, or provider key.
   */
  sessions?: {
    /** Sessions from the last snapshot; read-only and never a fresh remote read. */
    list(): Promise<SessionCardView[]>;
    /**
     * Exports one session to a local `.asif.zip`. The host opens the native save dialog
     * unless `options.path` is supplied, then returns a safe summary. `null` means the
     * user cancelled the picker; nothing was exported.
     */
    exportAsif(
      sessionId: string,
      options: AsifExportOptions,
    ): Promise<AsifExportResult | null>;
    /**
     * Imports one local `.asif.zip` as a new idle session. The host opens the native
     * open dialog unless `options.path` is supplied. `null` means the user cancelled.
     */
    importAsif(options: AsifImportOptions): Promise<AsifImportResult | null>;
  };

  /**
   * Optional D6 local-orchestrator surface. Present on the desktop and preview bridges;
   * the read-only Web bridge omits it.
   *
   * Read methods stay available while the gate is `Waiting` so the owner can see which
   * prerequisite is missing. Start/stop/resume/rotate/pair actions require the
   * `LocalOrchestratorGate` to be `ready`, `features.local_orchestrator_actions` to be
   * true and the local agent to be `connected`; the backend re-checks all three on every
   * call. Explicit cleanup (`revoke`, `stop`) remains available while the gate is
   * waiting. The renderer never receives an absolute Mac path, a surface token, a token
   * file or a key fingerprint.
   */
  localOrchestrators?: {
    /** Safe agent status, node card, surfaces, orchestrators and path maps. */
    overview(): Promise<LocalOrchestratorOverview>;
    /** Reads `vowp agent status --json` on this Mac. */
    agentStatus(): Promise<LocalAgentView>;
    /** Redeems a one-time pairing code on this Mac. */
    agentPair(input: AgentPairInput): Promise<AgentPairing>;
    /** Reads the Mac path map and records its validity for the action guard. */
    agentPaths(): Promise<AgentPathView[]>;
    /** Adds or updates one Mac path-map row. */
    agentPathAdd(input: AgentPathAddInput): Promise<AgentPathView>;
    /** Removes one Mac path-map row. */
    agentPathRemove(project: string): Promise<void>;
    /** Creates a pending home surface and returns its one-time pairing code. */
    surfacePair(input: SurfacePairInput): Promise<SurfacePairResult>;
    /** Lists home surfaces; available while the gate is waiting. */
    surfaceList(node?: string | null): Promise<SurfaceEntryView[]>;
    /** Rotates one surface token. */
    surfaceRotate(input: SurfaceRotateInput): Promise<SurfaceRotateResult>;
    /** Revokes one surface and every token it holds. */
    surfaceRevoke(surface: string): Promise<SurfaceRevokeResult>;
    /** Creates a local app host through `vowp orchestrator new --node ...`. */
    orchestratorNew(input: OrchestratorNewInput): Promise<OrchestratorView>;
    /** Lists orchestrators; available while the gate is waiting. */
    orchestratorList(): Promise<OrchestratorView[]>;
    /** Stops a local app host; explicit cleanup remains available when gated off. */
    orchestratorStop(name: string): Promise<OrchestratorView>;
    /** Resumes a local app host after re-checking every D6 prerequisite. */
    orchestratorResume(
      name: string,
      project?: string | null,
      node?: string | null,
    ): Promise<OrchestratorView>;
  };
}
