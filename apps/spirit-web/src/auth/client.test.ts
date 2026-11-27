import { afterEach, describe, expect, it, vi } from "vitest";

import type { CloudFetch } from "@stackspirit/ui";

import { WebCloudClient } from "./client";
import { setSessionToken } from "./session";

interface Recorded {
  url: URL;
  method: string;
  headers: Headers;
  body: unknown;
}

type Handler = (call: Recorded) => Response;

class FakeFetch {
  readonly calls: Recorded[] = [];
  private readonly handlers: Array<{ match: (url: URL) => boolean; respond: Handler }> = [];

  on(match: string, respond: Handler): this {
    this.handlers.push({ match: (url) => url.pathname === match, respond });
    return this;
  }

  readonly fetch: CloudFetch = async (input, init = {}) => {
    const raw =
      typeof input === "string" ? input : input instanceof URL ? input.toString() : input.url;
    const url = new URL(raw);
    const headers = new Headers(init.headers);
    const body =
      typeof init.body === "string" && init.body.length > 0
        ? (JSON.parse(init.body) as unknown)
        : null;
    const call: Recorded = { url, method: init.method ?? "GET", headers, body };
    this.calls.push(call);
    const handler = [...this.handlers].reverse().find((entry) => entry.match(url));
    if (!handler) return json({ error: "not_found" }, 404);
    return handler.respond(call);
  };
}

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

function makeClient(fetch: FakeFetch): WebCloudClient {
  return new WebCloudClient({ apiBaseUrl: "https://api.test/", fetchImpl: fetch.fetch });
}

afterEach(() => {
  setSessionToken(null);
});

describe("Web StackSpirit Account client", () => {
  it("starts Google and GitHub at the public GET endpoints", () => {
    const client = makeClient(new FakeFetch());
    expect(client.oauthStartUrl("google")).toBe(
      "https://api.test/v1/auth/google/start",
    );
    expect(client.oauthStartUrl("github")).toBe(
      "https://api.test/v1/auth/github/start",
    );
  });

  it("signs up, verifies the six-digit code and uses the bearer session without exposing it", async () => {
    const fetch = new FakeFetch();
    fetch
      .on("/v1/auth/signup", () => json({ verification_required: true }))
      .on("/v1/auth/verify-email", () => json({ access_token: "auth-session-value" }))
      .on("/v1/me", () =>
        json({ id: "acct_1", email: "owner@example.com", name: "Owner" }),
      );
    const client = makeClient(fetch);

    const created = await client.signUp("owner@example.com", "correct-horse-battery");
    expect(created).toEqual({
      email: "owner@example.com",
      verificationRequired: true,
      signedIn: false,
    });
    expect(fetch.calls[0]?.body).toEqual({
      email: "owner@example.com",
      password: "correct-horse-battery",
    });

    const verified = await client.verifyEmail("owner@example.com", "123456");
    expect(verified).toEqual({ signedIn: true });
    expect(client.hasSession).toBe(true);

    const account = await client.account();
    expect(account.email).toBe("owner@example.com");
    const me = fetch.calls.find((call) => call.url.pathname === "/v1/me");
    expect(me?.headers.get("Authorization")).toBe("Bearer auth-session-value");
  });

  it("logs in and fails cleanly when the API returns no session", async () => {
    const fetch = new FakeFetch();
    fetch
      .on("/v1/auth/login", () => json({ access_token: "auth-session-value" }))
      .on("/v1/me", () => json({ id: "acct_1", email: "owner@example.com", name: null }));
    const client = makeClient(fetch);

    await client.login("owner@example.com", "correct-horse-battery");
    expect(client.hasSession).toBe(true);
    expect((await client.account()).name).toBeNull();

    const empty = new FakeFetch().on("/v1/auth/login", () => json({ ok: true }));
    const failing = makeClient(empty);
    await expect(failing.login("owner@example.com", "correct-horse-battery")).rejects.toMatchObject(
      { code: "cloud_auth" },
    );
  });

  it("rejects a malformed verification code before making a request", async () => {
    const fetch = new FakeFetch();
    const client = makeClient(fetch);
    await expect(client.verifyEmail("owner@example.com", "12")).rejects.toMatchObject({
      code: "invalid_input",
    });
    expect(fetch.calls).toHaveLength(0);
  });

  it("parses EUR and USD plan fields with an honest USD fallback", async () => {
    const fetch = new FakeFetch().on("/v1/plans", () =>
      json({
        plans: [
          {
            id: "home-8",
            name: "Home 8",
            vcpu: 4,
            ram_gb: 8,
            disk_gb: 100,
            disk_kind: "SSD",
            monthly_eur: 6.6,
            monthly_usd: 7.2,
            setup_fee_eur: 9.9,
            setup_fee_usd: 10.8,
            provider_spec: "Cloud VPS 4",
          },
          {
            id: "home-12",
            name: "Home 12",
            vcpu: 6,
            ram_gb: 12,
            disk_gb: 200,
            disk_kind: "SSD",
            monthly_eur: 9,
            setup_fee_eur: 0,
            provider_spec: "Cloud VPS 6",
          },
        ],
        backups: { monthly_eur: 3.5, monthly_usd: 3.8, recommended: true },
      }),
    );
    const client = makeClient(fetch);
    const plans = await client.getPlans();

    const first = plans.plans[0] as (typeof plans.plans)[number] & {
      monthly_usd?: number | null;
      setup_fee_usd?: number | null;
    };
    const second = plans.plans[1] as (typeof plans.plans)[number] & {
      monthly_usd?: number | null;
    };
    expect(first.monthly_usd).toBe(7.2);
    expect(first.setup_fee_usd).toBe(10.8);
    expect(second.monthly_usd).toBeNull();
    expect((plans.backups as { monthly_usd?: number | null }).monthly_usd).toBe(3.8);
  });

  it("creates a one-time machine enrollment token through the account session", async () => {
    const fetch = new FakeFetch().on("/v1/machines/enroll-tokens", () =>
      json({ token: "enrollment-token-value", expires_at: "2026-11-26T10:15:00.000Z" }),
    );
    const client = makeClient(fetch);
    setSessionToken("auth-session-value");

    const token = await client.createEnrollmentToken("contabo-existing");
    expect(token.token).toBe("enrollment-token-value");
    expect(fetch.calls[0]?.body).toEqual({ name: "contabo-existing", ttl_secs: 900 });
    expect(fetch.calls[0]?.headers.get("Authorization")).toBe(
      "Bearer auth-session-value",
    );
  });

  it("normalizes API failures into typed cloud errors without a token", async () => {
    const fetch = new FakeFetch().on("/v1/auth/login", () =>
      json({ message: "invalid credentials" }, 401),
    );
    const client = makeClient(fetch);
    await expect(client.login("owner@example.com", "wrong")).rejects.toMatchObject({
      code: "cloud_auth",
      status: 401,
    });
    expect(client.hasSession).toBe(false);
  });

  it("does not write the session to browser storage", async () => {
    const storage = vi.spyOn(Storage.prototype, "setItem");
    const fetch = new FakeFetch().on("/v1/auth/login", () =>
      json({ access_token: "auth-session-value" }),
    );
    await makeClient(fetch).login("owner@example.com", "correct-horse-battery");
    expect(storage).not.toHaveBeenCalled();
    storage.mockRestore();
  });
});
