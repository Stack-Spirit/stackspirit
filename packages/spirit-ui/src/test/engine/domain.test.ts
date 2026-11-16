import { describe, expect, it } from "vitest";

import type { EngineStatus, GateSnapshot } from "../../bridge/wire";
import {
  GATE_NOT_IMPLEMENTED_ASIF,
  GATE_NOT_IMPLEMENTED_LOCAL,
  PHASE3_MILESTONES,
  engineLogs,
  engineProviders,
  engineSetupComplete,
  gateActionEnabled,
  gateActionReason,
  gateMissingMilestones,
  gatesProbed,
  includedModels,
  localAgentLabel,
  modelContextLabel,
  modelNotes,
  modelProviderLabel,
  needsEngineOnboarding,
  providerLabel,
  providerOptions,
} from "../../domain/engine";
import { engineWith, engineWithoutModel, readyAsifGates, readyLocalGates } from "./helpers";

describe("engine domain helpers", () => {
  it("provider labels fall back to the provider id", () => {
    expect(providerLabel("deepseek")).toBe("DeepSeek");
    expect(providerLabel("my-custom-provider")).toBe("My Custom Provider");
    expect(providerLabel("")).toBe("Unknown provider");
    expect(
      providerOptions([], []).map((option) => option.label),
    ).toContain("OpenAI");
  });

  it("byok providers come from the engine and exclude its included-model provider", () => {
    const status = {
      ...engineWith(),
      providers: [
        { id: "groq", label: "Groq" },
        { id: "mistral", label: "Mistral" },
        { id: "opencode", label: "OpenCode" },
      ],
    } as EngineStatus;

    expect(engineProviders(status).map((provider) => provider.id)).toEqual([
      "groq",
      "mistral",
      "opencode",
    ]);
    const options = providerOptions([], [], engineProviders(status));
    expect(options.map((option) => option.id)).toEqual(["groq", "mistral"]);
  });

  it("gate missing milestones never adds capabilities the probe did not report", () => {
    const gates: GateSnapshot = readyAsifGates();
    gates.local_orchestrator = {
      state: "waiting_for_vowp_phase3",
      missing: ["local cache"],
      detected_version: "0.4.0",
      evidence: ["vowp agent surface start --help: exit 2"],
    };
    gates.asif = {
      state: "waiting_for_vowp_asif",
      missing: ["v4-asif-cli"],
      detected_version: "0.4.0",
      evidence: ["session export --format asif: exit 2"],
    };

    expect(gateMissingMilestones(gates)).toEqual({
      local: ["local cache"],
      asif: ["v4-asif-cli"],
    });
  });

  it("included models keep engine implementation branding out of the main path", () => {
    expect(modelProviderLabel("opencode")).toBe("StackSpirit Engine");
    expect(modelProviderLabel("opencode-go")).toBe("StackSpirit Engine");
    expect(modelProviderLabel("deepseek")).toBe("DeepSeek");
    expect(modelNotes("opencode", "Free OpenCode model for a limited time")).toBe(
      "Free StackSpirit Engine model for a limited time",
    );
    expect(modelNotes("deepseek", "Any note")).toBe("Any note");
  });

  it("a saved setup and a probed snapshot are distinguishable from defaults", () => {
    const status = {
      ...engineWithoutModel(),
      onboarding_complete: true,
    } as EngineStatus;
    expect(engineSetupComplete(status)).toBe(true);
    expect(needsEngineOnboarding(status, false)).toBe(false);
    expect(engineSetupComplete(engineWithoutModel())).toBe(false);

    expect(gatesProbed(null)).toBe(false);
    expect(
      gatesProbed({
        probed_at: "1970-01-01T00:00:00.000Z",
        local_orchestrator: { state: "unknown" },
        asif: { state: "unknown" },
        local_agent: {
          state: "unknown",
          node: null,
          protocol: null,
          last_heartbeat: null,
          last_error: null,
        },
        features: { asif_actions: false, local_orchestrator_actions: false },
      }),
    ).toBe(false);
    expect(gatesProbed(readyLocalGates())).toBe(true);
  });

  it("engine logs are read structurally and capped again at 200 lines", () => {
    const logs = Array.from({ length: 205 }, (_, index) => `line ${index + 1}`);
    const status = { ...engineWith(), logs } as EngineStatus;
    expect(engineLogs(status)).toHaveLength(200);
    expect(engineLogs(status)[0]).toBe("line 6");
    expect(engineLogs(engineWith())).toEqual([]);
    expect(
      engineLogs({ ...engineWith(), logs: [1, "safe"] } as unknown as EngineStatus),
    ).toEqual(["safe"]);
  });

  it("models mark included access and context windows", () => {
    const status = engineWith();
    const models = status.selected_model ? [status.selected_model] : [];
    expect(includedModels(models)).toHaveLength(1);
    expect(modelContextLabel(128_000)).toBe("128K context");
    expect(modelContextLabel(1_050_000)).toBe("1.1M context");
    expect(modelContextLabel(null)).toBe("Context window unknown");
  });

  it("first-run is shown only before a bundled model is selected", () => {
    expect(needsEngineOnboarding(engineWithoutModel(), false)).toBe(true);
    expect(needsEngineOnboarding(engineWith(), false)).toBe(false);
    expect(needsEngineOnboarding(engineWithoutModel(), true)).toBe(false);
    const external = engineWith({
      choice: { type: "external", kind: "opencode", path: "/opt/bin/opencode" },
    });
    expect(needsEngineOnboarding(external, false)).toBe(false);
  });

  it("gate actions stay disabled until the action flag and evidence agree", () => {
    const asif = readyAsifGates();
    expect(gateActionEnabled(asif, "asif_export")).toBe(false);
    expect(gateActionReason(asif, "asif_export")).toBe(GATE_NOT_IMPLEMENTED_ASIF);
    asif.features.asif_actions = true;
    expect(gateActionEnabled(asif, "asif_export")).toBe(true);
    expect(gateActionEnabled(asif, "asif_import")).toBe(true);

    const local = readyLocalGates();
    local.local_agent.state = "connected";
    local.local_agent.node = "mac-studio";
    local.local_agent.protocol = "vowp-agent/1";
    expect(gateActionReason(local, "local_orchestrator")).toBe(
      GATE_NOT_IMPLEMENTED_LOCAL,
    );
    local.features.local_orchestrator_actions = true;
    expect(gateActionEnabled(local, "local_orchestrator")).toBe(true);
    local.local_agent.state = "not_installed";
    expect(gateActionEnabled(local, "local_orchestrator")).toBe(false);
    expect(gateActionReason(local, "local_orchestrator")).toContain(
      "local VOWP agent is not connected",
    );
  });

  it("waiting gates name the exact missing milestone", () => {
    const gates: GateSnapshot = readyAsifGates();
    gates.asif = {
      state: "waiting_for_vowp_asif",
      missing: ["v4-asif-core", "v4-asif-cli"],
      detected_version: "0.4.0",
      evidence: ["session export --format asif: exit 2"],
    };
    expect(gateActionReason(gates, "asif_export")).toBe(
      "Waiting for VOWP ASIF. Missing: v4-asif-core · v4-asif-cli",
    );
    expect(PHASE3_MILESTONES).toContain("backfill");
    expect(PHASE3_MILESTONES).toContain("local cache");
  });

  it("local agent labels cover every frozen state", () => {
    expect(localAgentLabel({ state: "connected", node: "mac", protocol: null, last_heartbeat: null, last_error: null })).toBe(
      "Connected · mac",
    );
    expect(
      localAgentLabel({
        state: "not_installed",
        node: null,
        protocol: null,
        last_heartbeat: null,
        last_error: null,
      }),
    ).toBe("Not installed on this Mac");
    expect(
      localAgentLabel({
        state: "incompatible",
        node: null,
        protocol: null,
        last_heartbeat: null,
        last_error: null,
      }),
    ).toBe("Incompatible protocol");
  });
});
