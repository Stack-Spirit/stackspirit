import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { TopBar } from "../app/TopBar";
import { gateStateLabel } from "../domain/connection";
import { seedStore } from "./render";
import {
  PLAN_10_2_SCENARIOS,
  SCENARIO_FIXTURES,
  scenario,
} from "./scenarios/fixtures";
import { FleetCanvas } from "../views/fleet/FleetCanvas";
import { CapacityRail } from "../views/capacity/CapacityRail";

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

const APP_EXPECTATIONS: Partial<Record<(typeof PLAN_10_2_SCENARIOS)[number], RegExp>> = {
  "engine-included": /Engine ready/,
  "engine-no-included": /Engine stopped/,
  "engine-external": /External: opencode/,
  "gate-phase3-waiting": /Local · Waiting/,
  "gate-phase3-ready": /Local · Ready/,
  "gate-asif-waiting": /ASIF · Waiting/,
  "gate-asif-ready": /ASIF · Ready/,
};

function renderTopBar() {
  return render(
    <TopBar
      onNavigate={() => undefined}
      onOpenInbox={() => undefined}
      onOpenCapacity={() => undefined}
    />,
  );
}

describe("PLAN §10.2 scenario suite", () => {
  it("covers every required fixture name", () => {
    expect(Object.keys(SCENARIO_FIXTURES).sort()).toEqual(
      [...PLAN_10_2_SCENARIOS].sort(),
    );
  });

  it.each(PLAN_10_2_SCENARIOS)("renders the %s scenario", (name) => {
    const fixture = scenario(name);
    seedStore(fixture.fleet, {
      engine: fixture.engine ?? null,
      gates: fixture.gates ?? null,
    });
    if (APP_EXPECTATIONS[name]) {
      renderTopBar();
      expect(screen.getAllByText(APP_EXPECTATIONS[name]).length).toBeGreaterThan(0);
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
