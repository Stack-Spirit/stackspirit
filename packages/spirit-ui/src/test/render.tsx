import { render, screen, waitFor } from "@testing-library/react";
import { expect } from "vitest";

import { MockBridge, type MockScenario } from "../bridge/MockBridge";
import type {
  ConnectionSnapshot,
  EngineStatus,
  FleetView,
  GateSnapshot,
  RawDocument,
} from "../bridge/wire";
import {
  createInitialFleetState,
  resetFleetStore,
  useFleetStore,
} from "../state/fleetStore";
import { HostManager } from "../views/hosts/HostManager";

export async function renderHostManager(scenario: MockScenario = "fresh") {
  resetFleetStore();
  const bridge = new MockBridge(scenario);
  await useFleetStore.getState().initialize(bridge);
  const result = render(<HostManager />);
  await waitFor(() => {
    expect(screen.getByTestId("host-manager")).toBeInTheDocument();
  });
  return { bridge, ...result };
}

export interface SeedOptions {
  bridge?: MockBridge;
  connection?: ConnectionSnapshot;
  raw?: RawDocument | null;
  engine?: EngineStatus | null;
  gates?: GateSnapshot | null;
}

/**
 * Seeds the store with a deterministic fleet view. This is how the D3 scenario suite
 * exercises view models without teaching components to parse raw VOWP JSON.
 */
export function seedStore(fleet: FleetView, options: SeedOptions = {}) {
  const bridge = options.bridge ?? new MockBridge("fresh");
  const connection: ConnectionSnapshot = options.connection ?? {
    state: "connected",
    host_id: fleet.host_id,
    since: fleet.fetched_at,
    latency: fleet.latency,
    last_ok_at: fleet.fetched_at,
    error: null,
  };
  useFleetStore.setState({
    ...createInitialFleetState(),
    bridge,
    preview: bridge.preview,
    status: "ready",
    revision: fleet.revision,
    connection,
    fleet,
    capacity: fleet.capacity,
    rawDocument: options.raw ?? null,
    engine: options.engine ?? null,
    gates: options.gates ?? null,
  });
  return bridge;
}

export function seedConnectedStale(fleet: FleetView, options: SeedOptions = {}) {
  return seedStore(fleet, {
    ...options,
    connection: {
      state: "stale",
      host_id: fleet.host_id,
      since: fleet.fetched_at,
      latency: fleet.latency,
      last_ok_at: fleet.fetched_at,
      error: {
        code: "remote_timeout",
        message: "the remote command did not finish within 20s",
        hint: "Retrying with backoff.",
        retryable: true,
      },
    },
  });
}
