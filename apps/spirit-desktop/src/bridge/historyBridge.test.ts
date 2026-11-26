import { beforeEach, describe, expect, it, vi } from "vitest";

import type { GcReportView, HistoryJobView } from "@stackspirit/ui";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import { TauriBridge } from "./TauriBridge";

const JOBS: HistoryJobView[] = [
  {
    job_id: "ss-1",
    project: "demo",
    title: "Old build",
    kind: "build",
    role: "worker",
    state: "done",
    updated_at: "2026-11-01T10:00:00Z",
  },
];

const PREVIEW: GcReportView = {
  dry_run: true,
  classes: [
    {
      class: "closed_session",
      files: 2,
      bytes: 2048,
      eligible_files: 1,
      eligible_bytes: 1024,
    },
  ],
  refused: 0,
  deleted_files: 0,
  freed_bytes: 0,
  compacted_sessions: 0,
  eligible_files: 1,
  eligible_bytes: 1024,
  errors: [],
};

const APPLIED: GcReportView = {
  ...PREVIEW,
  dry_run: false,
  deleted_files: 1,
  freed_bytes: 1024,
  compacted_sessions: 1,
};

describe("TauriBridge history", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it("runs the exact history commands with the exact argument shapes", async () => {
    const bridge = new TauriBridge();

    invoke.mockResolvedValueOnce(JOBS);
    await expect(bridge.history.load("demo")).resolves.toEqual(JOBS);
    expect(invoke).toHaveBeenLastCalledWith("project_history", { project: "demo" });

    invoke.mockResolvedValueOnce(PREVIEW);
    await expect(bridge.history.gcPreview("demo")).resolves.toEqual(PREVIEW);
    expect(invoke).toHaveBeenLastCalledWith("project_gc_preview", {
      project: "demo",
    });

    invoke.mockResolvedValueOnce(APPLIED);
    await expect(bridge.history.gcApply("demo")).resolves.toEqual(APPLIED);
    expect(invoke).toHaveBeenLastCalledWith("project_gc_apply", {
      project: "demo",
      confirmed: true,
    });

    expect(invoke.mock.calls.map(([command]) => command)).toEqual([
      "project_history",
      "project_gc_preview",
      "project_gc_apply",
    ]);
  });
});
