import { create } from "zustand";

import { toSpiritError, type SpiritError } from "../bridge/errors";
import type {
  ClusterConfigView,
  ClusterFormInput,
  ClusterFormedView,
  ClusterPromoteInput,
  ClusterPromoteOutcome,
  ClusterStateView,
} from "../bridge/wire";
import { useFleetStore, type StoreStatus } from "../state/fleetStore";

interface ClusterStoreState {
  cluster: ClusterStateView | null;
  clusterStatus: StoreStatus;
  clusterError: SpiritError | null;
  loadCluster(): Promise<void>;
  formCluster(input: ClusterFormInput): Promise<ClusterFormedView>;
  promoteClusterBackup(input: ClusterPromoteInput): Promise<ClusterPromoteOutcome>;
  removeClusterWorker(hostId: string): Promise<ClusterConfigView>;
}

const INITIAL_CLUSTER_STATE = {
  cluster: null as ClusterStateView | null,
  clusterStatus: "idle" as StoreStatus,
  clusterError: null as SpiritError | null,
};

/**
 * Cluster state beside the fleet store, not inside it: the saved cluster belongs to the
 * saved servers and Home's VOWP, while the fleet store tracks the active SSH connection.
 * The bridge instance still comes from the fleet store so the shell has exactly one.
 *
 * A failed health read stays local to the cluster UI and drops the last health, so the
 * sidebar can never show a stale pooled total.
 */
export const useClusterStore = create<ClusterStoreState>((set, get) => ({
  ...INITIAL_CLUSTER_STATE,

  loadCluster: async () => {
    const bridge = useFleetStore.getState().bridge;
    // Preview has no host: it must never show a cluster, real or invented.
    if (!bridge?.cluster || bridge.preview) {
      set({ cluster: null, clusterStatus: "idle", clusterError: null });
      return;
    }
    set({ clusterStatus: "loading" });
    try {
      const cluster = await bridge.cluster.health();
      set({ cluster, clusterStatus: "ready", clusterError: null });
    } catch (error) {
      // The connected server's fleet view is still valid and must not be dimmed by a
      // failed cluster read. Drop the last health so no stale total survives.
      const current = get().cluster;
      set({
        cluster: current ? { ...current, health: null, health_error: null } : null,
        clusterStatus: "error",
        clusterError: toSpiritError(error),
      });
    }
  },

  formCluster: async (input) => {
    const bridge = useFleetStore.getState().bridge;
    if (!bridge?.cluster) throw toSpiritError("bridge is not initialized");
    const formed = await bridge.cluster.form(input);
    set({
      cluster: {
        configured: true,
        config: formed.config,
        health: null,
        health_error: null,
        // A re-formed cluster starts a new promotion history.
        promotion: null,
      },
      clusterStatus: "ready",
      clusterError: null,
    });
    // The pooled numbers come from Home's VOWP read, never from the form input.
    void get().loadCluster();
    return formed;
  },

  promoteClusterBackup: async (input) => {
    const bridge = useFleetStore.getState().bridge;
    if (!bridge?.cluster) throw toSpiritError("bridge is not initialized");
    const outcome = await bridge.cluster.promote(input);
    const current = get().cluster;
    if (current) {
      set({
        cluster: {
          ...current,
          health: null,
          health_error: null,
          promotion: outcome,
        },
      });
    }
    return outcome;
  },

  removeClusterWorker: async (hostId) => {
    const bridge = useFleetStore.getState().bridge;
    if (!bridge?.cluster) throw toSpiritError("bridge is not initialized");
    const config = await bridge.cluster.removeWorker(hostId);
    const current = get().cluster;
    if (current) {
      set({
        cluster: { ...current, config, health: null, health_error: null },
        clusterStatus: "ready",
      });
    }
    void get().loadCluster();
    return config;
  },
}));

/** Reset for tests and for a bridge swap. */
export function resetClusterStore(): void {
  useClusterStore.setState(INITIAL_CLUSTER_STATE);
}

export const selectCluster = (state: ClusterStoreState): ClusterStateView | null => state.cluster;
export const selectClusterStatus = (state: ClusterStoreState): StoreStatus => state.clusterStatus;
export const selectClusterError = (state: ClusterStoreState): SpiritError | null => state.clusterError;

export function useCluster(): ClusterStateView | null {
  return useClusterStore(selectCluster);
}

export function useClusterStatus(): StoreStatus {
  return useClusterStore(selectClusterStatus);
}

export function useClusterError(): SpiritError | null {
  return useClusterStore(selectClusterError);
}
