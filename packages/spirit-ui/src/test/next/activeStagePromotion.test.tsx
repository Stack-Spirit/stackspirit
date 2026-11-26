import { render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { FleetView, JobCardView, JobLanes, WorkerCardView } from "../../bridge/wire";
import { JobRows } from "../../next/JobRows";
import { DagMap } from "../../next/NodeMap";
import {
  activeRunningWorkers,
  layoutDag,
  projectJobs,
  projectSummaries,
} from "../../next/summary";
import { WorkersPage } from "../../next/WorkersPage";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";

function card(overrides: Partial<JobCardView> = {}): JobCardView {
  return {
    job_id: "ss-1",
    project: "demo",
    title: null,
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
    needs: [],
    ...overrides,
  };
}

function worker(job_id: string, overrides: Partial<WorkerCardView> = {}): WorkerCardView {
  return {
    job_id,
    project: "demo",
    kind: "build",
    role: "worker",
    state: "active",
    stalled: false,
    executor: null,
    node: "vps",
    age_secs: 30,
    last_event_age_secs: 4,
    session_id: null,
    ...overrides,
  };
}

function fleet(
  lanes: Partial<JobLanes> = {},
  running: WorkerCardView[] = [],
  jobsByState: Record<string, number> = {},
): FleetView {
  return {
    projects: [
      {
        name: "demo",
        state: "active",
        paused_reason: "",
        cost_today_usd: null,
        jobs_by_state: jobsByState,
      },
    ],
    inbox: [],
    running,
    lanes: {
      running: [],
      ready: [],
      review_fix: [],
      parked_blocked: [],
      done: [],
      history: [],
      counts: {},
      queue_truncated: false,
      queue_shown: 0,
      queue_cap: 200,
      ...lanes,
    },
  } as unknown as FleetView;
}

function reviewFleet(): FleetView {
  return fleet(
    {
      running: [
        card({ job_id: "ss-131", state: "active", kind: "build" }),
        card({
          job_id: "ss-131-verify-0",
          state: "active",
          kind: "verify",
          role: "verifier",
          needs: ["ss-131"],
        }),
      ],
      ready: [card({ job_id: "ss-132", state: "pending", needs: ["ss-131"] })],
    },
    [worker("ss-131"), worker("ss-131-verify-0", { kind: "verify", role: "verifier" })],
    { running: 2, reviewing: 1, ready: 1, done: 4 },
  );
}

afterEach(() => resetFleetStore());

describe("active stage promotion", () => {
  it("replaces the finished Build with its active Review and re-anchors downstream jobs", () => {
    const view = reviewFleet();
    const jobs = projectJobs(view, "demo");

    expect(jobs.map((item) => item.job.job_id)).toEqual(["ss-131-verify-0", "ss-132"]);
    const review = jobs[0]!;
    expect(review.lane).toBe("running");
    expect(review.promotedFrom).toBe("ss-131");
    expect(review.stageTrail).toBe("Build ✓ → Review");
    expect(review.job.needs).toEqual([]);
    expect(jobs[1]!.job.needs).toEqual(["ss-131-verify-0"]);

    const layout = layoutDag("demo", "active", jobs, false);
    expect(layout.nodes.map((node) => node.id)).not.toContain("ss-131");
    const active = layout.nodes.find((node) => node.id === "ss-131-verify-0")!;
    expect(active.kind).toBe("review");
    expect(active.state).toBe("active");
    expect(active.col).toBe(1);
    expect(layout.nodes.find((node) => node.id === "ss-132")).toMatchObject({
      col: 2,
      kind: "build",
    });
    const edges = layout.edges.map((edge) => edge.id);
    expect(edges).toContain("orchestrator->ss-131-verify-0");
    expect(edges).toContain("ss-131-verify-0->ss-132");
    expect(edges).not.toContain("ss-131->ss-132");
    expect(edges).not.toContain("orchestrator->ss-131");
  });

  it("counts one genuinely executing worker, not the waiting Build parent", () => {
    const view = reviewFleet();
    expect(projectSummaries(view)[0]?.running).toBe(1);
    expect(activeRunningWorkers(view, "demo").map((item) => item.job_id)).toEqual([
      "ss-131-verify-0",
    ]);

    useFleetStore.setState({ fleet: view });
    render(<WorkersPage project="demo" onOpenSession={() => undefined} />);
    expect(screen.getByText(/1 running · 1 queued · 1 in review/)).toBeTruthy();
    expect(screen.queryByText(/2 running/)).toBeNull();
  });

  it("renders the stage trail on the promoted DAG card and job row", () => {
    const view = reviewFleet();
    const jobs = projectJobs(view, "demo");
    render(
      <>
        <DagMap
          project="demo"
          rootSub="active"
          label="demo structure"
          jobs={jobs}
          selectedId={null}
          onSelectJob={vi.fn()}
          onSelectRoot={vi.fn()}
        />
        <JobRows jobs={jobs} selectedId={null} onSelect={vi.fn()} showProject={false} />
      </>,
    );

    const node = document.querySelector('[data-node-id="ss-131-verify-0"]') as HTMLElement;
    expect(node).toHaveAttribute("data-stage-trail", "Build ✓ → Review");
    expect(within(node).getByTestId("dag-stage-trail")).toHaveTextContent("Build ✓ → Review");
    expect(screen.getByTestId("job-stage-trail")).toHaveTextContent("Build ✓ → Review");
    expect(document.querySelector('[data-node-id="ss-131"]')).toBeNull();
  });

  it("keeps the failed Review round in a Fix stage trail", () => {
    const view = fleet(
      {
        running: [
          card({ job_id: "ss-140", state: "active", kind: "build" }),
          card({ job_id: "ss-140-fix-1", state: "active", kind: "fix", needs: ["ss-140"] }),
        ],
      },
      [worker("ss-140"), worker("ss-140-fix-1", { kind: "fix" })],
    );
    const jobs = projectJobs(view, "demo");

    expect(jobs.map((item) => item.job.job_id)).toEqual(["ss-140-fix-1"]);
    expect(jobs[0]!.stageTrail).toBe("Build ✓ → Review ✗ → Fix 1");
    expect(jobs[0]!.promotedFrom).toBe("ss-140");

    render(<JobRows jobs={jobs} selectedId={null} onSelect={vi.fn()} showProject={false} />);
    expect(screen.getByTestId("job-stage-trail")).toHaveTextContent(
      "Build ✓ → Review ✗ → Fix 1",
    );
  });

  it("does not promote a Review that is still queued behind the running Build", () => {
    const view = fleet(
      {
        running: [card({ job_id: "ss-150", state: "active", kind: "build" })],
        ready: [
          card({
            job_id: "ss-150-verify-0",
            state: "pending",
            kind: "verify",
            role: "verifier",
            needs: ["ss-150"],
          }),
        ],
      },
      [worker("ss-150")],
    );
    const jobs = projectJobs(view, "demo");

    expect(jobs.map((item) => item.job.job_id)).toEqual(["ss-150", "ss-150-verify-0"]);
    expect(jobs[0]!.stageTrail).toBeUndefined();
    expect(jobs[0]!.promotedFrom).toBeUndefined();
  });
});
