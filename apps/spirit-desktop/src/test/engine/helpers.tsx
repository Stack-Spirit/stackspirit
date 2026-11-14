import { render, type RenderResult } from "@testing-library/react";

import { MockBridge, type MockScenario } from "../../bridge/MockBridge";
import type { EngineStatus, GateSnapshot } from "../../bridge/wire";
import { seedStore } from "../render";
import { scenario } from "../scenarios/fixtures";
import { EngineView } from "../../views/engine/EngineView";

export function engineWith(
  overrides: Partial<EngineStatus> = {},
): EngineStatus {
  const base = scenario("engine-included").engine;
  if (!base) throw new Error("engine-included fixture is missing its engine status");
  return structuredClone({ ...base, ...overrides });
}

export function engineWithoutModel(): EngineStatus {
  return engineWith({ selected_model: null });
}

export function waitingGates(): GateSnapshot {
  const gates = scenario("gate-phase3-waiting").gates;
  if (!gates) throw new Error("gate-phase3-waiting fixture is missing its gates");
  return structuredClone(gates);
}

export function readyLocalGates(): GateSnapshot {
  const gates = scenario("gate-phase3-ready").gates;
  if (!gates) throw new Error("gate-phase3-ready fixture is missing its gates");
  return structuredClone(gates);
}

export function readyAsifGates(): GateSnapshot {
  const gates = scenario("gate-asif-ready").gates;
  if (!gates) throw new Error("gate-asif-ready fixture is missing its gates");
  return structuredClone(gates);
}

export function renderEngine(
  engine: EngineStatus,
  options: {
    gates?: GateSnapshot;
    bridge?: MockBridge;
    scenario?: MockScenario;
  } = {},
): { bridge: MockBridge } & RenderResult {
  const bridge = options.bridge ?? new MockBridge(options.scenario ?? "engine-ready");
  seedStore(scenario("fresh-single").fleet, {
    bridge,
    engine,
    gates: options.gates ?? waitingGates(),
  });
  return { bridge, ...render(<EngineView />) };
}
