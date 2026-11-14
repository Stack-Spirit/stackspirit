import { create } from "zustand";

import type { SpiritBridge } from "../bridge";
import { toSpiritError, type SpiritError } from "../bridge/errors";
import type {
  CapacityTargetView,
  CapacityView,
  ConnectionSnapshot,
  EngineStatus,
  FleetView,
  GateSnapshot,
  HostKeyPin,
  HostProfile,
  HostProfilePatch,
  NewHostProfile,
  RawDocument,
  SetTargetInput,
  StateEvent,
} from "../bridge/wire";

export type StoreStatus = "idle" | "loading" | "ready" | "error";

/** PLAN §6.2: three consecutive poll failures mark the data stale, then back off 30 s. */
export const STALE_BACKOFF_MS = 30_000;

const DISCONNECTED: ConnectionSnapshot = {
  state: "disconnected",
  host_id: null,
  since: null,
  latency: null,
  last_ok_at: null,
  error: null,
};

interface FleetState {
  bridge: SpiritBridge | null;
  status: StoreStatus;
  preview: boolean;
  revision: number;
  connection: ConnectionSnapshot;
  fleet: FleetView | null;
  capacity: CapacityView | null;
  engine: EngineStatus | null;
  gates: GateSnapshot | null;
  rawDocument: RawDocument | null;
  hosts: HostProfile[];
  hostsLoaded: boolean;
  selectedHostId: string | null;
  projectFilter: string | null;
  latencyHistory: number[];
  lastError: SpiritError | null;
  /** Epoch milliseconds of the next automatic poll after a stale backoff. */
  nextRetryAt: number | null;

  initialize(bridge: SpiritBridge): Promise<() => void>;
  dispose(): void;
  applyEvent(event: StateEvent): void;
  setConnection(connection: ConnectionSnapshot): void;
  acceptFleet(fleet: FleetView | null, fromEvent?: boolean): void;
  loadHosts(): Promise<void>;
  loadRawDocument(): Promise<void>;
  connect(hostId: string): Promise<void>;
  disconnect(hostId: string): Promise<void>;
  refresh(): Promise<void>;
  selectHost(hostId: string): void;
  setProject(project: string | null): void;
  addHost(input: NewHostProfile): Promise<HostProfile>;
  updateHost(id: string, patch: HostProfilePatch): Promise<HostProfile>;
  removeHost(id: string): Promise<void>;
  trustFingerprint(id: string, pin: HostKeyPin): Promise<HostProfile>;
  setTarget(input: SetTargetInput): Promise<CapacityTargetView>;
  refreshEngine(): Promise<void>;
  probeGates(): Promise<void>;
  clearError(): void;
}

let activeUnsubscribe: (() => void) | null = null;

function connectionOfHost(
  hosts: HostProfile[],
  connection: ConnectionSnapshot,
  selected: string | null,
): string | null {
  const candidates = [connection.host_id, selected].filter(
    (value): value is string => typeof value === "string",
  );
  for (const candidate of candidates) {
    if (hosts.some((host) => host.id === candidate)) return candidate;
  }
  return hosts[0]?.id ?? null;
}

function appendLatency(history: number[], fleet: FleetView | null): number[] {
  if (!fleet || fleet.latency.samples === 0) return history;
  return [...history.slice(-19), fleet.latency.last_ms];
}

export function createInitialFleetState(): Pick<
  FleetState,
  | "bridge"
  | "status"
  | "preview"
  | "revision"
  | "connection"
  | "fleet"
  | "capacity"
  | "engine"
  | "gates"
  | "rawDocument"
  | "hosts"
  | "hostsLoaded"
  | "selectedHostId"
  | "projectFilter"
  | "latencyHistory"
  | "lastError"
  | "nextRetryAt"
> {
  return {
    bridge: null,
    status: "idle",
    preview: false,
    revision: 0,
    connection: DISCONNECTED,
    fleet: null,
    capacity: null,
    engine: null,
    gates: null,
    rawDocument: null,
    hosts: [],
    hostsLoaded: false,
    selectedHostId: null,
    projectFilter: null,
    latencyHistory: [],
    lastError: null,
    nextRetryAt: null,
  };
}

/** Re-arms the stale backoff when a countdown expires or no timer is set yet. */
function backoffDeadline(current: number | null, now: number): number {
  return current !== null && current > now ? current : now + STALE_BACKOFF_MS;
}

export const useFleetStore = create<FleetState>((set, get) => ({
  ...createInitialFleetState(),

  initialize: async (bridge) => {
    activeUnsubscribe?.();
    activeUnsubscribe = null;
    set({
      ...createInitialFleetState(),
      bridge,
      preview: bridge.preview,
      status: "loading",
    });
    const unsubscribe = bridge.connection.subscribe((event) => get().applyEvent(event));
    activeUnsubscribe = unsubscribe;

    const results = await Promise.allSettled([
      bridge.hosts.list(),
      bridge.connection.status(),
      bridge.fleet.get(),
      bridge.engine.status(),
      bridge.gates.get(),
      bridge.fleet.rawDocument(),
    ]);
    const [hosts, connection, fleet, engine, gates, rawDocument] = results;
    const errors: SpiritError[] = [];

    if (connection.status === "fulfilled") {
      get().setConnection(connection.value);
    } else {
      errors.push(toSpiritError(connection.reason));
    }
    if (fleet.status === "fulfilled" && fleet.value) {
      get().acceptFleet(fleet.value);
    }
    if (engine.status === "fulfilled") set({ engine: engine.value });
    if (gates.status === "fulfilled") set({ gates: gates.value });
    if (rawDocument.status === "fulfilled") set({ rawDocument: rawDocument.value });

    if (hosts.status === "fulfilled") {
      const current = get();
      set({
        hosts: hosts.value,
        hostsLoaded: true,
        selectedHostId: connectionOfHost(
          hosts.value,
          current.connection,
          current.selectedHostId,
        ),
      });
    } else {
      errors.push(toSpiritError(hosts.reason));
    }
    set({
      status: errors.length > 0 ? "error" : "ready",
      lastError: errors[0] ?? get().lastError,
    });
    return () => {
      unsubscribe();
      if (activeUnsubscribe === unsubscribe) activeUnsubscribe = null;
    };
  },

  dispose: () => {
    activeUnsubscribe?.();
    activeUnsubscribe = null;
  },

  applyEvent: (event) => {
    const state = get();
    // Revisions are monotonic across all event kinds. An older event is never
    // allowed to overwrite newer state. Equal revisions are accepted because core
    // pairs a fleet view and its connection snapshot at the same revision.
    if (event.revision < state.revision) return;
    switch (event.kind) {
      case "connection":
        set({ revision: Math.max(state.revision, event.revision) });
        get().setConnection(event.connection);
        break;
      case "fleet":
        set({ revision: Math.max(state.revision, event.revision) });
        get().acceptFleet(event.fleet, true);
        break;
      case "engine":
        set({
          revision: Math.max(state.revision, event.revision),
          engine: event.engine,
        });
        break;
      case "gates":
        set({ revision: Math.max(state.revision, event.revision), gates: event.gates });
        break;
      case "error":
        set({
          revision: Math.max(state.revision, event.revision),
          lastError: toSpiritError(event.error),
        });
        break;
    }
  },

  setConnection: (connection) => {
    const state = get();
    const history = connection.latency
      ? [...state.latencyHistory.slice(-19), connection.latency.last_ms]
      : state.latencyHistory;
    // Core keeps the last good view and marks the connection stale. Mirror that onto
    // the retained view so every consumer dims and disables writes immediately.
    const fleet =
      connection.state === "stale" && state.fleet
        ? { ...state.fleet, stale: true }
        : state.fleet;
    const now = Date.now();
    set({
      connection,
      fleet,
      capacity: fleet?.capacity ?? state.capacity,
      latencyHistory: history,
      lastError: connection.error ? toSpiritError(connection.error) : state.lastError,
      nextRetryAt:
        connection.state === "stale"
          ? backoffDeadline(state.nextRetryAt, now)
          : null,
    });
  },

  acceptFleet: (fleet, fromEvent = false) => {
    if (!fleet) {
      set({ fleet: null, capacity: null });
      return;
    }
    const state = get();
    if (!fromEvent && fleet.revision < state.revision) return;
    if (!fromEvent && state.fleet && fleet.revision < state.fleet.revision) return;
    set({
      fleet,
      capacity: fleet.capacity,
      revision: Math.max(state.revision, fleet.revision),
      latencyHistory: appendLatency(state.latencyHistory, fleet),
    });
  },

  loadHosts: async () => {
    const bridge = get().bridge;
    if (!bridge) return;
    try {
      const hosts = await bridge.hosts.list();
      const current = get();
      set({
        hosts,
        hostsLoaded: true,
        selectedHostId: connectionOfHost(hosts, current.connection, current.selectedHostId),
      });
    } catch (error) {
      set({ lastError: toSpiritError(error), status: "error" });
    }
  },

  loadRawDocument: async () => {
    const bridge = get().bridge;
    if (!bridge) return;
    try {
      set({ rawDocument: await bridge.fleet.rawDocument() });
    } catch (error) {
      set({ lastError: toSpiritError(error) });
    }
  },

  connect: async (hostId) => {
    const bridge = get().bridge;
    if (!bridge) return;
    set({ selectedHostId: hostId, lastError: null });
    try {
      const connection = await bridge.connection.connect(hostId);
      get().setConnection(connection);
      await get().loadHosts();
      const fleet = await bridge.fleet.get();
      if (fleet) get().acceptFleet(fleet);
      await get().loadRawDocument();
      set({ status: "ready", lastError: null });
    } catch (error) {
      const spiritError = toSpiritError(error);
      set({ lastError: spiritError });
      try {
        const connection = await bridge.connection.status();
        get().setConnection(connection);
      } catch {
        // The failed event already carries the safe error.
      }
    }
  },

  disconnect: async (hostId) => {
    const bridge = get().bridge;
    if (!bridge) return;
    try {
      await bridge.connection.disconnect(hostId);
      const connection = await bridge.connection.status();
      get().setConnection(connection);
    } catch (error) {
      set({ lastError: toSpiritError(error) });
    }
  },

  refresh: async () => {
    const bridge = get().bridge;
    if (!bridge) return;
    try {
      const fleet = await bridge.fleet.refresh();
      get().acceptFleet(fleet);
      await get().loadRawDocument();
      set({ lastError: null, nextRetryAt: null });
    } catch (error) {
      set({ lastError: toSpiritError(error) });
      try {
        get().setConnection(await bridge.connection.status());
      } catch {
        // Keep the current view; the status command is best-effort here.
      }
    }
  },

  selectHost: (hostId) => set({ selectedHostId: hostId }),
  setProject: (project) => set({ projectFilter: project }),

  addHost: async (input) => {
    const bridge = get().bridge;
    if (!bridge) throw toSpiritError("bridge is not initialized");
    const host = await bridge.hosts.add(input);
    await get().loadHosts();
    set({ selectedHostId: host.id, lastError: null });
    return host;
  },

  updateHost: async (id, patch) => {
    const bridge = get().bridge;
    if (!bridge) throw toSpiritError("bridge is not initialized");
    const host = await bridge.hosts.update(id, patch);
    await get().loadHosts();
    return host;
  },

  removeHost: async (id) => {
    const bridge = get().bridge;
    if (!bridge) throw toSpiritError("bridge is not initialized");
    await bridge.hosts.remove(id);
    await get().loadHosts();
  },

  trustFingerprint: async (id, pin) => {
    const bridge = get().bridge;
    if (!bridge) throw toSpiritError("bridge is not initialized");
    const host = await bridge.hosts.trustFingerprint(id, pin);
    await get().loadHosts();
    set({ lastError: null });
    return host;
  },

  setTarget: async (input) => {
    const bridge = get().bridge;
    if (!bridge) throw toSpiritError("bridge is not initialized");
    const target = await bridge.fleet.setTarget(input);
    const fleet = get().fleet;
    if (fleet) {
      get().acceptFleet({
        ...fleet,
        capacity: { ...fleet.capacity, target, advised_workers: target.advised_workers },
      });
    }
    return target;
  },

  refreshEngine: async () => {
    const bridge = get().bridge;
    if (!bridge) return;
    try {
      set({ engine: await bridge.engine.status() });
    } catch (error) {
      set({ lastError: toSpiritError(error) });
    }
  },

  probeGates: async () => {
    const bridge = get().bridge;
    if (!bridge) return;
    try {
      set({ gates: await bridge.gates.probe() });
    } catch (error) {
      set({ lastError: toSpiritError(error) });
    }
  },

  clearError: () => set({ lastError: null }),
}));

/** Clears all data between tests while keeping the action closures. */
export function resetFleetStore(): void {
  activeUnsubscribe?.();
  activeUnsubscribe = null;
  useFleetStore.setState(createInitialFleetState());
}

export const selectConnection = (state: FleetState): ConnectionSnapshot => state.connection;
export const selectFleet = (state: FleetState): FleetView | null => state.fleet;
export const selectCapacity = (state: FleetState): CapacityView | null => state.capacity;
export const selectEngine = (state: FleetState): EngineStatus | null => state.engine;
export const selectGates = (state: FleetState): GateSnapshot | null => state.gates;
export const selectHosts = (state: FleetState): HostProfile[] => state.hosts;
export const selectRevision = (state: FleetState): number => state.revision;
export const selectPreview = (state: FleetState): boolean => state.preview;
export const selectLastError = (state: FleetState): SpiritError | null => state.lastError;
export const selectNextRetryAt = (state: FleetState): number | null => state.nextRetryAt;
export const selectRawDocument = (state: FleetState): RawDocument | null => state.rawDocument;
export const selectSelectedHostId = (state: FleetState): string | null => state.selectedHostId;
export const selectProjectFilter = (state: FleetState): string | null => state.projectFilter;

/** Writes are allowed only while connected and while the last view is fresh. */
export const selectMutationsAllowed = (state: FleetState): boolean =>
  state.connection.state === "connected" && state.fleet !== null && !state.fleet.stale;

export const selectSelectedHost = (state: FleetState): HostProfile | null =>
  state.hosts.find((host) => host.id === state.selectedHostId) ?? null;

export const selectActiveHost = (state: FleetState): HostProfile | null => {
  const id = state.connection.host_id ?? state.selectedHostId;
  return state.hosts.find((host) => host.id === id) ?? null;
};

export const selectDataDimmed = (state: FleetState): boolean =>
  state.connection.state !== "connected" || state.fleet?.stale === true;

export function useConnection(): ConnectionSnapshot {
  return useFleetStore(selectConnection);
}
export function useFleet(): FleetView | null {
  return useFleetStore(selectFleet);
}
export function useCapacity(): CapacityView | null {
  return useFleetStore(selectCapacity);
}
export function useEngine(): EngineStatus | null {
  return useFleetStore(selectEngine);
}
export function useGates(): GateSnapshot | null {
  return useFleetStore(selectGates);
}
export function useHosts(): HostProfile[] {
  return useFleetStore(selectHosts);
}
export function usePreview(): boolean {
  return useFleetStore(selectPreview);
}
export function useSelectedHostId(): string | null {
  return useFleetStore(selectSelectedHostId);
}
export function useMutationsAllowed(): boolean {
  return useFleetStore(selectMutationsAllowed);
}
export function useDataDimmed(): boolean {
  return useFleetStore(selectDataDimmed);
}
export function useLastError(): SpiritError | null {
  return useFleetStore(selectLastError);
}
export function useNextRetryAt(): number | null {
  return useFleetStore(selectNextRetryAt);
}
export function useRawDocument(): RawDocument | null {
  return useFleetStore(selectRawDocument);
}
export function useProjectFilter(): string | null {
  return useFleetStore(selectProjectFilter);
}
