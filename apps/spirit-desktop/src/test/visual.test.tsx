import { render } from "@testing-library/react";
import type { ReactElement } from "react";
import { describe, expect, it } from "vitest";

import { GIB, scenario } from "./scenarios/fixtures";
import { CpuPanel } from "../views/capacity/CpuPanel";
import { DiskPanel } from "../views/capacity/DiskPanel";
import { RamPanel } from "../views/capacity/RamPanel";
import { WorkerSlotRing } from "../views/fleet/WorkerSlotRing";

function svgSnapshot(fleet: ReturnType<typeof scenario>["fleet"]) {
  const { container, unmount } = render(<WorkerSlotRing fleet={fleet} />);
  const svg = container.querySelector("svg");
  expect(svg?.outerHTML).toMatchSnapshot();
  unmount();
}

function panelSnapshot(node: ReactElement) {
  const { container, unmount } = render(node);
  expect(container.firstElementChild?.outerHTML).toMatchSnapshot();
  unmount();
}

describe("deterministic visual snapshots", () => {
  it("ring geometry at target 0", () => {
    const fixture = scenario("idle");
    fixture.fleet.capacity.target.target_workers = 0;
    fixture.fleet.capacity.advised_workers = 0;
    svgSnapshot(fixture.fleet);
  });

  it("ring geometry at 3/8", () => {
    svgSnapshot(scenario("fresh-single").fleet);
  });

  it("ring geometry above 12 slots", () => {
    const fixture = scenario("fresh-single");
    fixture.fleet.capacity.target.target_workers = 16;
    fixture.fleet.capacity.advised_workers = 16;
    svgSnapshot(fixture.fleet);
  });

  it("ring geometry when busy exceeds target", () => {
    const fixture = scenario("busy");
    fixture.fleet.capacity.target.target_workers = 6;
    fixture.fleet.capacity.advised_workers = 6;
    svgSnapshot(fixture.fleet);
  });

  it("disk panel at ok, warn, critical and unknown", () => {
    const ok = scenario("fresh-single");
    panelSnapshot(<DiskPanel capacity={ok.fleet.capacity} />);

    const warn = scenario("fresh-single");
    warn.fleet.capacity.host = {
      ...warn.fleet.capacity.host!,
      disk_free_home_bytes: 5.5 * GIB,
    };
    warn.fleet.capacity.disk = {
      ...warn.fleet.capacity.disk,
      free_home_bytes: 5.5 * GIB,
      state: "warn",
    };
    panelSnapshot(<DiskPanel capacity={warn.fleet.capacity} />);

    panelSnapshot(<DiskPanel capacity={scenario("storage-low").fleet.capacity} />);

    const unknown = scenario("fresh-single");
    unknown.fleet.capacity.host = null;
    unknown.fleet.capacity.disk = {
      ...unknown.fleet.capacity.disk,
      free_home_bytes: 0,
      free_worktrees_bytes: null,
      state: "unknown",
    };
    panelSnapshot(<DiskPanel capacity={unknown.fleet.capacity} />);
  });

  it("ram panel at ok, warn, critical and unknown", () => {
    panelSnapshot(
      <RamPanel
        capacity={scenario("fresh-single").fleet.capacity}
        running={scenario("fresh-single").fleet.running}
      />,
    );

    const warn = scenario("fresh-single");
    warn.fleet.capacity.host = {
      ...warn.fleet.capacity.host!,
      mem_available_bytes: 3 * GIB,
    };
    warn.fleet.capacity.ram = {
      ...warn.fleet.capacity.ram,
      available_bytes: 3 * GIB,
      state: "warn",
    };
    panelSnapshot(
      <RamPanel capacity={warn.fleet.capacity} running={warn.fleet.running} />,
    );

    panelSnapshot(
      <RamPanel
        capacity={scenario("memory-pressure").fleet.capacity}
        running={scenario("memory-pressure").fleet.running}
      />,
    );

    const unknown = scenario("fresh-single");
    unknown.fleet.capacity.host = null;
    unknown.fleet.capacity.ram = {
      ...unknown.fleet.capacity.ram,
      available_bytes: 0,
      state: "unknown",
    };
    panelSnapshot(
      <RamPanel capacity={unknown.fleet.capacity} running={unknown.fleet.running} />,
    );
  });

  it("cpu panel at ok, warn, critical and unknown", () => {
    panelSnapshot(<CpuPanel capacity={scenario("fresh-single").fleet.capacity} />);

    const warn = scenario("fresh-single");
    warn.fleet.capacity.cpu = {
      ...warn.fleet.capacity.cpu,
      load1: 6.4,
      load_ratio: 0.8,
      state: "warn",
    };
    panelSnapshot(<CpuPanel capacity={warn.fleet.capacity} />);

    const critical = scenario("fresh-single");
    critical.fleet.capacity.cpu = {
      ...critical.fleet.capacity.cpu,
      load1: 9,
      load_ratio: 1.125,
      state: "critical",
    };
    panelSnapshot(<CpuPanel capacity={critical.fleet.capacity} />);

    const unknown = scenario("fresh-single");
    unknown.fleet.capacity.host = null;
    unknown.fleet.capacity.cpu = {
      ...unknown.fleet.capacity.cpu,
      load1: 0,
      load_ratio: 0,
      state: "unknown",
    };
    panelSnapshot(<CpuPanel capacity={unknown.fleet.capacity} />);
  });
});
