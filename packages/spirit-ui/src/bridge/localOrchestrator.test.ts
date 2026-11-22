import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import { MockBridge, type MockScenario } from "./MockBridge";

const SESSION = "01JCF5WQ8ZK3M2V7N4P6R8T0AE";

async function probedLocal(scenario: MockScenario): Promise<MockBridge> {
  const bridge = new MockBridge(scenario);
  const host = (await bridge.hosts.list())[0];
  if (!host) throw new Error(`${scenario} has no host`);
  await bridge.connection.connect(host.id);
  await bridge.gates.probe();
  return bridge;
}

describe("D6 local-orchestrator bridge", () => {
  it("keeps every start action gated while Waiting but leaves reads available", async () => {
    const bridge = await probedLocal("gate-d6-waiting");
    const gates = await bridge.gates.get();
    expect(gates.features.local_orchestrator_actions).toBe(false);
    expect(gates.local_orchestrator.state).toBe("waiting_for_vowp_phase3");
    expect(gates.local_agent.state).toBe("not_enrolled");

    const overview = await bridge.localOrchestrators.overview();
    expect(overview.agent.state).toBe("not_enrolled");
    expect(overview.surfaces).toEqual([]);
    expect(overview.orchestrators).toEqual([]);

    await expect(
      bridge.localOrchestrators.surfacePair({
        session: SESSION,
        node: "mac-studio",
        kind: "opencode",
        app: "opencode",
      }),
    ).rejects.toMatchObject({
      code: "gate",
      message: expect.stringContaining("local agent connection"),
    });
    await expect(
      bridge.localOrchestrators.orchestratorNew({
        node: "mac-studio",
        app: "opencode",
        project: "vowp",
      }),
    ).rejects.toMatchObject({ code: "gate" });
    await expect(
      bridge.localOrchestrators.surfaceRotate({ surface: "01J9Z00000000000000000000A" }),
    ).rejects.toMatchObject({ code: "gate" });

    // Explicit cleanup is offered even when the gate is waiting; it may fail because
    // the item does not exist, but it must not fail on the gate.
    await expect(
      bridge.localOrchestrators.surfaceRevoke("01J9Z00000000000000000000A"),
    ).rejects.toMatchObject({ code: "not_found" });
    await expect(
      bridge.localOrchestrators.orchestratorStop("vowp-local"),
    ).rejects.toMatchObject({ code: "not_found" });
    expect(bridge.recordedLocalCalls()).toEqual([
      { action: "surface_revoke", name: "01J9Z00000000000000000000A" },
      { action: "orchestrator_stop", name: "vowp-local" },
    ]);
  });

  it("names an incompatible protocol and keeps the action disabled", async () => {
    const bridge = await probedLocal("gate-d6-incompatible");
    const gates = await bridge.gates.get();
    expect(gates.features.local_orchestrator_actions).toBe(false);
    expect(gates.local_orchestrator.state).toBe("incompatible");
    expect(gates.local_agent.state).toBe("incompatible");

    await expect(
      bridge.localOrchestrators.surfacePair({
        session: SESSION,
        node: "mac-studio",
        kind: "opencode",
      }),
    ).rejects.toMatchObject({
      code: "gate",
      message: expect.stringContaining("vowp-agent/0"),
    });
  });

  it("creates, pairs, rotates, revokes, stops and resumes when Ready and connected", async () => {
    const bridge = await probedLocal("gate-d6-ready");
    const gates = await bridge.gates.get();
    expect(gates.features.local_orchestrator_actions).toBe(true);
    expect(gates.local_agent.state).toBe("connected");

    const overview = await bridge.localOrchestrators.overview();
    expect(overview.node?.name).toBe("mac-studio");
    expect(overview.node?.presence).toBe("online");
    expect(overview.agent.protocol).toBe("vowp-agent/1");
    expect(overview.paths.map((path) => path.path_ref)).toEqual(["project:vowp"]);

    const running = overview.orchestrators.find((row) => row.name === "vowp-local");
    expect(running).toMatchObject({
      node: "mac-studio",
      protocol: "vowp-agent/1",
      lease_holder: "StackSpirit Desktop",
      session_state: "running",
    });
    expect(
      overview.orchestrators.find((row) => row.name === "chat-local")?.session_state,
    ).toBe("stopped");
    expect(
      overview.orchestrators.find((row) => row.name === "docs-local")?.session_state,
    ).toBe("degraded");
    expect(
      overview.surfaces.find((row) => row.surface_id === "01J9Z00000000000000000000B")
        ?.session_state,
    ).toBe("revoked");
    expect(
      overview.surfaces.find((row) => row.session_state === "superseded")?.can_write,
    ).toBe(false);

    const created = await bridge.localOrchestrators.orchestratorNew({
      node: "mac-studio",
      app: "opencode",
      project: "vowp",
      name: "docs-local-2",
      force: false,
    });
    expect(created).toMatchObject({ name: "docs-local-2", state: "running" });
    await expect(
      bridge.localOrchestrators.orchestratorNew({
        node: "mac-studio",
        app: "opencode",
        project: "vowp",
        name: "docs-local-2",
      }),
    ).rejects.toMatchObject({ code: "conflict" });

    const pairing = await bridge.localOrchestrators.surfacePair({
      session: SESSION,
      node: "mac-studio",
      kind: "opencode",
      app: "opencode",
    });
    expect(pairing.pairing.session).toBe(SESSION);
    expect(pairing.pairing.command).toContain("vowp agent pair --code-file -");
    expect(pairing.pairing.command).not.toContain(pairing.code);
    expect(pairing.code).not.toHaveLength(0);

    const paired = await bridge.localOrchestrators.agentPair({
      code: pairing.code,
      surface: pairing.pairing.surface,
      session: pairing.pairing.session,
      app: "opencode",
    });
    expect(paired.surface_id).toBe(pairing.pairing.surface);
    const afterPair = await bridge.localOrchestrators.overview();
    expect(
      afterPair.surfaces.find((row) => row.surface_id === pairing.pairing.surface),
    ).toMatchObject({ state: "attached", session_state: "active", can_write: true });

    const rotation = await bridge.localOrchestrators.surfaceRotate({
      surface: "01J9Z00000000000000000000A",
      node: "mac-studio",
      wait: "30s",
    });
    expect(rotation.pending).toBe(true);
    expect(
      (await bridge.localOrchestrators.overview()).surfaces.find(
        (row) => row.surface_id === "01J9Z00000000000000000000A",
      )?.rotation_pending,
    ).toBe(true);

    const revoked = await bridge.localOrchestrators.surfaceRevoke(
      "01J9Z00000000000000000000A",
    );
    expect(revoked.status).toBe("revoked");
    const afterRevoke = (await bridge.localOrchestrators.overview()).surfaces.find(
      (row) => row.surface_id === "01J9Z00000000000000000000A",
    );
    expect(afterRevoke).toMatchObject({
      state: "revoked",
      session_state: "revoked",
      can_write: false,
    });

    expect((await bridge.localOrchestrators.orchestratorStop("vowp-local")).state).toBe(
      "stopped",
    );
    expect(
      (await bridge.localOrchestrators.orchestratorResume("vowp-local", "vowp", "mac-studio"))
        .state,
    ).toBe("running");

    const added = await bridge.localOrchestrators.agentPathAdd({
      projectRoot: "/tmp/opencode/example-other",
      name: "other",
      allow: ["src"],
    });
    expect(added.path_ref).toBe("project:other");
    await bridge.localOrchestrators.agentPathRemove("other");
    expect((await bridge.localOrchestrators.overview()).paths.map((row) => row.project)).toEqual([
      "vowp",
    ]);

    const serialized = JSON.stringify(await bridge.localOrchestrators.overview());
    expect(serialized).not.toContain("/tmp/");
    expect(serialized).not.toContain("pairing-code-placeholder");
    expect(serialized).not.toContain("token_path");
    expect(bridge.recordedLocalCalls().map((call) => call.action)).toEqual([
      "orchestrator_new",
      "orchestrator_new",
      "surface_pair",
      "agent_pair",
      "surface_rotate",
      "surface_revoke",
      "orchestrator_stop",
      "orchestrator_resume",
      "agent_path_add",
      "agent_path_remove",
    ]);
  });

  it("rejects invalid input before recording an action", async () => {
    const bridge = await probedLocal("gate-d6-ready");
    await expect(
      bridge.localOrchestrators.surfacePair({
        session: "not-a-ulid",
        node: "mac-studio",
        kind: "opencode",
      }),
    ).rejects.toMatchObject({ code: "invalid_input" });
    await expect(
      bridge.localOrchestrators.agentPair({
        code: "",
        surface: "01J9Z00000000000000000000A",
        session: SESSION,
      }),
    ).rejects.toMatchObject({ code: "invalid_input" });
    await expect(
      bridge.localOrchestrators.orchestratorNew({
        node: "mac-studio",
        app: "browser" as "opencode",
      }),
    ).rejects.toMatchObject({ code: "invalid_input" });
    expect(bridge.recordedLocalCalls()).toEqual([]);
  });

  it("shows a revoked local agent as read-only and refuses every start action", async () => {
    const bridge = await probedLocal("gate-d6-revoked");
    const gates = await bridge.gates.get();
    expect(gates.features.local_orchestrator_actions).toBe(false);
    expect(gates.local_agent.state).toBe("revoked");
    expect(gates.local_orchestrator.state).toBe("waiting_for_vowp_phase3");

    const overview = await bridge.localOrchestrators.overview();
    expect(overview.agent.state).toBe("revoked");
    expect(overview.node?.presence).toBe("revoked");
    for (const surface of overview.surfaces) {
      expect(surface.can_write).toBe(false);
    }
    expect(
      overview.surfaces.find((row) => row.surface_id === "01J9Z00000000000000000000A")
        ?.session_state,
    ).toBe("revoked");
    expect(
      overview.orchestrators.find((row) => row.name === "vowp-local")?.session_state,
    ).toBe("revoked");

    await expect(
      bridge.localOrchestrators.surfacePair({
        session: SESSION,
        node: "mac-studio",
        kind: "opencode",
      }),
    ).rejects.toMatchObject({
      code: "gate",
      message: expect.stringContaining("local agent connection"),
    });
  });

  it("routes the desktop adapter and Tauri host to every D6 command", () => {
    const here = dirname(fileURLToPath(import.meta.url));
    const bridgeSource = readFileSync(
      resolve(here, "../../../../apps/spirit-desktop/src/bridge/TauriBridge.ts"),
      "utf8",
    );
    for (const command of [
      "local_orchestrator_overview",
      "local_agent_status",
      "local_agent_pair",
      "local_agent_paths",
      "local_agent_path_add",
      "local_agent_path_remove",
      "local_surface_pair",
      "local_surface_list",
      "local_surface_rotate",
      "local_surface_revoke",
      "local_orchestrator_new",
      "local_orchestrator_list",
      "local_orchestrator_stop",
      "local_orchestrator_resume",
    ]) {
      expect(bridgeSource).toContain(`("${command}"`);
    }
    expect(bridgeSource).not.toContain("pairingCode");
    expect(bridgeSource).not.toContain("allowedPaths");

    const libSource = readFileSync(
      resolve(here, "../../../../apps/spirit-desktop/src-tauri/src/lib.rs"),
      "utf8",
    );
    for (const command of [
      "local_orchestrator_overview",
      "local_agent_pair",
      "local_surface_pair",
      "local_surface_rotate",
      "local_surface_revoke",
      "local_orchestrator_new",
      "local_orchestrator_stop",
      "local_orchestrator_resume",
    ]) {
      expect(libSource).toContain(`commands::local_orchestrator::${command}`);
    }
  });
});

describe("projects.add bridge", () => {
  it("records the call and returns an outcome without a path", async () => {
    const bridge = await probedLocal("gate-d6-waiting");
    const input = {
      name: "demo-app",
      source: { kind: "git" as const, url: "https://example.invalid/org/demo-app.git" },
    };
    const outcome = await bridge.projects.add(input);
    expect(bridge.recordedProjectAdds()).toEqual([input]);
    expect(outcome.project).toBe("demo-app");
    expect(Object.keys(JSON.parse(JSON.stringify(outcome)))).not.toContain("path");
  });
});
