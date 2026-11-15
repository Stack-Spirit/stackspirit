import type {
  ActionResult,
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
  ModelChoice,
  NewHostProfile,
  RawDocument,
  SessionCardView,
  SetTargetInput,
  StateEvent,
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
}
