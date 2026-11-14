import { waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import type { ConnectionSnapshot, EngineStatus } from "../../bridge/wire";
import { engineSetupComplete } from "../../domain/engine";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";
import { engineWithoutModel, readyLocalGates } from "./helpers";

const NOW = "2026-11-14T12:00:00.000Z";
const HOST_A = "01MOCKHOST000000000000000001";
const HOST_B = "01MOCKHOST000000000000000002";

function connected(hostId: string): ConnectionSnapshot {
  return {
    state: "connected",
    host_id: hostId,
    since: NOW,
    latency: { last_ms: 42, ewma_ms: 42, p50_ms: 42, p95_ms: 42, samples: 1 },
    last_ok_at: NOW,
    error: null,
  };
}

describe("gate state lifecycle", () => {
  beforeEach(() => {
    resetFleetStore();
  });

  it("gates_are_dropped_when_the_connection_closes_or_the_host_changes", () => {
    useFleetStore.setState({
      connection: connected(HOST_A),
      gates: readyLocalGates(),
    });

    useFleetStore.getState().setConnection({
      ...connected(HOST_A),
      state: "disconnected",
      host_id: null,
      since: null,
      latency: null,
      last_ok_at: null,
    });
    expect(useFleetStore.getState().gates).toBeNull();

    useFleetStore.setState({
      connection: connected(HOST_A),
      gates: readyLocalGates(),
    });
    useFleetStore.getState().setConnection(connected(HOST_B));
    expect(useFleetStore.getState().gates).toBeNull();
  });

  it("connecting_a_host_probes_gates_without_opening_the_engine_view", async () => {
    const bridge = new MockBridge({ scenario: "engine-ready" });
    const probe = vi.spyOn(bridge.gates, "probe");
    await useFleetStore.getState().initialize(bridge);

    const hostId = useFleetStore.getState().hosts[0]?.id;
    expect(hostId).toBeTruthy();
    await useFleetStore.getState().connect(hostId ?? "");

    await waitFor(() => {
      expect(probe).toHaveBeenCalledTimes(1);
    });
    expect(useFleetStore.getState().gates).not.toBeNull();
  });

  it("engine_events_do_not_clear_the_persisted_setup_flag", () => {
    useFleetStore.setState({
      engine: {
        ...engineWithoutModel(),
        onboarding_complete: true,
      } as EngineStatus,
    });
    useFleetStore.getState().applyEvent({
      kind: "engine",
      revision: 1,
      engine: engineWithoutModel(),
    });
    expect(engineSetupComplete(useFleetStore.getState().engine)).toBe(true);
  });

  it("a_new_connection_does_not_reuse_a_gate_snapshot_from_another_host", () => {
    useFleetStore.setState({
      connection: connected(HOST_A),
      gates: readyLocalGates(),
    });

    useFleetStore.getState().setConnection({
      ...connected(HOST_B),
      state: "failed",
      error: {
        code: "ssh_auth",
        message: "SSH authentication failed",
        hint: null,
        retryable: true,
      },
    });
    expect(useFleetStore.getState().gates).toBeNull();
  });
});
