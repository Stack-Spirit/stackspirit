import { describe, expect, it } from "vitest";

import { MockBridge } from "../bridge/MockBridge";
import type { HostDraft, NewHostProfile } from "../bridge/wire";

const draft: HostDraft = {
  label: "test-vps",
  ssh: { user: "owner", host: "203.0.113.20", port: 22 },
  identity_file: null,
  vowp_home: null,
  default_project: null,
};

const confirmed: NewHostProfile = {
  ...draft,
  host_key: {
    algorithm: "ssh-ed25519",
    sha256: "SHA256:testFingerprint000000000000000000000000000000",
    pinned_at: "2026-11-14T12:00:00.000Z",
  },
};

describe("MockBridge", () => {
  it("rejects an incomplete host profile", async () => {
    const bridge = new MockBridge("empty");
    await expect(bridge.hosts.add({ ...confirmed, label: "" })).rejects.toMatchObject({
      code: "invalid_input",
    });
    await expect(
      bridge.hosts.add({ ...confirmed, ssh: { ...draft.ssh, port: 0 } }),
    ).rejects.toMatchObject({ code: "invalid_input" });
    await expect(
      bridge.hosts.add({ ...confirmed, ssh: { ...draft.ssh, host: "https://vps.example" } }),
    ).rejects.toMatchObject({ code: "invalid_input" });
  });

  it("saves a valid host and scans a deterministic fingerprint", async () => {
    const bridge = new MockBridge("empty");
    const host = await bridge.hosts.add(confirmed);
    expect(host.label).toBe("test-vps");
    expect(host.host_key.sha256).toBe(confirmed.host_key.sha256);
    const scan = await bridge.hosts.scanFingerprint(draft);
    expect(scan.keys[0]?.preferred).toBe(true);
    expect(scan.keys[0]?.algorithm).toBe("ssh-ed25519");
  });

  it("reports the failing probe stage for a bad SSH backend", async () => {
    const bridge = new MockBridge("probe-failure");
    const probe = await bridge.hosts.probe(draft, confirmed.host_key);
    expect(probe.reachable).toBe(false);
    expect(probe.stages.find((stage) => stage.stage === "ssh")).toMatchObject({
      ok: false,
    });
    expect(probe.stages.find((stage) => stage.stage === "snapshot")?.ok).toBe(false);
  });

  it("emits fleet then connection with increasing revisions on connect", async () => {
    const bridge = new MockBridge("fresh");
    const host = (await bridge.hosts.list())[0];
    if (!host) throw new Error("fixture host missing");
    const events: string[] = [];
    const revisions: number[] = [];
    const unsubscribe = bridge.connection.subscribe((event) => {
      events.push(event.kind);
      revisions.push(event.revision);
    });
    const connection = await bridge.connection.connect(host.id);
    expect(connection.state).toBe("connected");
    expect(events.slice(-2)).toEqual(["fleet", "connection"]);
    expect(revisions).toEqual([...revisions].sort((left, right) => left - right));
    unsubscribe();
    await bridge.connection.disconnect(host.id);
    expect((await bridge.connection.status()).state).toBe("disconnected");
  });

  it("carries parse warnings in the raw document", async () => {
    const bridge = new MockBridge("warnings");
    const host = (await bridge.hosts.list())[0];
    if (!host) throw new Error("fixture host missing");
    await bridge.connection.connect(host.id);
    const raw = await bridge.fleet.rawDocument();
    expect(raw?.status_warnings.map((warning) => warning.code)).toContain("unknown_enum");
    expect(raw?.capacity_warnings.map((warning) => warning.code)).toContain("truncated");
  });
});
