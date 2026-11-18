import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, expectTypeOf, it } from "vitest";

import { MockBridge } from "../bridge/MockBridge";
import type { SpiritBridge } from "../bridge/SpiritBridge";
import type {
  SecretEntryView,
  SecretListView,
  SecretRemoveOutcome,
  SecretSetInput,
  SecretSetOutcome,
} from "../bridge/wire";

const here = dirname(fileURLToPath(import.meta.url));

function read(relative: string): string {
  return readFileSync(resolve(here, relative), "utf8");
}

function tauriBridgeSource(): string {
  return read("../../../../apps/spirit-desktop/src/bridge/TauriBridge.ts");
}

function hostCommandSource(): string {
  return read("../../../../apps/spirit-desktop/src-tauri/src/commands/secrets.rs");
}

function spiritBridgeSource(): string {
  return read("../bridge/SpiritBridge.ts");
}

async function captureError(action: () => Promise<unknown>): Promise<unknown> {
  try {
    await action();
  } catch (error) {
    return error;
  }
  throw new Error("expected the call to fail");
}

describe("server secrets bridge contract", () => {
  it("exposes the optional namespace on both bridges", () => {
    const mock = new MockBridge("fresh");
    const secrets = mock.secrets;
    if (!secrets) throw new Error("the mock bridge has no secrets namespace");
    expect(Object.keys(secrets).sort()).toEqual(["list", "remove", "set"]);
    expect(typeof secrets.list).toBe("function");
    expect(typeof secrets.set).toBe("function");
    expect(typeof secrets.remove).toBe("function");

    const source = tauriBridgeSource();
    expect(source).toMatch(/secrets\s*=\s*\{/);
    expect(source).toContain('call("secrets_list")');
    expect(source).toContain('call("secrets_set", { input })');
    expect(source).toContain('call("secrets_remove", { name })');
    expect(source).toContain("list:");
    expect(source).toContain("set:");
    expect(source).toContain("remove:");

    expect(spiritBridgeSource()).toMatch(/secrets\?: \{/);
  });

  it("keeps the value write-only in the shared wire types", () => {
    const entry: SecretEntryView = {
      name: "OPENAI_API_KEY",
      last4: "9f2a",
      created_at: "2026-11-18T10:00:00.000Z",
      updated_at: "2026-11-18T10:00:00.000Z",
    };
    expect(Object.keys(entry).sort()).toEqual([
      "created_at",
      "last4",
      "name",
      "updated_at",
    ]);

    expectTypeOf<SecretEntryView>().not.toHaveProperty("value");
    expectTypeOf<SecretListView>().not.toHaveProperty("value");
    expectTypeOf<SecretListView["storage"]>().toEqualTypeOf<string | null>();
    expectTypeOf<SecretSetOutcome>().toEqualTypeOf<{ name: string; last4: string }>();
    expectTypeOf<SecretRemoveOutcome>().toEqualTypeOf<{
      name: string;
      removed: boolean;
    }>();
    expectTypeOf<SecretSetInput>().toHaveProperty("value");
    expectTypeOf<SecretSetInput["value"]>().toEqualTypeOf<string>();
  });

  it("fails every preview call with preview_unavailable and never fakes a store", async () => {
    const bridge = new MockBridge("fresh");
    const secrets = bridge.secrets;
    if (!secrets) throw new Error("the mock bridge has no secrets namespace");

    for (const action of [
      () => secrets.list(),
      () => secrets.set({ name: "OPENAI_API_KEY", value: "preview-value" }),
      () => secrets.remove("OPENAI_API_KEY"),
    ]) {
      const error = await captureError(action);
      expect(error).toMatchObject({ code: "preview_unavailable" });
      expect(String((error as { message?: unknown }).message)).toMatch(/desktop host/i);
    }

    // A second call is still an honest refusal: preview never caches or fakes a store.
    await expect(secrets.list()).rejects.toMatchObject({ code: "preview_unavailable" });
  });

  it("keeps the value out of the preview error path", async () => {
    const bridge = new MockBridge("fresh");
    const secrets = bridge.secrets;
    if (!secrets) throw new Error("the mock bridge has no secrets namespace");

    const value = ["sk", "preview", "marker"].join("-");
    const error = await captureError(() =>
      secrets.set({ name: "OPENAI_API_KEY", value }),
    );

    expect(JSON.stringify(error)).not.toContain(value);
    expect(String((error as { message?: unknown }).message)).not.toContain(value);
  });

  it("guards a blank name or value before any invoke in the Tauri adapter", () => {
    const source = tauriBridgeSource();
    const setNameGuard = source.indexOf("input.name.trim().length === 0");
    const setValueGuard = source.indexOf("input.value.trim().length === 0");
    const removeGuard = source.indexOf("typeof name !== \"string\" || name.trim().length === 0");
    const setInvoke = source.indexOf('call("secrets_set", { input })');
    const removeInvoke = source.indexOf('call("secrets_remove", { name })');

    expect(setNameGuard).toBeGreaterThan(-1);
    expect(setValueGuard).toBeGreaterThan(setNameGuard);
    expect(setInvoke).toBeGreaterThan(setValueGuard);
    expect(removeGuard).toBeGreaterThan(-1);
    expect(removeInvoke).toBeGreaterThan(removeGuard);
    expect(source).not.toContain("console.");
  });

  it("moves the value into zeroizing storage in the host command and never logs it", () => {
    const source = hostCommandSource();

    expect(source).toContain("std::mem::take(&mut input.value)");
    expect(source).toContain("SecretString::new");
    expect(source).toContain("SecretSetRequest::new");
    expect(source).toContain("client.secret_set(request)");
    expect(source).toContain("client.secret_list()");
    expect(source).toContain("client.secret_remove(&name)");

    for (const forbidden of ["tracing::", "dbg!", "println!", "eprintln!", "stderr"]) {
      expect(source).not.toContain(forbidden);
    }

    // `SecretSetInput` deliberately has no `Debug`: the value must never be formatted.
    const inputStruct =
      /#\[derive\(([^)]*)\)\]\s*#\[serde\(deny_unknown_fields\)\]\s*pub struct SecretSetInput/.exec(
        source,
      );
    expect(inputStruct?.[1]).toBe("Deserialize");
  });

  it("keeps the existing bridge contract unchanged", async () => {
    const bridge = new MockBridge("fresh");
    for (const name of [
      "hosts",
      "onboarding",
      "connection",
      "fleet",
      "engine",
      "gates",
      "sessions",
      "localOrchestrators",
    ] as const) {
      expect(bridge[name]).toBeDefined();
    }

    const hosts = await bridge.hosts.list();
    expect(hosts.length).toBeGreaterThan(0);
    await expect(bridge.onboarding?.keyStatus()).rejects.toMatchObject({
      code: "preview_unavailable",
    });

    // The new namespace is additive: a bridge without it is still a `SpiritBridge`.
    expectTypeOf<undefined>().toMatchTypeOf<SpiritBridge["secrets"]>();
  });
});
