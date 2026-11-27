import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { SpiritBridge } from "../../bridge/SpiritBridge";
import type {
  HostProfile,
  OnboardingInstallInput,
  OnboardingInstallOutcome,
  PasswordAuthOutcome,
} from "../../bridge/wire";
import { SettingsView } from "../../next/SettingsView";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";

const AT = "2026-11-18T11:00:00.000Z";
const ONE_TIME_PASSWORD = "settings-one-time-4c1d";

function host(overrides: Partial<HostProfile> = {}): HostProfile {
  return {
    id: "host-app",
    label: "production-vps",
    ssh: { user: "root", host: "203.0.113.60", port: 22 },
    identity_file: "/app-data/ssh-keys/stackspirit-onboarding",
    vowp_home: null,
    default_project: null,
    host_key: {
      algorithm: "ssh-ed25519",
      sha256: "SHA256:serverSecurityHostKey00000000000000000000",
      pinned_at: AT,
    },
    created_at: AT,
    updated_at: AT,
    last_connected_at: null,
    ...overrides,
  };
}

const APP_HOST = host();
const MANUAL_HOST = host({
  id: "host-manual",
  label: "manual-server",
  ssh: { user: "owner", host: "203.0.113.61", port: 22 },
  identity_file: "/home/owner/.ssh/id_ed25519",
});

function installOutcome(passwordAuth: PasswordAuthOutcome): OnboardingInstallOutcome {
  return {
    installed: true,
    fingerprint: "SHA256:serverSecurityAppKey0000000000000000000000",
    password_login_disabled: passwordAuth.state === "disabled",
    password_auth: passwordAuth,
    profile: APP_HOST,
  };
}

interface FakeOptions {
  hosts?: HostProfile[];
  installedHosts?: string[];
  preview?: boolean;
  installKey?: (input: OnboardingInstallInput) => Promise<OnboardingInstallOutcome>;
}

function renderSettings(options: FakeOptions = {}) {
  resetFleetStore();
  const hosts = options.hosts ?? [APP_HOST, MANUAL_HOST];
  const bridge = {
    kind: "tauri",
    preview: options.preview ?? false,
    hosts: { list: async () => hosts },
    onboarding: {
      keyStatus: async () => ({ exists: true, fingerprint: null, public_key: null }),
      createKey: async () => ({ exists: true, fingerprint: null, public_key: null }),
      installKey:
        options.installKey ??
        (async (input: OnboardingInstallInput) => ({
          ...installOutcome({ state: "disabled" }),
          profile: { ...APP_HOST, ssh: input.draft.ssh },
        })),
      installedHosts: async () => options.installedHosts ?? [APP_HOST.id],
    },
  } as unknown as SpiritBridge;
  useFleetStore.setState({
    bridge,
    preview: options.preview ?? false,
    status: "ready",
    hosts,
    hostsLoaded: true,
    selectedHostId: hosts[0]?.id ?? null,
  });
  return { bridge, ...render(<SettingsView theme="system" onTheme={() => undefined} initialSection="server" />) };
}

afterEach(() => {
  resetFleetStore();
});

describe("Settings password-login retry", () => {
  it("offers only saved servers whose profile uses the StackSpirit key", async () => {
    renderSettings({ installedHosts: [APP_HOST.id] });

    const card = await screen.findByTestId("server-security");
    const server = within(card).getByLabelText("Server");
    const choices = within(server).getAllByRole("option");
    expect(choices).toHaveLength(1);
    expect(choices[0]).toHaveTextContent("production-vps");
    expect(within(card).getByRole("button", { name: "Turn off password login" })).toBeDisabled();
  });

  it("hides the retry where no saved host uses the app key or in preview mode", async () => {
    const first = renderSettings({ installedHosts: [] });
    await waitFor(() => expect(useFleetStore.getState().hostsLoaded).toBe(true));
    expect(screen.queryByTestId("server-security")).not.toBeInTheDocument();
    first.unmount();

    renderSettings({ preview: true });
    expect(screen.queryByTestId("server-security")).not.toBeInTheDocument();
  });

  it("clears the password before awaiting and reports disabled honestly", async () => {
    let resolveInstall: ((outcome: OnboardingInstallOutcome) => void) | undefined;
    const installKey = vi.fn((input: OnboardingInstallInput) => {
      void input;
      return new Promise<OnboardingInstallOutcome>((resolve) => {
        resolveInstall = resolve;
      });
    });
    const user = userEvent.setup();
    renderSettings({ installKey });

    const card = await screen.findByTestId("server-security");
    const password = within(card).getByLabelText("One-time server password");
    await user.type(password, ONE_TIME_PASSWORD);
    await user.click(within(card).getByRole("button", { name: "Turn off password login" }));

    expect(await screen.findByText(/Re-checking the key/)).toBeInTheDocument();
    expect(password).toHaveValue("");
    expect(document.body.textContent).not.toContain(ONE_TIME_PASSWORD);
    expect(document.body.innerHTML).not.toContain(ONE_TIME_PASSWORD);
    expect(installKey).toHaveBeenCalledTimes(1);

    const input = installKey.mock.calls[0]?.[0];
    expect(input?.password).toBe(ONE_TIME_PASSWORD);
    expect(input?.disable_password_auth).toBe(true);
    expect(input?.draft.ssh).toEqual(APP_HOST.ssh);
    expect(input?.draft.identity_file).toBeNull();
    expect(input?.host_key.sha256).toBe(APP_HOST.host_key.sha256);

    await act(async () => {
      resolveInstall?.(installOutcome({ state: "disabled" }));
    });

    expect(await screen.findByTestId("password-auth-outcome")).toHaveTextContent(
      /Password login is now off/,
    );
    expect(screen.queryByText(/Re-checking the key/)).not.toBeInTheDocument();
    expect(document.body.textContent).not.toContain(ONE_TIME_PASSWORD);
  });

  const outcomes: Array<{ auth: PasswordAuthOutcome; copy: RegExp }> = [
    {
      auth: { state: "not_supported", reason: "no_passwordless_sudo" },
      copy: /Password login is still on: this account has no passwordless sudo.*\(no_passwordless_sudo\)/,
    },
    {
      auth: { state: "failed", detail: "the drop-in was removed again" },
      copy: /Password login is still on: the change failed \(the drop-in was removed again\)/,
    },
  ];

  it.each(outcomes)("reports a $auth.state password-auth outcome honestly", async ({ auth, copy }) => {
    const user = userEvent.setup();
    renderSettings({ installKey: async () => installOutcome(auth) });

    const card = await screen.findByTestId("server-security");
    await user.type(within(card).getByLabelText("One-time server password"), ONE_TIME_PASSWORD);
    await user.click(within(card).getByRole("button", { name: "Turn off password login" }));

    expect(await screen.findByTestId("password-auth-outcome")).toHaveTextContent(copy);
    expect(within(card).getByLabelText("One-time server password")).toHaveValue("");
    expect(document.body.textContent).not.toContain(ONE_TIME_PASSWORD);
  });

  it("reports an install error without keeping the password", async () => {
    const user = userEvent.setup();
    renderSettings({
      installKey: async () => {
        throw {
          code: "ssh_auth",
          message: "SSH authentication failed",
          hint: "Check the one-time password from your provider.",
          retryable: true,
        };
      },
    });

    const card = await screen.findByTestId("server-security");
    await user.type(within(card).getByLabelText("One-time server password"), ONE_TIME_PASSWORD);
    await user.click(within(card).getByRole("button", { name: "Turn off password login" }));

    const alert = await screen.findByTestId("password-auth-error");
    expect(alert).toHaveTextContent("SSH authentication failed");
    expect(alert).toHaveTextContent("The one-time password was cleared and is not stored.");
    expect(within(card).getByLabelText("One-time server password")).toHaveValue("");
    expect(document.body.textContent).not.toContain(ONE_TIME_PASSWORD);
    expect(screen.queryByTestId("password-auth-outcome")).not.toBeInTheDocument();
  });
});
