import { describe, expect, it } from "vitest";

import type { GateSnapshot } from "../../bridge/wire";
import {
  GATE_NOT_IMPLEMENTED_ASIF,
  GATE_NOT_IMPLEMENTED_LOCAL,
  PHASE3_MILESTONES,
  gateActionEnabled,
  gateActionReason,
  includedModels,
  localAgentLabel,
  modelContextLabel,
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
    expect(gateActionReason(local, "local_orchestrator")).toBe(
      GATE_NOT_IMPLEMENTED_LOCAL,
    );
    local.features.local_orchestrator_actions = true;
    local.local_agent.state = "connected";
    expect(gateActionEnabled(local, "local_orchestrator")).toBe(true);
    local.local_agent.state = "not_installed";
    expect(gateActionEnabled(local, "local_orchestrator")).toBe(false);
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
