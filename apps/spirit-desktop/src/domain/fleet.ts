import type {
  FleetView,
  JobCardView,
  JobLanes,
  ProjectView,
  WorkerCardView,
} from "../bridge/wire";

export type LaneKey = "running" | "ready" | "review_fix" | "parked_blocked" | "done";

export type LaneTone = "info" | "neutral" | "verifier" | "warn" | "ok";

export interface LaneDefinition {
  key: LaneKey;
  label: string;
  headerTone: LaneTone;
  footerLabel: string;
}

/** Exact lane order from docs/PLAN.md §8.3.3. */
export const LANES: readonly LaneDefinition[] = [
  { key: "running", label: "Running", headerTone: "info", footerLabel: "running" },
  { key: "ready", label: "Ready", headerTone: "neutral", footerLabel: "waiting" },
  {
    key: "review_fix",
    label: "Review & Fix",
    headerTone: "verifier",
    footerLabel: "in review",
  },
  {
    key: "parked_blocked",
    label: "Parked & Blocked",
    headerTone: "warn",
    footerLabel: "need attention",
  },
  { key: "done", label: "Done", headerTone: "ok", footerLabel: "done" },
];

const LANE_KEYS: readonly LaneKey[] = [
  "running",
  "ready",
  "review_fix",
  "parked_blocked",
  "done",
];

const ACTIVE_STATES = new Set([
  "active",
  "running",
  "stalled",
  "gating",
  "reviewing",
  "fixing",
  "merging",
]);

/**
 * Maps one canonical VOWP `JobState` to a lane. Unknown values are deliberately
 * surfaced in Parked & Blocked; they are never treated as ready or done.
 */
export function laneForState(state: string): LaneKey {
  switch (state) {
    case "running":
    case "stalled":
    case "merging":
      // Lock-holding run states represented by running cards.
      return "running";
    case "pending":
    case "ready":
      return "ready";
    case "gating":
    case "reviewing":
    case "fixing":
      // Counts land in Review & Fix even though running cards cover the lock.
      return "review_fix";
    case "parked":
    case "failed":
    case "cancelled":
      return "parked_blocked";
    case "done":
    case "replanned":
      return "done";
    default:
      return "parked_blocked";
  }
}

export function laneCards(lanes: JobLanes, key: LaneKey): JobCardView[] {
  return lanes[key];
}

function emptyCounts(): Record<LaneKey, number> {
  return {
    running: 0,
    ready: 0,
    review_fix: 0,
    parked_blocked: 0,
    done: 0,
  };
}

/** Lane counts come from `jobs_by_state`, never from the number of visible cards. */
export function countsFromProjects(projects: readonly ProjectView[]): Record<LaneKey, number> {
  const counts = emptyCounts();
  for (const project of projects) {
    for (const [state, count] of Object.entries(project.jobs_by_state)) {
      counts[laneForState(state)] += Math.max(0, count);
    }
  }
  return counts;
}

export interface FleetSelection {
  projects: ProjectView[];
  running: WorkerCardView[];
  lanes: JobLanes;
  counts: Record<LaneKey, number>;
  queueTruncated: boolean;
  queueShown: number;
  queueCap: number;
}

/**
 * Applies the project filter to lanes and worker cards. Capacity is node-wide and is
 * intentionally not part of the selection.
 */
export function selectFleet(fleet: FleetView, projectFilter: string | null): FleetSelection {
  const projects = projectFilter
    ? fleet.projects.filter((project) => project.name === projectFilter)
    : fleet.projects;
  const running = projectFilter
    ? fleet.running.filter((card) => card.project === projectFilter)
    : fleet.running;
  if (!projectFilter) {
    return {
      projects,
      running,
      lanes: fleet.lanes,
      counts: countsFromProjects(projects),
      queueTruncated: fleet.lanes.queue_truncated,
      queueShown: fleet.lanes.queue_shown,
      queueCap: fleet.lanes.queue_cap,
    };
  }

  const lanes: JobLanes = {
    running: fleet.lanes.running.filter((card) => card.project === projectFilter),
    ready: fleet.lanes.ready.filter((card) => card.project === projectFilter),
    review_fix: fleet.lanes.review_fix.filter((card) => card.project === projectFilter),
    parked_blocked: fleet.lanes.parked_blocked.filter(
      (card) => card.project === projectFilter,
    ),
    done: fleet.lanes.done.filter((card) => card.project === projectFilter),
    counts: {},
    queue_truncated: fleet.lanes.queue_truncated,
    queue_shown: 0,
    queue_cap: fleet.lanes.queue_cap,
  };
  const counts = countsFromProjects(projects);
  lanes.counts = Object.fromEntries(LANE_KEYS.map((key) => [key, counts[key]]));
  lanes.queue_shown = lanes.ready.length + lanes.parked_blocked.length;
  lanes.queue_truncated =
    fleet.lanes.queue_truncated &&
    (lanes.queue_shown > 0 || counts.ready >= fleet.lanes.queue_cap);
  return {
    projects,
    running,
    lanes,
    counts,
    queueTruncated: lanes.queue_truncated,
    queueShown: lanes.queue_shown,
    queueCap: lanes.queue_cap,
  };
}

export type RingSlotState = "busy" | "stalled" | "idle";

export interface RingSlot {
  index: number;
  state: RingSlotState;
}

export interface WorkerRingModel {
  target: number;
  advised: number | null;
  busy: number;
  idle: number;
  stalled: number;
  verifierRuns: number;
  otherRuns: number;
  overTarget: number;
  aggregateOverflow: number;
  slots: RingSlot[];
  targetZero: boolean;
  summary: string;
}

/**
 * Derives the slot ring. Busy/verifier counts come from running cards; the difference
 * against `capacity.running.runs` is surfaced as `otherRuns`, never hidden.
 */
export function workerRingModel(
  fleet: FleetView,
  running: readonly WorkerCardView[] = fleet.running,
): WorkerRingModel {
  const target = fleet.capacity.target.target_workers;
  const advised =
    fleet.capacity.target.advised_workers ?? fleet.capacity.advised_workers ?? null;
  const workerCards = running.filter((card) => card.role === "worker");
  const verifierCards = running.filter((card) => card.role === "verifier");
  const busy = workerCards.length;
  const stalled = workerCards.filter(
    (card) => card.stalled || card.state === "stalled",
  ).length;
  const idle = Math.max(0, target - busy);
  const overTarget = Math.max(0, busy - target);
  const otherRuns = Math.max(
    0,
    fleet.capacity.running.runs - workerCards.length - verifierCards.length,
  );
  const drawCount = Math.min(target, 12);
  const slots: RingSlot[] = Array.from({ length: drawCount }, (_, index) => ({
    index,
    state:
      index < Math.min(stalled, drawCount)
        ? "stalled"
        : index < Math.min(busy, drawCount)
          ? "busy"
          : "idle",
  }));
  const targetZero = target === 0;
  let summary: string;
  if (targetZero) {
    summary =
      busy > 0
        ? `No workers allowed — VOWP target is 0. ${busy} worker card${busy === 1 ? "" : "s"} still visible.`
        : "No workers allowed — VOWP target is 0.";
  } else {
    const advisedText =
      advised === null ? "VOWP has no advice yet" : `target advised by VOWP is ${advised}`;
    const parts = [
      `${target} worker slot${target === 1 ? "" : "s"}`,
      `${busy} busy`,
      `${idle} idle`,
      advisedText,
      `${otherRuns} other run${otherRuns === 1 ? "" : "s"}`,
    ];
    summary = `${parts.join(", ")}.`;
    if (overTarget > 0) summary += ` ${overTarget} above target.`;
    if (stalled > 0) summary += ` ${stalled} stalled.`;
  }
  return {
    target,
    advised,
    busy,
    idle,
    stalled,
    verifierRuns: verifierCards.length,
    otherRuns,
    overTarget,
    aggregateOverflow: Math.max(0, target - 12),
    slots,
    targetZero,
    summary,
  };
}

/** Stalled first, then oldest run, then job id, matching the core view order. */
export function sortWorkerCards(cards: readonly WorkerCardView[]): WorkerCardView[] {
  return [...cards].sort((left, right) => {
    const leftStalled = left.stalled || left.state === "stalled";
    const rightStalled = right.stalled || right.state === "stalled";
    return (
      Number(rightStalled) - Number(leftStalled) ||
      right.age_secs - left.age_secs ||
      left.job_id.localeCompare(right.job_id)
    );
  });
}

/** Finds a title when a queue or gate-failure card carries one. */
export function findJobTitle(fleet: FleetView, jobId: string): string | null {
  for (const lane of LANE_KEYS) {
    const card = fleet.lanes[lane].find((candidate) => candidate.job_id === jobId);
    if (card?.title) return card.title;
  }
  return null;
}

export function queueTruncationText(lanesOrCap: JobLanes | number): string {
  const cap =
    typeof lanesOrCap === "number"
      ? lanesOrCap
      : lanesOrCap.queue_cap > 0
        ? lanesOrCap.queue_cap
        : 200;
  return `showing the first ${cap} jobs`;
}

export function laneCount(lanes: JobLanes, key: LaneKey): number {
  return lanes.counts[key] ?? 0;
}

export function laneFooterText(key: LaneKey, count: number): string {
  const definition = LANES.find((lane) => lane.key === key);
  return `${count} ${definition?.footerLabel ?? key}`;
}

export type JobTone = "ok" | "info" | "warn" | "danger" | "verifier" | "unknown";

export interface StateChip {
  label: string;
  tone: JobTone;
}

/** State chips are explicit; unknown values never masquerade as `ready` or `running`. */
export function jobStateChip(state: string): StateChip {
  switch (state) {
    case "active":
      return { label: "Active", tone: "info" };
    case "running":
      return { label: "Running", tone: "info" };
    case "stalled":
      return { label: "Stalled", tone: "danger" };
    case "pending":
      return { label: "Pending", tone: "unknown" };
    case "ready":
      return { label: "Ready", tone: "info" };
    case "gating":
      return { label: "Gating", tone: "verifier" };
    case "reviewing":
      return { label: "Reviewing", tone: "verifier" };
    case "fixing":
      return { label: "Fixing", tone: "verifier" };
    case "merging":
      return { label: "Merging", tone: "info" };
    case "done":
      return { label: "Done", tone: "ok" };
    case "replanned":
      return { label: "Replanned", tone: "ok" };
    case "parked":
      return { label: "Parked", tone: "warn" };
    case "failed":
      return { label: "Failed", tone: "danger" };
    case "cancelled":
      return { label: "Cancelled", tone: "warn" };
    default:
      return { label: "Unknown", tone: "unknown" };
  }
}

export type JobActionName = "retry" | "park" | "cancel";

export interface ActionEligibility {
  enabled: boolean;
  reason: string | null;
}

export type JobActionEligibility = Record<JobActionName, ActionEligibility>;

/**
 * Action legality is intentionally conservative: an action is enabled only for the
 * states VOWP documents as lock-holding or terminal, and never while data is stale.
 */
export function jobActionEligibility(
  job: JobCardView,
  mutationsAllowed: boolean,
): JobActionEligibility {
  if (!mutationsAllowed) {
    return {
      retry: { enabled: false, reason: "Refresh before changing a job." },
      park: { enabled: false, reason: "Refresh before changing a job." },
      cancel: { enabled: false, reason: "Refresh before changing a job." },
    };
  }
  const active = ACTIVE_STATES.has(job.state);
  const retryable = job.state === "parked" || job.state === "failed" || job.state === "cancelled";
  if (!active && !retryable) {
    const reason = `VOWP does not allow job actions for state "${job.state}".`;
    return {
      retry: { enabled: false, reason },
      park: { enabled: false, reason },
      cancel: { enabled: false, reason },
    };
  }
  return {
    retry: retryable
      ? { enabled: true, reason: null }
      : { enabled: false, reason: `Retry is not allowed while the job is ${job.state}.` },
    park: active
      ? { enabled: true, reason: null }
      : { enabled: false, reason: "Park is only allowed for an active job." },
    cancel: active
      ? { enabled: true, reason: null }
      : { enabled: false, reason: "Cancel is only allowed for an active job." },
  };
}

export { LANE_KEYS };
