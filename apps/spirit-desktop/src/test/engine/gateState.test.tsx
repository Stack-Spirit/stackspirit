import { act, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import type { ConnectionSnapshot } from "../../bridge/wire";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";
import { engineWith, readyLocalGates, renderEngine } from "./helpers";

const NOW = "2026-11-14T12:00:00.000Z";
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

function disconnected(): ConnectionSnapshot {
  return {
    state: "disconnected",
    host_id: null,
    since: null,
    latency: null,
    last_ok_at: null,
    error: null,
  };
}

function stale(hostId: string): ConnectionSnapshot {
  return {
    state: "stale",
    host_id: hostId,
    since: NOW,
    latency: { last_ms: 900, ewma_ms: 900, p50_ms: 900, p95_ms: 900, samples: 1 },
    last_ok_at: NOW,
    error: {
      code: "stale",
      message: "last poll timed out",
      hint: null,
      retryable: true,
    },
  };
}

describe("gate state lifecycle", () => {
  beforeEach(() => {
    resetFleetStore();
  });

  it("manual_reprobe_is_disabled_while_the_connection_is_stale", async () => {
    const user = userEvent.setup();
    const bridge = new MockBridge({ scenario: "engine-ready" });
    const probe = vi.spyOn(bridge.gates, "probe");
    renderEngine(engineWith(), { bridge, gates: readyLocalGates() });

    await act(async () => {
      useFleetStore.getState().setConnection(stale(HOST_B));
    });

    const reprobe = screen.getByRole("button", { name: "Re-probe" });
    const probeGates = screen.getByRole("button", { name: "Probe gates" });
    expect(reprobe).toBeDisabled();
    expect(probeGates).toBeDisabled();
    expect(
      screen.getByText(
        "The VOWP connection is stale. Reconnect before probing capability gates.",
      ),
    ).toBeInTheDocument();
    await user.click(reprobe);
    expect(probe).not.toHaveBeenCalled();
  });

  it("gates_are_dropped_when_the_connection_closes", async () => {
    renderEngine(engineWith(), { gates: readyLocalGates() });
    expect(screen.getByTestId("gate-card-local")).toBeInTheDocument();

    await act(async () => {
      useFleetStore.getState().setConnection(disconnected());
    });

    expect(screen.queryByTestId("gate-card-local")).not.toBeInTheDocument();
    expect(screen.getByText("Capability gates not probed")).toBeInTheDocument();
  });

  it("a_new_connection_does_not_reuse_a_gate_snapshot_from_another_host", async () => {
    const bridge = new MockBridge({ scenario: "engine-ready" });
    // Hold the probe open: the old host's snapshot must disappear before any result.
    const probe = vi
      .spyOn(bridge.gates, "probe")
      .mockImplementation(() => new Promise(() => undefined));
    renderEngine(engineWith(), { bridge, gates: readyLocalGates() });

    await act(async () => {
      useFleetStore.getState().setConnection(connected(HOST_B));
    });

    await waitFor(() => {
      expect(probe).toHaveBeenCalledTimes(1);
    });
    expect(screen.queryByTestId("gate-card-local")).not.toBeInTheDocument();
    expect(screen.getByText("Capability gates not probed")).toBeInTheDocument();
  });

  it("a_failed_probe_never_shows_the_previous_hosts_ready_snapshot", async () => {
    const bridge = new MockBridge({ scenario: "engine-ready" });
    vi.spyOn(bridge.gates, "probe").mockRejectedValue(new Error("gate probe failed"));
    renderEngine(engineWith(), { bridge, gates: readyLocalGates() });

    await act(async () => {
      useFleetStore.getState().setConnection(connected(HOST_B));
    });

    await waitFor(() => {
      expect(useFleetStore.getState().lastError).not.toBeNull();
    });
    expect(screen.queryByTestId("gate-card-local")).not.toBeInTheDocument();
    expect(screen.getByText("Capability gates not probed")).toBeInTheDocument();
  });

  it("connecting_a_new_host_probes_gates_and_scopes_the_result", async () => {
    const bridge = new MockBridge({ scenario: "engine-ready" });
    const probe = vi.spyOn(bridge.gates, "probe");
    renderEngine(engineWith(), { bridge, gates: readyLocalGates() });

    await act(async () => {
      useFleetStore.getState().setConnection(connected(HOST_B));
    });

    await waitFor(() => {
      expect(probe).toHaveBeenCalledTimes(1);
    });
    await waitFor(() => {
      expect(screen.getByTestId("gate-card-local")).toBeInTheDocument();
    });
    expect(useFleetStore.getState().gates?.probed_at).toBe(NOW);
  });

  it("manual_reprobe_is_ignored_while_disconnected", async () => {
    const user = userEvent.setup();
    const bridge = new MockBridge({ scenario: "engine-ready" });
    const probe = vi.spyOn(bridge.gates, "probe");
    renderEngine(engineWith(), { bridge, gates: readyLocalGates() });

    await act(async () => {
      useFleetStore.getState().setConnection(disconnected());
    });
    const reprobe = screen.getByRole("button", { name: "Re-probe" });
    const probeGates = screen.getByRole("button", { name: "Probe gates" });
    expect(reprobe).toBeDisabled();
    expect(probeGates).toBeDisabled();
    expect(
      screen.getByText("Connect to a VOWP home before probing capability gates."),
    ).toBeInTheDocument();
    await user.click(reprobe);
    expect(probe).not.toHaveBeenCalled();
  });
});
