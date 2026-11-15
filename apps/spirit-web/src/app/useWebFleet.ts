import { useCallback, useEffect, useRef, useState } from "react";

import type {
  ConnectionSnapshot,
  SpiritBridge,
  SpiritError,
  WebBridgeStatus,
  WebFleetView,
} from "@stackspirit/ui";

import { asSpiritError, webError } from "../bridge/errors";

export interface WebFleetState {
  snapshot: WebFleetView | null;
  connection: ConnectionSnapshot;
  status: WebBridgeStatus | null;
  error: SpiritError | null;
  loading: boolean;
  refreshing: boolean;
}

const INITIAL_CONNECTION: ConnectionSnapshot = {
  state: "connecting",
  host_id: "daemon",
  since: null,
  latency: null,
  last_ok_at: null,
  error: null,
};

const INITIAL_STATE: WebFleetState = {
  snapshot: null,
  connection: INITIAL_CONNECTION,
  status: null,
  error: null,
  loading: true,
  refreshing: false,
};

/**
 * Subscribes the Web shell to the honest snapshot stream. The bridge owns reconnects and
 * polling; this hook only mirrors its state into React and disposes it on unmount.
 */
export function useWebFleet(bridge: SpiritBridge): WebFleetState & {
  refresh: () => Promise<void>;
  clearError: () => void;
} {
  const [state, setState] = useState<WebFleetState>(INITIAL_STATE);
  const disposed = useRef(false);

  useEffect(() => {
    disposed.current = false;
    setState(INITIAL_STATE);
    const cleanups: Array<() => void> = [];

    const unsubscribeConnection = bridge.connection.subscribe((event) => {
      if (disposed.current) return;
      if (event.kind === "connection") {
        setState((current) => ({ ...current, connection: event.connection }));
      } else if (event.kind === "error") {
        setState((current) => ({
          ...current,
          error: asSpiritError(event.error),
          loading: false,
        }));
      }
    });
    cleanups.push(unsubscribeConnection);

    if (bridge.fleet.watch) {
      cleanups.push(
        bridge.fleet.watch((snapshot) => {
          if (disposed.current) return;
          setState((current) => ({
            ...current,
            snapshot,
            loading: false,
          }));
        }),
      );
    }

    const read = async () => {
      try {
        if (!bridge.fleet.snapshot) {
          throw webError(
            "daemon_incompatible",
            "This bridge does not expose the Web snapshot.",
          );
        }
        const snapshot = await bridge.fleet.snapshot(false);
        if (disposed.current) return;
        setState((current) => ({
          ...current,
          snapshot: snapshot ?? current.snapshot,
          loading: false,
        }));
      } catch (error) {
        if (disposed.current) return;
        setState((current) => ({
          ...current,
          error: asSpiritError(error),
          loading: false,
        }));
      }
    };
    void read();

    const timer = window.setInterval(() => {
      if (disposed.current) return;
      const status = bridge.fleet.webStatus?.() ?? null;
      setState((current) => ({ ...current, status }));
    }, 1000);

    return () => {
      disposed.current = true;
      window.clearInterval(timer);
      for (const cleanup of cleanups) cleanup();
      bridge.dispose?.();
    };
  }, [bridge]);

  const refresh = useCallback(async () => {
    setState((current) => ({ ...current, refreshing: true, error: null }));
    try {
      if (!bridge.fleet.snapshot) {
        throw webError(
          "daemon_incompatible",
          "This bridge does not expose the Web snapshot.",
        );
      }
      const snapshot = await bridge.fleet.snapshot(true);
      setState((current) => ({
        ...current,
        snapshot: snapshot ?? current.snapshot,
        refreshing: false,
        loading: false,
      }));
    } catch (error) {
      setState((current) => ({
        ...current,
        error: asSpiritError(error),
        refreshing: false,
        loading: false,
      }));
    }
  }, [bridge]);

  const clearError = useCallback(() => {
    setState((current) => ({ ...current, error: null }));
  }, []);

  return { ...state, refresh, clearError };
}
