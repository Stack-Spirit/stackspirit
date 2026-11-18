import { beforeEach, describe, expect, it, vi } from "vitest";

import type {
  HostDraft,
  HostKeyPin,
  OnboardingInstallInput,
  OnboardingInstallOutcome,
} from "@stackspirit/ui";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import { TauriBridge } from "./TauriBridge";

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

function outcome(): OnboardingInstallOutcome {
  return {
    installed: true,
    fingerprint: PIN.sha256,
    password_login_disabled: false,
    password_auth: { state: "not_requested" },
    profile: {
      ...DRAFT,
      id: "01MOCKHOST000000000000000001",
      identity_file: "/app-data/ssh-keys/stackspirit-onboarding",
      host_key: PIN,
      created_at: PIN.pinned_at,
      updated_at: PIN.pinned_at,
      last_connected_at: null,
    },
  };
}

describe("TauriBridge onboarding", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it("rejects a missing or blank password before any invoke", async () => {
    const bridge = new TauriBridge();

    for (const password of ["", "   ", "\t\n"]) {
      const error = await bridge.onboarding
        .installKey(installInput(password))
        .catch((caught: unknown) => caught);
      expect(error).toMatchObject({ code: "invalid_input" });
    }
    const missing = await bridge.onboarding
      .installKey({ ...installInput("unused"), password: undefined } as unknown as OnboardingInstallInput)
      .catch((caught: unknown) => caught);
    expect(missing).toMatchObject({ code: "invalid_input" });

    expect(invoke).not.toHaveBeenCalled();
  });

  it("routes the three calls to their thin host commands", async () => {
    const bridge = new TauriBridge();
    invoke.mockResolvedValue({ exists: false, fingerprint: null, public_key: null });

    await bridge.onboarding.keyStatus();
    await bridge.onboarding.createKey();
    expect(invoke.mock.calls.map(([command]) => command)).toEqual([
      "onboarding_key_status",
      "onboarding_create_key",
    ]);

    const expected = outcome();
    invoke.mockResolvedValueOnce(expected);
    await expect(bridge.onboarding.installKey(installInput("one-time"))).resolves.toEqual(
      expected,
    );
    expect(invoke).toHaveBeenLastCalledWith("onboarding_install_key", {
      input: installInput("one-time"),
    });
  });
});
