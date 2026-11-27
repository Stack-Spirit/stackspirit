import { readFile, readdir } from "node:fs/promises";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import type { SpiritBridge } from "@stackspirit/ui";

import {
  HttpSseBridge,
  JOB_DETAIL_ROUTE_PATTERNS,
  KNOWN_UNUSED_ROUTES,
  READ_ROUTES,
  WRITE_ROUTES,
  isReadRoute,
} from "./HttpSseBridge";
import { WebMockBridge } from "./WebMockBridge";

type MethodClass = "available" | "unavailable" | "desktop-only" | "write-gated";

/**
 * Every SpiritBridge method this Web build exposes, with its contract class. The test fails
 * when a method is added without a class. `fleet.snapshot`, `fleet.watch`, `fleet.webStatus`
 * and `daemon.health` are the additive Web read model; desktop bridges may omit them.
 */
export const WEB_METHOD_CLASSES: Record<string, MethodClass> = {
  "hosts.list": "desktop-only",
  "hosts.add": "desktop-only",
  "hosts.update": "desktop-only",
  "hosts.remove": "desktop-only",
  "hosts.scanFingerprint": "desktop-only",
  "hosts.trustFingerprint": "desktop-only",
  "hosts.probe": "desktop-only",
  "connection.connect": "desktop-only",
  "connection.disconnect": "desktop-only",
  "connection.status": "available",
  "connection.subscribe": "available",
  "fleet.get": "unavailable",
  "fleet.refresh": "unavailable",
  "fleet.capacity": "unavailable",
  "fleet.job": "available",
  "fleet.jobAction": "write-gated",
  "fleet.setJobExecutor": "write-gated",
  "fleet.inbox": "available",
  "fleet.answerInbox": "write-gated",
  "fleet.dismissInbox": "write-gated",
  "fleet.setTarget": "unavailable",
  "fleet.rawDocument": "unavailable",
  "fleet.snapshot": "available",
  "fleet.watch": "available",
  "fleet.webStatus": "available",
  "engine.status": "desktop-only",
  "engine.models": "desktop-only",
  "engine.selectModel": "desktop-only",
  "engine.storeKey": "desktop-only",
  "engine.verifyKey": "desktop-only",
  "engine.deleteKey": "desktop-only",
  "engine.keyStatus": "desktop-only",
  "engine.scanExternal": "desktop-only",
  "engine.selectExternal": "desktop-only",
  "engine.useBundled": "desktop-only",
  "engine.restart": "desktop-only",
  "gates.get": "desktop-only",
  "gates.probe": "desktop-only",
  "daemon.health": "available",
};

function methodKeys(bridge: SpiritBridge): string[] {
  const keys: string[] = [];
  for (const group of ["hosts", "connection", "fleet", "engine", "gates"] as const) {
    for (const key of Object.keys(bridge[group])) {
      keys.push(`${group}.${key}`);
    }
  }
  if (bridge.daemon) {
    for (const key of Object.keys(bridge.daemon)) keys.push(`daemon.${key}`);
  }
  return keys;
}

describe("SpiritBridge coverage", () => {
  it("classifies every Web bridge method", () => {
    const bridge = new HttpSseBridge({
      baseUrl: "http://web.test",
      pageOrigin: "http://web.test",
      fetchImpl: async () => new Response("{}"),
    });
    const keys = methodKeys(bridge);
    expect(new Set(keys)).toEqual(new Set(Object.keys(WEB_METHOD_CLASSES)));
    bridge.dispose();
  });

  it("gives the preview bridge the same method surface", () => {
    const bridge = new WebMockBridge("fresh");
    expect(new Set(methodKeys(bridge))).toEqual(new Set(Object.keys(WEB_METHOD_CLASSES)));
    bridge.dispose();
  });

  it("keeps the Web bridge read-only: no desktop file manager surface", () => {
    const http = new HttpSseBridge({
      baseUrl: "http://web.test",
      pageOrigin: "http://web.test",
      fetchImpl: async () => new Response("{}"),
    });
    const preview = new WebMockBridge("fresh");
    expect((http as SpiritBridge).files).toBeUndefined();
    expect((preview as SpiritBridge).files).toBeUndefined();
    http.dispose();
    preview.dispose();
  });

  it("keeps every route constant explicit", () => {
    expect(Object.values(READ_ROUTES).every((route) => route.startsWith("/api/v1/"))).toBe(
      true,
    );
    expect(KNOWN_UNUSED_ROUTES.every((route) => route.startsWith("/api/v1/"))).toBe(true);
    expect(WRITE_ROUTES).toContain("/api/v1/capacity/target");
    expect(Object.values(READ_ROUTES)).not.toContain("/api/v1/nodes");
    expect(Object.values(READ_ROUTES)).not.toContain("/api/v1/cost");
    expect(JOB_DETAIL_ROUTE_PATTERNS).toEqual([
      "/api/v1/jobs/{id}",
      "/api/v1/jobs/{id}/runs",
      "/api/v1/jobs/{id}/gates",
    ]);
    expect(KNOWN_UNUSED_ROUTES).not.toContain("/api/v1/jobs/{id}");
  });

  it("allows only the fixed reads and the job-detail reads", () => {
    for (const route of Object.values(READ_ROUTES)) {
      expect(isReadRoute(route)).toBe(true);
    }
    expect(isReadRoute("/api/v1/jobs/ss-1-web-job-detail")).toBe(true);
    expect(isReadRoute("/api/v1/jobs/ss-1-web-job-detail/runs")).toBe(true);
    expect(isReadRoute("/api/v1/jobs/ss-1-web-job-detail/gates")).toBe(true);
    expect(isReadRoute("/api/v1/jobs/")).toBe(false);
    expect(isReadRoute("/api/v1/jobs/ss-1-web-job-detail/retry")).toBe(false);
    expect(isReadRoute("/api/v1/nodes")).toBe(false);
  });
});

async function sourceFiles(root: string): Promise<string[]> {
  const entries = await readdir(root, { withFileTypes: true });
  const files: string[] = [];
  for (const entry of entries) {
    const path = join(root, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "test" || entry.name === "e2e") continue;
      files.push(...(await sourceFiles(path)));
    } else if (/\.(ts|tsx)$/.test(entry.name) && !entry.name.includes(".test.")) {
      files.push(path);
    }
  }
  return files;
}

describe("browser security boundary", () => {
  it("keeps the bearer header in the account client and never stores the token", async () => {
    const files = await sourceFiles(join(process.cwd(), "src"));
    expect(files.length).toBeGreaterThan(10);
    const authClient = join(process.cwd(), "src/auth/client.ts");
    const problems: string[] = [];
    for (const file of files) {
      const text = await readFile(file, "utf8");
      if (/Authorization/i.test(text) && file !== authClient) {
        problems.push(`${file}: Authorization outside the account client`);
      }
      if (/Bearer\s+[A-Za-z0-9._~-]{12,}/i.test(text)) problems.push(`${file}: bearer value`);
      if (/VOWP_API_TOKEN\s*=/.test(text)) problems.push(`${file}: token assignment`);
      if (/localStorage/.test(text)) problems.push(`${file}: localStorage`);
    }
    expect(problems).toEqual([]);
  });

  it("keeps the Web session token in memory, never in browser storage", async () => {
    const files = await sourceFiles(join(process.cwd(), "src"));
    const hits: string[] = [];
    for (const file of files) {
      const text = await readFile(file, "utf8");
      if (/auth_token/.test(text)) hits.push(file);
    }
    expect(hits).toContain(join(process.cwd(), "src/auth/session.ts"));
    expect(hits.every((file) => file.endsWith("auth/session.ts"))).toBe(true);
  });

  it("uses sessionStorage only for the daemon base address", async () => {
    const files = await sourceFiles(join(process.cwd(), "src"));
    const hits: string[] = [];
    for (const file of files) {
      const text = await readFile(file, "utf8");
      if (text.includes("sessionStorage")) hits.push(file);
    }
    expect(hits).toContain(join(process.cwd(), "src/bridge/create.ts"));
    expect(hits.every((file) => file.endsWith("create.ts"))).toBe(true);
  });
});
