import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import type { FleetView, JobCardView } from "../../bridge/wire";
import { filterHistory } from "../../next/HistoryBrowser";
import { projectJobs, projectSummaries } from "../../next/summary";
import { WorkersPage } from "../../next/WorkersPage";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";

function card(overrides: Partial<JobCardView> = {}): JobCardView {
  return {
    job_id: "ss-1",
    project: "demo",
    title: "One",
    kind: "build",
    role: "worker",
    state: "ready",
    priority: 1,
    age_secs: 30,
    reason: null,
    detail: null,
    gate: null,
    gate_at: null,
    gate_evidence: null,
    executor: null,
    session_id: null,
    ...overrides,
  };
}

function fleet(): FleetView {
  return {
    projects: [
      {
        name: "demo",
        state: "active",
        paused_reason: "",
        cost_today_usd: null,
        jobs_by_state: {},
      },
    ],
    inbox: [],
    running: [],
    lanes: {
      running: [],
      ready: [],
      review_fix: [],
      parked_blocked: [
        card({ job_id: "ss-1-fix-1", state: "failed" }),
        card({ job_id: "ss-1-verify-1", state: "cancelled" }),
        card({ job_id: "ss-2-fix-1", state: "failed" }),
        card({ job_id: "ss-3", state: "failed" }),
        card({ job_id: "ss-4-fix-1", state: "failed" }),
      ],
      done: [
        card({ job_id: "ss-1", state: "replanned" }),
        card({ job_id: "ss-4", state: "done" }),
      ],
      history: [],
      counts: {},
      queue_truncated: false,
      queue_shown: 0,
      queue_cap: 200,
    },
  } as unknown as FleetView;
}

afterEach(() => resetFleetStore());

describe("Needs attention", () => {
  it("hides failed fix/verify children of replanned or done parents only", () => {
    const view = fleet();
    const ids = projectJobs(view, "demo").map((item) => item.job.job_id);
    // ss-2 has no known parent and ss-3 is a genuine failure: both stay visible.
    expect(ids).toEqual(["ss-2-fix-1", "ss-3"]);
    expect(projectSummaries(view)[0]?.blocked).toBe(2);
  });

  it("keeps a failed child visible while its parent is still live", () => {
    const view = fleet();
    view.lanes.running = [card({ job_id: "ss-2", state: "active" })];
    expect(projectJobs(view, "demo").map((item) => item.job.job_id)).toContain(
      "ss-2-fix-1",
    );
  });
});

describe("history filter", () => {
  const jobs = [
    {
      job_id: "a",
      project: "demo",
      title: "Alpha",
      kind: "build",
      role: "worker",
      state: "done",
      updated_at: null,
    },
    {
      job_id: "b",
      project: "demo",
      title: "Beta",
      kind: "fix",
      role: "worker",
      state: "failed",
      updated_at: null,
    },
    {
      job_id: "c",
      project: "demo",
      title: "Gamma",
      kind: "build",
      role: "worker",
      state: "cancelled",
      updated_at: null,
    },
    {
      job_id: "d",
      project: "demo",
      title: "Delta",
      kind: "build",
      role: "worker",
      state: "replanned",
      updated_at: null,
    },
  ];

  it("filters by status group and search text", () => {
    expect(filterHistory(jobs, "completed", "").map((j) => j.job_id)).toEqual([
      "a",
      "d",
    ]);
    expect(filterHistory(jobs, "failed", "").map((j) => j.job_id)).toEqual([
      "b",
    ]);
    expect(filterHistory(jobs, "cancelled", "").map((j) => j.job_id)).toEqual([
      "c",
    ]);
    expect(filterHistory(jobs, "all", "gam").map((j) => j.job_id)).toEqual([
      "c",
    ]);
  });
});

describe("History tab", () => {
  async function open() {
    const bridge = new MockBridge("busy");
    useFleetStore.setState({ bridge, fleet: fleet() });
    const user = userEvent.setup();
    render(<WorkersPage project="demo" onOpenSession={() => undefined} />);
    await user.click(screen.getByRole("button", { name: /^History/ }));
    return { bridge, user };
  }

  it("loads historic jobs only after clicking Load history", async () => {
    const { bridge, user } = await open();
    expect(bridge.historyCalls).toEqual([]);
    expect(screen.queryByTestId("history-list")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Load history" }));
    const list = await screen.findByTestId("history-list");
    expect(bridge.historyCalls).toEqual([{ call: "load", project: "demo" }]);
    expect(within(list).getAllByRole("listitem")).toHaveLength(4);

    await user.click(screen.getByRole("button", { name: /^Failed/ }));
    expect(
      within(screen.getByTestId("history-list")).getAllByRole("listitem"),
    ).toHaveLength(1);
    await user.click(screen.getByRole("button", { name: /^All/ }));
    await user.type(
      screen.getByRole("searchbox", { name: "Search history" }),
      "dropped",
    );
    expect(
      within(screen.getByTestId("history-list")).getAllByRole("listitem"),
    ).toHaveLength(1);
  });

  it("does not repeat fleet history rows after loading", async () => {
    const bridge = new MockBridge("busy");
    const view = fleet();
    view.lanes.done = [];
    view.lanes.history = [
      card({ job_id: "ss-9-history-view", state: "done", title: "History view" }),
      card({ job_id: "ss-fleet-only", state: "cancelled", title: "Fleet only" }),
    ];
    useFleetStore.setState({ bridge, fleet: view });
    const user = userEvent.setup();
    render(<WorkersPage project="demo" onOpenSession={() => undefined} />);
    await user.click(screen.getByRole("button", { name: /^History/ }));

    const before = screen.getByTestId("history-fleet-list");
    expect(within(before).getAllByRole("listitem")).toHaveLength(2);

    await user.click(screen.getByRole("button", { name: "Load history" }));
    const loaded = await screen.findByTestId("history-list");
    expect(within(loaded).getAllByRole("listitem")).toHaveLength(4);

    // ss-9-history-view came from both sources; only the on-demand row remains.
    const after = screen.getByTestId("history-fleet-list");
    expect(within(after).getAllByRole("listitem")).toHaveLength(1);
    expect(within(after).queryByText("History view")).toBeNull();
    expect(within(after).getByText("Fleet only")).toBeTruthy();
  });

  it("previews clean-up, deletes nothing until confirmed, then reports what was freed", async () => {
    const { bridge, user } = await open();
    await user.click(screen.getByRole("button", { name: "Clean up history" }));
    const preview = await screen.findByTestId("cleanup-preview");
    expect(preview.textContent).toContain("13 files");
    expect(preview.textContent).toContain(
      "Active work and the job database are never touched",
    );
    expect(bridge.historyCalls).toEqual([{ call: "preview", project: "demo" }]);

    await user.click(screen.getByTestId("cleanup-confirm"));
    const result = await screen.findByTestId("cleanup-result");
    expect(result.textContent).toContain("Freed 13 files");
    expect(bridge.historyCalls.map((c) => c.call)).toEqual([
      "preview",
      "apply",
    ]);
    await waitFor(() =>
      expect(screen.queryByTestId("cleanup-preview")).toBeNull(),
    );
  });

  it("cancelling the preview never applies", async () => {
    const { bridge, user } = await open();
    await user.click(screen.getByRole("button", { name: "Clean up history" }));
    await screen.findByTestId("cleanup-preview");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(bridge.historyCalls.map((c) => c.call)).toEqual(["preview"]);
  });
});
