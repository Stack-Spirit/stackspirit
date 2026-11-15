import { render, type RenderResult } from "@testing-library/react";
import { useState } from "react";
import { vi } from "vitest";

import { MockBridge, type MockScenario } from "../../bridge/MockBridge";
import type {
  ConnectionSnapshot,
  FleetView,
  GateSnapshot,
  SessionCardView,
} from "../../bridge/wire";
import { parseExecutor } from "../../domain/executor";
import { SessionsDrawer } from "../../views/sessions/SessionsDrawer";
import { seedStore } from "../render";
import { NOW, scenario } from "../scenarios/fixtures";

export const EXPORT_SESSION_ID = "01JCF5WQ8ZK3M2V7N4P6R8T0AB";
export const IDLE_SESSION_ID = "01JCF5WQ8ZK3M2V7N4P6R8T0AC";

export function sessionFixtures(): SessionCardView[] {
  return [
    {
      id: EXPORT_SESSION_ID,
      project: "vowp",
      job_id: "ss-2-desktop-shell",
      role: "worker",
      title: "Desktop shell",
      state: "running",
      executor: parseExecutor("opencode:deepseek/deepseek-flash#max"),
      node: "vps",
      active_surface: null,
      last_event_at: NOW,
    },
    {
      id: IDLE_SESSION_ID,
      project: "vowp",
      job_id: null,
      role: null,
      title: "Imported session",
      state: "idle",
      executor: null,
      node: "vps",
      active_surface: null,
      last_event_at: "2026-11-14T11:58:00.000Z",
    },
  ];
}

export function fleetWithSessions(): FleetView {
  const fleet = scenario("fresh-single").fleet;
  fleet.sessions = sessionFixtures();
  return fleet;
}

export async function gatesFrom(scenarioName: MockScenario): Promise<GateSnapshot> {
  return new MockBridge(scenarioName).gates.get();
}

export interface RenderSessionsOptions {
  bridge?: MockBridge;
  gates?: GateSnapshot | null;
  fleet?: FleetView;
  connection?: ConnectionSnapshot;
  open?: boolean;
  onClose?: () => void;
}

export function renderSessions(
  options: RenderSessionsOptions = {},
): { bridge: MockBridge; onClose: () => void } & RenderResult {
  const bridge = options.bridge ?? new MockBridge("gate-asif-ready");
  const fleet = options.fleet ?? fleetWithSessions();
  seedStore(fleet, {
    bridge,
    gates: options.gates === undefined ? null : options.gates,
    connection: options.connection,
  });
  const onClose = options.onClose ?? vi.fn();
  const result = render(<SessionsDrawer open={options.open ?? true} onClose={onClose} />);
  return { bridge, onClose, ...result };
}

export function SessionsHarness() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" onClick={() => setOpen(true)}>
        Open sessions
      </button>
      <SessionsDrawer open={open} onClose={() => setOpen(false)} />
    </>
  );
}

export function renderSessionsHarness(
  options: RenderSessionsOptions = {},
): { bridge: MockBridge } & RenderResult {
  const bridge = options.bridge ?? new MockBridge("gate-asif-ready");
  const fleet = options.fleet ?? fleetWithSessions();
  seedStore(fleet, {
    bridge,
    gates: options.gates === undefined ? null : options.gates,
    connection: options.connection,
  });
  return { bridge, ...render(<SessionsHarness />) };
}
