import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import type { SpiritBridge } from "../../bridge/SpiritBridge";
import type { FleetView, GcReportView, HistoryJobView, JobCardView } from "../../bridge/wire";
import { HistoryBrowser, filterHistory } from "../../next/HistoryBrowser";
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

/**
 * The desktop History tab regressed to a silent no-op once: a click reached the
 * backend, the backend refused, and the page still looked unchanged. These tests pin
 * the three visible outcomes (busy, result, typed error with a retry) so that cannot
 * happen again without a red test.
 */
describe("HistoryBrowser outcomes", () => {
  const rows: HistoryJobView[] = [
    {
      job_id: "ss-1",
      project: "demo",
      title: "Old build",
      kind: "build",
      role: "worker",
      state: "done",
      updated_at: "2026-11-26T10:00:00Z",
    },
    {
      job_id: "ss-2",
      project: "demo",
      title: "Dropped",
      kind: "build",
      role: "worker",
      state: "cancelled",
      updated_at: "2026-11-25T10:00:00Z",
    },
  ];

  const preview: GcReportView = {
    dry_run: true,
    classes: [
      {
        class: "run_output",
        files: 2,
        bytes: 2048,
        eligible_files: 2,
        eligible_bytes: 2000,
      },
    ],
    refused: 0,
    deleted_files: 0,
    freed_bytes: 0,
    compacted_sessions: 0,
    eligible_files: 2,
    eligible_bytes: 2000,
    errors: [],
  };

  const applied: GcReportView = {
    ...preview,
    dry_run: false,
    deleted_files: 2,
    freed_bytes: 2000,
    compacted_sessions: 1,
  };

  function historyApi(
    overrides: Partial<NonNullable<SpiritBridge["history"]>> = {},
  ): NonNullable<SpiritBridge["history"]> {
    return {
      load: vi.fn(async () => rows),
      gcPreview: vi.fn(async () => preview),
      gcApply: vi.fn(async () => applied),
      ...overrides,
    };
  }

  it("shows a busy line, then the real rows and their count", async () => {
    let release!: (value: HistoryJobView[]) => void;
    const load = vi.fn(
      () =>
        new Promise<HistoryJobView[]>((resolve) => {
          release = resolve;
        }),
    );
    const user = userEvent.setup();
    render(<HistoryBrowser project="demo" api={historyApi({ load })} />);

    await user.click(screen.getByRole("button", { name: "Load history" }));
    expect(screen.getByTestId("history-loading").textContent).toContain(
      "Loading history",
    );
    expect(screen.queryByTestId("history-list")).toBeNull();

    release(rows);
    const list = await screen.findByTestId("history-list");
    expect(within(list).getAllByRole("listitem")).toHaveLength(2);
    expect(screen.getByTestId("history-loaded").textContent).toContain(
      "Loaded 2 historic jobs for demo",
    );
  });

  it("surfaces a load error with its hint and a retry that replaces it", async () => {
    const load = vi
      .fn<NonNullable<SpiritBridge["history"]>["load"]>()
      .mockRejectedValueOnce({
        code: "vowp_command",
        message: "stackspirit exited 2",
        hint: "Install StackSpirit on the host, then retry.",
        retryable: true,
      })
      .mockResolvedValueOnce(rows);
    const user = userEvent.setup();
    render(<HistoryBrowser project="demo" api={historyApi({ load })} />);

    await user.click(screen.getByRole("button", { name: "Load history" }));
    const alert = await screen.findByTestId("history-error");
    expect(alert.getAttribute("data-code")).toBe("vowp_command");
    expect(alert.textContent).toContain("Could not load history.");
    expect(alert.textContent).toContain("stackspirit exited 2");
    expect(alert.textContent).toContain("Install StackSpirit on the host, then retry.");

    await user.click(within(alert).getByRole("button", { name: "Try again" }));
    await screen.findByTestId("history-list");
    expect(screen.queryByTestId("history-error")).toBeNull();
    expect(load).toHaveBeenCalledTimes(2);
  });

  it("surfaces a clean-up preview error instead of doing nothing", async () => {
    const gcPreview = vi
      .fn<NonNullable<SpiritBridge["history"]>["gcPreview"]>()
      .mockRejectedValueOnce({
        code: "remote_timeout",
        message: "the host did not answer in time",
        hint: null,
        retryable: true,
      });
    const user = userEvent.setup();
    render(<HistoryBrowser project="demo" api={historyApi({ gcPreview })} />);

    await user.click(screen.getByRole("button", { name: "Clean up history" }));
    const alert = await screen.findByTestId("cleanup-error");
    expect(alert.getAttribute("data-code")).toBe("remote_timeout");
    expect(alert.textContent).toContain("Could not check the clean-up.");
    expect(alert.textContent).toContain("the host did not answer in time");
    expect(screen.queryByTestId("cleanup-preview")).toBeNull();
  });

  it("keeps the preview open and surfaces an apply error", async () => {
    const gcApply = vi
      .fn<NonNullable<SpiritBridge["history"]>["gcApply"]>()
      .mockRejectedValueOnce({
        code: "vowp_command",
        message: "stackspirit exited 1",
        hint: null,
        retryable: false,
      });
    const user = userEvent.setup();
    render(<HistoryBrowser project="demo" api={historyApi({ gcApply })} />);

    await user.click(screen.getByRole("button", { name: "Clean up history" }));
    await screen.findByTestId("cleanup-preview");
    await user.click(screen.getByTestId("cleanup-confirm"));

    const alert = await screen.findByTestId("cleanup-error");
    expect(alert.getAttribute("data-code")).toBe("vowp_command");
    expect(alert.textContent).toContain("Could not clean up.");
    expect(alert.textContent).toContain("stackspirit exited 1");
    expect(screen.getByTestId("cleanup-preview")).toBeTruthy();
    expect(screen.queryByTestId("cleanup-result")).toBeNull();
  });

  it("reports freed files and compacted sessions after a confirmed apply", async () => {
    const user = userEvent.setup();
    render(<HistoryBrowser project="demo" api={historyApi()} />);

    await user.click(screen.getByRole("button", { name: "Clean up history" }));
    await screen.findByTestId("cleanup-preview");
    await user.click(screen.getByTestId("cleanup-confirm"));

    const result = await screen.findByTestId("cleanup-result");
    expect(result.textContent).toContain("Freed 2 files");
    expect(result.textContent).toContain("compacted 1 session");
  });
});
