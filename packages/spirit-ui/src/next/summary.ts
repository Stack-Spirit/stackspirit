import { harnessLabel, executorProviderLabel } from "../domain/executor";
import type { FleetView, MergeView, InboxItemView, JobCardView, JobLanes } from "../bridge/wire";

/** What the sidebar tree and the server page show for one project. */
export interface ProjectSummary {
  name: string;
  state: string;
  running: number;
  ready: number;
  review: number;
  blocked: number;
  /** Real completed count from `jobs_by_state` (done + replanned). */
  done: number;
  cancelled: number;
  lastMerge: MergeView | null;
  inbox: number;
}

export type LaneKey = "running" | "ready" | "review_fix" | "parked_blocked" | "done";
export type NodeTone = "ok" | "neutral" | "info" | "warn" | "danger" | "faint";

/** Lanes in the order the node map and row filters show them. */
export const LANE_INFO: ReadonlyArray<{ key: LaneKey; label: string; tone: NodeTone }> = [
  { key: "running", label: "Running", tone: "ok" },
  { key: "ready", label: "Queued / Waiting", tone: "neutral" },
  { key: "review_fix", label: "In review / fixing", tone: "info" },
  { key: "parked_blocked", label: "Needs attention", tone: "warn" },
  { key: "done", label: "Done", tone: "faint" },
];

/** Lanes shown by default; `done` lives behind the History tab. */
export const ACTIVE_LANES: ReadonlyArray<LaneKey> = ["running", "ready", "review_fix", "parked_blocked"];

export interface ProjectJob {
  lane: LaneKey;
  job: JobCardView;
}

export function openInbox(fleet: FleetView | null, project?: string | null): InboxItemView[] {
  if (!fleet) return [];
  return fleet.inbox.filter(
    (item) => item.state === "open" && (project === undefined || item.project === project),
  );
}

function laneJobs(lanes: JobLanes, key: LaneKey): JobCardView[] {
  return lanes[key];
}

const CHILD_JOB = /^(.+)-(?:fix|verify)-[^-].*$/;

/**
 * A retired failed/cancelled `*-fix-*` or `*-verify-*` child whose parent job is already
 * `replanned` or `done`: it can no longer need a decision, so Needs attention hides it.
 * Genuine failures (parent still live, or not a child job) stay visible.
 */
export function isRetiredChild(job: JobCardView, fleet: FleetView): boolean {
  if (!/fail|cancel/i.test(job.state)) return false;
  const parentId = CHILD_JOB.exec(job.job_id)?.[1];
  if (!parentId) return false;
  const cards = [
    ...fleet.lanes.done,
    ...(fleet.lanes.history ?? []),
    ...fleet.lanes.running,
    ...fleet.lanes.ready,
    ...fleet.lanes.review_fix,
    ...fleet.lanes.parked_blocked,
  ];
  const parent = cards.find((card) => card.job_id === parentId && card.project === job.project);
  return parent !== undefined && /^(replanned|done)$/.test(parent.state);
}

/** Every job of a project (or of all projects when `project` is null), tagged with its lane. */
export function projectJobs(fleet: FleetView | null, project: string | null): ProjectJob[] {
  if (!fleet) return [];
  return LANE_INFO.filter(({ key }) => ACTIVE_LANES.includes(key)).flatMap(({ key }) =>
    laneJobs(fleet.lanes, key)
      .filter((job) => project === null || job.project === project)
      .filter((job) => key !== "parked_blocked" || !isRetiredChild(job, fleet))
      .map((job) => ({ lane: key, job })),
  );
}

/** Historical work for the History tab: cancelled jobs and past gate failures. */
export function historyJobs(fleet: FleetView | null, project: string | null): JobCardView[] {
  if (!fleet) return [];
  return [...(fleet.lanes.history ?? []), ...fleet.lanes.done].filter(
    (job) => project === null || job.project === project,
  );
}

export function findJob(fleet: FleetView | null, project: string, jobId: string): ProjectJob | null {
  return projectJobs(fleet, project).find((item) => item.job.job_id === jobId) ?? null;
}

/** A job reads as failed when VOWP put it in the attention lane with a failure state. */
export function jobTone(item: ProjectJob): NodeTone {
  if (item.lane === "parked_blocked" && /fail|error|crash/i.test(item.job.state)) return "danger";
  return LANE_INFO.find((lane) => lane.key === item.lane)?.tone ?? "neutral";
}

/** Concise provider and model for an active job, or null when no executor is known. */
export function providerModel(job: JobCardView): string | null {
  const executor = job.executor;
  if (!executor) return null;
  const display = executor.display || executor.model;
  if (!display) return null;
  const parts = [executorProviderLabel(executor.provider), harnessLabel(executor.harness), display].filter(
    (part, index, all) => part && part !== "unknown" && all.indexOf(part) === index,
  );
  return parts.join(" · ");
}

/** Current phase of an active job: running, stalled, gating, reviewing or fixing. */
export function jobPhase(item: ProjectJob): string | null {
  const { job } = item;
  if (item.lane === "running") return job.state === "stalled" ? "stalled" : "running";
  if (item.lane === "review_fix") return /^(gating|reviewing|fixing)$/.test(job.state) ? job.state : "gating";
  return null;
}

/** Why a queued job is waiting. Uses VOWP's `waiting_reason`, else the queue state. */
export function waitReason(job: JobCardView): string {
  const reason = job.reason?.trim();
  if (reason) return reason;
  return job.state === "pending" ? "Waiting on its dependencies" : "Waiting for a free worker slot";
}

export function jobName(job: JobCardView): string {
  return job.title?.trim() || job.job_id;
}

export function projectSummaries(fleet: FleetView | null): ProjectSummary[] {
  if (!fleet) return [];
  const count = (list: Array<{ project: string }>, name: string) =>
    list.filter((item) => item.project === name).length;
  return fleet.projects.map((project) => ({
    name: project.name,
    state: project.state,
    running: count(fleet.running, project.name),
    ready: count(fleet.lanes.ready, project.name),
    review:
      (project.jobs_by_state.gating ?? 0) +
      (project.jobs_by_state.reviewing ?? 0) +
      (project.jobs_by_state.fixing ?? 0),
    blocked: fleet.lanes.parked_blocked.filter(
      (job) => job.project === project.name && !isRetiredChild(job, fleet),
    ).length,
    done: (project.jobs_by_state.done ?? 0) + (project.jobs_by_state.replanned ?? 0),
    cancelled: project.jobs_by_state.cancelled ?? 0,
    lastMerge: project.last_merge ?? null,
    inbox: openInbox(fleet, project.name).length,
  }));
}

/** Fraction used, clamped to 0..1, or null when the total is unknown. */
export function usedRatio(free: number | null | undefined, total: number | null | undefined): number | null {
  if (free == null || total == null || total <= 0) return null;
  return Math.min(1, Math.max(0, 1 - free / total));
}

export function meterTone(ratio: number | null): "" | "warn" | "danger" {
  if (ratio == null) return "";
  if (ratio >= 0.9) return "danger";
  if (ratio >= 0.75) return "warn";
  return "";
}

// ---- Dependency-DAG layout (Structure map and Whiteboard) ----
/** What kind of session a node is: the orchestrator root, a Build worker, a Review verifier, or a Fix round. */
export type DagKind = "root" | "build" | "review" | "fix";
/** Whether the session is running now, waiting in the queue, or stopped and needing a decision. */
export type DagState = "root" | "active" | "queued" | "attention";

export interface DagNode {
  id: string;
  label: string;
  /** Short title shown in compact mode next to the id. */
  title: string;
  kind: DagKind;
  state: DagState;
  tone: NodeTone;
  /** Verbose metadata line: phase, provider, harness and model, or the wait reason. */
  sub: string;
  /** Provider · harness · model, when an executor is known. */
  model: string | null;
  wait: string | null;
  col: number;
  row: number;
  x: number;
  y: number;
  needs: string[];
}

export interface DagEdge {
  id: string;
  from: string;
  to: string;
  /** SVG cubic Bézier path from the parent's right edge to the child's left edge. */
  path: string;
}

export interface DagLayout {
  nodes: DagNode[];
  edges: DagEdge[];
  width: number;
  height: number;
  nodeW: number;
  nodeH: number;
  /** Number of job nodes, excluding the root. */
  jobCount: number;
}

/** More than this many job nodes switches the embedded map to compact nodes. */
export const COMPACT_JOB_THRESHOLD = 5;
/** Container widths below this (px) also switch to compact nodes. */
export const COMPACT_WIDTH = 520;

const PAD = 18;
const COL_GAP = 64;
const ROW_GAP = 12;
const SIZES = {
  full: { w: 236, h: 62 },
  compact: { w: 156, h: 30 },
};

export function dagKind(kind: string): DagKind {
  if (kind === "verify") return "review";
  if (kind === "fix") return "fix";
  return "build";
}

export const KIND_LABEL: Record<DagKind, string> = { root: "Orchestrator", build: "Build", review: "Review", fix: "Fix" };
export const STATE_LABEL: Record<DagState, string> = {
  root: "Active",
  active: "Active",
  queued: "Queued",
  attention: "Needs attention",
};

function dagState(item: ProjectJob): DagState {
  if (item.lane === "running" || item.lane === "review_fix") return "active";
  if (item.lane === "ready") return "queued";
  return "attention";
}

/**
 * Lays active and queued jobs out as a left-to-right dependency DAG. The orchestrator
 * is column 0; a job sits one column right of its deepest upstream job that is itself
 * in the graph. Jobs with no upstream in the graph (active work, and queued work that
 * waits only on capacity) hang directly off the root in column 1. Upstream ids that
 * are not in the graph (finished jobs) are ignored.
 */
export function layoutDag(
  project: string,
  rootSub: string,
  jobs: ProjectJob[],
  compact: boolean,
): DagLayout {
  const size = compact ? SIZES.compact : SIZES.full;
  const byId = new Map<string, ProjectJob>();
  for (const item of jobs) if (!byId.has(item.job.job_id)) byId.set(item.job.job_id, item);
  const upstream = (id: string): string[] =>
    (byId.get(id)?.job.needs ?? []).filter((need, i, all) => need !== id && byId.has(need) && all.indexOf(need) === i);

  const colOf = new Map<string, number>();
  const visiting = new Set<string>();
  const column = (id: string): number => {
    const known = colOf.get(id);
    if (known !== undefined) return known;
    if (visiting.has(id)) return 1; // dependency cycle: break it
    visiting.add(id);
    const col = upstream(id).reduce((max, need) => Math.max(max, column(need) + 1), 1);
    visiting.delete(id);
    colOf.set(id, col);
    return col;
  };
  for (const id of byId.keys()) column(id);

  const maxCol = Math.max(0, ...colOf.values());
  const columns: string[][] = Array.from({ length: maxCol + 1 }, () => []);
  for (const id of byId.keys()) columns[colOf.get(id) ?? 1]?.push(id);

  // Order each column by the mean row of its parents so edges cross as little as possible.
  const rowOf = new Map<string, number>();
  columns.forEach((ids, col) => {
    if (col === 0) return;
    const score = (id: string) => {
      const parents = upstream(id);
      if (parents.length === 0) return -1;
      return parents.reduce((sum, p) => sum + (rowOf.get(p) ?? 0), 0) / parents.length;
    };
    const order = new Map(ids.map((id, i) => [id, i]));
    ids.sort((a, b) => score(a) - score(b) || (order.get(a) ?? 0) - (order.get(b) ?? 0));
    ids.forEach((id, row) => rowOf.set(id, row));
  });

  const tallest = Math.max(1, ...columns.slice(1).map((ids) => ids.length));
  const innerH = tallest * size.h + (tallest - 1) * ROW_GAP;
  const colX = (col: number) => PAD + col * (size.w + COL_GAP);
  const colY = (col: number, row: number) => {
    const count = columns[col]?.length ?? 1;
    const colH = count * size.h + (count - 1) * ROW_GAP;
    return PAD + (innerH - colH) / 2 + row * (size.h + ROW_GAP);
  };

  const nodes: DagNode[] = [
    {
      id: "orchestrator",
      label: `${project} orchestrator`,
      title: `${project} orchestrator`,
      kind: "root",
      state: "root",
      tone: "ok",
      sub: rootSub,
      model: null,
      wait: null,
      col: 0,
      row: 0,
      x: colX(0),
      y: PAD + (innerH - size.h) / 2,
      needs: [],
    },
  ];
  for (const [id, item] of byId) {
    const col = colOf.get(id) ?? 1;
    const row = rowOf.get(id) ?? 0;
    const state = dagState(item);
    const model = providerModel(item.job);
    const wait = state === "queued" ? waitReason(item.job) : null;
    const phase = jobPhase(item);
    nodes.push({
      id,
      label: id,
      title: jobName(item.job),
      kind: dagKind(item.job.kind),
      state,
      tone: jobTone(item),
      sub:
        wait !== null
          ? `Waiting: ${wait}`
          : [phase, model].filter(Boolean).join(" · ") ||
            (state === "attention" ? (item.job.reason ?? "needs attention") : `${item.job.kind} · ${item.job.role}`),
      model,
      wait,
      col,
      row,
      x: colX(col),
      y: colY(col, row),
      needs: upstream(id),
    });
  }

  const at = new Map(nodes.map((n) => [n.id, n]));
  const edges: DagEdge[] = [];
  const connect = (from: DagNode, to: DagNode) => {
    const x1 = from.x + size.w;
    const y1 = from.y + size.h / 2;
    const x2 = to.x;
    const y2 = to.y + size.h / 2;
    const dx = Math.max(24, (x2 - x1) / 2);
    edges.push({
      id: `${from.id}->${to.id}`,
      from: from.id,
      to: to.id,
      path: `M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`,
    });
  };
  const root = nodes[0] as DagNode;
  for (const node of nodes.slice(1)) {
    if (node.needs.length === 0) connect(root, node);
    else for (const need of node.needs) connect(at.get(need) as DagNode, node);
  }

  return {
    nodes,
    edges,
    width: PAD * 2 + (maxCol + 1) * size.w + maxCol * COL_GAP,
    height: PAD * 2 + innerH,
    nodeW: size.w,
    nodeH: size.h,
    jobCount: byId.size,
  };
}
