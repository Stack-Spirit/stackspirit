import { describe, expect, it } from "vitest";

import { MockBridge } from "../bridge/MockBridge";
import {
  resetFleetStore,
  selectMutationsAllowed,
  STALE_BACKOFF_MS,
  useFleetStore,
} from "../state/fleetStore";

async function connected(scenario: "fresh" | "stale-after-3" = "fresh") {
  resetFleetStore();
  const bridge = new MockBridge(scenario);
  await useFleetStore.getState().initialize(bridge);
  const host = (await bridge.hosts.list())[0];
  if (!host) throw new Error("fixture host missing");
  await useFleetStore.getState().connect(host.id);
  return { bridge, host };
}

describe("fleetStore", () => {
  it("ignores an event with an older revision", async () => {
    const { host } = await connected("fresh");
    const before = useFleetStore.getState();
    expect(before.connection.state).toBe("connected");
    const oldRevision = Math.max(0, before.revision - 1);
    useFleetStore.getState().applyEvent({
      kind: "connection",
      revision: oldRevision,
      connection: {
        state: "disconnected",
        host_id: null,
        since: null,
        latency: null,
        last_ok_at: null,
        error: null,
      },
    });
    const after = useFleetStore.getState();
    expect(after.revision).toBe(before.revision);
    expect(after.connection.state).toBe("connected");
    expect(after.connection.host_id).toBe(host.id);
  });

  it("dims the view, disables writes and starts a backoff after three failed polls", async () => {
    const { bridge } = await connected("stale-after-3");
    const fleetBefore = useFleetStore.getState().fleet;
    expect(fleetBefore).not.toBeNull();
    const before = Date.now();
    for (let attempt = 0; attempt < 3; attempt += 1) {
      await useFleetStore.getState().refresh();
    }
    const state = useFleetStore.getState();
    expect(state.connection.state).toBe("stale");
    expect(state.fleet?.stale).toBe(true);
    expect(state.lastError?.code).toBe("remote_timeout");
    expect(selectMutationsAllowed(state)).toBe(false);
    expect(state.nextRetryAt).not.toBeNull();
    expect(state.nextRetryAt ?? 0).toBeGreaterThanOrEqual(before + STALE_BACKOFF_MS);
    expect(await bridge.fleet.rawDocument()).not.toBeNull();
  });

  it("clears the stale backoff when a refresh succeeds", async () => {
    const { bridge } = await connected("stale-after-3");
    for (let attempt = 0; attempt < 3; attempt += 1) {
      await useFleetStore.getState().refresh();
    }
    expect(useFleetStore.getState().connection.state).toBe("stale");
    expect(useFleetStore.getState().nextRetryAt).not.toBeNull();

    bridge.restoreBackend();
    await useFleetStore.getState().refresh();

    const state = useFleetStore.getState();
    expect(state.connection.state).toBe("connected");
    expect(state.fleet?.stale).toBe(false);
    expect(state.lastError).toBeNull();
    expect(state.nextRetryAt).toBeNull();
    expect(selectMutationsAllowed(state)).toBe(true);
  });

  it("keeps the last fleet view dimmed after disconnect", async () => {
    const { host } = await connected("fresh");
    const fleet = useFleetStore.getState().fleet;
    await useFleetStore.getState().disconnect(host.id);
    const state = useFleetStore.getState();
    expect(state.connection.state).toBe("disconnected");
    expect(state.fleet).not.toBeNull();
    expect(state.fleet?.revision).toBe(fleet?.revision);
    expect(selectMutationsAllowed(state)).toBe(false);
  });

  it("marks preview data and never claims mutations are allowed while disconnected", async () => {
    const { bridge } = await connected("fresh");
    expect(bridge.preview).toBe(true);
    expect(useFleetStore.getState().preview).toBe(true);
    expect(selectMutationsAllowed(useFleetStore.getState())).toBe(true);
  });
});
