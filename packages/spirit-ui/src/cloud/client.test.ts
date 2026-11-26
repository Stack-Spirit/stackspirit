import { describe, expect, it, vi } from "vitest";

import { StackSpiritCloudClient } from "./client";
import type { CloudFetch } from "./types";

interface Recorded {
  url: URL;
  init: RequestInit;
  method: string;
  headers: Headers;
  body: unknown;
}

type Handler = (call: Recorded) => Response;

class FakeFetch {
  readonly calls: Recorded[] = [];
  private readonly handlers: Array<{ match: (url: URL) => boolean; respond: Handler }> = [];

  on(match: string | RegExp, respond: Handler): this {
    this.handlers.push({
      match: (url) =>
        typeof match === "string" ? url.pathname === match : match.test(url.pathname),
      respond,
    });
    return this;
  }

  readonly fetch: CloudFetch = async (input, init = {}) => {
    const raw =
      typeof input === "string" ? input : input instanceof URL ? input.toString() : input.url;
    const url = new URL(raw);
    const headers = new Headers(init.headers);
    let body: unknown = null;
    if (typeof init.body === "string" && init.body.length > 0) {
      body = JSON.parse(init.body) as unknown;
    }
    const call: Recorded = { url, init, method: init.method ?? "GET", headers, body };
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

function jwt(claims: Record<string, unknown>): string {
  const payload = btoa(JSON.stringify(claims))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
  return `header.${payload}.signature`;
}

const CLIENT_KEY = { kty: "EC" as const, crv: "P-256" as const, x: "x-value", y: "y-value" };
const NOW = 1_764_072_000_000;

function makeClient(fetch: FakeFetch): StackSpiritCloudClient {
  return new StackSpiritCloudClient({
    apiBaseUrl: "https://api.test/",
    fetchImpl: fetch.fetch,
    now: () => NOW,
  });
}

describe("StackSpirit account client", () => {
  it("makes no request on construction", () => {
    const fetch = new FakeFetch();
    const client = makeClient(fetch);
    expect(client.hasSession).toBe(false);
    expect(fetch.calls).toHaveLength(0);
  });

  it("starts and completes a device-code sign-in without exposing the token", async () => {
    const fetch = new FakeFetch();
    fetch
      .on("/v1/auth/device", () =>
        json({
          device_code: "device-123",
          user_code: "ABCD-EFGH",
          verification_uri: "https://stackspirit.io/device",
          expires_in: 600,
          interval: 5,
        }),
      )
      .on("/v1/auth/device/token", () => json({ access_token: "access-token-value" }))
      .on("/v1/me", () => json({ id: "acct_1", email: "owner@example.com", name: "Owner" }))
      .on("/v1/machines", () =>
        json({
          machines: [
            {
              id: "mch_1",
              name: "contabo-1",
              status: "online",
              project: "vowp",
              last_seen_at: "2026-11-26T10:00:00.000Z",
            },
            {
              id: "mch_2",
              name: "home-mini",
              status: "offline",
              project: null,
              last_seen_at: null,
            },
          ],
        }),
      );

    const client = makeClient(fetch);
    const device = await client.startDeviceSignIn();
    expect(device.user_code).toBe("ABCD-EFGH");
    expect(device.expires_at).toBe(new Date(NOW + 600_000).toISOString());
    expect(fetch.calls[0]?.url.pathname).toBe("/v1/auth/device");
    expect(fetch.calls[0]?.headers.has("Authorization")).toBe(false);
    expect(fetch.calls[0]?.body).toMatchObject({ client_id: "stackspirit-ui", kind: "web" });

    expect(await client.pollDeviceSignIn("device-123")).toBe("authorized");
    expect(client.hasSession).toBe(true);
    const tokenCall = fetch.calls[1];
    expect(tokenCall?.url.pathname).toBe("/v1/auth/device/token");
    expect(tokenCall?.body).toEqual({ device_code: "device-123" });

    const account = await client.account();
    expect(account).toEqual({ id: "acct_1", email: "owner@example.com", name: "Owner" });
    expect(fetch.calls[2]?.headers.get("Authorization")).toBe("Bearer access-token-value");

    const machines = await client.listMachines();
    expect(machines.map((machine) => [machine.id, machine.status])).toEqual([
      ["mch_1", "online"],
      ["mch_2", "offline"],
    ]);
    // No read model or error carries the access token.
    expect(JSON.stringify(account)).not.toContain("access-token-value");
    expect(JSON.stringify(machines)).not.toContain("access-token-value");
  });

  it("reports a pending device poll and a declined one", async () => {
    const fetch = new FakeFetch();
    fetch
      .on("/v1/auth/device/token", () => json({ error: "authorization_pending" }, 400))
      .on("/v1/auth/denied", () => json({ error: "access_denied" }, 400));
    const client = makeClient(fetch);
    expect(await client.pollDeviceSignIn("device-123")).toBe("pending");
    expect(client.hasSession).toBe(false);

    fetch.on("/v1/auth/device/token", () => json({ error: "slow_down" }, 400));
    expect(await client.pollDeviceSignIn("device-123")).toBe("slow_down");

    fetch.on("/v1/auth/device/token", () => json({ error: "access_denied" }, 400));
    await expect(client.pollDeviceSignIn("device-123")).rejects.toMatchObject({
      code: "cloud_auth",
    });
  });

  it("mints a scoped relay ticket with only the public client key", async () => {
    const ticket = jwt({
      iss: "https://api.stackspirit.io",
      aud: "relay.stackspirit.io",
      sub: "client:cl_1",
      acct: "acct_1",
      machine: "mch_1",
      kind: "web",
      scopes: ["http.read"],
      cnf: { jkt: "thumbprint" },
      mkey: "test-machine-key",
      mkey_id: "ed25519:test-machine-key",
      iat: 1_764_072_000,
      exp: 1_764_072_300,
      jti: "jti_1",
    });
    const fetch = new FakeFetch();
    fetch
      .on("/v1/auth/device/token", () => json({ access_token: "access-token-value" }))
      .on("/v1/machines/mch_1/ticket", () =>
        json({
          ticket,
          expires_at: "2026-11-26T10:05:00.000Z",
          relay_url: "wss://relay.stackspirit.io/v1/client",
          machine: {
            id: "mch_1",
            name: "contabo-1",
            key_id: "ed25519:test-machine-key",
            public_key: "test-machine-key",
            project: "vowp",
          },
        }),
      );
    const client = makeClient(fetch);
    await client.pollDeviceSignIn("device-123");

    const minted = await client.mintRelayTicket("mch_1", {
      kind: "web",
      scopes: ["http.read", "mcp.read"],
      client_key: CLIENT_KEY,
      client_nonce: "client-nonce",
      ttl_secs: 900,
    });
    expect(minted.machine.name).toBe("contabo-1");
    const call = fetch.calls.at(-1);
    expect(call?.method).toBe("POST");
    expect(call?.url.pathname).toBe("/v1/machines/mch_1/ticket");
    expect(call?.headers.get("Authorization")).toBe("Bearer access-token-value");
    expect(call?.body).toEqual({
      kind: "web",
      scopes: ["http.read", "mcp.read"],
      client_key: CLIENT_KEY,
      client_nonce: "client-nonce",
      ttl_secs: 300,
    });
    const serialized = JSON.stringify(call?.body);
    for (const forbidden of [
      "private_key",
      "ssh_key",
      "passphrase",
      "password",
      "refresh_token",
      "access_token",
    ]) {
      expect(serialized).not.toContain(forbidden);
    }
  });

  it("falls back to the design's account-wide ticket route only on a 404", async () => {
    const ticket = jwt({
      machine: "mch_1",
      mkey: "test-machine-key",
      mkey_id: "ed25519:test-machine-key",
      jti: "jti_1",
      exp: Math.floor(NOW / 1000) + 300,
    });
    const fetch = new FakeFetch();
    fetch
      .on("/v1/auth/device/token", () => json({ access_token: "access-token-value" }))
      .on("/v1/machines/mch_1/ticket", () => json({ message: "not found" }, 404))
      .on("/v1/relay-tickets", () =>
        json({
          ticket,
          expires_at: "2026-11-26T10:05:00.000Z",
          relay_url: "wss://relay.stackspirit.io/v1/client",
          machine: {
            id: "mch_1",
            name: "contabo-1",
            key_id: "ed25519:test-machine-key",
            public_key: "test-machine-key",
            project: null,
          },
        }),
      );
    const client = makeClient(fetch);
    await client.pollDeviceSignIn("device-123");
    const minted = await client.mintRelayTicket("mch_1", {
      kind: "web",
      scopes: ["http.read"],
      client_key: CLIENT_KEY,
      client_nonce: "nonce",
      ttl_secs: 300,
    });
    expect(minted.machine.id).toBe("mch_1");
    const paths = fetch.calls.map((call) => call.url.pathname);
    expect(paths).toContain("/v1/machines/mch_1/ticket");
    expect(paths).toContain("/v1/relay-tickets");
    const fallback = fetch.calls.at(-1);
    expect(fallback?.body).toMatchObject({ machine_id: "mch_1", kind: "web", scopes: ["http.read"] });
    expect(JSON.stringify(fallback?.body)).not.toMatch(/private|ssh_key|passphrase/i);
  });

  it("refuses an unknown scope and an unsafe machine id", async () => {
    const fetch = new FakeFetch();
    const client = makeClient(fetch);
    await expect(
      client.mintRelayTicket("mch_1", {
        kind: "web",
        // Deliberately a string outside the known scope union, as an injected fake might send.
        scopes: ["root"] as never,
        client_key: CLIENT_KEY,
        client_nonce: "nonce",
        ttl_secs: 300,
      }),
    ).rejects.toMatchObject({ code: "invalid_input" });
    await expect(
      client.mintRelayTicket("../admin", {
        kind: "web",
        scopes: ["http.read"],
        client_key: CLIENT_KEY,
        client_nonce: "nonce",
        ttl_secs: 300,
      }),
    ).rejects.toMatchObject({ code: "invalid_input" });
    expect(fetch.calls).toHaveLength(0);
  });

  it("parses the published plan catalog and never invents a tier", async () => {
    const fetch = new FakeFetch();
    fetch.on("/v1/plans", () =>
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
            setup_fee_eur: 9.9,
            currency: "EUR",
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
            currency: "EUR",
            provider_spec: "Cloud VPS 6",
          },
        ],
        backups: {
          monthly_eur: 3.5,
          recommended: true,
          provider: "cloudflare-r2",
          // The server must not be able to turn this on by default.
          default_enabled: true,
        },
      }),
    );
    const client = makeClient(fetch);
    const catalog = await client.getPlans();
    expect(catalog.plans.map((plan) => plan.name)).toEqual(["Home 8", "Home 12"]);
    expect(catalog.plans[0]).toMatchObject({ monthly_eur: 6.6, setup_fee_eur: 9.9 });
    expect(catalog.backups).toEqual({
      monthly_eur: 3.5,
      recommended: true,
      provider: "cloudflare-r2",
    });
    expect(catalog.plans).toHaveLength(2);
    expect(fetch.calls[0]?.headers.has("Authorization")).toBe(false);
  });

  it("maps auth, not-found and parse failures without leaking a token", async () => {
    const fetch = new FakeFetch();
    fetch.on("/v1/me", () => json({ message: "expired" }, 401));
    fetch.on("/v1/plans", () => json({ plans: [{ name: "broken" }] }));
    const client = makeClient(fetch);
    await expect(client.account()).rejects.toMatchObject({
      code: "cloud_auth",
      message: "Sign in to StackSpirit first.",
    });
    fetch.on("/v1/auth/device/token", () => json({ access_token: "access-token-value" }));
    await client.pollDeviceSignIn("device-123");
    await expect(client.account()).rejects.toMatchObject({
      code: "cloud_auth",
      message: "Sign in to StackSpirit again.",
    });
    await expect(client.getPlans()).rejects.toMatchObject({ code: "cloud_parse" });
  });

  it("reads live provisioning status and scrubs absolute local paths", async () => {
    const fetch = new FakeFetch();
    fetch
      .on("/v1/auth/device/token", () => json({ access_token: "access-token-value" }))
      .on("/v1/machines/mch_1/provisioning", () =>
        json({
          machine_id: "mch_1",
          phase: "provisioning",
          step: "cloud-init",
          progress_pct: 47.6,
          detail: "writing /home/stackspirit/.stackspirit/config.toml",
          updated_at: "2026-11-26T10:00:00.000Z",
          home_url: "https://app.stackspirit.io/machines/mch_1",
        }),
      );
    const client = makeClient(fetch);
    await client.pollDeviceSignIn("device-123");
    const status = await client.getProvisioning("mch_1");
    expect(status.phase).toBe("provisioning");
    expect(status.progress_pct).toBe(48);
    expect(status.detail).toContain("[path]");
    expect(status.detail).not.toContain("/home/");
  });

  it("treats an unreachable API as retryable and keeps local mode usable", async () => {
    const fetch: CloudFetch = vi.fn(async () => {
      throw new TypeError("network down");
    });
    const client = new StackSpiritCloudClient({
      apiBaseUrl: "https://api.test",
      fetchImpl: fetch,
    });
    await expect(client.getPlans()).rejects.toMatchObject({
      code: "cloud_unreachable",
      retryable: true,
    });
    expect(client.hasSession).toBe(false);
  });
});
