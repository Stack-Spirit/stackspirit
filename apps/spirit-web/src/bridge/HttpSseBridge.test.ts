import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { HttpSseBridge, READ_ROUTES } from "../bridge/HttpSseBridge";
import { resolveBaseUrl, parseBaseUrl, isSameOrigin } from "../bridge/baseUrl";
import {
  BOARD,
  FakeEventSource,
  FakeFetch,
  HEALTH,
  INBOX,
  INBOX_ANSWERED,
  INBOX_DISMISSED,
  JOBS,
  SESSIONS,
  deferred,
  flush,
  jsonResponse,
  redirectResponse,
  resetFakeEventSources,
  textResponse,
  NOW,
  type RecordedCall,
} from "../test/fakes";

function memoryStorage(): Storage {
  const map = new Map<string, string>();
  return {
    get length() {
      return map.size;
    },
    clear: () => map.clear(),
    getItem: (key: string) => map.get(key) ?? null,
    key: (index: number) => [...map.keys()][index] ?? null,
    removeItem: (key: string) => {
      map.delete(key);
    },
    setItem: (key: string, value: string) => {
      map.set(key, value);
    },
  };
}

function makeBridge(overrides: {
  now?: () => number;
  fetch?: FakeFetch;
  baseUrl?: string;
  pageOrigin?: string;
} = {}) {
  const clock = { value: NOW };
  const fetch = overrides.fetch ?? new FakeFetch();
  if (!overrides.fetch) {
    fetch
      .onPath("/api/v1/health", () => jsonResponse(HEALTH))
      .onPath("/api/v1/board", () => jsonResponse(BOARD))
      .onPath("/api/v1/jobs", () => jsonResponse(JOBS))
      .onPath("/api/v1/sessions", () => jsonResponse(SESSIONS))
      .onQuery("/api/v1/inbox", "open", () => jsonResponse(INBOX))
      .onQuery("/api/v1/inbox", "answered", () => jsonResponse(INBOX_ANSWERED))
      .onQuery("/api/v1/inbox", "dismissed", () => jsonResponse(INBOX_DISMISSED));
  }
  const bridge = new HttpSseBridge({
    baseUrl: overrides.baseUrl ?? "http://web.test",
    pageOrigin: overrides.pageOrigin ?? "http://web.test",
    fetchImpl: fetch.fetch,
    eventSourceFactory: (url) => new FakeEventSource(url),
    now: overrides.now ?? (() => clock.value),
  });
  return { bridge, fetch, clock };
}

async function advance(clock: { value: number }, ms: number): Promise<void> {
  clock.value += ms;
  await vi.advanceTimersByTimeAsync(ms);
  await flush();
}

beforeEach(() => {
  vi.useFakeTimers();
  resetFakeEventSources();
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("base URL resolution", () => {
  it("prefers the daemon query parameter, strips the trailing slash, stores it, and removes it", () => {
    const storage = memoryStorage();
    const replaced: string[] = [];
    const result = resolveBaseUrl({
      search: "?daemon=http%3A%2F%2Fweb.test%3A8080%2F&view=fleet",
      envUrl: "http://env.test",
      storage,
      history: { replaceState: (_data, _title, url) => replaced.push(String(url)) },
      location: { pathname: "/app", hash: "", origin: "http://web.test" },
    });
    expect(result).toEqual({
      baseUrl: "http://web.test:8080",
      source: "query",
      error: null,
    });
    expect(storage.getItem("spirit.daemon.base")).toBe("http://web.test:8080");
    expect(replaced).toEqual(["/app?view=fleet"]);
  });

  it("uses the saved address before the build environment", () => {
    const storage = memoryStorage();
    storage.setItem("spirit.daemon.base", "http://web.test:9000/");
    const result = resolveBaseUrl({ search: "", envUrl: "http://env.test", storage });
    expect(result.baseUrl).toBe("http://web.test:9000");
    expect(result.source).toBe("storage");
  });

  it("uses the environment when there is no query or saved address", () => {
    const result = resolveBaseUrl({ search: "", envUrl: "https://vowp.example/vowp/" });
    expect(result.baseUrl).toBe("https://vowp.example/vowp");
    expect(result.source).toBe("env");
  });

  it("reports a missing base instead of guessing a port", () => {
    expect(resolveBaseUrl({ search: "" })).toEqual({
      baseUrl: null,
      source: "none",
      error: null,
    });
  });

  it.each([
    ["userinfo", "http://user:pass@web.test:8080"],
    ["query", "http://web.test:8080/?x=1"],
    ["fragment", "http://web.test:8080/#x"],
    ["bad scheme", "ftp://web.test:8080"],
    ["relative", "/api"],
  ])("rejects a base URL with %s", (_name, value) => {
    const result = parseBaseUrl(value);
    expect(result.baseUrl).toBeNull();
    expect(result.error?.code).toBe("invalid_input");
    expect(result.error?.message).not.toContain(value);
  });

  it("keeps a path prefix for a reverse proxy but drops the trailing slash", () => {
    expect(parseBaseUrl("https://example.test/vowp/").baseUrl).toBe(
      "https://example.test/vowp",
    );
  });

  it("compares origins only, so a path prefix stays same-origin", () => {
    expect(isSameOrigin("http://web.test/vowp", "http://web.test")).toBe(true);
    expect(isSameOrigin("http://127.0.0.1:8080", "http://web.test")).toBe(false);
  });
});

describe("startup and snapshot", () => {
  it("reads health, board, jobs, sessions, and open inbox, then opens the board stream", async () => {
    const { bridge, fetch } = makeBridge();
    const snapshot = await bridge.fleet.snapshot(false);
    expect(snapshot).not.toBeNull();
    expect(snapshot?.revision).toBe(1);
    expect(snapshot?.host_id).toBe("daemon");
    expect(snapshot?.home).toBe("vps");
    expect(snapshot?.project).toEqual({
      name: "vowp",
      state: "unknown",
      paused_reason: "",
      jobs_by_state: BOARD.jobs,
      cost_today_usd: 1.25,
    });
    expect(snapshot?.capacity_unavailable).toMatchObject({
      code: "no_route",
      route: "/api/v1/capacity",
    });
    expect(snapshot?.supervisor_unavailable.code).toBe("no_route");
    expect(snapshot?.source).toBe("daemon");
    expect(snapshot?.stale).toBe(false);
    expect(snapshot?.daemon).toMatchObject({
      origin: "http://web.test",
      version: "0.3.0-test",
      node: "vps",
      project: "vowp",
      schema_version: 7,
    });

    const calls = fetch.calls.map((call) => `${call.method} ${call.url.pathname}`);
    expect(calls).toContain("GET /api/v1/health");
    expect(calls).toContain("GET /api/v1/board");
    expect(calls).toContain("GET /api/v1/jobs");
    expect(calls).toContain("GET /api/v1/sessions");
    expect(calls).toContain("GET /api/v1/inbox");
    expect(fetch.calls.every((call) => call.method === "GET")).toBe(true);
    expect(FakeEventSource.instances).toHaveLength(1);
    expect(FakeEventSource.instances[0]?.url).toBe(
      "http://web.test/api/v1/board/stream",
    );
  });

  it("maps board runs, joins the newest session, and never invents a missing job", async () => {
    const { bridge } = makeBridge();
    const snapshot = await bridge.fleet.snapshot(false);
    if (!snapshot) throw new Error("snapshot missing");
    expect(snapshot.running).toHaveLength(3);
    expect(snapshot.running[0]).toMatchObject({
      job_id: "ss-1-run",
      project: "vowp",
      kind: "build",
      state: "active",
      stalled: false,
      node: "vps",
      session_id: "sess-new",
    });
    expect(snapshot.running[1]).toMatchObject({
      job_id: "ss-2-stalled",
      state: "stalled",
      stalled: true,
      executor: null,
      session_id: "sess-stalled",
    });
    expect(snapshot.running[2]).toMatchObject({
      job_id: "ss-orphan",
      project: "unknown",
      kind: "unknown",
      node: null,
      session_id: null,
    });

    expect(snapshot.lanes.counts).toMatchObject({
      running: 3,
      ready: 2,
      review_fix: 1,
      parked_blocked: 2,
      done: 4,
    });
    expect(snapshot.lanes.review_fix).toEqual([]);
    expect(snapshot.lanes.queue_cap).toBeNull();
    expect(snapshot.lanes.queue_shown).toBe(3);
    expect(snapshot.lanes.ready.map((card) => card.job_id)).toEqual(["ss-3-ready"]);
    expect(snapshot.lanes.parked_blocked.map((card) => card.job_id)).toEqual([
      "ss-5-parked",
    ]);
    expect(snapshot.lanes.parked_blocked[0]?.reason).toBe("question");
    expect(snapshot.sessions).toEqual([]);
    expect(snapshot.orchestrators).toEqual([]);
    expect(snapshot.unavailable.map((notice) => notice.feature)).toContain(
      "capacity_report",
    );
    expect(snapshot.unavailable.map((notice) => notice.feature)).toContain(
      "gate_failures",
    );
  });

  it("labels a paginated job page as truncated while keeping queue_cap null", async () => {
    const fetch = new FakeFetch();
    fetch
      .onPath("/api/v1/health", () => jsonResponse(HEALTH))
      .onPath("/api/v1/board", () => jsonResponse(BOARD))
      .onPath("/api/v1/jobs", () => jsonResponse({ ...JOBS, next: 6 }))
      .onPath("/api/v1/sessions", () => jsonResponse(SESSIONS))
      .onQuery("/api/v1/inbox", "open", () => jsonResponse(INBOX));
    const { bridge } = makeBridge({ fetch });
    const snapshot = await bridge.fleet.snapshot(false);
    expect(snapshot?.lanes.queue_truncated).toBe(true);
    expect(snapshot?.lanes.queue_cap).toBeNull();
  });

  it("reports a project mismatch as daemon_incompatible instead of mixing views", async () => {
    const fetch = new FakeFetch();
    fetch
      .onPath("/api/v1/health", () => jsonResponse({ ...HEALTH, project: "other" }))
      .onPath("/api/v1/board", () => jsonResponse(BOARD))
      .onPath("/api/v1/jobs", () => jsonResponse(JOBS))
      .onPath("/api/v1/sessions", () => jsonResponse(SESSIONS))
      .onQuery("/api/v1/inbox", "open", () => jsonResponse(INBOX));
    const { bridge } = makeBridge({ fetch });
    await expect(bridge.fleet.snapshot(false)).rejects.toMatchObject({
      code: "daemon_incompatible",
    });
  });

  it("returns the retained snapshot when a newer frame already won the race", async () => {
    const { bridge } = makeBridge();
    const first = await bridge.fleet.snapshot(false);
    if (!first) throw new Error("snapshot missing");
    const source = FakeEventSource.instances[0];
    if (!source) throw new Error("source missing");
    source.open();
    source.emit("board", { ...BOARD, jobs: { ...BOARD.jobs, ready: 3 } });
    await flush();
    const afterFrame = await bridge.fleet.snapshot(false);
    expect(afterFrame?.revision).toBe(2);
    expect(afterFrame?.project.jobs_by_state.ready).toBe(3);
  });
});

describe("board stream", () => {
  it("applies one revision per changed board frame, ignores heartbeats, and ignores unknown events", async () => {
    const { bridge } = makeBridge();
    await bridge.fleet.snapshot(false);
    const source = FakeEventSource.instances[0];
    if (!source) throw new Error("source missing");
    source.open();
    const firstRevision = bridge.fleet.webStatus();
    expect(firstRevision.stream_state).toBe("open");

    source.emit("heartbeat", { at: "2026-11-15T12:00:15.000Z" });
    await flush();
    expect((await bridge.fleet.snapshot(false))?.revision).toBe(1);

    source.emit("board", { ...BOARD, jobs: { ...BOARD.jobs, done: 5 } });
    await flush();
    const changed = await bridge.fleet.snapshot(false);
    expect(changed?.revision).toBe(2);
    expect(changed?.project.jobs_by_state.done).toBe(5);

    source.emit("unknown", { anything: true });
    await flush();
    expect((await bridge.fleet.snapshot(false))?.revision).toBe(2);
  });

  it("keeps the previous view and emits a parse error for bad board JSON", async () => {
    const { bridge, fetch } = makeBridge();
    await bridge.fleet.snapshot(false);
    const source = FakeEventSource.instances[0];
    if (!source) throw new Error("source missing");
    source.open();
    const events: string[] = [];
    bridge.connection.subscribe((event) => {
      if (event.kind === "error") events.push(event.error.code);
    });
    const before = fetch.boardCalls();
    source.emit("board", "{not json");
    await flush();
    expect(events).toContain("parse");
    expect(fetch.boardCalls()).toBe(before);
    expect((await bridge.fleet.snapshot(false))?.revision).toBe(1);
  });

  it("starts the 5 s poll on a stream error and reconnects with 1, 2, 4 s backoff", async () => {
    const { bridge, clock, fetch } = makeBridge();
    await bridge.fleet.snapshot(false);
    const source = FakeEventSource.instances[0];
    if (!source) throw new Error("source missing");
    source.open();
    source.fail();
    expect(bridge.fleet.webStatus().stream_state).toBe("reconnecting");
    const callsBefore = fetch.boardCalls();
    await advance(clock, 5_000);
    expect(fetch.boardCalls()).toBeGreaterThan(callsBefore);

    await advance(clock, 1_000);
    expect(FakeEventSource.instances).toHaveLength(2);
    FakeEventSource.instances[1]?.fail();
    await advance(clock, 2_000);
    expect(FakeEventSource.instances).toHaveLength(3);
    FakeEventSource.instances[2]?.fail();
    await advance(clock, 4_000);
    expect(FakeEventSource.instances).toHaveLength(4);
    FakeEventSource.instances[3]?.open();
    expect(bridge.fleet.webStatus().stream_state).toBe("open");
  });

  it("polls without an EventSource and never faster than 5 s", async () => {
    vi.stubGlobal("EventSource", undefined);
    const clock = { value: NOW };
    const fetch = new FakeFetch()
      .onPath("/api/v1/health", () => jsonResponse(HEALTH))
      .onPath("/api/v1/board", () => jsonResponse(BOARD))
      .onPath("/api/v1/jobs", () => jsonResponse(JOBS))
      .onPath("/api/v1/sessions", () => jsonResponse(SESSIONS))
      .onQuery("/api/v1/inbox", "open", () => jsonResponse(INBOX));
    const bridge = new HttpSseBridge({
      baseUrl: "http://web.test",
      pageOrigin: "http://web.test",
      fetchImpl: fetch.fetch,
      now: () => clock.value,
    });
    await bridge.fleet.snapshot(false);
    expect(bridge.fleet.webStatus().polling_only).toBe(true);
    expect(bridge.fleet.webStatus().stream_state).toBe("polling");
    const before = fetch.boardCalls();
    await advance(clock, 4_000);
    expect(fetch.boardCalls()).toBe(before);
    await advance(clock, 1_000);
    expect(fetch.boardCalls()).toBe(before + 1);
    expect((await bridge.connection.status()).state).toBe("connected");
    bridge.dispose();
  });

  it("treats 45 s without a frame as a stream failure", async () => {
    const { bridge, clock } = makeBridge();
    await bridge.fleet.snapshot(false);
    const source = FakeEventSource.instances[0];
    if (!source) throw new Error("source missing");
    source.open();
    await advance(clock, 61_000);
    expect(bridge.fleet.webStatus().stream_state).toBe("reconnecting");
    expect(source.closed).toBe(true);
  });

  it("falls back to stale after three poll failures and recovers on a later poll", async () => {
    const { bridge, clock, fetch } = makeBridge();
    await bridge.fleet.snapshot(false);
    const source = FakeEventSource.instances[0];
    if (!source) throw new Error("source missing");
    source.fail();
    fetch.removePath("/api/v1/board");
    fetch.onPath("/api/v1/board", () => {
      throw new TypeError("Failed to fetch");
    });
    await advance(clock, 5_000);
    await advance(clock, 5_000);
    await advance(clock, 5_000);
    const stale = await bridge.fleet.snapshot(false);
    expect(stale?.stale).toBe(true);
    expect((await bridge.connection.status()).state).toBe("stale");
    expect(bridge.fleet.webStatus().next_retry_at).not.toBeNull();
    expect(bridge.fleet.webStatus().stream_state).toBe("reconnecting");

    fetch.removePath("/api/v1/board");
    fetch.onPath("/api/v1/board", () => jsonResponse(BOARD));
    await advance(clock, 30_000);
    const recovered = await bridge.fleet.snapshot(false);
    expect(recovered?.stale).toBe(false);
    expect((await bridge.connection.status()).state).toBe("connected");
  });

  it("does not refetch jobs, sessions, or inbox for an unchanged board poll", async () => {
    const { bridge, clock, fetch } = makeBridge();
    await bridge.fleet.snapshot(false);
    const source = FakeEventSource.instances[0];
    if (!source) throw new Error("source missing");
    source.fail();
    const jobsBefore = fetch.jobsCalls();
    await advance(clock, 5_000);
    expect(fetch.jobsCalls()).toBe(jobsBefore);
  });

  it("refetches the secondary rows once for a changed board poll", async () => {
    const { bridge, clock, fetch } = makeBridge();
    await bridge.fleet.snapshot(false);
    const source = FakeEventSource.instances[0];
    if (!source) throw new Error("source missing");
    source.fail();
    const jobsBefore = fetch.jobsCalls();
    fetch.removePath("/api/v1/board");
    fetch.onPath("/api/v1/board", () =>
      jsonResponse({ ...BOARD, jobs: { ...BOARD.jobs, ready: 3 } }),
    );
    await advance(clock, 5_000);
    expect(fetch.jobsCalls()).toBe(jobsBefore + 1);
    expect((await bridge.fleet.snapshot(false))?.project.jobs_by_state.ready).toBe(3);
  });

  it("drops an older explicit refresh after a newer SSE frame applied", async () => {
    const { bridge, fetch } = makeBridge();
    await bridge.fleet.snapshot(false);
    const source = FakeEventSource.instances[0];
    if (!source) throw new Error("source missing");
    source.open();

    const gate = deferred<void>();
    let boardCalls = 0;
    fetch.removePath("/api/v1/board");
    fetch.onPath("/api/v1/board", async () => {
      boardCalls += 1;
      if (boardCalls === 1) {
        await gate.promise;
        return jsonResponse({ ...BOARD, jobs: { ...BOARD.jobs, ready: 9 } });
      }
      return jsonResponse(BOARD);
    });

    const pendingRefresh = bridge.fleet.snapshot(true);
    await flush();
    source.emit("board", { ...BOARD, jobs: { ...BOARD.jobs, ready: 5 } });
    await flush();
    expect((await bridge.fleet.snapshot(false))?.project.jobs_by_state.ready).toBe(5);
    expect((await bridge.fleet.snapshot(false))?.revision).toBe(2);

    gate.resolve();
    await pendingRefresh;
    await flush();
    const retained = await bridge.fleet.snapshot(false);
    expect(retained?.revision).toBe(2);
    expect(retained?.project.jobs_by_state.ready).toBe(5);
  });

  it("coalesces a second board change during a refetch into one more fetch", async () => {
    const { bridge, fetch } = makeBridge();
    await bridge.fleet.snapshot(false);
    const source = FakeEventSource.instances[0];
    if (!source) throw new Error("source missing");
    source.open();

    const gate = deferred<void>();
    let jobCalls = 0;
    fetch.removePath("/api/v1/jobs");
    fetch.onPath("/api/v1/jobs", async () => {
      jobCalls += 1;
      if (jobCalls === 1) await gate.promise;
      return jsonResponse(JOBS);
    });

    source.emit("board", { ...BOARD, jobs: { ...BOARD.jobs, ready: 3 } });
    await flush();
    source.emit("board", { ...BOARD, jobs: { ...BOARD.jobs, ready: 4 } });
    await flush();
    expect(jobCalls).toBe(1);

    gate.resolve();
    await flush();
    await flush();
    expect(jobCalls).toBe(2);
    const snapshot = await bridge.fleet.snapshot(false);
    expect(snapshot?.revision).toBe(3);
    expect(snapshot?.project.jobs_by_state.ready).toBe(4);
  });
});

describe("inbox", () => {
  it("fetches only the open state when all is false", async () => {
    const { bridge, fetch } = makeBridge();
    const items = await bridge.fleet.inbox(false);
    expect(items).toHaveLength(1);
    expect(fetch.calls.some((call) => call.url.searchParams.get("state") === "all")).toBe(
      false,
    );
    expect(fetch.calls.filter((call) => call.url.pathname === "/api/v1/inbox")).toHaveLength(
      1,
    );
  });

  it("merges the three states, sorts by created_at then id, and never asks for state=all", async () => {
    const fetch = new FakeFetch();
    fetch
      .onPath("/api/v1/health", () => jsonResponse(HEALTH))
      .onPath("/api/v1/board", () => jsonResponse(BOARD))
      .onPath("/api/v1/jobs", () => jsonResponse(JOBS))
      .onPath("/api/v1/sessions", () => jsonResponse(SESSIONS))
      .onQuery("/api/v1/inbox", "open", () =>
        jsonResponse({
          items: [
            { ...INBOX.items[0], id: "b", created_at: "2026-11-15T11:58:00.000Z" },
            { ...INBOX.items[0], id: "a", created_at: "2026-11-15T11:58:00.000Z" },
          ],
          next: null,
          total: 2,
        }),
      )
      .onQuery("/api/v1/inbox", "answered", () =>
        jsonResponse({
          items: [{ ...INBOX_ANSWERED.items[0], id: "c", created_at: "2026-11-15T11:56:00.000Z" }],
          next: null,
          total: 1,
        }),
      )
      .onQuery("/api/v1/inbox", "dismissed", () => jsonResponse(INBOX_DISMISSED));
    const { bridge } = makeBridge({ fetch });
    const items = await bridge.fleet.inbox(true);
    expect(items.map((item) => item.id)).toEqual(["c", "a", "b"]);
    const states = fetch.calls
      .filter((call) => call.url.pathname === "/api/v1/inbox")
      .map((call) => call.url.searchParams.get("state"))
      .sort();
    expect(states).toEqual(["answered", "dismissed", "open"]);
  });
});

describe("desktop-only and read-only methods", () => {
  it("rejects every desktop-only method without making a network request", async () => {
    const { bridge, fetch } = makeBridge();
    const desktopCalls: Array<() => Promise<unknown>> = [
      () => bridge.hosts.list(),
      () => bridge.hosts.add({} as never),
      () => bridge.hosts.update("host", {}),
      () => bridge.hosts.remove("host"),
      () => bridge.hosts.scanFingerprint({} as never),
      () => bridge.hosts.trustFingerprint("host", {} as never),
      () => bridge.hosts.probe({} as never),
      () => bridge.connection.connect("host"),
      () => bridge.connection.disconnect("host"),
      () => bridge.engine.status(),
      () => bridge.engine.models(),
      () => bridge.engine.selectModel("model"),
      () => bridge.engine.storeKey("provider", "fixture-key"),
      () => bridge.engine.verifyKey("provider"),
      () => bridge.engine.deleteKey("provider"),
      () => bridge.engine.keyStatus(),
      () => bridge.engine.scanExternal(),
      () => bridge.engine.selectExternal("opencode", "/bin/opencode"),
      () => bridge.engine.useBundled(),
      () => bridge.engine.restart(),
      () => bridge.gates.get(),
      () => bridge.gates.probe(),
    ];
    for (const method of desktopCalls) {
      await expect(method()).rejects.toMatchObject({ code: "desktop_only" });
    }
    expect(fetch.calls).toHaveLength(0);
  });

  it("returns no capacity, no raw document, and no desktop FleetView", async () => {
    const { bridge, fetch } = makeBridge();
    await expect(bridge.fleet.capacity()).resolves.toBeNull();
    await expect(bridge.fleet.rawDocument()).resolves.toBeNull();
    await expect(bridge.fleet.get()).resolves.toBeNull();
    await expect(bridge.fleet.refresh()).rejects.toMatchObject({ code: "no_route" });
    expect(fetch.calls).toHaveLength(0);
  });

  it("rejects setTarget and both writes before any fetch", async () => {
    const { bridge, fetch } = makeBridge();
    await expect(
      bridge.fleet.setTarget({ target: 4, mode: "fixed", override: false, reason: "test" }),
    ).rejects.toMatchObject({ code: "no_route" });
    await expect(bridge.fleet.jobAction("ss-1-run", "retry")).rejects.toMatchObject({
      code: "writes_disabled",
    });
    await expect(bridge.fleet.answerInbox("inbox-open", "yes")).rejects.toMatchObject({
      code: "writes_disabled",
    });
    expect(fetch.calls).toHaveLength(0);
  });
});

describe("typed errors", () => {
  async function expectHealthError(status: number, body: unknown, code: string) {
    const fetch = new FakeFetch();
    fetch.onPath("/api/v1/health", () => jsonResponse(body, status));
    const { bridge } = makeBridge({ fetch });
    await expect(bridge.fleet.snapshot(false)).rejects.toMatchObject({ code });
  }

  it("maps 400, 401, 404, 409, 413, 415, 500, and 503", async () => {
    await expectHealthError(400, { error: { kind: "invalid_input", message: "bad" } }, "invalid_input");
    await expectHealthError(401, { error: { kind: "auth", message: "no" } }, "auth");
    await expectHealthError(
      404,
      { error: { kind: "not_found", message: "unknown endpoint", hint: null } },
      "daemon_incompatible",
    );
    await expectHealthError(409, { error: { kind: "conflict", message: "changed" } }, "conflict");
    await expectHealthError(413, { error: { kind: "invalid_input", message: "large" } }, "invalid_input");
    await expectHealthError(415, { error: { kind: "invalid_input", message: "type" } }, "invalid_input");
    await expectHealthError(503, { error: { kind: "config", message: "disabled" } }, "writes_disabled");
    await expectHealthError(500, { error: { kind: "internal", message: "boom" } }, "remote");
  });

  it("retries a 500 once and then succeeds", async () => {
    const fetch = new FakeFetch();
    let healthCalls = 0;
    fetch
      .onPath("/api/v1/health", () => {
        healthCalls += 1;
        if (healthCalls === 1) {
          return jsonResponse({ error: { kind: "internal", message: "boom" } }, 500);
        }
        return jsonResponse(HEALTH);
      })
      .onPath("/api/v1/board", () => jsonResponse(BOARD))
      .onPath("/api/v1/jobs", () => jsonResponse(JOBS))
      .onPath("/api/v1/sessions", () => jsonResponse(SESSIONS))
      .onQuery("/api/v1/inbox", "open", () => jsonResponse(INBOX));
    const { bridge } = makeBridge({ fetch });
    await expect(bridge.fleet.snapshot(false)).resolves.not.toBeNull();
    expect(healthCalls).toBe(2);
  });

  it("maps a non-JSON success to parse and a redirect to daemon_incompatible", async () => {
    const invalid = new FakeFetch();
    invalid
      .onPath("/api/v1/health", () => textResponse("not json"))
      .onPath("/api/v1/board", () => jsonResponse(BOARD));
    const { bridge: parseBridge } = makeBridge({ fetch: invalid });
    await expect(parseBridge.fleet.snapshot(false)).rejects.toMatchObject({ code: "parse" });

    const redirected = new FakeFetch();
    redirected.onPath("/api/v1/health", () => redirectResponse());
    const { bridge: redirectBridge } = makeBridge({ fetch: redirected });
    await expect(redirectBridge.fleet.snapshot(false)).rejects.toMatchObject({
      code: "daemon_incompatible",
    });
  });

  it("maps a network failure to daemon_unreachable", async () => {
    const fetch = new FakeFetch();
    fetch.onPath("/api/v1/health", () => {
      throw new TypeError("Failed to fetch");
    });
    const { bridge } = makeBridge({ fetch });
    await expect(bridge.fleet.snapshot(false)).rejects.toMatchObject({
      code: "daemon_unreachable",
    });
  });

  it("maps a read timeout to remote_timeout", async () => {
    const fetch = new FakeFetch();
    fetch.onPath("/api/v1/health", (call: RecordedCall) => {
      return new Promise<Response>((_resolve, reject) => {
        call.init.signal?.addEventListener("abort", () => {
          reject(new DOMException("aborted", "AbortError"));
        });
      });
    });
    const { bridge } = makeBridge({ fetch });
    const pending = bridge.fleet.snapshot(false);
    const assertion = expect(pending).rejects.toMatchObject({ code: "remote_timeout" });
    await vi.advanceTimersByTimeAsync(10_000);
    await assertion;
  });
});

describe("security and cleanup", () => {
  it("never sends Authorization, POST, or a body, and always uses omit/manual", async () => {
    const { bridge, fetch } = makeBridge();
    await bridge.fleet.snapshot(false);
    await bridge.fleet.inbox(true);
    expect(fetch.calls.length).toBeGreaterThan(0);
    for (const call of fetch.calls) {
      expect(call.method).toBe("GET");
      expect(call.headers.get("authorization")).toBeNull();
      expect(call.init.body).toBeUndefined();
      expect(call.init.credentials).toBe("omit");
      expect(call.init.redirect).toBe("manual");
      expect(call.init.cache).toBe("no-store");
    }
  });

  it("only calls the documented read routes", async () => {
    const { bridge, fetch } = makeBridge();
    await bridge.fleet.snapshot(false);
    const allowed = new Set<string>(Object.values(READ_ROUTES));
    for (const call of fetch.calls) {
      expect(allowed.has(call.url.pathname)).toBe(true);
    }
  });

  it("closes the stream, clears timers, and aborts in-flight reads on dispose", async () => {
    const { bridge, clock, fetch } = makeBridge();
    await bridge.fleet.snapshot(false);
    const source = FakeEventSource.instances[0];
    if (!source) throw new Error("source missing");
    source.open();
    bridge.dispose();
    expect(source.closed).toBe(true);
    const before = fetch.calls.length;
    await advance(clock, 60_000);
    expect(fetch.calls.length).toBe(before);
    expect(bridge.fleet.webStatus().stream_state).toBe("closed");
  });

  it("returns daemon health from the optional health read", async () => {
    const { bridge } = makeBridge();
    const health = await bridge.daemon.health();
    expect(health).toMatchObject({
      origin: "http://web.test",
      version: "0.3.0-test",
      node: "vps",
      project: "vowp",
      schema_version: 7,
      heartbeat_at: "2026-11-15T11:59:30.000Z",
    });
  });
});
