import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { GateSnapshot } from "../bridge/wire";
import { gateStateLabel } from "../domain/connection";
import { CapacityRail } from "../views/capacity/CapacityRail";
import { EngineView } from "../views/engine/EngineView";
import { GateCard } from "../views/engine/GateCard";
import { FleetCanvas } from "../views/fleet/FleetCanvas";
import { seedStore } from "./render";
import {
  PLAN_10_2_SCENARIOS,
  SCENARIO_FIXTURES,
  scenario,
} from "./scenarios/fixtures";

const FLEET_EXPECTATIONS: Partial<Record<(typeof PLAN_10_2_SCENARIOS)[number], RegExp>> = {
  "fresh-single": /8 worker slots, 3 busy, 5 idle/,
  idle: /No running workers/,
  busy: /8 busy/,
  "storage-low": /Below the floor\. New runs will wait/,
  "memory-pressure": /Memory pressure stayed above the safe threshold/,
  "provider-limit": /deepseek/,
  "supervisor-down": /supervisor stopped/,
  "stale-cache": /stale/,
  "multi-project": /northnoir/,
  "queue-cap": /showing the first 200 jobs/,
  "unknown-states": /Unknown/,
  "no-advice": /VOWP has no advice yet/,
  "disk-disagreement": /source capacity report/,
};

type AppExpectation =
  | { surface: "engine"; text: RegExp }
  | { surface: "gate"; kind: "local" | "asif"; title: string; state: string };

// Meadow has no top bar. The old classic chrome expectations map to the Engine
// surface: engine status comes from EngineView's status card/onboarding, and the
// local/ASIF gate cards carry the same gateStateLabel copy.
const APP_EXPECTATIONS: Partial<
  Record<(typeof PLAN_10_2_SCENARIOS)[number], AppExpectation>
> = {
  "engine-included": { surface: "engine", text: /Engine ready/ },
  "engine-no-included": { surface: "engine", text: /Engine stopped/ },
  "engine-external": { surface: "engine", text: /External: opencode/ },
  "gate-phase3-waiting": {
    surface: "gate",
    kind: "local",
    title: "Local orchestrator",
    state: "Waiting",
  },
  "gate-phase3-ready": {
    surface: "gate",
    kind: "local",
    title: "Local orchestrator",
    state: "Ready",
  },
  "gate-asif-waiting": {
    surface: "gate",
    kind: "asif",
    title: ".asif.zip sessions",
    state: "Waiting",
  },
  "gate-asif-ready": {
    surface: "gate",
    kind: "asif",
    title: ".asif.zip sessions",
    state: "Ready",
  },
};

function renderGateCards(gates: GateSnapshot) {
  return render(
    <>
      <GateCard kind="local" gates={gates} onDetails={() => undefined} />
      <GateCard kind="asif" gates={gates} onDetails={() => undefined} />
    </>,
  );
}

describe("PLAN §10.2 scenario suite", () => {
  it("covers every required fixture name", () => {
    expect(Object.keys(SCENARIO_FIXTURES).sort()).toEqual(
      [...PLAN_10_2_SCENARIOS].sort(),
    );
  });

  it.each(PLAN_10_2_SCENARIOS)("renders the %s scenario", async (name) => {
    const fixture = scenario(name);
    seedStore(fixture.fleet, {
      engine: fixture.engine ?? null,
      gates: fixture.gates ?? null,
    });
    const expectation = APP_EXPECTATIONS[name];
    if (expectation) {
      if (expectation.surface === "engine") {
        render(<EngineView />);
        expect(
          (await screen.findAllByText(expectation.text)).length,
        ).toBeGreaterThan(0);
      } else {
        if (!fixture.gates) throw new Error(`${name} has no gates fixture`);
        renderGateCards(fixture.gates);
        const card = await screen.findByTestId(`gate-card-${expectation.kind}`);
        expect(within(card).getByText(expectation.title)).toBeInTheDocument();
        expect(within(card).getByText(expectation.state)).toBeInTheDocument();
      }
      return;
    }
    render(
      <>
        <FleetCanvas />
        <CapacityRail />
      </>,
    );
    expect(screen.getByTestId("fleet-canvas")).toBeInTheDocument();
    expect(screen.getByTestId("capacity-rail")).toBeInTheDocument();
    expect(
      screen.getAllByText(FLEET_EXPECTATIONS[name] ?? /./).length,
    ).toBeGreaterThan(0);
  });

  it("gate_badge_shows_all_six_states", () => {
    expect(gateStateLabel("unknown")).toBe("Unknown");
    expect(gateStateLabel("probing")).toBe("Probing");
    expect(gateStateLabel("waiting_for_vowp_phase3")).toBe("Waiting");
    expect(gateStateLabel("waiting_for_vowp_asif")).toBe("Waiting");
    expect(gateStateLabel("incompatible")).toBe("Incompatible");
    expect(gateStateLabel("unreachable")).toBe("Unreachable");
    expect(gateStateLabel("ready")).toBe("Ready");
  });
});
