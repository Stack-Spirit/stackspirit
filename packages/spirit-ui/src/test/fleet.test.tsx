import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { useState } from "react";
import { describe, expect, it } from "vitest";

import { MockBridge } from "../bridge/MockBridge";
import type { JobCardView } from "../bridge/wire";
import { parseExecutor } from "../domain/executor";
import { seedStore } from "./render";
import { scenario } from "./scenarios/fixtures";
import { useFleetStore } from "../state/fleetStore";
import { FleetCanvas } from "../views/fleet/FleetCanvas";
import { JobDrawer } from "../views/fleet/JobDrawer";
import { JobLane } from "../views/fleet/JobLane";
import { RunningWorkerGrid } from "../views/fleet/RunningWorkerGrid";
import { VpsRootCard } from "../views/fleet/VpsRootCard";
import { WorkerCard } from "../views/fleet/WorkerCard";
import { WorkerSlotRing } from "../views/fleet/WorkerSlotRing";
import { CapacityRail } from "../views/capacity/CapacityRail";
import { LANES } from "../domain/fleet";

const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"];

async function wcagViolations(container: HTMLElement) {
  const results = await axe.run(container, {
    runOnly: { type: "tag", values: WCAG_TAGS },
  });
  return results.violations;
}

async function connectedBridge(): Promise<MockBridge> {
  const bridge = new MockBridge("fresh");
  const host = (await bridge.hosts.list())[0];
  if (host) await bridge.connection.connect(host.id);
  return bridge;
}

describe("VPS root and worker slots", () => {
  it("vps_root_shows_supervisor_and_version", () => {
    const fixture = scenario("fresh-single");
    render(<VpsRootCard fleet={fixture.fleet} />);
    expect(screen.getByTestId("vps-root-card")).toHaveTextContent("vps");
    expect(screen.getByTestId("vps-root-card")).toHaveTextContent(/supervisor up/);
    expect(screen.getByTestId("vps-root-card")).toHaveTextContent("VOWP 0.4.0");
    expect(screen.getByTestId("vps-root-card")).toHaveTextContent("Projects");
    expect(screen.getByTestId("vps-root-card")).toHaveTextContent("Limits");
  });

  it("ring_renders_three_busy_of_eight", () => {
    const fixture = scenario("fresh-single");
    render(<WorkerSlotRing fleet={fixture.fleet} />);
    const ring = screen.getByTestId("worker-slot-ring");
    expect(ring).toHaveAttribute("data-target", "8");
    expect(ring).toHaveAttribute("data-busy", "3");
    expect(screen.getByTestId("ring-summary")).toHaveTextContent(
      /8 worker slots, 3 busy, 5 idle/,
    );
    expect(within(ring).getByText("3 / 8")).toBeInTheDocument();
  });

  it("ring_aggregates_above_twelve_slots", () => {
    const fixture = scenario("fresh-single");
    fixture.fleet.capacity.target.target_workers = 16;
    fixture.fleet.capacity.advised_workers = 16;
    render(<WorkerSlotRing fleet={fixture.fleet} />);
    expect(screen.getByTestId("ring-overflow")).toHaveTextContent("+4");
    expect(screen.getByTestId("ring-overflow").querySelector("title")?.textContent).toBe(
      "16 worker slots, 3 busy",
    );
    const ring = screen.getByTestId("worker-slot-ring");
    expect(ring.querySelectorAll("[data-slot-state]")).toHaveLength(12);
  });

  it("ring_target_zero_explains_why", () => {
    const fixture = scenario("idle");
    fixture.fleet.capacity.target.target_workers = 0;
    fixture.fleet.capacity.advised_workers = 0;
    render(<WorkerSlotRing fleet={fixture.fleet} />);
    expect(screen.getByTestId("ring-summary")).toHaveTextContent(
      "No workers allowed — VOWP target is 0",
    );
    expect(screen.getByTestId("worker-slot-ring")).toHaveAttribute("data-busy", "0");
  });

  it("busy_greater_than_target_is_shown_as_an_inconsistency", () => {
    const fixture = scenario("busy");
    fixture.fleet.capacity.target.target_workers = 6;
    fixture.fleet.capacity.advised_workers = 6;
    render(<WorkerSlotRing fleet={fixture.fleet} />);
    expect(screen.getByText(/over target \+2/)).toBeInTheDocument();
    expect(screen.getByTestId("ring-summary")).toHaveTextContent("2 above target");
  });

  it("renders verifier runs on a separate inner arc", () => {
    const fixture = scenario("fresh-single");
    fixture.fleet.running.push({
      ...fixture.fleet.running[0]!,
      job_id: "ss-verifier",
      role: "verifier",
      executor: parseExecutor("claude-code:model-a@medium"),
    });
    fixture.fleet.capacity.running.runs = 4;
    render(<WorkerSlotRing fleet={fixture.fleet} />);
    expect(screen.getByTestId("worker-slot-ring")).toHaveAttribute("data-verifier", "1");
    expect(screen.getAllByTestId("verifier-run")).toHaveLength(1);
  });
});

describe("running worker cards", () => {
  it("executor_badge_splits_harness_provider_model_variant", () => {
    const fixture = scenario("fresh-single");
    render(<WorkerCard card={fixture.fleet.running[0]!} />);
    expect(screen.getByTestId("executor-harness")).toHaveTextContent("opencode");
    expect(screen.getByTestId("executor-model")).toHaveTextContent("deepseek");
    expect(screen.getByTestId("executor-model")).toHaveTextContent("DeepSeek V4.1 Flash");
    expect(screen.getByTestId("executor-modifier")).toHaveTextContent("max");
  });

  it("running_card_shell_has_no_model_badge", () => {
    const fixture = scenario("fresh-single");
    render(<WorkerCard card={fixture.fleet.running[2]!} />);
    expect(screen.getByTestId("executor-harness")).toHaveTextContent("shell");
    expect(screen.queryByTestId("executor-model")).not.toBeInTheDocument();
    expect(screen.getByTestId("worker-title-ss-4-engine-onboarding")).toHaveTextContent(
      /title unavailable in snapshot/,
    );
  });

  it("unknown_executor_keeps_raw_string", () => {
    const fixture = scenario("fresh-single");
    const card = {
      ...fixture.fleet.running[0]!,
      executor: parseExecutor("future-harness:model-x"),
    };
    render(<WorkerCard card={card} />);
    expect(screen.getByTestId("executor-harness")).toHaveTextContent("Unknown harness");
    expect(screen.getByTestId("executor-model")).toHaveTextContent("future-harness:model-x");
  });

  it("worker_card_state_marks_stalled", () => {
    const fixture = scenario("fresh-single");
    const card = { ...fixture.fleet.running[0]!, stalled: true, state: "stalled" };
    render(<WorkerCard card={card} />);
    expect(screen.getByTestId(`worker-card-${card.job_id}`)).toHaveAttribute(
      "data-state",
      "stalled",
    );
    expect(screen.getByTestId(`worker-event-${card.job_id}`)).toHaveTextContent("stalled");
  });

  it("renders the grid with a quiet empty state", () => {
    const fixture = scenario("idle");
    render(<RunningWorkerGrid fleet={fixture.fleet} cards={[]} />);
    expect(screen.getByText("No running workers")).toBeInTheDocument();
  });

  it("matches running card titles by project and job id", () => {
    const fixture = scenario("multi-project");
    const card = {
      ...fixture.fleet.running[1]!,
      project: "northnoir",
      job_id: "duplicate-id",
    };
    const base = fixture.fleet.lanes.ready[0];
    expect(base).toBeDefined();
    if (!base) return;
    fixture.fleet.lanes.ready = [
      { ...base, project: "vowp", job_id: "duplicate-id", title: "Wrong project" },
      {
        ...base,
        project: "northnoir",
        job_id: "duplicate-id",
        title: "North Noir title",
      },
    ];
    render(<RunningWorkerGrid fleet={fixture.fleet} cards={[card]} />);
    expect(screen.getByTestId("worker-title-duplicate-id")).toHaveTextContent(
      "North Noir title",
    );
  });
});

describe("job lanes", () => {
  it("blocked_card_does_not_invent_title", () => {
    const fixture = scenario("fresh-single");
    const lane = LANES.find((candidate) => candidate.key === "parked_blocked");
    expect(lane).toBeDefined();
    if (!lane) return;
    render(
      <JobLane
        lane={lane}
        cards={fixture.fleet.lanes.parked_blocked}
        count={1}
        onOpenJob={() => undefined}
      />,
    );
    const card = screen.getByTestId("job-card-ss-8-blocked-review");
    expect(card).toHaveTextContent("ss-8-blocked-review");
    expect(card).toHaveTextContent("question");
    expect(card).toHaveTextContent("Waiting for the owner to choose.");
    expect(card).not.toHaveTextContent("undefined");
  });

  it("gate_failure_card_links_to_review_lane", async () => {
    const fixture = scenario("fresh-single");
    const lane = LANES.find((candidate) => candidate.key === "review_fix");
    expect(lane).toBeDefined();
    if (!lane) return;
    const opened: { job: JobCardView | null } = { job: null };
    render(
      <JobLane
        lane={lane}
        cards={fixture.fleet.lanes.review_fix}
        count={1}
        onOpenJob={(job) => {
          opened.job = job;
        }}
      />,
    );
    const card = screen.getByTestId("job-card-ss-9-gate-check");
    expect(card).toHaveTextContent("secret_scan");
    expect(card).toHaveTextContent("example evidence line");
    await userEvent.setup().click(card);
    expect(opened.job?.job_id).toBe("ss-9-gate-check");
    expect(opened.job?.gate).toBe("secret_scan");
  });

  it("empty lane says No jobs", () => {
    const lane = LANES.find((candidate) => candidate.key === "running");
    expect(lane).toBeDefined();
    if (!lane) return;
    render(<JobLane lane={lane} cards={[]} count={0} onOpenJob={() => undefined} />);
    expect(screen.getByText("No jobs")).toBeInTheDocument();
  });

  it("keeps queue truncation explicit in the ready lane footer", () => {
    const fixture = scenario("queue-cap");
    const lane = LANES.find((candidate) => candidate.key === "ready");
    expect(lane).toBeDefined();
    if (!lane) return;
    render(
      <JobLane
        lane={lane}
        cards={fixture.fleet.lanes.ready}
        count={200}
        queueCap={200}
        truncated
        onOpenJob={() => undefined}
      />,
    );
    expect(screen.getByTestId("lane-truncated-ready")).toHaveTextContent(
      "showing the first 200 jobs",
    );
  });

  it("never renders more cards than the queue cap", () => {
    const fixture = scenario("queue-cap");
    const lane = LANES.find((candidate) => candidate.key === "ready");
    expect(lane).toBeDefined();
    if (!lane) return;
    const extra = {
      ...fixture.fleet.lanes.ready[0]!,
      job_id: "ss-999-overflow",
    };
    render(
      <JobLane
        lane={lane}
        cards={[...fixture.fleet.lanes.ready, extra]}
        count={201}
        queueCap={200}
        onOpenJob={() => undefined}
      />,
    );
    expect(
      within(screen.getByTestId("lane-ready")).getAllByTestId(/job-card-/),
    ).toHaveLength(200);
    expect(screen.getByTestId("lane-truncated-ready")).toHaveTextContent(
      "showing the first 200 jobs",
    );
  });
});

function DrawerHarness({ job }: { job: JobCardView }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" onClick={() => setOpen(true)}>
        Open job
      </button>
      <JobDrawer
        job={job}
        open={open}
        mutationsAllowed
        onClose={() => setOpen(false)}
      />
    </>
  );
}

describe("job drawer", () => {
  it("job_actions_disabled_for_ineligible_states", () => {
    const fixture = scenario("fresh-single");
    const done = { ...fixture.fleet.lanes.review_fix[0]!, state: "done" };
    seedStore(fixture.fleet);
    render(
      <JobDrawer job={done} open mutationsAllowed onClose={() => undefined} />,
    );
    expect(screen.getByTestId("job-action-retry")).toBeDisabled();
    expect(screen.getByTestId("job-action-park")).toBeDisabled();
    expect(screen.getByTestId("job-action-cancel")).toBeDisabled();
    expect(screen.getByTestId("job-action-reason")).toHaveTextContent(/done/);
  });

  it("job_drawer_escape_and_focus_restore", async () => {
    const fixture = scenario("fresh-single");
    const job = fixture.fleet.lanes.running[0]!;
    seedStore(fixture.fleet);
    render(<DrawerHarness job={job} />);
    const user = userEvent.setup();
    const trigger = screen.getByRole("button", { name: "Open job" });
    await user.click(trigger);
    expect(await screen.findByRole("dialog", { name: /Job ss-2-desktop-shell/ })).toBeVisible();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(document.activeElement).toBe(trigger);
  });

  it("cancel asks for confirmation before calling VOWP", async () => {
    const fixture = scenario("fresh-single");
    seedStore(fixture.fleet);
    render(
      <JobDrawer
        job={{ ...fixture.fleet.lanes.review_fix[0]!, state: "active" }}
        open
        mutationsAllowed
        onClose={() => undefined}
      />,
    );
    const user = userEvent.setup();
    await user.click(screen.getByTestId("job-action-cancel"));
    expect(await screen.findByRole("dialog", { name: /Cancel ss-9-gate-check/ })).toBeVisible();
    expect(screen.getByRole("button", { name: "Cancel job" })).toBeInTheDocument();
  });

  it("sends the entered park reason to VOWP", async () => {
    const fixture = scenario("fresh-single");
    const bridge = await connectedBridge();
    seedStore(fixture.fleet, { bridge });
    const job = { ...fixture.fleet.lanes.review_fix[0]!, state: "reviewing" };
    render(<JobDrawer job={job} open mutationsAllowed onClose={() => undefined} />);
    const user = userEvent.setup();
    await user.click(screen.getByTestId("job-action-park"));
    const reason = await screen.findByRole("textbox", { name: /Reason/ });
    await user.clear(reason);
    await user.type(reason, "Owner paused the run");
    await user.click(screen.getByRole("button", { name: "Park job" }));
    expect(bridge.recordedActions()).toHaveLength(1);
    expect(bridge.recordedActions()[0]).toMatchObject({
      job_id: "ss-9-gate-check",
      action: "park",
      reason: "Owner paused the run",
    });
  });
});

describe("fleet canvas", () => {
  it("lane_counts_come_from_jobs_by_state", () => {
    const fixture = scenario("fresh-single");
    fixture.fleet.projects[0]!.jobs_by_state.ready = 9;
    seedStore(fixture.fleet);
    render(<FleetCanvas />);
    expect(screen.getByTestId("lane-count-ready")).toHaveTextContent("9");
    expect(within(screen.getByTestId("lane-ready")).getAllByTestId(/job-card-/)).toHaveLength(3);
  });

  it("queue_cap_200_says_showing_first_200", () => {
    seedStore(scenario("queue-cap").fleet);
    render(<FleetCanvas />);
    expect(screen.getByTestId("lane-truncated-ready")).toHaveTextContent(
      "showing the first 200 jobs",
    );
  });

  it("canvas_has_an_accessible_text_summary", () => {
    seedStore(scenario("fresh-single").fleet);
    render(<FleetCanvas />);
    expect(screen.getByTestId("ring-summary")).toHaveTextContent(
      /8 worker slots, 3 busy, 5 idle/,
    );
  });

  it("filters lanes and worker cards by project", async () => {
    seedStore(scenario("multi-project").fleet);
    render(<FleetCanvas />);
    const user = userEvent.setup();
    await user.selectOptions(screen.getByLabelText("Project filter"), "northnoir");
    expect(screen.getAllByText("nn-4-score").length).toBeGreaterThan(0);
    expect(screen.queryAllByText("ss-2-desktop-shell")).toHaveLength(0);
    expect(screen.getByTestId("lane-count-ready")).toHaveTextContent("1");
  });

  it("keeps the open drawer in sync with a polled card", async () => {
    const fixture = scenario("fresh-single");
    seedStore(fixture.fleet);
    render(<FleetCanvas />);
    const user = userEvent.setup();
    await user.click(screen.getByTestId("job-card-ss-2-desktop-shell"));
    expect(
      await screen.findByRole("dialog", { name: /Job ss-2-desktop-shell/ }),
    ).toBeVisible();
    expect(screen.getByTestId("job-action-cancel")).toBeEnabled();

    act(() => {
      const fleet = useFleetStore.getState().fleet;
      if (!fleet) throw new Error("fleet missing");
      useFleetStore.getState().acceptFleet({
        ...fleet,
        revision: fleet.revision + 1,
        lanes: {
          ...fleet.lanes,
          running: fleet.lanes.running.map((card) =>
            card.job_id === "ss-2-desktop-shell" ? { ...card, state: "done" } : card,
          ),
        },
      });
    });

    expect(screen.getByTestId("job-action-cancel")).toBeDisabled();
    expect(screen.getByTestId("job-action-park")).toBeDisabled();
    expect(screen.getByTestId("job-action-retry")).toBeDisabled();
  });

  it("shows stale data with a reason and a refresh action", () => {
    seedStore(scenario("stale-cache").fleet);
    render(<FleetCanvas />);
    expect(screen.getByText(/As of/)).toBeInTheDocument();
    expect(screen.getByTestId("fleet-refresh")).toBeEnabled();
  });

  it("has no WCAG 2.2 AA axe violations on the fleet canvas", async () => {
    const fixture = scenario("fresh-single");
    seedStore(fixture.fleet);
    const { container } = render(<FleetCanvas />);
    expect(await wcagViolations(container)).toEqual([]);
  });

  it("axe_has_no_critical_violations_on_fleet_and_capacity", async () => {
    const fixture = scenario("busy");
    seedStore(fixture.fleet);
    const { container } = render(
      <>
        <FleetCanvas />
        <CapacityRail />
      </>,
    );
    const violations = (await wcagViolations(container)).filter(
      (violation) => violation.impact === "critical" || violation.impact === "serious",
    );
    expect(violations).toEqual([]);
  });
});
