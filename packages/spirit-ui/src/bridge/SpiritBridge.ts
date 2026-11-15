import type {
  ActionResult,
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
  KeyStatus,
  ModelChoice,
  NewHostProfile,
  RawDocument,
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
}
