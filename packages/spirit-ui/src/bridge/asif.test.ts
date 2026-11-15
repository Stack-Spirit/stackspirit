import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import { MockBridge, type MockScenario } from "./MockBridge";
import type { AsifExportOptions, AsifImportOptions } from "./wire";

const SESSION = "01JCF5WQ8ZK3M2V7N4P6R8T0AB";

async function probedBridge(scenario: MockScenario): Promise<MockBridge> {
  const bridge = new MockBridge(scenario);
  const host = (await bridge.hosts.list())[0];
  if (!host) throw new Error(`${scenario} has no host`);
  await bridge.connection.connect(host.id);
  await bridge.gates.probe();
  return bridge;
}

describe("ASIF sessions bridge", () => {
  it("lists sessions from the last snapshot without a new read", async () => {
    const bridge = new MockBridge("gate-asif-ready");
    expect(await bridge.sessions.list()).toEqual([]);

    const host = (await bridge.hosts.list())[0];
    if (!host) throw new Error("fixture host missing");
    await bridge.connection.connect(host.id);
    const first = await bridge.sessions.list();
    expect(first.map((session) => session.state)).toContain("idle");
    expect(await bridge.sessions.list()).toEqual(first);
    expect(bridge.preview).toBe(true);
  });

  it("shows the D5 flag but no action without a ready gate", async () => {
    const bridge = await probedBridge("gate-asif-waiting");
    const gates = await bridge.gates.get();
    expect(gates.features.asif_actions).toBe(true);
    expect(gates.features.local_orchestrator_actions).toBe(false);
    expect(gates.asif.state).toBe("waiting_for_vowp_asif");

    await expect(bridge.sessions.exportAsif(SESSION, {})).rejects.toMatchObject({
      code: "gate",
      message: expect.stringContaining("v4-asif-core"),
    });
    await expect(bridge.sessions.importAsif({})).rejects.toMatchObject({ code: "gate" });
    expect(bridge.recordedAsifCalls()).toHaveLength(0);
  });

  it("enables export but refuses import when only export is proven", async () => {
    const bridge = await probedBridge("gate-asif-ready-export-only");
    const result = await bridge.sessions.exportAsif(SESSION, {});
    expect(result).toMatchObject({
      session_id: SESSION,
      file_name: `${SESSION}.asif.zip`,
      bytes: 18342,
      loss_count: 2,
    });

    await expect(bridge.sessions.importAsif({})).rejects.toMatchObject({
      code: "gate",
      message: "ASIF import capability is not proven.",
    });
  });

  it("returns export losses and unsupported operations without a path", async () => {
    const bridge = await probedBridge("asif-export-losses");
    const result = await bridge.sessions.exportAsif(SESSION, { allowLoss: true });
    expect(result?.losses.map((loss) => loss.kind)).toEqual([
      "cwd-omitted",
      "contexts-reconstructed",
    ]);
    expect(result?.unsupported).toContain("vowp.jobs");
    const serialized = JSON.stringify(result);
    expect(serialized).not.toContain("/srv");
    expect(serialized).not.toContain("/tmp");
    expect(serialized).not.toContain("path");
    expect(bridge.recordedAsifCalls()[0]).toMatchObject({
      kind: "export",
      session_id: SESSION,
      strict: false,
      allow_loss: true,
    });
  });

  it("returns an idle imported session with warnings and losses", async () => {
    const bridge = await probedBridge("asif-import-warnings");
    const result = await bridge.sessions.importAsif({ branch: null });
    expect(result).toMatchObject({
      session_id: "01JCF5WQ8ZK3M2V7N4P6R8T0AF",
      loss_count: 3,
      state: "idle",
    });
    expect(result?.warnings).toEqual(["project example-source left null"]);
    expect(result?.losses.map((loss) => loss.kind)).toContain("session-id-changed");
    expect(bridge.recordedAsifCalls()).toHaveLength(1);
    expect(bridge.recordedActions()).toEqual([]);

    const serialized = JSON.stringify(result);
    expect(serialized).not.toContain("/srv/vowp");
    expect(serialized).not.toContain('"cwd":');
  });

  it("surfaces a strict refusal and an explicit allow-loss retry", async () => {
    const bridge = await probedBridge("asif-strict-refusal");
    await expect(
      bridge.sessions.exportAsif(SESSION, { strict: true }),
    ).rejects.toMatchObject({
      code: "refused",
      message: expect.stringContaining("blocking or integrity loss"),
    });
    expect(await bridge.sessions.exportAsif(SESSION, { allowLoss: true })).not.toBeNull();
    expect(bridge.recordedAsifCalls().map((call) => call.strict)).toEqual([true, false]);
  });

  it("surfaces a conflict once and never retries with a mutated package", async () => {
    const bridge = await probedBridge("asif-conflict");
    await expect(bridge.sessions.importAsif({})).rejects.toMatchObject({
      code: "conflict",
      message: expect.stringContaining("event ids"),
    });
    expect(bridge.recordedAsifCalls().filter((call) => call.kind === "import")).toHaveLength(
      1,
    );
  });

  it("rejects a hidden force switch and a remote cwd", async () => {
    const bridge = await probedBridge("gate-asif-ready");
    await expect(
      bridge.sessions.exportAsif(SESSION, {
        strict: true,
        allowLoss: true,
      }),
    ).rejects.toMatchObject({ code: "invalid_input" });
    await expect(
      bridge.sessions.exportAsif(SESSION, {
        allowUnsupported: true,
      } as unknown as AsifExportOptions),
    ).rejects.toMatchObject({ code: "invalid_input" });
    await expect(
      bridge.sessions.importAsif({ cwd: "/srv/vowp" } as unknown as AsifImportOptions),
    ).rejects.toMatchObject({ code: "invalid_input" });
    expect(bridge.recordedAsifCalls()).toHaveLength(0);
  });

  it("routes the desktop adapter to the ASIF commands without remote fields", () => {
    const here = dirname(fileURLToPath(import.meta.url));
    const source = readFileSync(
      resolve(here, "../../../../apps/spirit-desktop/src/bridge/TauriBridge.ts"),
      "utf8",
    );
    expect(source).toContain('call("asif_export", { sessionId, options })');
    expect(source).toContain('call("asif_import", { options })');
    expect(source).toContain('call("gates_get")');
    expect(source).toContain('call("probe_gates")');
    expect(source).not.toContain("allowUnsupported");
    expect(source).not.toContain("cwd");
  });
});
