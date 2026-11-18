import type { FleetView, InboxItemView, JobCardView, JobLanes } from "../bridge/wire";

/** What the sidebar tree and the server page show for one project. */
export interface ProjectSummary {
  name: string;
  state: string;
  running: number;
  ready: number;
  review: number;
  blocked: number;
  done: number;
  inbox: number;
}

export type LaneKey = "running" | "ready" | "review_fix" | "parked_blocked" | "done";
export type NodeTone = "ok" | "neutral" | "info" | "warn" | "danger" | "faint";

/** Lanes in the order the node map and row filters show them. */
export const LANE_INFO: ReadonlyArray<{ key: LaneKey; label: string; tone: NodeTone }> = [
  { key: "running", label: "Running", tone: "ok" },
  { key: "ready", label: "Ready", tone: "neutral" },
  { key: "review_fix", label: "Review & fix", tone: "info" },
  { key: "parked_blocked", label: "Needs attention", tone: "warn" },
  { key: "done", label: "Done", tone: "faint" },
];

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
  return LANE_INFO.flatMap(({ key }) =>
    laneJobs(fleet.lanes, key)
      .filter((job) => project === null || job.project === project)
      .map((job) => ({ lane: key, job })),
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
    review: count(fleet.lanes.review_fix, project.name),
    blocked: count(fleet.lanes.parked_blocked, project.name),
    done: count(fleet.lanes.done, project.name),
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
