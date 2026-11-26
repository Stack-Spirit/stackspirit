import { afterEach, describe, expect, it, vi } from "vitest";

import type {
  CloudRelayConnector,
  RelayConnectionStatus,
  RelayHttpResponse,
} from "@stackspirit/ui";

import { createRelayBridge, createRelayFetch, RELAY_BRIDGE_ORIGIN } from "./relayBridge";
import {
  BOARD,
  HEALTH,
  INBOX,
  JOB_DETAIL,
  JOB_GATES,
  JOB_RUNS,
  JOBS,
  NOW,
  SESSIONS,
} from "../test/fakes";

class FakeRelay implements CloudRelayConnector {
  readonly calls: string[] = [];
  readonly responses = new Map<string, RelayHttpResponse>();

  async connect(): Promise<void> {
    throw new Error("not used");
  }

  disconnect(): void {
    // no-op
  }

  status(): RelayConnectionStatus {
    return {
      state: "secure",
      machine_id: "mch_1",
      channel_id: "01JQ8Z6K4W5N7R9T2V3X4Y5Z6A",
      attempts: 1,
      next_retry_at: null,
      error: null,
    };
  }

  subscribe(): () => void {
    return () => undefined;
  }

  async request(path: string): Promise<RelayHttpResponse> {
    this.calls.push(path);
    const url = new URL(path, RELAY_BRIDGE_ORIGIN);
    if (url.pathname === "/api/v1/health") {
      return json(HEALTH);
    }
    if (url.pathname === "/api/v1/board") {
      return json(BOARD);
    }
    if (url.pathname === "/api/v1/jobs") {
      return json(JOBS);
    }
    if (url.pathname === "/api/v1/sessions") {
      return json(SESSIONS);
    }
    if (url.pathname === "/api/v1/inbox") {
      return json(INBOX);
    }
    if (url.pathname.startsWith("/api/v1/jobs/")) {
      if (url.pathname.endsWith("/runs")) return json(JOB_RUNS);
      if (url.pathname.endsWith("/gates")) return json(JOB_GATES);
      return json(JOB_DETAIL);
    }
    return {
      status: 404,
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ message: "not found" }),
    };
  }
}

function json(body: unknown): RelayHttpResponse {
  return {
    status: 200,
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("relay-backed Web bridge", () => {
  it("maps a same-relay URL to one http_request path and builds a Response", async () => {
    const relay = new FakeRelay();
    relay.responses.set("x", json({ ok: true }));
    const fetchImpl = createRelayFetch(relay);
    const response = await fetchImpl(
      `${RELAY_BRIDGE_ORIGIN}/api/v1/board?x=1`,
      { method: "GET", headers: { Accept: "application/json" } },
    );
    expect(response.status).toBe(200);
    expect(await response.json()).toEqual(BOARD);
    expect(relay.calls).toEqual(["/api/v1/board?x=1"]);
  });

  it("rejects another origin and a non-text body before calling the relay", async () => {
    const relay = new FakeRelay();
    const fetchImpl = createRelayFetch(relay);
    await expect(
      fetchImpl("https://evil.test/api/v1/board"),
    ).rejects.toMatchObject({ code: "relay_scope_denied" });
    await expect(
      fetchImpl(`${RELAY_BRIDGE_ORIGIN}/api/v1/board`, {
        method: "POST",
        body: new Uint8Array([1, 2, 3]),
      }),
    ).rejects.toMatchObject({ code: "invalid_input" });
    expect(relay.calls).toHaveLength(0);
  });

  it("never forwards Set-Cookie or WWW-Authenticate response headers", async () => {
    const relay = new FakeRelay();
    const original = relay.request.bind(relay);
    relay.request = async (path) => {
      const response = await original(path);
      return {
        ...response,
        headers: {
          ...response.headers,
          "Set-Cookie": "__Host-ss_session=secret",
          "WWW-Authenticate": "Bearer",
        },
      };
    };
    const fetchImpl = createRelayFetch(relay);
    const response = await fetchImpl(`${RELAY_BRIDGE_ORIGIN}/api/v1/health`);
    expect(response.headers.get("set-cookie")).toBeNull();
    expect(response.headers.get("www-authenticate")).toBeNull();
  });

  it("drives the existing read model and polling fallback through the relay", async () => {
    const relay = new FakeRelay();
    const bridge = createRelayBridge(relay, {
      now: () => NOW,
      pollIntervalMs: 60_000,
    });
    const snapshot = await bridge.fleet.snapshot?.(true);
    expect(snapshot?.source).toBe("daemon");
    expect(snapshot?.daemon.origin).toBe(RELAY_BRIDGE_ORIGIN);
    expect(snapshot?.project.name).toBe("vowp");
    expect(snapshot?.lanes.running.length).toBeGreaterThan(0);
    expect(relay.calls).toContain("/api/v1/health");
    expect(relay.calls).toContain("/api/v1/board");
    expect(relay.calls).toContain("/api/v1/jobs");
    expect(relay.calls).toContain("/api/v1/sessions");
    expect(relay.calls).toContain("/api/v1/inbox?state=open");
    // No EventSource is available over the relay in v1; the bridge reports the
    // polling fallback rather than silently pretending to stream.
    expect(bridge.fleet.webStatus?.().polling_only).toBe(true);
    bridge.dispose();
  });

  it("surfaces a relay transport failure as a typed Web error", async () => {
    const relay = new FakeRelay();
    relay.request = async () => {
      throw {
        code: "relay_offline",
        message: "The Always-On Home is offline.",
        hint: null,
        retryable: true,
        status: 4004,
      };
    };
    const bridge = createRelayBridge(relay, { now: () => NOW });
    await expect(bridge.fleet.snapshot?.(true)).rejects.toMatchObject({
      code: "daemon_unreachable",
    });
    bridge.dispose();
  });
});
