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

/** Every job of a project (or of all projects when `project` is null), tagged with its lane. */
export function projectJobs(fleet: FleetView | null, project: string | null): ProjectJob[] {
  if (!fleet) return [];
  return LANE_INFO.filter(({ key }) => ACTIVE_LANES.includes(key)).flatMap(({ key }) =>
    laneJobs(fleet.lanes, key)
      .filter((job) => project === null || job.project === project)
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
    blocked: count(fleet.lanes.parked_blocked, project.name),
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
