import type {
  BrakeView,
  EstimateView,
  RamView,
  TargetView,
  WaitingJobView,
  WorkerCardView,
} from "../bridge/wire";

export const CAPACITY_DETAIL_UNAVAILABLE = "Capacity detail unavailable — retrying.";

export interface EstimateQuery {
  project?: string | null;
  jobKind?: string | null;
  harness?: string | null;
  model?: string | null;
  buildMode?: string | null;
}

function nullish(value: string | null | undefined): string | null {
  return value ?? null;
}

/**
 * Mirrors `spirit-core::view::capacity::select_estimate`:
 * exact → pooled (kind + harness) → default (kind + harness) → largest measured.
 */
export function selectEstimate(
  estimates: readonly EstimateView[],
  query: EstimateQuery,
): EstimateView | null {
  const project = nullish(query.project);
  const jobKind = nullish(query.jobKind);
  const harness = nullish(query.harness);
  const model = nullish(query.model);
  const buildMode = nullish(query.buildMode);

  if (project !== null) {
    const exact = estimates.find(
      (estimate) =>
        estimate.project === project &&
        estimate.job_kind === jobKind &&
        estimate.harness === harness &&
        estimate.model === model &&
        estimate.build_mode === buildMode,
    );
    if (exact) return exact;
  }
  const pooled = estimates.find(
    (estimate) =>
      estimate.source === "pooled" &&
      estimate.project === null &&
      estimate.model === null &&
      estimate.build_mode === null &&
      estimate.job_kind === jobKind &&
      estimate.harness === harness,
  );
  if (pooled) return pooled;

  const fallback = estimates.find(
    (estimate) =>
      estimate.source === "default" &&
      estimate.job_kind === jobKind &&
      estimate.harness === harness,
  );
  if (fallback) return fallback;

  let largest: EstimateView | null = null;
  for (const estimate of estimates) {
    if (estimate.samples <= 0) continue;
    if (!largest || estimate.ram_p90_bytes > largest.ram_p90_bytes) largest = estimate;
  }
  return largest;
}

export function estimateQueryForWorker(card: WorkerCardView): EstimateQuery {
  return {
    project: card.project,
    jobKind: card.kind,
    harness: card.executor?.harness ?? null,
    model: card.executor?.model ?? null,
    buildMode: null,
  };
}

export function firstWorkerCard(
  running: readonly WorkerCardView[],
): WorkerCardView | null {
  return running.find((card) => card.role === "worker") ?? running[0] ?? null;
}

export function railEstimateQuery(running: readonly WorkerCardView[]): EstimateQuery {
  const card = firstWorkerCard(running);
  return card ? estimateQueryForWorker(card) : {};
}

/**
 * Adds the selected per-worker estimate and the advisory `~N more by RAM` value. When
 * there is no estimate the fields stay null; the UI never invents a slot count.
 */
export function ramWithEstimate(
  ram: RamView,
  estimates: readonly EstimateView[],
  query: EstimateQuery,
): RamView {
  const estimate = selectEstimate(estimates, query);
  if (!estimate) return { ...ram };
  const headroom = Math.max(0, ram.available_bytes - ram.reserve_bytes);
  const extraSlots =
    estimate.ram_p90_bytes > 0
      ? Math.min(99, Math.floor(headroom / estimate.ram_p90_bytes))
      : null;
  return {
    ...ram,
    per_worker_p90_bytes: estimate.ram_p90_bytes,
    per_worker_source: estimate.source,
    per_worker_samples: estimate.samples,
    estimated_extra_slots: extraSlots,
  };
}

export function waitReasonLabel(code: string): string {
  switch (code) {
    case "ram":
      return "RAM";
    case "cpu":
      return "CPU";
    case "disk":
      return "Disk";
    case "provider":
      return "Provider limit";
    case "brake":
      return "Brake";
    case "target":
      return "Target";
    default:
      return code;
  }
}

export function brakeReasonLabel(code: string): string {
  switch (code) {
    case "disk_low":
      return "Disk low";
    case "memory_pressure":
      return "Memory pressure";
    case "swap_growth":
      return "Swap growth";
    case "provider":
      return "Provider limit";
    default:
      return code;
  }
}

export function limitKindLabel(kind: string): string {
  switch (kind) {
    case "quota":
      return "Quota";
    case "credits":
      return "Credits";
    case "rate":
      return "Rate";
    case "context":
      return "Context";
    case "auth":
      return "Auth";
    case "unavailable":
      return "Unavailable";
    default:
      return kind;
  }
}

export interface WaitingGroup {
  code: string;
  label: string;
  count: number;
  rows: WaitingJobView[];
}

const WAIT_ORDER = ["ram", "cpu", "disk", "provider", "brake", "target"];

/** Groups waiting jobs by `reason.code`; unknown codes are kept and sorted last. */
export function groupWaiting(waiting: readonly WaitingJobView[]): WaitingGroup[] {
  const groups = new Map<string, WaitingJobView[]>();
  for (const job of waiting) {
    const rows = groups.get(job.code) ?? [];
    rows.push(job);
    groups.set(job.code, rows);
  }
  return [...groups.entries()]
    .sort(([left], [right]) => {
      const leftIndex = WAIT_ORDER.indexOf(left);
      const rightIndex = WAIT_ORDER.indexOf(right);
      const leftRank = leftIndex === -1 ? WAIT_ORDER.length : leftIndex;
      const rightRank = rightIndex === -1 ? WAIT_ORDER.length : rightIndex;
      return leftRank - rightRank || left.localeCompare(right);
    })
    .map(([code, rows]) => ({
      code,
      label: waitReasonLabel(code),
      count: rows.length,
      rows,
    }));
}

export function waitingAgeSeconds(
  since: string,
  generatedAt: string | null,
): number {
  const sinceMs = Date.parse(since);
  const referenceMs = generatedAt ? Date.parse(generatedAt) : Date.now();
  if (Number.isNaN(sinceMs) || Number.isNaN(referenceMs)) return 0;
  return Math.max(0, Math.floor((referenceMs - sinceMs) / 1000));
}

export type BrakeTone = "danger" | "warn" | "ok";

/** Red for disk/memory pressure, amber for provider/swap warnings. */
export function brakeTone(brake: BrakeView): BrakeTone {
  if (!brake.active) return "ok";
  const codes = brake.reasons.map((reason) => reason.code);
  if (codes.some((code) => code === "disk_low" || code === "memory_pressure")) {
    return "danger";
  }
  return "warn";
}

export type TargetRelation = "none" | "below" | "equal" | "above";

export interface TargetAdvice {
  relation: TargetRelation;
  advised: number | null;
  label: string;
  tone: "ok" | "info" | "warn" | "unknown";
  applyLabel: string;
  canApply: boolean;
}

export function targetAdvice(
  current: number,
  advised: number | null,
  isOverride = false,
): TargetAdvice {
  if (advised === null) {
    return {
      relation: "none",
      advised,
      label: "VOWP has no advice yet",
      tone: "unknown",
      applyLabel: "Apply advised",
      canApply: false,
    };
  }
  if (current < advised) {
    return {
      relation: "below",
      advised,
      label: "headroom available",
      tone: "info",
      applyLabel: "Apply advised",
      canApply: true,
    };
  }
  if (current > advised) {
    return {
      relation: "above",
      advised,
      label: isOverride ? "owner override · above advice" : "above advice",
      tone: "warn",
      applyLabel: "Match advised",
      canApply: true,
    };
  }
  return {
    relation: "equal",
    advised,
    label: "matches advice",
    tone: "ok",
    applyLabel: "Apply advised",
    canApply: true,
  };
}

export type ResourceTone = "ok" | "warn" | "danger" | "unknown";

/** Maps a resource state to the badge tone; colour only, VOWP remains authoritative. */
export function resourceTone(state: string): ResourceTone {
  switch (state) {
    case "ok":
      return "ok";
    case "warn":
      return "warn";
    case "critical":
      return "danger";
    default:
      return "unknown";
  }
}

export function diskStateCopy(state: string): string {  switch (state) {
    case "ok":
      return "Above the reserve floor.";
    case "warn":
      return "Close to the floor. VOWP may stop admitting runs soon.";
    case "critical":
      return "Below the floor. New runs will wait (disk).";
    default:
      return "No disk sample yet.";
  }
}

export function ramStateCopy(state: string): string {
  switch (state) {
    case "ok":
      return "Above the RAM reserve.";
    case "warn":
      return "Close to the RAM reserve.";
    case "critical":
      return "Below the RAM reserve. VOWP will brake new runs.";
    default:
      return "No memory sample yet.";
  }
}

export function cpuStateCopy(state: string): string {
  switch (state) {
    case "ok":
      return "CPU headroom is healthy.";
    case "warn":
      return "CPU is busy; new runs may wait.";
    case "critical":
      return "CPU is over capacity; VOWP may brake new runs.";
    default:
      return "No CPU sample yet.";
  }
}

export interface ProviderCap {
  provider: string;
  concurrency: number;
}

export function providerCaps(caps: Record<string, number>): ProviderCap[] {
  return Object.entries(caps)
    .map(([provider, concurrency]) => ({ provider, concurrency }))
    .sort((left, right) => left.provider.localeCompare(right.provider));
}

export function targetModeLabel(mode: string): string {
  return mode === "auto" ? "Auto" : mode === "fixed" ? "Fixed" : mode;
}

export function targetSourceTitle(target: TargetView): string {
  return `Set by ${target.set_by} at ${target.set_at}. Reason: ${target.reason}`;
}


