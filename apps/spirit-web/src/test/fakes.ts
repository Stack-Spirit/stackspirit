import type { FetchLike, EventSourceLike, EventSourceMessage } from "../bridge/HttpSseBridge";

export const NOW = Date.parse("2026-11-15T12:00:10.000Z");

export const HEALTH = {
  version: "0.3.0-test",
  node: "vps",
  project: "vowp",
  schema_version: 7,
  heartbeat: "2026-11-15T11:59:30.000Z",
};

export const BOARD = {
  at: "2026-11-15T12:00:00.000Z",
  project: "vowp",
  jobs: {
    running: 2,
    stalled: 1,
    ready: 2,
    reviewing: 1,
    parked: 1,
    done: 4,
    failed: 1,
  },
  running: [
    {
      job: "ss-1-run",
      executor: "opencode:deepseek/deepseek-flash#max",
      age_secs: 120,
      last_event_age_secs: 3,
    },
    {
      job: "ss-2-stalled",
      executor: null,
      age_secs: 30,
      last_event_age_secs: null,
    },
    {
      job: "ss-orphan",
      executor: "claude-code:model-a@medium",
      age_secs: 10,
      last_event_age_secs: 1,
    },
  ],
  slots_busy: 3,
  slots_total: 8,
  planner: null,
  keeper: "deepseek/deepseek-flash",
  limits: [],
  cost_today_usd: 1.25,
  inbox_open: 2,
  last_checkpoint: "2026-11-15T11:59:00.000Z",
};

export const JOBS = {
  items: [
    {
      id: "ss-1-run",
      project: "vowp",
      title: "Build the web shell",
      kind: "build",
      role: "worker",
      state: "running",
      priority: 80,
      parked_reason: null,
      state_changed_at: "2026-11-15T11:58:00.000Z",
      updated_at: "2026-11-15T12:00:00.000Z",
    },
    {
      id: "ss-2-stalled",
      project: "vowp",
      title: "Fix the stalled worker",
      kind: "fix",
      role: "verifier",
      state: "stalled",
      priority: 50,
      parked_reason: null,
      state_changed_at: "2026-11-15T11:57:00.000Z",
      updated_at: "2026-11-15T11:59:00.000Z",
    },
    {
      id: "ss-3-ready",
      project: "vowp",
      title: "Write the bridge tests",
      kind: "test",
      role: "worker",
      state: "ready",
      priority: 60,
      parked_reason: null,
      state_changed_at: "2026-11-15T11:56:00.000Z",
      updated_at: "2026-11-15T11:56:00.000Z",
    },
    {
      id: "ss-4-review",
      project: "vowp",
      title: "Review the contract",
      kind: "verify",
      role: "verifier",
      state: "reviewing",
      priority: 40,
      parked_reason: null,
      state_changed_at: "2026-11-15T11:55:00.000Z",
      updated_at: "2026-11-15T11:55:00.000Z",
    },
    {
      id: "ss-5-parked",
      project: "vowp",
      title: "Wait for an owner answer",
      kind: "decision",
      role: "worker",
      state: "parked",
      priority: 10,
      parked_reason: "question",
      state_changed_at: "2026-11-15T11:54:00.000Z",
      updated_at: "2026-11-15T11:54:00.000Z",
    },
    {
      id: "ss-6-done",
      project: "vowp",
      title: "Read the board",
      kind: "research",
      role: "worker",
      state: "done",
      priority: 0,
      parked_reason: null,
      state_changed_at: "2026-11-15T11:00:00.000Z",
      updated_at: "2026-11-15T11:00:00.000Z",
    },
  ],
  next: null,
  total: 6,
};

export const SESSIONS = {
  items: [
    {
      id: "sess-old",
      job_id: "ss-1-run",
      node: "old-node",
      state: "idle",
      executor: "opencode:deepseek/deepseek-flash#max",
      updated_at: "2026-11-15T11:50:00.000Z",
    },
    {
      id: "sess-new",
      job_id: "ss-1-run",
      node: "vps",
      state: "running",
      executor: "opencode:deepseek/deepseek-flash#max",
      updated_at: "2026-11-15T12:00:00.000Z",
    },
    {
      id: "sess-stalled",
      job_id: "ss-2-stalled",
      node: "vps",
      state: "blocked",
      executor: null,
      updated_at: "2026-11-15T11:59:00.000Z",
    },
  ],
  next: null,
  total: 3,
};

/** `GET /api/v1/jobs/ss-4-review`: the reviewing job the review/fix lane opens. */
export const JOB_DETAIL = {
  id: "ss-4-review",
  project: "vowp",
  title: "Review the contract",
  kind: "verify",
  role: "verifier",
  profile: "verifier",
  state: "reviewing",
  priority: 40,
  needs: ["ss-3-ready"],
  owns: ["docs/**"],
  reads: ["docs/WEB.md"],
  brief: ".vowp/jobs/ss-4-review/brief.md",
  report: ".vowp/jobs/ss-4-review/report.md",
  gates: [],
  loop: { until: "gates", max_attempts: 2, on_exhausted: "escalate" },
  branch: "job/ss-4-review",
  // An absolute worktree path the drawer must never receive or render.
  worktree: "/home/owner/.vowp/worktrees/ss-4-review",
  attempts: 1,
  continues: 0,
  hang_restarts: 0,
  parked_reason: null,
  note: "Checked docs/WEB.md and /home/owner/private/notes.md.",
  created_by: "planner",
  last_progress_at: "2026-11-15T11:58:00.000Z",
  state_changed_at: "2026-11-15T11:55:00.000Z",
  created_at: "2026-11-15T10:00:00.000Z",
  updated_at: "2026-11-15T11:59:00.000Z",
};

/** `GET /api/v1/jobs/ss-4-review/runs`; deliberately newest first to exercise sorting. */
export const JOB_RUNS = {
  items: [
    {
      id: "run-new",
      job_id: "ss-4-review",
      session_id: null,
      role: "verifier",
      purpose: "verify",
      profile: "verifier",
      rung: 0,
      executor: {
        harness: "claude-code",
        model: "model-a",
        effort: "medium",
        variant: null,
      },
      provider: "claude",
      node: "vps",
      native_session_id: null,
      pid: 1235,
      window: null,
      cwd: "/home/owner/.vowp/worktrees/ss-4-review",
      started_at: "2026-11-15T11:57:00.000Z",
      ended_at: "2026-11-15T11:58:00.000Z",
      exit_code: 1,
      outcome: "error",
      limit_kind: null,
      cost: {},
      detail: "Report gate failed; see /home/owner/.vowp/jobs/ss-4-review/report.md.",
    },
    {
      id: "run-old",
      job_id: "ss-4-review",
      session_id: null,
      role: "worker",
      purpose: "start",
      profile: "verifier",
      rung: 0,
      executor: {
        harness: "opencode",
        model: "deepseek/deepseek-flash",
        effort: null,
        variant: "max",
      },
      provider: "deepseek",
      node: "vps",
      native_session_id: null,
      pid: 1234,
      window: null,
      cwd: null,
      started_at: "2026-11-15T11:40:00.000Z",
      ended_at: "2026-11-15T11:50:00.000Z",
      exit_code: 0,
      outcome: "ok",
      limit_kind: null,
      cost: {},
      detail: "Read docs/WEB.md.",
    },
  ],
  next: null,
  total: 2,
};

/** `GET /api/v1/jobs/ss-4-review/gates`; deliberately out of gate-index order. */
export const JOB_GATES = {
  items: [
    {
      id: 3,
      job_id: "ss-4-review",
      run_id: "run-new",
      attempt: 1,
      gate_index: 2,
      gate: { secret_scan: true },
      passed: false,
      evidence: {
        kind: "secret_scan",
        files_scanned: 4,
        hits: [{ path: "docs/example.md", line: 2, rule: "generic_token" }],
      },
      output_blob: null,
      duration_ms: 30,
      ts: "2026-11-15T11:58:00.000Z",
    },
    {
      id: 1,
      job_id: "ss-4-review",
      run_id: "run-new",
      attempt: 1,
      gate_index: 0,
      gate: { report: { fields: ["status", "branch", "commits", "tests"] } },
      passed: false,
      evidence: {
        kind: "report",
        front_matter: null,
        missing_fields: ["tests"],
        problems: ["front matter has no closing delimiter"],
      },
      output_blob: null,
      duration_ms: 12,
      ts: "2026-11-15T11:58:00.000Z",
    },
    {
      id: 2,
      job_id: "ss-4-review",
      run_id: "run-old",
      attempt: 1,
      gate_index: 1,
      gate: { owns_respected: true },
      passed: true,
      evidence: {
        kind: "owns_respected",
        changed: ["docs/WEB.md"],
        violations: [],
      },
      output_blob: null,
      duration_ms: 5,
      ts: "2026-11-15T11:58:00.000Z",
    },
  ],
  next: null,
  total: 3,
};

export const INBOX = {
  items: [
    {
      id: "inbox-open",
      project: "vowp",
      job_id: "ss-5-parked",
      kind: "question",
      title: "Choose a merge policy",
      body: "One at a time, or batch ready jobs?",
      options: ["One at a time", "Batch"],
      state: "open",
      answer: null,
      answered_by: null,
      created_at: "2026-11-15T11:58:00.000Z",
      answered_at: null,
    },
  ],
  next: null,
  total: 1,
};

export const INBOX_ANSWERED = {
  items: [
    {
      id: "inbox-answered",
      project: "vowp",
      job_id: null,
      kind: "notice",
      title: "Merged",
      body: "The merge is complete.",
      options: [],
      state: "answered",
      answer: "acknowledged",
      answered_by: "owner",
      created_at: "2026-11-15T11:57:00.000Z",
      answered_at: "2026-11-15T11:58:30.000Z",
    },
  ],
  next: null,
  total: 1,
};

export const INBOX_DISMISSED = {
  items: [],
  next: null,
  total: 0,
};

export function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

export function textResponse(body: string, status = 200): Response {
  return new Response(body, { status, headers: { "Content-Type": "text/plain" } });
}

export function redirectResponse(): Response {
  return {
    type: "opaqueredirect",
    status: 0,
    ok: false,
    text: async () => "",
  } as unknown as Response;
}

export interface RecordedCall {
  url: URL;
  init: RequestInit;
  method: string;
  headers: Headers;
}

type Handler = (call: RecordedCall) => Promise<Response> | Response;

export class FakeFetch {
  readonly calls: RecordedCall[] = [];
  private readonly handlers: Array<{ match: (url: URL) => boolean; respond: Handler }> = [];

  onPath(path: string, respond: Handler): this {
    this.handlers.push({ match: (url) => url.pathname === path, respond });
    return this;
  }

  onQuery(path: string, state: string, respond: Handler): this {
    this.handlers.push({
      match: (url) => url.pathname === path && url.searchParams.get("state") === state,
      respond,
    });
    return this;
  }

  removePath(path: string): void {
    for (let index = this.handlers.length - 1; index >= 0; index -= 1) {
      const handler = this.handlers[index];
      if (handler && handler.match(new URL(`http://web.test${path}`))) {
        this.handlers.splice(index, 1);
      }
    }
  }

  readonly fetch: FetchLike = async (input, init = {}) => {
    const url = new URL(
      typeof input === "string" ? input : input instanceof URL ? input.toString() : input.url,
    );
    const call: RecordedCall = {
      url,
      init,
      method: (init.method ?? "GET").toUpperCase(),
      headers: new Headers(init.headers),
    };
    this.calls.push(call);
    const handler = [...this.handlers].reverse().find((candidate) => candidate.match(url));
    if (!handler) {
      return jsonResponse(
        { error: { kind: "not_found", message: "unknown endpoint", hint: null } },
        404,
      );
    }
    return handler.respond(call);
  };

  boardCalls(): number {
    return this.calls.filter((call) => call.url.pathname === "/api/v1/board").length;
  }

  jobsCalls(): number {
    return this.calls.filter((call) => call.url.pathname === "/api/v1/jobs").length;
  }
}

export class FakeEventSource implements EventSourceLike {
  static instances: FakeEventSource[] = [];

  readonly url: string;
  readonly listeners = new Map<string, Set<(event: EventSourceMessage) => void>>();
  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  closed = false;

  constructor(url: string) {
    this.url = url;
    FakeEventSource.instances.push(this);
  }

  addEventListener(type: string, listener: (event: EventSourceMessage) => void): void {
    const set = this.listeners.get(type) ?? new Set();
    set.add(listener);
    this.listeners.set(type, set);
  }

  removeEventListener(type: string, listener: (event: EventSourceMessage) => void): void {
    this.listeners.get(type)?.delete(listener);
  }

  close(): void {
    this.closed = true;
  }

  open(): void {
    this.onopen?.();
  }

  fail(): void {
    this.onerror?.();
  }

  emit(type: string, data: unknown): void {
    const payload = typeof data === "string" ? data : JSON.stringify(data);
    for (const listener of this.listeners.get(type) ?? []) {
      listener({ data: payload });
    }
  }
}

export function resetFakeEventSources(): void {
  FakeEventSource.instances = [];
}

export async function flush(): Promise<void> {
  for (let index = 0; index < 100; index += 1) {
    await Promise.resolve();
  }
}

export interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason?: unknown) => void;
}

export function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}
