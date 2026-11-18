import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, expectTypeOf, it } from "vitest";

import { MockBridge } from "../bridge/MockBridge";
import type {
  AppKeyView,
  HostDraft,
  HostKeyPin,
  OnboardingInstallInput,
  OnboardingInstallOutcome,
} from "../bridge/wire";

const here = dirname(fileURLToPath(import.meta.url));

const DRAFT: HostDraft = {
  label: "onboarding-test",
  ssh: { user: "owner", host: "203.0.113.30", port: 22 },
  identity_file: null,
  vowp_home: null,
  default_project: null,
};

const PIN: HostKeyPin = {
  algorithm: "ssh-ed25519",
  sha256: "SHA256:onboardingFingerprint0000000000000000000000",
  pinned_at: "2026-11-18T10:00:00.000Z",
};

function installInput(password: string): OnboardingInstallInput {
  return {
    draft: DRAFT,
    host_key: PIN,
    password,
    disable_password_auth: true,
  };
}

function tauriBridgeSource(): string {
  return readFileSync(
    resolve(here, "../../../../apps/spirit-desktop/src/bridge/TauriBridge.ts"),
    "utf8",
  );
}

async function captureError(action: () => Promise<unknown>): Promise<unknown> {
  try {
    await action();
  } catch (error) {
    return error;
  }
  throw new Error("expected the call to fail");
}

describe("onboarding bridge contract", () => {
  it("exposes the namespace and every declared method on both bridges", () => {
    const mock = new MockBridge("fresh");
    expect(mock.onboarding).toBeDefined();
    expect(Object.keys(mock.onboarding ?? {}).sort()).toEqual([
      "createKey",
      "installKey",
      "keyStatus",
    ]);
    expect(typeof mock.onboarding?.keyStatus).toBe("function");
    expect(typeof mock.onboarding?.createKey).toBe("function");
    expect(typeof mock.onboarding?.installKey).toBe("function");

    const source = tauriBridgeSource();
    expect(source).toMatch(/onboarding\s*=\s*\{/);
    expect(source).toContain("onboarding_key_status");
    expect(source).toContain("onboarding_create_key");
    expect(source).toContain("onboarding_install_key");
    expect(source).toContain("keyStatus:");
    expect(source).toContain("createKey:");
    expect(source).toContain("installKey:");
  });

  it("returns a typed preview_unavailable error and never claims an install", async () => {
    const bridge = new MockBridge("fresh");
    const onboarding = bridge.onboarding;
    if (!onboarding) throw new Error("the mock bridge has no onboarding namespace");

    const password = "one-time-password-4f8c";
    await expect(onboarding.keyStatus()).rejects.toMatchObject({
      code: "preview_unavailable",
    });
    await expect(onboarding.createKey()).rejects.toMatchObject({
      code: "preview_unavailable",
    });

    let outcome: OnboardingInstallOutcome | null = null;
    try {
      outcome = await onboarding.installKey(installInput(password));
    } catch (error) {
      expect(error).toMatchObject({ code: "preview_unavailable" });
    }
    expect(outcome).toBeNull();
  });

  it("keeps the password out of the preview error path", async () => {
    const bridge = new MockBridge("fresh");
    const onboarding = bridge.onboarding;
    if (!onboarding) throw new Error("the mock bridge has no onboarding namespace");

    const password = "one-time-password-9a1b";
    const error = await captureError(() => onboarding.installKey(installInput(password)));

    expect(JSON.stringify(error)).not.toContain(password);
    expect(String((error as { message?: unknown }).message)).not.toContain(password);
    expect(JSON.stringify(error)).not.toContain("installed");

    // The request itself is the only place the one-time password travels; it has no
    // app-key path field, because the host resolves that path itself.
    const serialized = JSON.stringify(installInput(password));
    expect(Object.keys(JSON.parse(serialized) as Record<string, unknown>)).toEqual([
      "draft",
      "host_key",
      "password",
      "disable_password_auth",
    ]);
  });

  it("keeps the app-key view path-free by contract", () => {
    const view: AppKeyView = {
      exists: true,
      fingerprint: "SHA256:onboardingFingerprint0000000000000000000000",
      public_key: "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMockOwnerKey onboarding-test",
    };
    expect(Object.keys(view).sort()).toEqual(["exists", "fingerprint", "public_key"]);

    expectTypeOf<AppKeyView>().not.toHaveProperty("private_key_path");
    expectTypeOf<AppKeyView>().not.toHaveProperty("path");
    expectTypeOf<OnboardingInstallInput>().not.toHaveProperty("private_key_path");
    expectTypeOf<OnboardingInstallOutcome>().toHaveProperty("profile");
    expectTypeOf<
      Extract<OnboardingInstallOutcome["password_auth"], { state: "not_supported" }>
    >().toHaveProperty("reason");

    const wire = readFileSync(resolve(here, "../bridge/wire.ts"), "utf8");
    const appKeyView = /export interface AppKeyView \{([\s\S]*?)\}/.exec(wire)?.[1] ?? "";
    expect(appKeyView).not.toMatch(/path/i);
  });

  it("guards a blank password before any invoke in the Tauri adapter", () => {
    const source = tauriBridgeSource();
    const guard = source.indexOf("input.password.trim().length === 0");
    const installInvoke = source.indexOf('call("onboarding_install_key", { input })');
    expect(guard).toBeGreaterThan(-1);
    expect(installInvoke).toBeGreaterThan(guard);
    expect(source).not.toContain("private_key_path");
  });
});
