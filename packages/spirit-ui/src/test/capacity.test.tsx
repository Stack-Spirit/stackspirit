import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { describe, expect, it, vi } from "vitest";

import { MockBridge, type MockScenario } from "../bridge/MockBridge";
import {
  seedConnectedStale,
  seedStore,
} from "./render";
import { FAKE_SECRET_TOKENS, rawWithWarnings, scenario } from "./scenarios/fixtures";
import { BrakeBanner } from "../views/capacity/BrakeBanner";
import { CapacityRail } from "../views/capacity/CapacityRail";
import { CpuPanel } from "../views/capacity/CpuPanel";
import { DiskPanel } from "../views/capacity/DiskPanel";
import { LimitsPanel } from "../views/capacity/LimitsPanel";
import { RamPanel } from "../views/capacity/RamPanel";
import { RawDocumentDrawer } from "../views/capacity/RawDocumentDrawer";
import { TargetAdvisor } from "../views/capacity/TargetAdvisor";
import { WaitingPanel } from "../views/capacity/WaitingPanel";

const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"];

async function wcagViolations(container: HTMLElement) {
  const results = await axe.run(container, {
    runOnly: { type: "tag", values: WCAG_TAGS },
  });
  return results.violations;
}

async function connectedBridge(name: MockScenario = "fresh"): Promise<MockBridge> {
  const bridge = new MockBridge(name);
  const host = (await bridge.hosts.list())[0];
  if (host) await bridge.connection.connect(host.id);
  return bridge;
}

describe("capacity panels", () => {
  it("disk_below_floor_is_critical_and_says_new_runs_wait", () => {
    const fixture = scenario("storage-low");
    render(<DiskPanel capacity={fixture.fleet.capacity} />);
    expect(screen.getByTestId("disk-panel")).toHaveAttribute("data-state", "critical");
    expect(screen.getByText("Below the floor. New runs will wait (disk).")).toBeInTheDocument();
  });

  it("disk_disagreement_prefers_capacity_report_and_marks_source", () => {
    const fixture = scenario("disk-disagreement");
    render(<DiskPanel capacity={fixture.fleet.capacity} />);
    const panel = screen.getByTestId("disk-panel");
    expect(panel).toHaveAttribute("data-source", "capacity_report");
    expect(panel).toHaveTextContent(/source capacity report/);
    expect(panel).toHaveTextContent(/18.0 GiB free/);
  });

  it("disk with no sample says Unknown, not zero bytes", () => {
    const fixture = scenario("fresh-single");
    fixture.fleet.capacity.host = null;
    fixture.fleet.capacity.measured_at = null;
    fixture.fleet.capacity.disk = {
      ...fixture.fleet.capacity.disk,
      free_home_bytes: 0,
      free_worktrees_bytes: null,
      state: "unknown",
    };
    render(<DiskPanel capacity={fixture.fleet.capacity} />);
    const panel = screen.getByTestId("disk-panel");
    expect(panel).toHaveTextContent("Unknown");
    expect(panel).not.toHaveTextContent("0 B");
  });

  it("ram_estimate_shows_source_and_samples", () => {
    const fixture = scenario("fresh-single");
    render(
      <RamPanel capacity={fixture.fleet.capacity} running={fixture.fleet.running} />,
    );
    const estimate = screen.getByTestId("ram-estimate");
    expect(estimate).toHaveTextContent("Per-worker p90 1.2 GiB (pooled, 12 samples)");
    expect(screen.getByTestId("ram-extra-slots")).toHaveTextContent(/more by RAM/);
  });

  it("ram_without_estimate_does_not_invent_slots", () => {
    const fixture = scenario("fresh-single");
    fixture.fleet.capacity.estimates = [];
    fixture.fleet.capacity.ram = {
      ...fixture.fleet.capacity.ram,
      per_worker_p90_bytes: null,
      per_worker_source: null,
      per_worker_samples: null,
      estimated_extra_slots: null,
    };
    render(
      <RamPanel capacity={fixture.fleet.capacity} running={fixture.fleet.running} />,
    );
    expect(screen.getByTestId("ram-no-estimate")).toHaveTextContent(
      "Not enough samples for a per-worker estimate.",
    );
    expect(screen.queryByTestId("ram-extra-slots")).not.toBeInTheDocument();
  });

  it("cpu_pressure_absent_says_not_reported", () => {
    const fixture = scenario("fresh-single");
    fixture.fleet.capacity.host = {
      ...fixture.fleet.capacity.host!,
      pressure_cpu_avg10: null,
    };
    fixture.fleet.capacity.cpu = {
      ...fixture.fleet.capacity.cpu,
      pressure_avg10: null,
    };
    render(<CpuPanel capacity={fixture.fleet.capacity} />);
    expect(screen.getByTestId("cpu-panel")).toHaveTextContent("not reported");
  });

  it("brake_banner_lists_each_reason", () => {
    const fixture = scenario("memory-pressure");
    render(<BrakeBanner brake={fixture.fleet.capacity.brake} />);
    const banner = screen.getByTestId("brake-banner");
    expect(banner).toHaveTextContent("Memory pressure");
    expect(banner).toHaveTextContent("Memory pressure stayed above the safe threshold.");
    expect(banner).toHaveTextContent("Swap growth");
    expect(banner).toHaveTextContent("New runs wait; running runs continue.");
    expect(banner).toHaveAttribute("data-tone", "danger");
  });

  it("waiting_reasons_group_by_code", () => {
    const fixture = scenario("fresh-single");
    fixture.fleet.capacity.waiting = [
      {
        project: "vowp",
        job: "ss-1",
        code: "ram",
        detail: "RAM 1.2 GB short",
        since: "2026-11-14T11:58:00.000Z",
      },
      {
        project: "vowp",
        job: "ss-2",
        code: "ram",
        detail: "RAM 0.8 GB short",
        since: "2026-11-14T11:59:00.000Z",
      },
      {
        project: "vowp",
        job: "ss-3",
        code: "cpu",
        detail: "CPU load is high",
        since: "2026-11-14T11:57:00.000Z",
      },
      {
        project: "vowp",
        job: "ss-4",
        code: "disk",
        detail: "Home disk is low",
        since: "2026-11-14T11:56:00.000Z",
      },
      {
        project: "vowp",
        job: "ss-5",
        code: "provider",
        detail: "Provider cap reached",
        since: "2026-11-14T11:55:00.000Z",
      },
      {
        project: "vowp",
        job: "ss-6",
        code: "target",
        detail: "Target reached",
        since: "2026-11-14T11:54:00.000Z",
      },
    ];
    render(<WaitingPanel capacity={fixture.fleet.capacity} />);
    const panel = screen.getByTestId("waiting-panel");
    expect(panel).toHaveTextContent("RAM · 2");
    expect(panel).toHaveTextContent("CPU · 1");
    expect(panel).toHaveTextContent("Disk · 1");
    expect(panel).toHaveTextContent("Provider limit · 1");
    expect(screen.getAllByTestId("waiting-row")).toHaveLength(5);
    expect(panel).toHaveTextContent("Showing the first 5 of 6 waiting jobs.");
  });

  it("provider_limits_do_not_look_like_disk_problems", () => {
    const fixture = scenario("provider-limit");
    render(<LimitsPanel capacity={fixture.fleet.capacity} />);
    const panel = screen.getByTestId("limits-panel");
    expect(panel).toHaveTextContent("deepseek");
    expect(panel).toHaveTextContent("Quota");
    expect(panel).toHaveTextContent("Provider limits are separate from disk, RAM and CPU.");
    expect(within(panel).queryByText(/Below the floor/)).not.toBeInTheDocument();
    expect(screen.getByTestId("provider-caps")).toHaveTextContent("deepseek → 2");
  });
});

describe("capacity rail", () => {
  it("capacity_report_unloaded_shows_unavailable_not_empty", () => {
    const fixture = scenario("fresh-single");
    fixture.fleet.capacity = {
      ...fixture.fleet.capacity,
      report_loaded: false,
      waiting: [],
      estimates: [],
      provider_caps: {},
      ram: {
        ...fixture.fleet.capacity.ram,
        per_worker_p90_bytes: null,
        per_worker_source: null,
        per_worker_samples: null,
        estimated_extra_slots: null,
      },
    };
    seedStore(fixture.fleet);
    render(<CapacityRail />);
    expect(screen.getByTestId("waiting-panel")).toHaveTextContent(
      "Capacity detail unavailable — retrying.",
    );
    expect(screen.queryByText("No jobs are waiting for capacity.")).not.toBeInTheDocument();
    expect(screen.getByTestId("ram-detail-unavailable")).toBeInTheDocument();
    expect(screen.getByTestId("disk-panel")).toBeInTheDocument();
  });

  it("stale_capacity_disables_target_edit", () => {
    const fixture = scenario("fresh-single");
    seedConnectedStale(fixture.fleet);
    render(<TargetAdvisor />);
    expect(screen.getByLabelText("Worker target")).toBeDisabled();
    expect(screen.getByLabelText("Target mode")).toBeDisabled();
    expect(screen.getByTestId("target-stale-reason")).toHaveTextContent(
      "Refresh before changing the worker target.",
    );
  });

  it("target_above_advice_requires_override_confirmation", async () => {
    const fixture = scenario("fresh-single");
    const bridge = await connectedBridge("fresh");
    const setTarget = vi.spyOn(bridge.fleet, "setTarget");
    seedStore(fixture.fleet, { bridge });
    render(<TargetAdvisor />);
    const user = userEvent.setup();
    const input = screen.getByLabelText("Worker target");
    await user.clear(input);
    await user.type(input, "10");
    await user.keyboard("{Enter}");
    expect(
      await screen.findByRole("dialog", { name: "Confirm owner override" }),
    ).toBeVisible();
    expect(setTarget).not.toHaveBeenCalled();
    await user.click(screen.getByTestId("confirm-override"));
    await waitFor(() => expect(setTarget).toHaveBeenCalledTimes(1));
    expect(setTarget.mock.calls[0]?.[0]).toMatchObject({
      target: 10,
      override: true,
    });
  });

  it("apply_advised_calls_capacity_set_and_refreshes", async () => {
    const fixture = scenario("fresh-single");
    fixture.fleet.capacity.target = {
      ...fixture.fleet.capacity.target,
      target_workers: 4,
      advised_workers: 8,
    };
    fixture.fleet.capacity.advised_workers = 8;
    const bridge = await connectedBridge("fresh");
    const setTarget = vi.spyOn(bridge.fleet, "setTarget");
    const refresh = vi.spyOn(bridge.fleet, "refresh");
    seedStore(fixture.fleet, { bridge });
    render(<TargetAdvisor />);
    await userEvent.setup().click(screen.getByTestId("apply-advised"));
    await waitFor(() => expect(setTarget).toHaveBeenCalledTimes(1));
    expect(setTarget.mock.calls[0]?.[0]).toMatchObject({
      target: 8,
      override: false,
    });
    await waitFor(() => expect(refresh).toHaveBeenCalled());
  });

  it("shows no advice and disables the advice action", () => {
    const fixture = scenario("no-advice");
    seedStore(fixture.fleet);
    render(<TargetAdvisor />);
    expect(screen.getAllByText("VOWP has no advice yet").length).toBeGreaterThan(0);
    expect(screen.getByTestId("apply-advised")).toBeDisabled();
  });

  it("freshness footer opens the raw-document drawer", async () => {
    const fixture = scenario("fresh-single");
    seedStore(fixture.fleet, { raw: rawWithWarnings() });
    render(<CapacityRail />);
    const user = userEvent.setup();
    await user.click(screen.getByTestId("capacity-freshness"));
    expect(
      await screen.findByRole("dialog", { name: "Raw VOWP document" }),
    ).toBeVisible();
  });

  it("raw_document_drawer_shows_parse_warnings", () => {
    const fixture = scenario("fresh-single");
    seedStore(fixture.fleet, { raw: rawWithWarnings() });
    render(<RawDocumentDrawer open onClose={() => undefined} />);
    const drawer = screen.getByTestId("raw-document-drawer");
    expect(drawer).toHaveTextContent("unknown_enum");
    expect(drawer).toHaveTextContent("missing_optional");
    expect(drawer).toHaveTextContent("truncated");
    expect(drawer).toHaveTextContent("snapshot.projects[0].state");
    expect(screen.getByText("3 parse warnings")).toBeInTheDocument();
    expect(screen.getByTestId("parsed-fleet-status")).toBeVisible();
    expect(screen.getByTestId("parsed-capacity-report")).toBeVisible();
  });

  it("raw_document_drawer_never_dumps_raw_secrets", () => {
    const fixture = scenario("fresh-single");
    seedStore(fixture.fleet, { raw: rawWithWarnings() });
    render(<RawDocumentDrawer open onClose={() => undefined} />);
    const text = document.body.textContent ?? "";
    expect(text).not.toContain(FAKE_SECRET_TOKENS.sk);
    expect(text).not.toContain(FAKE_SECRET_TOKENS.ghp);
    expect(text).not.toContain("Bearer");
    expect(text).toContain("[redacted]");
    expect(text).toContain("generated_at");
    expect(text).toContain("fetched_at");
  });

  it("has no WCAG 2.2 AA axe violations on the capacity rail", async () => {
    const fixture = scenario("memory-pressure");
    seedStore(fixture.fleet);
    const { container } = render(<CapacityRail />);
    expect(await wcagViolations(container)).toEqual([]);
  });
});
