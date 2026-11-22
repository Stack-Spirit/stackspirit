import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { FleetView, JobCardView } from "../../bridge/wire";
import { JobRows } from "../../next/JobRows";
import {
  historyJobs,
  jobPhase,
  projectJobs,
  projectSummaries,
  providerModel,
  waitReason,
} from "../../next/summary";

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

const executor = {
  raw: "claude-code:claude-sonnet-5-5",
  harness: "claude-code",
  model: "claude-sonnet-5-5",
  effort: null,
  variant: null,
  provider: "Anthropic",
  display: "Claude Sonnet 5.5",
};

function fleet(): FleetView {
  return {
    projects: [{ name: "demo", state: "active", paused_reason: "", cost_today_usd: null, jobs_by_state: { done: 12, replanned: 1, cancelled: 3 } }],
    inbox: [],
    running: [],
    lanes: {
      running: [card({ job_id: "run", state: "active", executor })],
      ready: [card({ job_id: "wait", reason: "RAM 1.2 GB short" })],
      review_fix: [card({ job_id: "rev", state: "reviewing" })],
      parked_blocked: [],
      done: [],
      history: [card({ job_id: "old", state: "cancelled" }), card({ job_id: "gate", gate: "secret_scan" })],
      counts: {},
      queue_truncated: false,
      queue_shown: 0,
      queue_cap: 200,
    },
  } as unknown as FleetView;
}

describe("active versus history", () => {
  it("project jobs are only active lanes; history is separate", () => {
    const view = fleet();
    expect(projectJobs(view, "demo").map((item) => item.job.job_id)).toEqual(["run", "wait", "rev"]);
    expect(historyJobs(view, "demo").map((job) => job.job_id)).toEqual(["old", "gate"]);
  });

  it("done count comes from jobs_by_state, not cards", () => {
    const [summary] = projectSummaries(fleet());
    expect(summary?.done).toBe(13);
    expect(summary?.cancelled).toBe(3);
  });

  it("shows provider/model, phase and wait reasons", () => {
    const view = fleet();
    expect(providerModel(view.lanes.running[0]!)).toBe("Anthropic · Claude Sonnet 5.5");
    expect(providerModel(card())).toBeNull();
    expect(jobPhase(projectJobs(view, "demo")[2]!)).toBe("reviewing");
    expect(waitReason(card({ reason: "capacity brake" }))).toBe("capacity brake");
    expect(waitReason(card({ state: "pending" }))).toMatch(/dependencies/);
    expect(waitReason(card())).toMatch(/worker slot/);
  });

  it("rows render badges and hide the empty Needs attention filter", () => {
    render(<JobRows jobs={projectJobs(fleet(), "demo")} selectedId={null} onSelect={() => {}} showProject={false} />);
    expect(screen.getByTestId("job-model").textContent).toContain("Claude Sonnet 5.5");
    expect(screen.getByTestId("job-wait-reason").textContent).toContain("RAM 1.2 GB short");
    expect(screen.queryByRole("button", { name: /Needs attention/ })).toBeNull();
    expect(screen.queryByRole("button", { name: /Done/ })).toBeNull();
  });
});
