import { readFileSync } from "node:fs";

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

  it("propagates a typed backend error instead of swallowing the click", async () => {
    const bridge = new TauriBridge();
    invoke.mockRejectedValueOnce({
      code: "vowp_command",
      message: "stackspirit exited 2: unexpected argument '--all'",
      hint: "Update the engine on the host, then retry.",
      retryable: true,
    });

    await expect(bridge.history.load("demo")).rejects.toMatchObject({
      code: "vowp_command",
      message: "stackspirit exited 2: unexpected argument '--all'",
      hint: "Update the engine on the host, then retry.",
      retryable: true,
    });
  });

  it("keeps every history command registered in the Tauri handler", () => {
    // The renderer tests can only prove the names it sends; this reads the host's
    // actual `generate_handler!` list so a command that is dropped or renamed in Rust
    // fails here instead of turning the History tab into a silent no-op.
    const lib = readFileSync(
      new URL("../../src-tauri/src/lib.rs", import.meta.url),
      "utf8",
    );
    const handler = lib.slice(lib.indexOf("tauri::generate_handler!["));
    // The trailing comma matters: a renamed command such as `project_history_v2`
    // must not satisfy a plain prefix check.
    for (const command of [
      "project_history",
      "project_gc_preview",
      "project_gc_apply",
    ]) {
      expect(handler).toMatch(
        new RegExp(`commands::history::${command}\\s*,`),
      );
    }
  });
});
