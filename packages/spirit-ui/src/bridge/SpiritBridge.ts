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
  BackupPolicyView,
  BridgeCapabilities,
  CapacityTargetView,
  CapacityView,
  ChatEvent,
  ChatLogPage,
  ChatLogQuery,
  ChatPermissionDecision,
  ChatSendInput,
  ChatSendReceipt,
  ChatSessionView,
  ChatStartInput,
  ClusterConfigView,
  ClusterFormInput,
  ClusterFormedView,
  ClusterPromoteInput,
  ClusterPromoteOutcome,
  ClusterStateView,
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
  OrchestratorMoveTarget,
  OrchestratorNewInput,
  OrchestratorView,
  ProjectAddInput,
  ProjectAddOutcome,
  RawDocument,
  SecretListView,
  SecretRemoveOutcome,
  SecretSetInput,
  SecretSetOutcome,
  SessionCardView,
  SessionTakeoverResult,
  SessionId,
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

/**
 * The optional orchestrator chat surface (`docs/ORCHESTRATOR-CHAT.md` §4.3).
 *
 * Presence is the capability: desktop `TauriBridge` and `MockBridge` implement it;
 * `HttpSseBridge`/`WebMockBridge` omit it and keep the Web shell read-only.
 *
 * Gate semantics: `setup` runs on the bundled StackSpirit Engine. `global` and
 * `project:<name>` attach only to the orchestrator server VOWP reports. They stay gated
 * until VOWP 0.5's `vowp orchestrator endpoint` ships or the owner approves the
 * app-owned SSH forward; a gated call rejects with a typed `chat_gated`/
 * `chat_not_mirrored`/`chat_not_configured` error and never invents an assistant reply.
 *
 * Preview honesty: only `MockBridge` produces `origin: "preview"` messages. Every
 * preview assistant message says it is a preview in its first paragraph; a real bridge
 * never constructs that origin.
 */
export interface ChatBridgeSurface {
  /** Ensure the engine/endpoint and attach/create the native session; idempotent. */
  start(input: ChatStartInput): Promise<ChatSessionView>;
  /**
   * One-click Start for a project orchestrator that has no local record yet. The host
   * attaches VOWP's local endpoint when it exists, otherwise starts a project-scoped
   * session on the bundled engine with the owner's connected models. Idempotent.
   * Optional: an older bridge may omit it.
   */
  provisionOrchestrator?(project: string): Promise<ChatSessionView>;
  /** Reuse the stored native session; never creates one. */
  attach(session: SessionId): Promise<ChatSessionView>;
  /**
   * Accept one user turn; assistant text arrives via `subscribe`. Rejects while the
   * session is `streaming` with `chat_busy` (there is no queue) and rejects empty or
   * over-long text with `invalid_input`. `accepted` means accepted locally, not
   * answered by a model.
   */
  send(input: ChatSendInput): Promise<ChatSendReceipt>;
  /**
   * Abort the current turn (when supported) and return the stopped view. It never stops
   * the engine, the VOWP orchestrator or the server, and never deletes a session.
   */
  stop(session: SessionId): Promise<ChatSessionView>;
  /**
   * Switch the setup assistant's model to one in `available_models` and return the
   * updated view. An explicit choice is persisted locally and never written to VOWP. An
   * orchestrator owns its own model config, so that session rejects rather than
   * pretending the app can change it. Optional: an older bridge may omit it.
   */
  setModel?(session: SessionId, modelId: string): Promise<ChatSessionView>;
  /**
   * Answer one pending permission request (`ChatSessionView.pending_permissions`) from an
   * owner click and return the refreshed view. Nothing is ever approved implicitly. A
   * request that is already gone resolves with the current view. Optional: an older
   * bridge may omit it.
   */
  replyPermission?(
    session: SessionId,
    permissionId: string,
    decision: ChatPermissionDecision,
  ): Promise<ChatSessionView>;
  /** Read the conversation log; pages move backward from newest, each oldest-first. */
  log(session: SessionId, query?: ChatLogQuery): Promise<ChatLogPage>;
  /**
   * Returns an unsubscribe function; callers must clean up on unmount. This is one
   * in-process subscription, not a second SSE connection per caller. Each event carries
   * a monotonic `revision`; a gap means call `log` and replace the messages.
   */
  subscribe(listener: (event: ChatEvent) => void): () => void;
}

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

  /**
   * One-step project bootstrap through `vowp project add`. Absent on the read-only Web
   * bridge. The outcome never carries the home-side path, and the call only happens on
   * an explicit user action, never on connect or render.
   */
  projects?: {
    add(input: ProjectAddInput): Promise<ProjectAddOutcome>;
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
    /** Dismisses an item without an answer (`vowp inbox dismiss`); the only way to clear a notice. */
    dismissInbox(id: string): Promise<InboxItemView>;
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
   * Optional orchestrator chat. Presence is the capability; the Web bridge omits it.
   * Desktop `TauriBridge` and `MockBridge` implement it. See `ChatBridgeSurface` for
   * the gate and preview-honesty rules.
   */
  chat?: ChatBridgeSurface;

  /**
   * Optional pooled-compute and failover cluster surface. Present on the desktop host
   * and on the preview bridge, which reports a typed `preview_unavailable` error
   * instead of inventing a cluster.
   *
   * `form` and `promote` mutate real servers over verified SSH and are gated by an
   * explicit owner confirmation; the host re-checks it. `health` reads the saved
   * cluster; a failed read comes back as `health_error` with the saved config still
   * present. No value returned here carries an SSH target, absolute path, backup
   * destination, pairing code or token; form inputs use remote paths only once and the
   * host never stores them.
   */
  cluster?: {
    /** Enrolls every worker, enables pooled compute and registers Home backups. */
    form(input: ClusterFormInput): Promise<ClusterFormedView>;
    /** Reads the saved cluster and its pooled health; never forms anything. */
    health(): Promise<ClusterStateView>;
    /** Promotes one worker's backup to Home after the exact confirmation token. */
    promote(input: ClusterPromoteInput): Promise<ClusterPromoteOutcome>;
    /** Reads the saved backup policy, or null when no cluster is configured. */
    backupPolicy(): Promise<BackupPolicyView | null>;
    /**
     * Removes one worker from the local cluster record. This never revokes that
     * worker's VOWP agent credentials and never touches Home's backup schedule.
     */
    removeWorker(hostId: string): Promise<ClusterConfigView>;
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
    /**
     * Takes a session over for StackSpirit (`vowp session takeover`). The host resolves
     * the local node, the session's project and the surface label; the renderer sends
     * only the session id. The idle-versus-running confirmation is the caller's job. An
     * older VOWP rejects with a `not_supported` or `chat_gated` error and ownership is
     * never faked.
     */
    takeoverSession(sessionId: string): Promise<SessionTakeoverResult>;
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
    /**
     * Moves an orchestrator to a node or back home. The backend re-checks the D6 gate
     * and the connected agent; the result is only the card VOWP printed.
     */
    orchestratorMove(
      name: string,
      target: OrchestratorMoveTarget,
    ): Promise<OrchestratorView>;
  };
}
