import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { JobCardView } from "../../bridge/wire";
import { DagMap, Whiteboard } from "../../next/NodeMap";
import { COMPACT_JOB_THRESHOLD, layoutDag } from "../../next/summary";
import type { ProjectJob } from "../../next/summary";

function card(job_id: string, overrides: Partial<JobCardView> = {}): JobCardView {
  return {
    job_id,
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

const executor = {
  raw: "claude-code:claude-sonnet-5-5",
  harness: "claude-code",
  model: "claude-sonnet-5-5",
  effort: null,
  variant: null,
  provider: "Anthropic",
  display: "Claude Sonnet 5.5",
};

const running = (id: string, o: Partial<JobCardView> = {}): ProjectJob => ({
  lane: "running",
  job: card(id, { state: "active", executor, ...o }),
});
const queued = (id: string, needs: string[] = [], o: Partial<JobCardView> = {}): ProjectJob => ({
  lane: "ready",
  job: card(id, { state: "pending", needs, ...o }),
});

const converging: ProjectJob[] = [
  running("ss-116b"),
  running("ss-117"),
  queued("ss-118", ["ss-116b", "ss-117"]),
  queued("ss-119", ["ss-118"]),
  queued("ss-120"),
];

describe("layoutDag", () => {
  it("places active and independent queued jobs in column 1 and dependents behind their needs", () => {
    const layout = layoutDag("demo", "active", converging, false);
    const col = (id: string) => layout.nodes.find((n) => n.id === id)?.col;
    expect(col("orchestrator")).toBe(0);
    expect(col("ss-116b")).toBe(1);
    expect(col("ss-117")).toBe(1);
    expect(col("ss-120")).toBe(1);
    expect(col("ss-118")).toBe(2);
    expect(col("ss-119")).toBe(3);
  });

  it("draws converging edges for a job with two upstream jobs and root edges for the rest", () => {
    const ids = layoutDag("demo", "", converging, false).edges.map((e) => e.id);
    expect(ids).toContain("ss-116b->ss-118");
    expect(ids).toContain("ss-117->ss-118");
    expect(ids).toContain("ss-118->ss-119");
    expect(ids).toContain("orchestrator->ss-116b");
    expect(ids).toContain("orchestrator->ss-120");
    expect(ids).not.toContain("orchestrator->ss-118");
  });

  it("uses cubic Bézier paths and ignores unknown or cyclic needs", () => {
    const layout = layoutDag(
      "demo",
      "",
      [queued("a", ["b", "gone"]), queued("b", ["a"]), queued("c", ["c"])],
      false,
    );
    expect(layout.edges.every((e) => /^M [\d. -]+ C /.test(e.path))).toBe(true);
    expect(layout.nodes).toHaveLength(4);
  });

  it("chains Build -> Review -> Fix and tags kind and state", () => {
    const layout = layoutDag(
      "demo",
      "",
      [
        running("ss-1"),
        queued("ss-1-verify-0", ["ss-1"], { kind: "verify", role: "verifier" }),
        queued("ss-1-fix-1", ["ss-1-verify-0"], { kind: "fix" }),
      ],
      false,
    );
    const n = (id: string) => layout.nodes.find((x) => x.id === id);
    expect([n("ss-1")?.kind, n("ss-1-verify-0")?.kind, n("ss-1-fix-1")?.kind]).toEqual(["build", "review", "fix"]);
    expect([n("ss-1")?.state, n("ss-1-verify-0")?.state]).toEqual(["active", "queued"]);
    expect([n("ss-1")?.col, n("ss-1-verify-0")?.col, n("ss-1-fix-1")?.col]).toEqual([1, 2, 3]);
  });
});

describe("DagMap", () => {
  const props = { project: "demo", rootSub: "active", label: "demo structure", selectedId: null, onSelectRoot: vi.fn() };

  it("renders converging edges and shows verbose metadata on small graphs", () => {
    render(<DagMap {...props} jobs={converging.slice(0, 3)} onSelectJob={vi.fn()} />);
    expect(screen.getByTestId("dag-canvas").dataset.compact).toBe("false");
    expect(document.querySelector('[data-edge="ss-116b->ss-118"]')).not.toBeNull();
    expect(document.querySelector('[data-edge="ss-117->ss-118"]')).not.toBeNull();
    expect(screen.getAllByText(/Claude Sonnet 5.5/).length).toBeGreaterThan(0);
  });

  it("switches to compact nodes above the job threshold and hides the metadata", () => {
    const many = Array.from({ length: COMPACT_JOB_THRESHOLD + 1 }, (_, i) => running(`ss-${i}`));
    render(<DagMap {...props} jobs={many} onSelectJob={vi.fn()} />);
    expect(screen.getByTestId("dag-canvas").dataset.compact).toBe("true");
    expect(screen.queryByText(/Claude Sonnet 5.5/)).toBeNull();
    expect(screen.getAllByTestId("dag-node")).toHaveLength(many.length + 1);
  });

  it("shows distinct Build, Review and Fix nodes with active versus queued badges", () => {
    render(
      <DagMap
        {...props}
        jobs={[
          running("ss-1"),
          running("ss-3-verify-0", { kind: "verify" }),
          queued("ss-3-fix-1", ["ss-3-verify-0"], { kind: "fix" }),
          queued("ss-2", [], { kind: "build" }),
        ]}
        onSelectJob={vi.fn()}
      />,
    );
    const node = (id: string) => document.querySelector<HTMLElement>(`[data-node-id="${id}"]`) as HTMLElement;
    expect(within(node("ss-1")).getByText("Build")).toBeTruthy();
    expect(within(node("ss-1")).getByText("Active")).toBeTruthy();
    expect(within(node("ss-3-verify-0")).getByText("Review")).toBeTruthy();
    expect(within(node("ss-3-verify-0")).getByText("Active")).toBeTruthy();
    expect(within(node("ss-3-fix-1")).getByText("Fix")).toBeTruthy();
    expect(within(node("ss-3-fix-1")).getByText("Queued")).toBeTruthy();
    expect(node("ss-3-fix-1").dataset.state).toBe("queued");
    expect(node("ss-3-verify-0").dataset.kind).toBe("review");
  });

  it("selects a job on click", () => {
    const onSelectJob = vi.fn();
    render(<DagMap {...props} jobs={converging.slice(0, 2)} onSelectJob={onSelectJob} />);
    fireEvent.click(document.querySelector('[data-node-id="ss-117"]') as HTMLElement);
    expect(onSelectJob).toHaveBeenCalledWith("ss-117");
  });
});

describe("Whiteboard", () => {
  function open(onClose = vi.fn()) {
    render(
      <Whiteboard
        project="demo"
        rootSub="active"
        jobs={converging}
        onClose={onClose}
        onOpenJob={vi.fn()}
        onOpenSession={vi.fn()}
      />,
    );
    return onClose;
  }

  it("is a detailed (non-compact) dialog with zoom controls", () => {
    open();
    expect(screen.getByRole("dialog", { name: /whiteboard/i })).toBeTruthy();
    expect(screen.getByTestId("dag-canvas").dataset.compact).toBe("false");
    const zoom = screen.getByTestId("whiteboard-zoom");
    expect(zoom.textContent).toBe("100%");
    fireEvent.click(screen.getByRole("button", { name: "Zoom in" }));
    expect(zoom.textContent).toBe("125%");
    fireEvent.click(screen.getByRole("button", { name: "Zoom out" }));
    fireEvent.click(screen.getByRole("button", { name: "Zoom out" }));
    expect(zoom.textContent).toBe("80%");
    fireEvent.click(zoom);
    expect(zoom.textContent).toBe("100%");
    fireEvent.click(screen.getByRole("button", { name: "Fit view" }));
    expect(screen.getByTestId("whiteboard-stage").style.transform).toContain("scale(");
  });

  it("opens a node detail card and closes on Close and Escape", () => {
    const onClose = open();
    fireEvent.click(document.querySelector('[data-node-id="ss-118"]') as HTMLElement);
    const cardEl = screen.getByTestId("whiteboard-card");
    expect(within(cardEl).getByText("ss-116b, ss-117")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(onClose).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it("pans on drag", () => {
    open();
    const viewport = screen.getByTestId("whiteboard-viewport");
    const before = screen.getByTestId("whiteboard-stage").style.transform;
    fireEvent(viewport, new MouseEvent("pointerdown", { clientX: 10, clientY: 10, button: 0, bubbles: true }));
    fireEvent(window, new MouseEvent("pointermove", { clientX: 90, clientY: 40 }));
    fireEvent(window, new MouseEvent("pointerup"));
    expect(screen.getByTestId("whiteboard-stage").style.transform).not.toBe(before);
  });
});
