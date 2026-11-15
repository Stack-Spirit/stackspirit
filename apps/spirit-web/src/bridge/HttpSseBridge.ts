import type {
  BridgeCapabilities,
  ConnectionSnapshot,
  DaemonHealth,
  ExecutorView,
  InboxItemView,
  JobCardView,
  JobDetailView,
  JobGateView,
  JobRunView,
  LatencyStats,
  ProjectView,
  SpiritBridge,
  SpiritErrorDto,
  StateEvent,
  Timestamp,
  UnavailableNotice,
  WebBridgeStatus,
  WebFleetView,
  WebJobLanes,
  WorkerCardView,
} from "@stackspirit/ui";
import { parseExecutor } from "@stackspirit/ui";

import {
  CAPACITY_DETAIL_UNAVAILABLE,
  GATE_FAILURES_READ_FAILED,
  GATE_FAILURES_UNAVAILABLE,
  JOB_DETAIL_NOT_FOUND,
  ORCHESTRATORS_UNAVAILABLE,
  PROJECT_STATE_UNAVAILABLE,
  RAW_DOCUMENT_UNAVAILABLE,
  SESSION_SURFACES_UNAVAILABLE,
  SUPERVISOR_STATE_UNAVAILABLE,
  WRITES_DISABLED,
} from "../copy";
import { countsFromProjects, laneForState } from "../shared/domain";
import { isSameOrigin, sameOriginError } from "./baseUrl";
import { asSpiritError, webError } from "./errors";

/** Routes W1 calls. Tests assert the bridge never calls a route outside this set. */
export const READ_ROUTES = {
  health: "/api/v1/health",
  board: "/api/v1/board",
  boardStream: "/api/v1/board/stream",
  jobs: "/api/v1/jobs",
  sessions: "/api/v1/sessions",
  inbox: "/api/v1/inbox",
} as const;

/** The three token-free job-detail reads documented for `fleet.job(id)`. */
export const JOB_DETAIL_ROUTES = {
  job: (id: string) => `/api/v1/jobs/${encodeURIComponent(id)}`,
  runs: (id: string) => `/api/v1/jobs/${encodeURIComponent(id)}/runs`,
  gates: (id: string) => `/api/v1/jobs/${encodeURIComponent(id)}/gates`,
} as const;

/** Route templates for the job-detail reads; the report and tests name these exactly. */
export const JOB_DETAIL_ROUTE_PATTERNS = [
  "/api/v1/jobs/{id}",
  "/api/v1/jobs/{id}/runs",
  "/api/v1/jobs/{id}/gates",
] as const;

const JOB_DETAIL_PATH =
  /^\/api\/v1\/jobs\/(?!$)[^/]+(?:\/(?:runs|gates))?$/;

/** True for every fixed or job-detail path the bridge may read. */
export function isReadRoute(pathname: string): boolean {
  if ((Object.values(READ_ROUTES) as string[]).includes(pathname)) return true;
  return JOB_DETAIL_PATH.test(pathname);
}

/**
 * Routes the v0.3 daemon serves but W1 deliberately does not call. They are listed so a
 * route-allowlist test can prove the browser never touches them.
 */
export const KNOWN_UNUSED_ROUTES = [
  "/api/v1/capacity",
  "/api/v1/fleet",
  "/api/v1/cost",
  "/api/v1/nodes",
  "/api/v1/sessions/{id}",
  "/api/v1/sessions/{id}/events",
  "/api/v1/sessions/{id}/events/stream",
  "/api/v1/sessions/{id}/export/asif",
  "/api/v1/sessions/{id}/capsules/{capsuleId}",
  "/api/v1/sessions/import/asif",
  "/api/v1/sessions/{id}/switch",
] as const;

export const WRITE_ROUTES = [
  "/api/v1/jobs/{id}/retry",
  "/api/v1/jobs/{id}/park",
  "/api/v1/jobs/{id}/cancel",
  "/api/v1/inbox/{id}/answer",
  "/api/v1/capacity/target",
] as const;

export const WEB_READ_ONLY_CAPABILITIES: BridgeCapabilities = {
  capacity_detail: false,
  supervisor_state: false,
  host_management: false,
  engine_management: false,
  capability_gates: false,
  job_actions: false,
  inbox_answer: false,
  set_target: false,
  raw_documents: false,
  session_detail: false,
};

export interface EventSourceMessage {
  data: string;
}

/** The tiny `EventSource` surface the bridge needs; tests provide a fake. */
export interface EventSourceLike {
  onopen: (() => void) | null;
  onerror: (() => void) | null;
  addEventListener(type: string, listener: (event: EventSourceMessage) => void): void;
  removeEventListener(
    type: string,
    listener: (event: EventSourceMessage) => void,
  ): void;
  close(): void;
}

export type FetchLike = (
  input: RequestInfo | URL,
  init?: RequestInit,
) => Promise<Response>;

export type WebFleetApi = SpiritBridge["fleet"] & {
  snapshot: (refresh?: boolean) => Promise<WebFleetView | null>;
  watch: (listener: (snapshot: WebFleetView) => void) => () => void;
  webStatus: () => WebBridgeStatus;
};

export interface HttpSseBridgeOptions {
  baseUrl: string;
  /** Browser page origin used for the same-origin guard. Defaults to `location.origin`. */
  pageOrigin?: string;
  fetchImpl?: FetchLike;
  eventSourceFactory?: (url: string) => EventSourceLike;
  now?: () => number;
  readTimeoutMs?: number;
  pollIntervalMs?: number;
  staleBackoffMs?: number;
  heartbeatIntervalMs?: number;
  heartbeatTimeoutMs?: number;
  reconnectBaseMs?: number;
  reconnectMaxMs?: number;
}

interface HealthDoc {
  version: string;
  node: string;
  project: string;
  schema_version: number;
  heartbeat: Timestamp | null;
}

interface BoardRunDoc {
  job: string;
  executor: string | null;
  age_secs: number;
  last_event_age_secs: number | null;
}

interface BoardLimitDoc {
  provider: string;
  kind: string;
  resets_at: Timestamp | null;
}

interface BoardDoc {
  at: Timestamp;
  project: string;
  jobs: Record<string, number>;
  running: BoardRunDoc[];
  slots_busy: number;
  slots_total: number;
  planner: string | null;
  keeper: string | null;
  limits: BoardLimitDoc[];
  cost_today_usd: number | null;
  inbox_open: number;
  last_checkpoint: Timestamp | null;
}

interface JobDoc {
  id: string;
  project: string;
  title: string;
  kind: string;
  role: string;
  state: string;
  priority: number;
  parked_reason: string | null;
  state_changed_at: Timestamp;
  updated_at: Timestamp;
}

interface SessionDoc {
  id: string;
  job_id: string | null;
  node: string;
  state: string;
  executor: string | null;
  updated_at: Timestamp;
}

interface InboxDoc {
  id: string;
  project: string | null;
  job_id: string | null;
  kind: string;
  title: string;
  body: string | null;
  options: string[];
  state: string;
  answer: string | null;
  answered_by: string | null;
  created_at: Timestamp;
  answered_at: Timestamp | null;
}

interface PageDoc<T> {
  items: T[];
  next: number | null;
  total: number;
}

interface JobDetailDoc {
  id: string;
  project: string;
  title: string;
  kind: string;
  role: string;
  state: string;
  priority: number;
  needs: string[];
  attempts: number;
  note: string;
  report_path: string | null;
  parked_reason: string | null;
  created_at: Timestamp;
  state_changed_at: Timestamp;
  updated_at: Timestamp;
  last_progress_at: Timestamp | null;
}

interface SecondaryRows {
  jobs: JobDoc[];
  sessions: SessionDoc[];
  inbox: InboxDoc[];
  jobsTruncated: boolean;
  /** Detail reads for jobs in the review/fix lane, keyed by job id. */
  reviewDetails: Map<string, JobDetailView>;
  /** A gate/run detail read failed for a reason other than the route being absent. */
  reviewReadError: SpiritErrorDto | null;
  /** The daemon does not serve one of the job-detail routes. */
  reviewRouteMissing: boolean;
}

interface PendingBoard {
  board: BoardDoc;
  seq: number;
}

const REQUEST_TIMEOUT_MS = 10_000;
const POLL_INTERVAL_MS = 5_000;
const STALE_BACKOFF_MS = 30_000;
const HEARTBEAT_INTERVAL_MS = 15_000;
const HEARTBEAT_TIMEOUT_MS = 45_000;
const RECONNECT_BASE_MS = 1_000;
const RECONNECT_MAX_MS = 15_000;
const MAX_MESSAGE_CHARS = 500;

function clone<T>(value: T): T {
  return structuredClone(value);
}

function iso(ms: number): Timestamp {
  return new Date(ms).toISOString();
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function requireString(value: unknown, field: string): string {
  if (typeof value !== "string" || value.length === 0) {
    throw webError("parse", `The daemon returned an unreadable ${field}.`);
  }
  return value;
}

function optionalString(value: unknown, field: string): string | null {
  if (value === null || value === undefined) return null;
  if (typeof value !== "string") {
    throw webError("parse", `The daemon returned an unreadable ${field}.`);
  }
  return value;
}

function requireNumber(value: unknown, field: string): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw webError("parse", `The daemon returned an unreadable ${field}.`);
  }
  return value;
}

function optionalNumber(value: unknown, field: string): number | null {
  if (value === null || value === undefined) return null;
  return requireNumber(value, field);
}

function requireArray(value: unknown, field: string): unknown[] {
  if (!Array.isArray(value)) {
    throw webError("parse", `The daemon returned an unreadable ${field}.`);
  }
  return value;
}

function requireTimestamp(value: unknown, field: string): Timestamp {
  const text = requireString(value, field);
  if (Number.isNaN(Date.parse(text))) {
    throw webError("parse", `The daemon returned an unreadable ${field}.`);
  }
  return text;
}

function optionalTimestamp(value: unknown, field: string): Timestamp | null {
  if (value === null || value === undefined) return null;
  return requireTimestamp(value, field);
}

function requireStringArray(value: unknown, field: string): string[] {
  return requireArray(value, field).map((item, index) =>
    requireString(item, `${field}[${index}]`),
  );
}

function sanitizeMessage(value: string): string {
  let withoutControls = "";
  for (const character of value) {
    const code = character.codePointAt(0) ?? 0;
    withoutControls += code < 0x20 || code === 0x7f ? " " : character;
  }
  const cleaned = withoutControls.replace(/\s+/g, " ").trim();
  const chars = Array.from(cleaned);
  return chars.length > MAX_MESSAGE_CHARS
    ? `${chars.slice(0, MAX_MESSAGE_CHARS).join("")}…`
    : cleaned;
}

function parseHealthDoc(value: unknown): HealthDoc {
  if (!isRecord(value)) throw webError("parse", "The daemon returned an unreadable health document.");
  return {
    version: requireString(value.version, "health.version"),
    node: requireString(value.node, "health.node"),
    project: requireString(value.project, "health.project"),
    schema_version: requireNumber(value.schema_version, "health.schema_version"),
    heartbeat: optionalTimestamp(value.heartbeat, "health.heartbeat"),
  };
}

function parseBoardDoc(value: unknown): BoardDoc {
  if (!isRecord(value)) throw webError("parse", "The daemon returned an unreadable board document.");
  if (!isRecord(value.jobs)) {
    throw webError("parse", "The daemon returned an unreadable board.jobs map.");
  }
  const jobs: Record<string, number> = {};
  for (const [state, count] of Object.entries(value.jobs)) {
    jobs[state] = Math.max(0, requireNumber(count, `board.jobs.${state}`));
  }
  const running = requireArray(value.running, "board.running").map((item, index) => {
    if (!isRecord(item)) {
      throw webError("parse", `The daemon returned an unreadable board.running[${index}].`);
    }
    return {
      job: requireString(item.job, `board.running[${index}].job`),
      executor: optionalString(item.executor, `board.running[${index}].executor`),
      age_secs: Math.max(0, requireNumber(item.age_secs, `board.running[${index}].age_secs`)),
      last_event_age_secs: optionalNumber(
        item.last_event_age_secs,
        `board.running[${index}].last_event_age_secs`,
      ),
    };
  });
  const limits = requireArray(value.limits, "board.limits").map((item, index) => {
    if (!isRecord(item)) {
      throw webError("parse", `The daemon returned an unreadable board.limits[${index}].`);
    }
    return {
      provider: requireString(item.provider, `board.limits[${index}].provider`),
      kind: requireString(item.kind, `board.limits[${index}].kind`),
      resets_at: optionalTimestamp(item.resets_at, `board.limits[${index}].resets_at`),
    };
  });
  return {
    at: requireTimestamp(value.at, "board.at"),
    project: requireString(value.project, "board.project"),
    jobs,
    running,
    slots_busy: Math.max(0, requireNumber(value.slots_busy, "board.slots_busy")),
    slots_total: Math.max(0, requireNumber(value.slots_total, "board.slots_total")),
    planner: optionalString(value.planner, "board.planner"),
    keeper: optionalString(value.keeper, "board.keeper"),
    limits,
    cost_today_usd: optionalNumber(value.cost_today_usd, "board.cost_today_usd"),
    inbox_open: Math.max(0, requireNumber(value.inbox_open, "board.inbox_open")),
    last_checkpoint: optionalTimestamp(value.last_checkpoint, "board.last_checkpoint"),
  };
}

function parseJobDoc(value: unknown, index: number): JobDoc {
  if (!isRecord(value)) {
    throw webError("parse", `The daemon returned an unreadable job row at ${index}.`);
  }
  const priority = value.priority === undefined ? 50 : requireNumber(value.priority, "job.priority");
  return {
    id: requireString(value.id, "job.id"),
    project: requireString(value.project, "job.project"),
    title: requireString(value.title, "job.title"),
    kind: requireString(value.kind, "job.kind"),
    role: requireString(value.role, "job.role"),
    state: requireString(value.state, "job.state"),
    priority,
    parked_reason: optionalString(value.parked_reason, "job.parked_reason"),
    state_changed_at: requireTimestamp(value.state_changed_at, "job.state_changed_at"),
    updated_at: requireTimestamp(value.updated_at, "job.updated_at"),
  };
}

function parseSessionDoc(value: unknown, index: number): SessionDoc {
  if (!isRecord(value)) {
    throw webError("parse", `The daemon returned an unreadable session row at ${index}.`);
  }
  return {
    id: requireString(value.id, "session.id"),
    job_id: optionalString(value.job_id, "session.job_id"),
    node: requireString(value.node, "session.node"),
    state: requireString(value.state, "session.state"),
    executor: optionalString(value.executor, "session.executor"),
    updated_at: requireTimestamp(value.updated_at, "session.updated_at"),
  };
}

function parseInboxDoc(value: unknown, index: number): InboxDoc {
  if (!isRecord(value)) {
    throw webError("parse", `The daemon returned an unreadable inbox row at ${index}.`);
  }
  return {
    id: requireString(value.id, "inbox.id"),
    project: optionalString(value.project, "inbox.project"),
    job_id: optionalString(value.job_id, "inbox.job_id"),
    kind: requireString(value.kind, "inbox.kind"),
    title: requireString(value.title, "inbox.title"),
    body: optionalString(value.body, "inbox.body"),
    options: requireStringArray(value.options, "inbox.options"),
    state: requireString(value.state, "inbox.state"),
    answer: optionalString(value.answer, "inbox.answer"),
    answered_by: optionalString(value.answered_by, "inbox.answered_by"),
    created_at: requireTimestamp(value.created_at, "inbox.created_at"),
    answered_at: optionalTimestamp(value.answered_at, "inbox.answered_at"),
  };
}

function parsePage<T>(value: unknown, parseItem: (item: unknown, index: number) => T): PageDoc<T> {
  if (!isRecord(value)) throw webError("parse", "The daemon returned an unreadable page.");
  const items = requireArray(value.items, "page.items").map(parseItem);
  const next = value.next === null || value.next === undefined ? null : requireNumber(value.next, "page.next");
  const total = value.total === undefined ? items.length : requireNumber(value.total, "page.total");
  return { items, next, total };
}

function requireBoolean(value: unknown, field: string): boolean {
  if (typeof value !== "boolean") {
    throw webError("parse", `The daemon returned an unreadable ${field}.`);
  }
  return value;
}

function requireInteger(value: unknown, field: string): number {
  const number = requireNumber(value, field);
  if (!Number.isInteger(number)) {
    throw webError("parse", `The daemon returned an unreadable ${field}.`);
  }
  return number;
}

function optionalInteger(value: unknown, field: string): number | null {
  if (value === null || value === undefined) return null;
  return requireInteger(value, field);
}

/** Rejects absolute, home, traversal, and Windows paths; keeps a repo-relative path. */
function safeRepoPath(value: string | null): string | null {
  if (!value) return null;
  const trimmed = value.trim();
  if (!trimmed) return null;
  if (/^(?:\/|~|[A-Za-z]:|\\\\)/.test(trimmed)) return null;
  if (trimmed.split(/[\\/]/).includes("..")) return null;
  return trimmed;
}

const LOCAL_PATH_PATTERN =
  /(^|[\s("'=])((?:\/home\/|\/Users\/|\/root\/|\/tmp\/|\/var\/|\/etc\/|\/opt\/|\/srv\/)[^\s"'`)]*)/g;

/** Removes control characters, local absolute paths, and over-long text. */
function safeDetail(value: string): string {
  return sanitizeMessage(value).replace(
    LOCAL_PATH_PATTERN,
    (_match, prefix: string) => `${prefix}[path]`,
  );
}

function safeDetailList(value: unknown, field: string): string[] {
  return requireStringArray(value, field).map((item) => safeDetail(item));
}

function parseExecutorDoc(value: unknown, field: string): ExecutorView | null {
  if (value === null || value === undefined) return null;
  if (typeof value === "string") return parseExecutor(safeDetail(value));
  if (!isRecord(value)) {
    throw webError("parse", `The daemon returned an unreadable ${field}.`);
  }
  const harness = requireString(value.harness, `${field}.harness`);
  const model = requireString(value.model, `${field}.model`);
  const effort = optionalString(value.effort, `${field}.effort`);
  const variant = optionalString(value.variant, `${field}.variant`);
  const raw = `${harness}:${model}${effort ? `@${effort}` : ""}${variant ? `#${variant}` : ""}`;
  return parseExecutor(safeDetail(raw));
}

function parseJobDetailDoc(value: unknown): JobDetailDoc {
  if (!isRecord(value)) {
    throw webError("parse", "The daemon returned an unreadable job document.");
  }
  return {
    id: requireString(value.id, "job.id"),
    project: requireString(value.project, "job.project"),
    title: requireString(value.title, "job.title"),
    kind: requireString(value.kind, "job.kind"),
    role: requireString(value.role, "job.role"),
    state: requireString(value.state, "job.state"),
    priority:
      value.priority === undefined ? 50 : requireInteger(value.priority, "job.priority"),
    needs: value.needs === undefined ? [] : safeDetailList(value.needs, "job.needs"),
    attempts:
      value.attempts === undefined
        ? 0
        : Math.max(0, requireInteger(value.attempts, "job.attempts")),
    note: safeDetail(optionalString(value.note, "job.note") ?? ""),
    report_path: safeRepoPath(optionalString(value.report, "job.report")),
    parked_reason: optionalString(value.parked_reason, "job.parked_reason"),
    created_at: requireTimestamp(value.created_at, "job.created_at"),
    state_changed_at: requireTimestamp(value.state_changed_at, "job.state_changed_at"),
    updated_at: requireTimestamp(value.updated_at, "job.updated_at"),
    last_progress_at: optionalTimestamp(value.last_progress_at, "job.last_progress_at"),
  };
}

function parseRunDoc(value: unknown, index: number): JobRunView {
  if (!isRecord(value)) {
    throw webError("parse", `The daemon returned an unreadable run row at ${index}.`);
  }
  return {
    id: requireString(value.id, "run.id"),
    role: requireString(value.role, "run.role"),
    purpose: requireString(value.purpose, "run.purpose"),
    executor: parseExecutorDoc(value.executor, "run.executor"),
    started_at: requireTimestamp(value.started_at, "run.started_at"),
    ended_at: optionalTimestamp(value.ended_at, "run.ended_at"),
    outcome: optionalString(value.outcome, "run.outcome"),
    exit_code: optionalInteger(value.exit_code, "run.exit_code"),
    detail: safeDetail(optionalString(value.detail, "run.detail") ?? ""),
  };
}

function evidenceKind(value: unknown): string | null {
  if (!isRecord(value) || typeof value.kind !== "string") return null;
  return value.kind;
}

function gateKind(value: unknown): string {
  if (!isRecord(value)) return "unknown";
  const keys = Object.keys(value);
  return keys.length === 1 ? (keys[0] ?? "unknown") : "unknown";
}

function evidenceSummary(value: unknown): string | null {
  const kind = evidenceKind(value);
  if (!isRecord(value) || !kind) return null;
  const parts: string[] = [];
  switch (kind) {
    case "report": {
      const missing = safeDetailList(value.missing_fields ?? [], "gate.evidence.missing_fields");
      const problems = safeDetailList(value.problems ?? [], "gate.evidence.problems");
      if (missing.length > 0) parts.push(`missing: ${missing.join(", ")}`);
      if (problems.length > 0) {
        parts.push(`${problems.length} problem${problems.length === 1 ? "" : "s"}`);
      }
      return parts.join(" · ") || "report parsed";
    }
    case "cmd":
    case "script": {
      if (value.timed_out === true) parts.push("timed out");
      const code = optionalInteger(value.exit_code, "gate.evidence.exit_code");
      parts.push(code === null ? "no exit code" : `exit ${code}`);
      return parts.join(" · ");
    }
    case "owns_respected": {
      const changed = safeDetailList(value.changed ?? [], "gate.evidence.changed");
      const violations = safeDetailList(value.violations ?? [], "gate.evidence.violations");
      parts.push(`${changed.length} changed`);
      if (violations.length > 0) {
        parts.push(
          `${violations.length} outside owns: ${violations.slice(0, 3).join(", ")}${
            violations.length > 3 ? ", …" : ""
          }`,
        );
      }
      return parts.join(" · ");
    }
    case "pushed": {
      const branch = optionalString(value.branch, "gate.evidence.branch");
      return branch ? `branch ${safeDetail(branch)}` : "no branch reported";
    }
    case "review": {
      const verdict = optionalString(value.verdict, "gate.evidence.verdict");
      const findings = safeDetailList(value.findings ?? [], "gate.evidence.findings");
      parts.push(verdict ? `verdict ${safeDetail(verdict)}` : "no verdict");
      if (findings.length > 0) {
        parts.push(`${findings.length} finding${findings.length === 1 ? "" : "s"}`);
      }
      return parts.join(" · ");
    }
    case "deploy": {
      const code = optionalInteger(value.exit_code, "gate.evidence.exit_code");
      parts.push(code === null ? "no exit code" : `exit ${code}`);
      return parts.join(" · ");
    }
    case "secret_scan": {
      const files = optionalInteger(value.files_scanned, "gate.evidence.files_scanned") ?? 0;
      const hits = Array.isArray(value.hits) ? value.hits : [];
      parts.push(`${files} files scanned`);
      if (hits.length > 0) parts.push(`${hits.length} hit${hits.length === 1 ? "" : "s"}`);
      return parts.join(" · ");
    }
    default:
      return null;
  }
}

function parseGateDoc(value: unknown, index: number): JobGateView {
  if (!isRecord(value)) {
    throw webError("parse", `The daemon returned an unreadable gate row at ${index}.`);
  }
  const fromGate = gateKind(value.gate);
  const fromEvidence = evidenceKind(value.evidence);
  return {
    attempt: Math.max(0, requireInteger(value.attempt, "gate.attempt")),
    gate_index: requireInteger(value.gate_index, "gate.gate_index"),
    kind: safeDetail(fromGate !== "unknown" ? fromGate : (fromEvidence ?? "unknown")),
    passed: requireBoolean(value.passed, "gate.passed"),
    evidence_summary: evidenceSummary(value.evidence),
    duration_ms: Math.max(0, requireInteger(value.duration_ms, "gate.duration_ms")),
    checked_at: requireTimestamp(value.ts, "gate.ts"),
  };
}

function sortRuns(runs: JobRunView[]): JobRunView[] {
  return [...runs].sort((left, right) => {
    const byStarted = left.started_at.localeCompare(right.started_at);
    return byStarted !== 0 ? byStarted : left.id.localeCompare(right.id);
  });
}

function sortGates(gates: JobGateView[]): JobGateView[] {
  return [...gates].sort((left, right) => {
    const byIndex = left.gate_index - right.gate_index;
    return byIndex !== 0 ? byIndex : left.checked_at.localeCompare(right.checked_at);
  });
}

function ageSeconds(timestamp: Timestamp, nowMs: number): number {
  const parsed = Date.parse(timestamp);
  if (Number.isNaN(parsed)) return 0;
  return Math.max(0, Math.floor((nowMs - parsed) / 1000));
}

function boardKey(board: BoardDoc): string {
  // `Board.at` is a wall-clock collection time; the daemon resends only when the rest of
  // the document changes, so it must not decide whether a frame is new.
  return JSON.stringify({ ...board, at: "" });
}

type NotFoundClass = "item" | "route";

function errorFromStatus(
  status: number,
  path: string,
  body: unknown,
  notFound: NotFoundClass = "route",
): SpiritErrorDto {
  const apiMessage = isRecord(body) && isRecord(body.error) && typeof body.error.message === "string"
    ? safeDetail(body.error.message)
    : "";
  switch (status) {
    case 400:
      return webError("invalid_input", apiMessage || "The daemon rejected the request.", null, false);
    case 401:
    case 403:
      return webError(
        "auth",
        "The write proxy rejected this session.",
        null,
        false,
      );
    case 404:
      if (notFound === "item") {
        return webError(
          "not_found",
          JOB_DETAIL_NOT_FOUND,
          "Refresh the job list; the job may have been replanned or removed.",
          false,
        );
      }
      return webError(
        "daemon_incompatible",
        `The daemon does not serve ${path}.`,
        "This Web build only calls routes documented as present in the current daemon.",
        false,
      );
    case 409:
      return webError(
        "conflict",
        apiMessage || "VOWP changed this item; refreshing.",
        "Refresh and inspect the new state.",
        false,
      );
    case 413:
    case 415:
      return webError("invalid_input", apiMessage || "The item is too large for this daemon.", null, false);
    case 503:
      return webError("writes_disabled", WRITES_DISABLED, null, false);
    case 500:
      return webError(
        "remote",
        apiMessage || "The daemon failed to serve the request.",
        "Retry once; if it persists, inspect the daemon log.",
        true,
      );
    default:
      return webError(
        "remote",
        apiMessage || `The daemon answered with HTTP ${status}.`,
        null,
        status >= 500,
      );
  }
}

function timeoutError(): SpiritErrorDto {
  return webError(
    "remote_timeout",
    "The daemon did not answer in time.",
    "The read timed out after 10 seconds; retry when the daemon is responsive.",
    true,
  );
}

function unreachableError(origin: string): SpiritErrorDto {
  return webError(
    "daemon_unreachable",
    `The daemon at ${origin} did not answer.`,
    "Start the daemon (or its reverse proxy) and retry.",
    true,
  );
}

function parseError(): SpiritErrorDto {
  return webError(
    "parse",
    "The daemon returned a document StackSpirit could not read.",
    "Keep the last view; check the daemon version and the API contract.",
    false,
  );
}

function incompatibleError(message: string): SpiritErrorDto {
  return webError("daemon_incompatible", message, "Refresh after the daemon and the API agree.", false);
}

function desktopOnly(message: string): SpiritErrorDto {
  return webError("desktop_only", message, null, false);
}

function isSpiritErrorDto(value: unknown): value is SpiritErrorDto {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as { code?: unknown; message?: unknown };
  return typeof candidate.code === "string" && typeof candidate.message === "string";
}

/** The Web bridge over the daemon's read-only HTTP and board SSE API. */
export class HttpSseBridge {
  readonly kind = "http-sse" as const;
  readonly preview = false;
  readonly capabilities = WEB_READ_ONLY_CAPABILITIES;

  private readonly baseUrl: string;
  private readonly origin: string;
  private readonly pageOrigin: string | null;
  private readonly fetchImpl: FetchLike;
  private readonly eventSourceFactory: ((url: string) => EventSourceLike) | null;
  private readonly now: () => number;
  private readonly readTimeoutMs: number;
  private readonly pollIntervalMs: number;
  private readonly staleBackoffMs: number;
  private readonly heartbeatIntervalMs: number;
  private readonly heartbeatTimeoutMs: number;
  private readonly reconnectBaseMs: number;
  private readonly reconnectMaxMs: number;

  private connectionState: ConnectionSnapshot = {
    state: "connecting",
    host_id: "daemon",
    since: null,
    latency: null,
    last_ok_at: null,
    error: null,
  };
  private lastPublishedConnection: string | null = null;
  private health: HealthDoc | null = null;
  private healthReceivedAt: number | null = null;
  private lastSnapshot: WebFleetView | null = null;
  private revision = 0;
  private requestSeq = 0;
  private appliedSeq = 0;
  private lastBoardKey: string | null = null;
  private secondary: SecondaryRows = {
    jobs: [],
    sessions: [],
    inbox: [],
    jobsTruncated: false,
    reviewDetails: new Map(),
    reviewReadError: null,
    reviewRouteMissing: false,
  };
  private latencySamples: number[] = [];
  private ewmaMs = 0;
  private lastFrameAt: number | null = null;
  private lastHeartbeatAt: number | null = null;
  private nextRetryAt: number | null = null;
  private streamState: WebBridgeStatus["stream_state"] = "idle";
  private pollFailures = 0;
  private started = false;
  private startPromise: Promise<void> | null = null;
  private disposed = false;
  private pollingOnly = false;

  private stream: EventSourceLike | null = null;
  private streamOpen = false;
  private reconnectDelayMs: number;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private pollTimer: ReturnType<typeof setTimeout> | null = null;
  private heartbeatTimer: ReturnType<typeof setInterval> | null = null;
  private pendingBoard: PendingBoard | null = null;
  private boardApplyInFlight = false;
  private boardApplyDirty = false;

  private readonly eventListeners = new Set<(event: StateEvent) => void>();
  private readonly snapshotListeners = new Set<(snapshot: WebFleetView) => void>();
  private readonly inflight = new Set<AbortController>();
  private readonly requestTimers = new Set<ReturnType<typeof setTimeout>>();

  constructor(options: HttpSseBridgeOptions) {
    this.baseUrl = options.baseUrl.replace(/\/+$/, "");
    this.origin = new URL(this.baseUrl).origin;
    this.pageOrigin =
      options.pageOrigin ??
      (typeof globalThis.location !== "undefined" ? globalThis.location.origin : null);
    if (this.pageOrigin && !isSameOrigin(this.baseUrl, this.pageOrigin)) {
      throw sameOriginError();
    }
    this.fetchImpl = options.fetchImpl ?? ((input, init) => fetch(input, init));
    this.eventSourceFactory = options.eventSourceFactory ?? null;
    this.now = options.now ?? (() => Date.now());
    this.readTimeoutMs = options.readTimeoutMs ?? REQUEST_TIMEOUT_MS;
    this.pollIntervalMs = options.pollIntervalMs ?? POLL_INTERVAL_MS;
    this.staleBackoffMs = options.staleBackoffMs ?? STALE_BACKOFF_MS;
    this.heartbeatIntervalMs = options.heartbeatIntervalMs ?? HEARTBEAT_INTERVAL_MS;
    this.heartbeatTimeoutMs = options.heartbeatTimeoutMs ?? HEARTBEAT_TIMEOUT_MS;
    this.reconnectBaseMs = options.reconnectBaseMs ?? RECONNECT_BASE_MS;
    this.reconnectMaxMs = options.reconnectMaxMs ?? RECONNECT_MAX_MS;
    this.reconnectDelayMs = this.reconnectBaseMs;
  }

  connectionApi: SpiritBridge["connection"] = {
    connect: async (): Promise<ConnectionSnapshot> => {
      throw desktopOnly(
        "The Web build already speaks HTTP to one daemon; there is no SSH connection to establish.",
      );
    },
    disconnect: async (): Promise<void> => {
      throw desktopOnly(
        "The Web build has no daemon connect/disconnect control; the page already speaks to one daemon.",
      );
    },
    status: async (): Promise<ConnectionSnapshot> => clone(this.connectionState),
    subscribe: (listener: (event: StateEvent) => void): (() => void) => {
      this.eventListeners.add(listener);
      return () => this.eventListeners.delete(listener);
    },
  };

  hostsApi: SpiritBridge["hosts"] = {
    list: async () => {
      throw desktopOnly(
        "Host profiles and SSH keys live in the desktop app. This Web build talks to one daemon and cannot list hosts.",
      );
    },
    add: async () => {
      throw desktopOnly(
        "Host profiles and SSH keys live in the desktop app. This Web build cannot add hosts.",
      );
    },
    update: async () => {
      throw desktopOnly(
        "Host profiles and SSH keys live in the desktop app. This Web build cannot update hosts.",
      );
    },
    remove: async () => {
      throw desktopOnly(
        "Host profiles and SSH keys live in the desktop app. This Web build cannot remove hosts.",
      );
    },
    scanFingerprint: async () => {
      throw desktopOnly("SSH host-key scanning is a desktop action.");
    },
    trustFingerprint: async () => {
      throw desktopOnly("The app-private known_hosts entry lives on the desktop.");
    },
    probe: async () => {
      throw desktopOnly(
        "SSH reachability, auth, host key, and remote VOWP checks are desktop actions.",
      );
    },
  };

  fleetApi: WebFleetApi = {
    /**
     * The desktop-shaped `FleetView` cannot be honest here: the daemon exposes neither
     * capacity nor supervisor state. The Web shell uses `snapshot()` instead.
     */
    get: async (): Promise<null> => null,
    refresh: async (): Promise<never> => {
      throw webError(
        "no_route",
        "The daemon has no capacity or supervisor route, so the desktop FleetView is unavailable.",
        "Use fleet.snapshot(true) for the honest Web read model.",
      );
    },
    capacity: async (): Promise<null> => null,
    job: async (id: string): Promise<JobDetailView> => {
      if (this.disposed) throw webError("internal", "The Web bridge is disposed.");
      return this.readJobDetail(id);
    },
    jobAction: async (): Promise<never> => {
      throw webError("writes_disabled", WRITES_DISABLED);
    },
    inbox: async (all: boolean): Promise<InboxItemView[]> => this.readInbox(all),
    answerInbox: async (): Promise<never> => {
      throw webError("writes_disabled", WRITES_DISABLED);
    },
    setTarget: async (): Promise<never> => {
      throw webError(
        "no_route",
        "The daemon has no capacity target route (POST /api/v1/capacity/target).",
        "The target editor is unavailable in this build.",
      );
    },
    rawDocument: async (): Promise<null> => null,
    snapshot: async (refresh = false): Promise<WebFleetView | null> => this.snapshot(refresh),
    watch: (listener: (snapshot: WebFleetView) => void): (() => void) => {
      if (this.disposed) return () => undefined;
      this.snapshotListeners.add(listener);
      if (this.lastSnapshot && !this.disposed) {
        queueMicrotask(() => {
          if (this.lastSnapshot && !this.disposed) listener(clone(this.lastSnapshot));
        });
      }
      void this.ensureStarted().catch(() => undefined);
      return () => this.snapshotListeners.delete(listener);
    },
    webStatus: (): WebBridgeStatus => this.webStatus(),
  };

  engineApi: SpiritBridge["engine"] = {
    status: async () => {
      throw desktopOnly(
        "The StackSpirit Engine runs beside the desktop app. This Web build cannot read engine status.",
      );
    },
    models: async () => {
      throw desktopOnly("Models and provider keys stay in the desktop app.");
    },
    selectModel: async () => {
      throw desktopOnly("Model selection stays in the desktop app.");
    },
    storeKey: async () => {
      throw desktopOnly("Provider keys never enter the browser.");
    },
    verifyKey: async () => {
      throw desktopOnly("Provider keys never enter the browser.");
    },
    deleteKey: async () => {
      throw desktopOnly("Provider keys never enter the browser.");
    },
    keyStatus: async () => {
      throw desktopOnly("Provider keys never enter the browser.");
    },
    scanExternal: async () => {
      throw desktopOnly("External harness discovery runs beside the desktop app.");
    },
    selectExternal: async () => {
      throw desktopOnly("External harness selection stays in the desktop app.");
    },
    useBundled: async () => {
      throw desktopOnly("Engine choice stays in the desktop app.");
    },
    restart: async () => {
      throw desktopOnly("Engine supervision stays in the desktop app.");
    },
  };

  gatesApi: SpiritBridge["gates"] = {
    get: async () => {
      throw desktopOnly(
        "Capability gates are probed over SSH from the desktop app. This Web build cannot probe them.",
      );
    },
    probe: async () => {
      throw desktopOnly(
        "Capability gates are probed over SSH from the desktop app. This Web build cannot probe them.",
      );
    },
  };

  daemonApi: NonNullable<SpiritBridge["daemon"]> = {
    health: async (): Promise<DaemonHealth> => {
      if (this.disposed) throw webError("internal", "The Web bridge is disposed.");
      const health = await this.getHealth(true);
      return this.daemonHealth(health);
    },
  };

  hosts = this.hostsApi;
  connection = this.connectionApi;
  fleet = this.fleetApi;
  engine = this.engineApi;
  gates = this.gatesApi;
  daemon = this.daemonApi;

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.started = false;
    this.startPromise = null;
    this.closeStream();
    this.clearPollTimer();
    this.clearReconnectTimer();
    this.clearHeartbeatTimer();
    for (const controller of this.inflight) controller.abort();
    this.inflight.clear();
    for (const timer of this.requestTimers) clearTimeout(timer);
    this.requestTimers.clear();
    this.snapshotListeners.clear();
    this.eventListeners.clear();
    this.streamState = "closed";
  }

  private async snapshot(refresh: boolean): Promise<WebFleetView | null> {
    if (this.disposed) return this.lastSnapshot;
    await this.ensureStarted();
    if (refresh) return this.refreshSnapshot();
    return this.lastSnapshot;
  }

  private ensureStarted(): Promise<void> {
    if (this.disposed) return Promise.resolve();
    if (this.started) return Promise.resolve();
    if (!this.startPromise) {
      this.startPromise = this.start().finally(() => {
        this.startPromise = null;
      });
    }
    return this.startPromise;
  }

  private async start(): Promise<void> {
    if (this.started) return;
    try {
      await this.getHealth(false);
      await this.refreshSnapshot();
      this.started = true;
      this.connectStream();
    } catch (error) {
      this.started = false;
      const mapped = asSpiritError(error);
      this.publishError(mapped);
      this.updateConnection({
        state: "failed",
        error: mapped,
        since: null,
        last_ok_at: this.connectionState.last_ok_at,
      });
      throw mapped;
    }
  }

  private get url(): string {
    return this.baseUrl;
  }

  private route(path: string): string {
    return `${this.url}${path}`;
  }

  private async refreshSnapshot(): Promise<WebFleetView | null> {
    const seq = ++this.requestSeq;
    try {
      let health = this.health;
      try {
        health = await this.getHealth(false);
      } catch (error) {
        if (!health) throw error;
        this.publishError(asSpiritError(error));
      }
      if (!health) throw unreachableError(this.origin);
      const board = await this.getBoard();
      if (seq < this.appliedSeq) return this.lastSnapshot;
      const applied = await this.applyGeneration(board, health, seq);
      this.updateConnection({
        state: "connected",
        latency: this.latencyStats(),
        last_ok_at: iso(this.now()),
        error: null,
      });
      return applied;
    } catch (error) {
      if (this.disposed) return this.lastSnapshot;
      const mapped = asSpiritError(error);
      this.publishError(mapped);
      if (!this.lastSnapshot) {
        this.updateConnection({ state: "failed", error: mapped });
      }
      throw mapped;
    }
  }

  private async applyGeneration(
    board: BoardDoc,
    health: HealthDoc,
    seq: number,
  ): Promise<WebFleetView | null> {
    if (seq < this.appliedSeq) return this.lastSnapshot;
    const rows = await this.fetchSecondary();
    if (this.disposed || seq < this.appliedSeq) return this.lastSnapshot;
    const nextRevision = this.revision + 1;
    const snapshot = this.buildSnapshot(board, health, this.now(), rows, nextRevision);
    this.appliedSeq = seq;
    this.revision = nextRevision;
    this.lastBoardKey = boardKey(board);
    this.pollFailures = 0;
    this.nextRetryAt = null;
    this.lastFrameAt = this.now();
    this.lastSnapshot = snapshot;
    this.publishSnapshot(snapshot);
    return snapshot;
  }

  private async fetchSecondary(): Promise<SecondaryRows> {
    const results = await Promise.allSettled([
      this.getJobs(),
      this.getSessions(),
      this.getInboxPage("open"),
    ]);
    const [jobs, sessions, inbox] = results;
    if (jobs.status === "rejected") {
      this.publishError(asSpiritError(jobs.reason));
    }
    if (sessions.status === "rejected") {
      this.publishError(asSpiritError(sessions.reason));
    }
    if (inbox.status === "rejected") {
      this.publishError(asSpiritError(inbox.reason));
    }
    const jobsRows = jobs.status === "fulfilled" ? jobs.value.items : this.secondary.jobs;
    let reviewDetails = this.secondary.reviewDetails;
    let reviewReadError = this.secondary.reviewReadError;
    let reviewRouteMissing = this.secondary.reviewRouteMissing;
    if (jobs.status === "fulfilled") {
      reviewDetails = new Map();
      reviewReadError = null;
      reviewRouteMissing = false;
      const targets = jobsRows.filter((job) => laneForState(job.state) === "review_fix");
      const details = await Promise.allSettled(
        targets.map((job) => this.readJobDetail(job.id)),
      );
      details.forEach((result, index) => {
        if (result.status === "fulfilled") {
          const job = targets[index];
          if (job) reviewDetails.set(job.id, result.value);
          return;
        }
        const error = asSpiritError(result.reason);
        if (error.code === "not_found") return;
        if (error.code === "daemon_incompatible") {
          reviewRouteMissing = true;
          return;
        }
        reviewReadError = reviewReadError ?? error;
      });
    }
    const rows: SecondaryRows = {
      jobs: jobsRows,
      sessions:
        sessions.status === "fulfilled" ? sessions.value.items : this.secondary.sessions,
      inbox: inbox.status === "fulfilled" ? inbox.value.items : this.secondary.inbox,
      jobsTruncated:
        jobs.status === "fulfilled"
          ? jobs.value.next !== null
          : this.secondary.jobsTruncated,
      reviewDetails,
      reviewReadError,
      reviewRouteMissing,
    };
    this.secondary = rows;
    return rows;
  }

  private buildSnapshot(
    board: BoardDoc,
    health: HealthDoc,
    nowMs: number,
    rows: SecondaryRows,
    revision: number,
  ): WebFleetView {
    if (health.project !== board.project) {
      throw incompatibleError(
        `GET /api/v1/health reports project "${health.project}" but GET /api/v1/board reports "${board.project}".`,
      );
    }
    const jobsById = new Map(rows.jobs.map((job) => [job.id, job]));
    const sessionsByJob = new Map<string, SessionDoc>();
    for (const session of rows.sessions) {
      if (!session.job_id) continue;
      const current = sessionsByJob.get(session.job_id);
      if (!current || Date.parse(session.updated_at) > Date.parse(current.updated_at)) {
        sessionsByJob.set(session.job_id, session);
      }
    }

    const running: WorkerCardView[] = board.running.map((run) => {
      const job = jobsById.get(run.job);
      const session = sessionsByJob.get(run.job);
      const stalled = job?.state === "stalled";
      return {
        job_id: run.job,
        project: job?.project ?? "unknown",
        kind: job?.kind ?? "unknown",
        role: job?.role ?? "unknown",
        state: stalled ? "stalled" : "active",
        stalled,
        executor: run.executor ? parseExecutor(run.executor) : null,
        node: session?.node ?? null,
        age_secs: run.age_secs,
        last_event_age_secs: run.last_event_age_secs,
        session_id: session?.id ?? null,
      };
    });

    const project: ProjectView = {
      name: health.project,
      state: "unknown",
      paused_reason: "",
      jobs_by_state: board.jobs,
      cost_today_usd: board.cost_today_usd,
    };
    const counts = countsFromProjects([project]);

    const runningLane: JobCardView[] = board.running.map((run) => {
      const job = jobsById.get(run.job);
      const session = sessionsByJob.get(run.job);
      return {
        job_id: run.job,
        project: job?.project ?? "unknown",
        title: job?.title ?? null,
        kind: job?.kind ?? "unknown",
        role: job?.role ?? "unknown",
        state: job?.state ?? "running",
        priority: job?.priority ?? 0,
        age_secs: run.age_secs,
        reason: null,
        detail: job ? null : "job detail unavailable",
        gate: null,
        gate_at: null,
        gate_evidence: null,
        executor: run.executor ? parseExecutor(run.executor) : null,
        session_id: session?.id ?? null,
      };
    });

    const ready: JobCardView[] = [];
    const reviewFix: JobCardView[] = [];
    const parked: JobCardView[] = [];
    let queueShown = 0;
    for (const job of rows.jobs) {
      const lane = laneForState(job.state);
      if (lane !== "done" && lane !== "running") queueShown += 1;
      if (lane === "ready") {
        ready.push(this.jobCard(job, nowMs));
      } else if (lane === "review_fix") {
        reviewFix.push(this.reviewFixCard(job, rows.reviewDetails.get(job.id) ?? null, nowMs));
      } else if (lane === "parked_blocked") {
        parked.push(this.jobCard(job, nowMs));
      }
    }

    const lanes: WebJobLanes = {
      running: runningLane,
      ready,
      review_fix: reviewFix,
      parked_blocked: parked,
      done: [],
      counts: { ...counts },
      queue_truncated: rows.jobsTruncated,
      queue_shown: queueShown,
      queue_cap: null,
    };

    const unavailable: UnavailableNotice[] = [
      {
        feature: "capacity_report",
        code: "no_route",
        route: "/api/v1/capacity",
        detail: CAPACITY_DETAIL_UNAVAILABLE,
      },
      {
        feature: "supervisor_state",
        code: "no_route",
        route: "/api/v1/fleet",
        detail: SUPERVISOR_STATE_UNAVAILABLE,
      },
      {
        feature: "project_state",
        code: "no_route",
        route: "/api/v1/fleet",
        detail: PROJECT_STATE_UNAVAILABLE,
      },
      {
        feature: "active_surfaces",
        code: "no_route",
        route: "/api/v1/fleet",
        detail: SESSION_SURFACES_UNAVAILABLE,
      },
      {
        feature: "orchestrators",
        code: "no_route",
        route: "/api/v1/fleet",
        detail: ORCHESTRATORS_UNAVAILABLE,
      },
      {
        feature: "raw_documents",
        code: "no_route",
        route: null,
        detail: RAW_DOCUMENT_UNAVAILABLE,
      },
    ];
    if (rows.reviewRouteMissing) {
      unavailable.push({
        feature: "gate_failures",
        code: "no_route",
        route: "/api/v1/jobs/{id}/gates",
        detail: GATE_FAILURES_UNAVAILABLE,
      });
    } else if (rows.reviewReadError) {
      unavailable.push({
        feature: "gate_failures",
        code: "read_failed",
        route: "/api/v1/jobs/{id}/gates",
        detail: GATE_FAILURES_READ_FAILED,
      });
    }
    return {
      revision,
      host_id: "daemon",
      fetched_at: iso(nowMs),
      generated_at: board.at,
      source: "daemon",
      stale: false,
      home: health.node,
      project,
      board: {
        slots_busy: board.slots_busy,
        slots_total: board.slots_total,
        planner: board.planner,
        keeper: board.keeper,
        limits: board.limits,
        cost_today_usd: board.cost_today_usd,
        inbox_open: board.inbox_open,
        last_checkpoint: board.last_checkpoint,
      },
      capacity_unavailable: {
        code: "no_route",
        route: "/api/v1/capacity",
        detail: CAPACITY_DETAIL_UNAVAILABLE,
      },
      supervisor_unavailable: {
        code: "no_route",
        detail: SUPERVISOR_STATE_UNAVAILABLE,
      },
      running,
      lanes,
      inbox: rows.inbox.map((item) => this.inboxView(item, nowMs)),
      sessions: [],
      orchestrators: [],
      latency: this.latencyStats(),
      unavailable,
      daemon: this.daemonHealth(health),
    };
  }

  private jobCard(job: JobDoc, nowMs: number): JobCardView {
    return {
      job_id: job.id,
      project: job.project,
      title: job.title,
      kind: job.kind,
      role: job.role,
      state: job.state,
      priority: job.priority,
      age_secs: ageSeconds(job.state_changed_at, nowMs),
      reason: job.parked_reason ?? (job.state === "parked" ? "parked" : null),
      detail: null,
      gate: null,
      gate_at: null,
      gate_evidence: null,
      executor: null,
      session_id: null,
    };
  }

  /**
   * A review/fix card carries the real failed gates read from
   * `GET /api/v1/jobs/{id}/gates`, or an honest "no gate results" label. It never falls
   * back to the old "route unavailable" placeholder when the read succeeded.
   */
  private reviewFixCard(
    job: JobDoc,
    detail: JobDetailView | null,
    nowMs: number,
  ): JobCardView {
    const card = this.jobCard(job, nowMs);
    if (!detail) {
      return {
        ...card,
        detail: "Gate results were not read for this job; the board card is still real.",
      };
    }
    const failed = detail.gates.filter((gate) => !gate.passed);
    if (failed.length > 0) {
      const kinds = [...new Set(failed.map((gate) => gate.kind))];
      const first = failed[0];
      return {
        ...card,
        gate: kinds.join(", "),
        gate_at: first?.checked_at ?? null,
        gate_evidence:
          first?.evidence_summary ??
          `${failed.length} gate${failed.length === 1 ? "" : "s"} failed`,
      };
    }
    if (detail.gates.length === 0) {
      return {
        ...card,
        detail: "No gate results recorded for the newest attempt.",
      };
    }
    return {
      ...card,
      detail: "All recorded gates passed for the newest attempt.",
    };
  }

  private inboxView(item: InboxDoc, nowMs: number): InboxItemView {
    return {
      id: item.id,
      project: item.project,
      job_id: item.job_id,
      kind: item.kind,
      title: item.title,
      body: item.body,
      options: item.options,
      state: item.state,
      answer: item.answer,
      answered_by: item.answered_by,
      created_at: item.created_at,
      answered_at: item.answered_at,
      age_secs: ageSeconds(item.created_at, nowMs),
    };
  }

  private async readInbox(all: boolean): Promise<InboxItemView[]> {
    const nowMs = this.now();
    const pages = all
      ? await Promise.all([
          this.getInboxPage("open"),
          this.getInboxPage("answered"),
          this.getInboxPage("dismissed"),
        ])
      : [await this.getInboxPage("open")];
    const byId = new Map<string, InboxDoc>();
    for (const page of pages) {
      for (const item of page.items) byId.set(item.id, item);
    }
    return [...byId.values()]
      .sort((left, right) => {
        const byCreated = left.created_at.localeCompare(right.created_at);
        return byCreated !== 0 ? byCreated : left.id.localeCompare(right.id);
      })
      .map((item) => this.inboxView(item, nowMs));
  }

  private async getHealth(force: boolean): Promise<HealthDoc> {
    if (
      !force &&
      this.health &&
      this.healthReceivedAt !== null &&
      this.now() - this.healthReceivedAt < 30_000
    ) {
      return this.health;
    }
    const value = await this.requestJson<unknown>(READ_ROUTES.health);
    const health = parseHealthDoc(value);
    this.health = health;
    this.healthReceivedAt = this.now();
    return health;
  }

  private daemonHealth(health: HealthDoc): DaemonHealth {
    return {
      origin: this.origin,
      version: health.version,
      node: health.node,
      project: health.project,
      schema_version: health.schema_version,
      heartbeat_at: health.heartbeat,
      received_at: iso(this.healthReceivedAt ?? this.now()),
    };
  }

  private async getBoard(): Promise<BoardDoc> {
    const startedAt = this.now();
    const value = await this.requestJson<unknown>(READ_ROUTES.board);
    this.recordLatency(Math.max(0, this.now() - startedAt));
    return parseBoardDoc(value);
  }

  /**
   * Reads the three token-free job-detail routes. The job read comes first so a `404` can
   * be reported as `not_found`; a `404` from `/runs` or `/gates` after the job read is a
   * missing route, not an empty list.
   */
  private async readJobDetail(id: string): Promise<JobDetailView> {
    const trimmed = id.trim();
    if (!/^[a-z0-9][a-z0-9-]{0,63}$/.test(trimmed) || trimmed.endsWith("-")) {
      throw webError("invalid_input", "A job id must be a lowercase slug.", null, false);
    }
    const jobValue = await this.requestJson<unknown>(JOB_DETAIL_ROUTES.job(trimmed), "item");
    const base = parseJobDetailDoc(jobValue);
    if (base.id !== trimmed) {
      throw incompatibleError(
        `GET /api/v1/jobs/${trimmed} returned a document for job "${base.id}".`,
      );
    }
    const [runsValue, gatesValue] = await Promise.all([
      this.requestJson<unknown>(JOB_DETAIL_ROUTES.runs(trimmed), "route"),
      this.requestJson<unknown>(JOB_DETAIL_ROUTES.gates(trimmed), "route"),
    ]);
    const runsPage = parsePage(runsValue, parseRunDoc);
    const gatesPage = parsePage(gatesValue, parseGateDoc);
    return {
      ...base,
      fetched_at: iso(this.now()),
      runs: sortRuns(runsPage.items),
      runs_truncated: runsPage.next !== null,
      gates: sortGates(gatesPage.items),
      gates_truncated: gatesPage.next !== null,
    };
  }

  private async getJobs(): Promise<PageDoc<JobDoc>> {
    const value = await this.requestJson<unknown>(READ_ROUTES.jobs);
    return parsePage(value, parseJobDoc);
  }

  private async getSessions(): Promise<PageDoc<SessionDoc>> {
    const value = await this.requestJson<unknown>(READ_ROUTES.sessions);
    return parsePage(value, parseSessionDoc);
  }

  private async getInboxPage(state: "open" | "answered" | "dismissed"): Promise<PageDoc<InboxDoc>> {
    const value = await this.requestJson<unknown>(
      `${READ_ROUTES.inbox}?state=${encodeURIComponent(state)}`,
    );
    return parsePage(value, parseInboxDoc);
  }

  private async requestJson<T>(
    path: string,
    notFound: NotFoundClass = "route",
  ): Promise<T> {
    let lastError: SpiritErrorDto | null = null;
    for (let attempt = 0; attempt < 2; attempt += 1) {
      try {
        return await this.requestJsonOnce<T>(path, notFound);
      } catch (error) {
        const mapped = asSpiritError(error);
        lastError = mapped;
        if (mapped.code !== "remote" || !mapped.retryable || attempt === 1) throw mapped;
      }
    }
    throw lastError ?? webError("remote", "The daemon failed to serve the request.");
  }

  private async requestJsonOnce<T>(
    path: string,
    notFound: NotFoundClass = "route",
  ): Promise<T> {
    const controller = new AbortController();
    this.inflight.add(controller);
    let timedOut = false;
    const timeout = setTimeout(() => {
      timedOut = true;
      controller.abort();
    }, this.readTimeoutMs);
    this.requestTimers.add(timeout);
    try {
      let response: Response;
      try {
        response = await this.fetchImpl(this.route(path), {
          method: "GET",
          redirect: "manual",
          credentials: "omit",
          cache: "no-store",
          signal: controller.signal,
        });
      } catch (error) {
        if (this.disposed) throw error;
        if (timedOut) throw timeoutError();
        throw unreachableError(this.origin);
      }
      if (
        response.type === "opaqueredirect" ||
        (response.status >= 300 && response.status < 400)
      ) {
        throw incompatibleError(
          "The daemon answered with a redirect. API requests are never followed to a login page.",
        );
      }
      const text = await response.text();
      let body: unknown = null;
      if (text.trim()) {
        try {
          body = JSON.parse(text);
        } catch {
          if (response.ok) throw parseError();
        }
      }
      if (!response.ok) {
        throw errorFromStatus(response.status, path.split("?")[0] ?? path, body, notFound);
      }
      if (body === null) throw parseError();
      return body as T;
    } catch (error) {
      if (isSpiritErrorDto(error)) throw error;
      if (this.disposed) throw error;
      if (timedOut) throw timeoutError();
      throw unreachableError(this.origin);
    } finally {
      clearTimeout(timeout);
      this.requestTimers.delete(timeout);
      this.inflight.delete(controller);
    }
  }

  private recordLatency(ms: number): void {
    const sample = Math.max(0, ms);
    this.ewmaMs = this.latencySamples.length === 0 ? sample : this.ewmaMs * 0.7 + sample * 0.3;
    this.latencySamples = [...this.latencySamples.slice(-19), sample];
  }

  private latencyStats(): LatencyStats {
    const sorted = [...this.latencySamples].sort((left, right) => left - right);
    const pick = (percent: number): number => {
      if (sorted.length === 0) return 0;
      const index = Math.min(
        sorted.length - 1,
        Math.max(0, Math.ceil((sorted.length * percent) / 100) - 1),
      );
      return sorted[index] ?? 0;
    };
    const last = this.latencySamples[this.latencySamples.length - 1] ?? 0;
    return {
      last_ms: last,
      ewma_ms: this.ewmaMs,
      p50_ms: pick(50),
      p95_ms: pick(95),
      samples: this.latencySamples.length,
    };
  }

  private publishError(error: SpiritErrorDto): void {
    if (this.disposed) return;
    const event: StateEvent = {
      kind: "error",
      revision: this.revision,
      error: clone(error),
    };
    for (const listener of this.eventListeners) listener(event);
  }

  private publishSnapshot(snapshot: WebFleetView): void {
    if (this.disposed) return;
    for (const listener of this.snapshotListeners) {
      listener(clone(snapshot));
    }
  }

  private updateConnection(patch: Partial<ConnectionSnapshot>): void {
    const previous = this.connectionState;
    const next: ConnectionSnapshot = { ...previous, ...patch };
    if (next.state === "connected" && previous.state !== "connected" && !next.since) {
      next.since = iso(this.now());
    }
    if (next.state !== "connected" && next.state !== "stale" && next.state !== "reconnecting") {
      next.since = null;
    }
    if (next.latency === null && this.latencySamples.length > 0) {
      next.latency = this.latencyStats();
    }
    this.connectionState = next;
    const key = JSON.stringify(next);
    if (key === this.lastPublishedConnection) return;
    this.lastPublishedConnection = key;
    const event: StateEvent = {
      kind: "connection",
      revision: this.revision,
      connection: clone(next),
    };
    for (const listener of this.eventListeners) listener(event);
  }

  private connectStream(): void {
    if (this.disposed || this.stream || this.streamOpen) return;
    this.clearReconnectTimer();
    let source: EventSourceLike | null;
    try {
      source = this.makeEventSource();
    } catch {
      this.pollingOnly = true;
      this.streamState = "polling";
      this.schedulePoll(this.pollIntervalMs);
      return;
    }
    if (!source) {
      this.pollingOnly = true;
      this.streamState = "polling";
      this.schedulePoll(this.pollIntervalMs);
      return;
    }
    this.stream = source;
    this.streamState = "reconnecting";
    source.onopen = () => {
      if (this.stream !== source || this.disposed) return;
      this.handleStreamOpen();
    };
    source.onerror = () => {
      if (this.stream !== source || this.disposed) return;
      this.handleStreamError();
    };
    source.addEventListener("board", (event) => {
      if (this.stream !== source || this.disposed) return;
      this.handleBoardFrame(event.data);
    });
    source.addEventListener("heartbeat", (event) => {
      if (this.stream !== source || this.disposed) return;
      this.handleHeartbeat(event.data);
    });
  }

  private makeEventSource(): EventSourceLike | null {
    if (this.eventSourceFactory) {
      return this.eventSourceFactory(this.route(READ_ROUTES.boardStream));
    }
    if (typeof EventSource === "undefined") return null;
    return new EventSource(this.route(READ_ROUTES.boardStream), {
      withCredentials: false,
    }) as unknown as EventSourceLike;
  }

  private handleStreamOpen(): void {
    this.streamOpen = true;
    this.pollingOnly = false;
    this.streamState = "open";
    this.pollFailures = 0;
    this.nextRetryAt = null;
    this.reconnectDelayMs = this.reconnectBaseMs;
    this.stopPolling();
    this.lastFrameAt = this.now();
    this.lastHeartbeatAt = this.now();
    this.updateConnection({
      state: "connected",
      latency: this.latencyStats(),
      last_ok_at: iso(this.now()),
      error: null,
    });
    this.clearStale();
    this.startHeartbeatWatchdog();
  }

  private handleStreamError(): void {
    if (this.disposed) return;
    this.closeStream();
    this.streamState = "reconnecting";
    this.updateConnection({
      state: "reconnecting",
      last_ok_at: this.connectionState.last_ok_at,
    });
    this.schedulePoll(this.pollIntervalMs);
    this.scheduleReconnect();
  }

  private closeStream(): void {
    if (this.stream) {
      try {
        this.stream.close();
      } catch {
        // A closed source is already clean.
      }
      this.stream = null;
    }
    this.streamOpen = false;
    this.clearHeartbeatTimer();
  }

  private startHeartbeatWatchdog(): void {
    this.clearHeartbeatTimer();
    this.heartbeatTimer = setInterval(() => {
      if (this.disposed || !this.streamOpen) return;
      const last = this.lastFrameAt ?? this.now();
      if (this.now() - last > this.heartbeatTimeoutMs) {
        this.handleStreamError();
      }
    }, this.heartbeatIntervalMs);
  }

  private scheduleReconnect(): void {
    if (this.disposed || this.reconnectTimer !== null) return;
    if (this.eventSourceFactory === null && typeof EventSource === "undefined") return;
    const delay = this.reconnectDelayMs;
    this.reconnectDelayMs = Math.min(this.reconnectDelayMs * 2, this.reconnectMaxMs);
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      if (this.disposed || this.streamOpen) return;
      this.connectStream();
    }, delay);
  }

  private schedulePoll(delayMs: number): void {
    if (this.disposed) return;
    this.clearPollTimer();
    this.streamState = this.pollingOnly ? "polling" : "reconnecting";
    this.pollTimer = setTimeout(() => {
      this.pollTimer = null;
      void this.pollOnce();
    }, delayMs);
  }

  private clearPollTimer(): void {
    if (this.pollTimer !== null) {
      clearTimeout(this.pollTimer);
      this.pollTimer = null;
    }
  }

  private stopPolling(): void {
    this.clearPollTimer();
  }

  private clearReconnectTimer(): void {
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
  }

  private clearHeartbeatTimer(): void {
    if (this.heartbeatTimer !== null) {
      clearInterval(this.heartbeatTimer);
      this.heartbeatTimer = null;
    }
  }

  private async pollOnce(): Promise<void> {
    if (this.disposed || this.streamOpen) return;
    this.streamState = this.pollingOnly ? "polling" : "reconnecting";
    const seq = ++this.requestSeq;
    try {
      const board = await this.getBoard();
      if (this.disposed || this.streamOpen) return;
      this.pollFailures = 0;
      this.nextRetryAt = null;
      this.lastFrameAt = this.now();
      this.updateConnection({
        state: "connected",
        latency: this.latencyStats(),
        last_ok_at: iso(this.now()),
        error: null,
      });
      this.clearStale();
      if (boardKey(board) !== this.lastBoardKey) {
        const health = this.health ?? (await this.getHealth(false));
        await this.applyGeneration(board, health, seq);
      }
      if (!this.streamOpen) this.schedulePoll(this.pollIntervalMs);
    } catch (error) {
      if (this.disposed) return;
      const mapped = asSpiritError(error);
      this.publishError(mapped);
      this.pollFailures += 1;
      if (this.pollFailures >= 3) {
        this.markStale(mapped);
        this.schedulePoll(this.staleBackoffMs);
      } else {
        this.updateConnection({
          state: this.lastSnapshot ? "reconnecting" : "failed",
          error: mapped,
        });
        this.schedulePoll(this.pollIntervalMs);
      }
    }
  }

  private markStale(error: SpiritErrorDto): void {
    this.nextRetryAt = this.now() + this.staleBackoffMs;
    if (this.lastSnapshot) {
      this.lastSnapshot = { ...this.lastSnapshot, stale: true };
      this.publishSnapshot(this.lastSnapshot);
    }
    this.updateConnection({
      state: "stale",
      error,
      last_ok_at: this.connectionState.last_ok_at,
    });
  }

  private clearStale(): void {
    this.nextRetryAt = null;
    if (this.lastSnapshot?.stale) {
      this.lastSnapshot = { ...this.lastSnapshot, stale: false };
      this.publishSnapshot(this.lastSnapshot);
    }
    if (this.connectionState.state === "stale") {
      this.updateConnection({ state: "connected", error: null });
    }
  }

  private handleHeartbeat(data: string): void {
    try {
      const value: unknown = JSON.parse(data);
      if (!isRecord(value)) return;
      const at = optionalTimestamp(value.at, "heartbeat.at");
      this.lastHeartbeatAt = at ? Date.parse(at) : this.now();
    } catch {
      // A malformed heartbeat is liveness only; it never changes data.
      this.lastHeartbeatAt = this.now();
    }
    this.lastFrameAt = this.now();
    this.reconnectDelayMs = this.reconnectBaseMs;
  }

  private handleBoardFrame(data: string): void {
    let raw: unknown;
    try {
      raw = JSON.parse(data) as unknown;
    } catch {
      this.publishError(parseError());
      return;
    }
    let board: BoardDoc;
    try {
      board = parseBoardDoc(raw);
    } catch (error) {
      this.publishError(asSpiritError(error));
      return;
    }
    this.lastFrameAt = this.now();
    this.reconnectDelayMs = this.reconnectBaseMs;
    this.pollFailures = 0;
    this.nextRetryAt = null;
    const key = boardKey(board);
    if (key === this.lastBoardKey) return;
    this.pendingBoard = { board, seq: ++this.requestSeq };
    void this.applyPendingBoard();
  }

  private async applyPendingBoard(): Promise<void> {
    if (this.disposed) return;
    if (this.boardApplyInFlight) {
      this.boardApplyDirty = true;
      return;
    }
    const pending = this.pendingBoard;
    if (!pending) return;
    this.boardApplyInFlight = true;
    try {
      const health = this.health ?? (await this.getHealth(false));
      const rows = await this.fetchSecondary();
      if (this.disposed) return;
      if (pending.seq < this.appliedSeq) {
        // A newer explicit refresh won. Keep a still-newer pending frame if one arrived
        // while this refetch was in flight.
        if (this.pendingBoard === pending) this.pendingBoard = null;
        return;
      }
      const nextRevision = this.revision + 1;
      const snapshot = this.buildSnapshot(
        pending.board,
        health,
        this.now(),
        rows,
        nextRevision,
      );
      this.appliedSeq = pending.seq;
      this.revision = nextRevision;
      this.lastBoardKey = boardKey(pending.board);
      this.lastSnapshot = snapshot;
      this.updateConnection({
        state: "connected",
        latency: this.latencyStats(),
        last_ok_at: iso(this.now()),
        error: null,
      });
      this.clearStale();
      this.publishSnapshot(this.lastSnapshot);
    } catch (error) {
      if (!this.disposed) this.publishError(asSpiritError(error));
    } finally {
      this.boardApplyInFlight = false;
      if (this.boardApplyDirty) {
        this.boardApplyDirty = false;
        void this.applyPendingBoard();
      } else {
        this.pendingBoard = null;
      }
    }
  }

  private webStatus(): WebBridgeStatus {
    return {
      polling_only: this.pollingOnly,
      next_retry_at: this.nextRetryAt,
      last_frame_at: this.lastFrameAt === null ? null : iso(this.lastFrameAt),
      heartbeat_at: this.lastHeartbeatAt === null ? null : iso(this.lastHeartbeatAt),
      stream_state: this.disposed ? "closed" : this.streamState,
    };
  }
}


