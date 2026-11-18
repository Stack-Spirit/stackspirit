import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { SpiritBridge } from "../../bridge/SpiritBridge";
import type {
  AppKeyView,
  HostKeyPin,
  HostKeyScan,
  OnboardingInstallInput,
  OnboardingInstallOutcome,
  PasswordAuthOutcome,
} from "../../bridge/wire";
import { ChatView } from "../../next/ChatView";
import { useConversations } from "../../next/conversations";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";

const HOST = "203.0.113.50";
const ONE_TIME_PASSWORD = "one-time-password-7f3a";
const FINGERPRINT = "SHA256:onboardingCardFingerprint0000000000000000000";
const OTHER_FINGERPRINT = "SHA256:onboardingCardEcdsa0000000000000000000000";
const APP_KEY_FINGERPRINT = "SHA256:onboardingCardAppKey000000000000000000000";
const SCANNED_AT = "2026-11-18T09:00:00.000Z";

function scanResult(scannedAt = SCANNED_AT): HostKeyScan {
  return {
    keys: [
      { algorithm: "ssh-ed25519", sha256: FINGERPRINT, preferred: true },
      { algorithm: "ecdsa-sha2-nistp256", sha256: OTHER_FINGERPRINT, preferred: false },
    ],
    scanned_at: scannedAt,
  };
}

function appKey(): AppKeyView {
  return {
    exists: true,
    fingerprint: FINGERPRINT,
    public_key: "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITestOnly onboarding-test",
  };
}

function outcomeFor(
  passwordAuth: PasswordAuthOutcome,
  installed = true,
): (input: OnboardingInstallInput) => OnboardingInstallOutcome {
  return (input) => ({
    installed,
    fingerprint: APP_KEY_FINGERPRINT,
    password_login_disabled: passwordAuth.state === "disabled",
    password_auth: passwordAuth,
    profile: {
      ...input.draft,
      id: "host-onboarding-card",
      host_key: input.host_key,
      created_at: SCANNED_AT,
      updated_at: SCANNED_AT,
      last_connected_at: null,
    },
  });
}

interface FakeBridgeOptions {
  scan?: () => Promise<HostKeyScan>;
  createKey?: () => Promise<AppKeyView>;
  install?: (input: OnboardingInstallInput) => Promise<OnboardingInstallOutcome>;
}

/**
 * Only the parts ChatView calls are implemented; the cast keeps the test focused on
 * the secure card instead of re-stubbing the whole bridge.
 */
function fakeBridge(options: FakeBridgeOptions = {}): SpiritBridge {
  return {
    kind: "mock",
    preview: true,
    hosts: {
      scanFingerprint: options.scan ?? (async () => scanResult()),
    },
    onboarding: {
      keyStatus: async () => appKey(),
      createKey: options.createKey ?? (async () => appKey()),
      installKey:
        options.install ??
        (async (input: OnboardingInstallInput) => outcomeFor({ state: "disabled" })(input)),
    },
  } as unknown as SpiritBridge;
}

function renderCard(bridge: SpiritBridge) {
  resetFleetStore();
  useFleetStore.setState({ bridge, preview: true, status: "ready" });
  useConversations.setState({
    messages: {
      setup: [
        {
          id: "m-card",
          role: "assistant",
          text: ["Let's connect it."],
          card: { kind: "secure-server" },
        },
      ],
    },
    location: {},
  });
  return render(<ChatView session="setup" serverLabel={null} />);
}

async function fillForm(user: ReturnType<typeof userEvent.setup>) {
  const form = screen.getByRole("form", { name: "Secure server details" });
  await user.type(within(form).getByLabelText("Server address"), HOST);
  await user.type(within(form).getByLabelText("One-time password from your provider"), ONE_TIME_PASSWORD);
  return form;
}

async function scan(user: ReturnType<typeof userEvent.setup>) {
  const form = await fillForm(user);
  await user.click(within(form).getByRole("button", { name: "Connect securely" }));
  const installButton = await screen.findByRole("button", { name: "Install key" });
  return { form, installButton };
}

async function confirmAndInstall(user: ReturnType<typeof userEvent.setup>) {
  const { installButton } = await scan(user);
  await user.click(screen.getByRole("checkbox", { name: /I verified this fingerprint/ }));
  await user.click(installButton);
}

afterEach(() => {
  useConversations.setState({ messages: {}, location: {} });
});

describe("secure server onboarding card", () => {
  it("shows a typed scan failure and retries without installing anything", async () => {
    const user = userEvent.setup();
    const scanFingerprint = vi
      .fn<() => Promise<HostKeyScan>>()
      .mockRejectedValueOnce({
        code: "ssh_unavailable",
        message: "ssh-keyscan is not available",
        hint: "Install OpenSSH and try again.",
        retryable: true,
      })
      .mockResolvedValueOnce(scanResult());
    const install = vi.fn(async (input: OnboardingInstallInput) =>
      outcomeFor({ state: "disabled" })(input),
    );
    renderCard(fakeBridge({ scan: scanFingerprint, install }));

    const form = await fillForm(user);
    await user.click(within(form).getByRole("button", { name: "Connect securely" }));

    const alert = await screen.findByTestId("secure-error");
    expect(alert).toHaveTextContent("ssh-keyscan is not available");
    expect(alert).toHaveTextContent("Install OpenSSH and try again.");
    expect(install).not.toHaveBeenCalled();
    expect(screen.queryByTestId("secure-received")).not.toBeInTheDocument();

    await user.click(within(alert).getByRole("button", { name: "Retry scan" }));
    await screen.findByText(new RegExp(`Server details received for ${HOST}`));
    expect(scanFingerprint).toHaveBeenCalledTimes(2);
    expect(screen.getByText(FINGERPRINT)).toBeInTheDocument();
    expect(install).not.toHaveBeenCalled();
  });

  it("requires the fingerprint confirmation before calling createKey or installKey", async () => {
    const user = userEvent.setup();
    const calls: string[] = [];
    const createKey = vi.fn(async () => {
      calls.push("createKey");
      return appKey();
    });
    const install = vi.fn(async (input: OnboardingInstallInput) => {
      calls.push("install");
      return outcomeFor({ state: "disabled" })(input);
    });
    renderCard(fakeBridge({ createKey, install }));

    const { installButton } = await scan(user);
    expect(screen.getByText(FINGERPRINT)).toBeInTheDocument();
    expect(screen.getByText(OTHER_FINGERPRINT)).toBeInTheDocument();
    expect(installButton).toBeDisabled();

    await user.click(installButton);
    expect(install).not.toHaveBeenCalled();
    expect(calls).toEqual([]);

    const confirm = screen.getByRole("checkbox", { name: /I verified this fingerprint/ });
    expect(confirm).not.toBeChecked();
    await user.click(confirm);
    expect(installButton).toBeEnabled();
    await user.click(installButton);

    await waitFor(() => expect(install).toHaveBeenCalledTimes(1));
    expect(calls).toEqual(["createKey", "install"]);
    const input = install.mock.calls[0]?.[0];
    expect(input?.host_key).toMatchObject({
      algorithm: "ssh-ed25519",
      sha256: FINGERPRINT,
    });
    expect(input?.draft.ssh).toEqual({ user: "root", host: HOST, port: 22 });
    expect(input?.disable_password_auth).toBe(true);
    expect(input?.password).toBe(ONE_TIME_PASSWORD);
  });

  it("clears the password before a failed install and leaves the card usable for a retry", async () => {
    const user = userEvent.setup();
    let rejectInstall: ((reason: unknown) => void) | undefined;
    const install = vi.fn(
      () =>
        new Promise<OnboardingInstallOutcome>((_resolve, reject) => {
          rejectInstall = reject;
        }),
    );
    renderCard(fakeBridge({ install }));

    await confirmAndInstall(user);
    expect(await screen.findByText(/Installing the StackSpirit key/)).toBeInTheDocument();

    // The secret is already out of React state and the rendered form.
    expect(screen.queryByLabelText("One-time password from your provider")).not.toBeInTheDocument();
    expect(document.body.textContent).not.toContain(ONE_TIME_PASSWORD);
    expect(document.body.innerHTML).not.toContain(ONE_TIME_PASSWORD);

    expect(rejectInstall).toBeTypeOf("function");
    await act(async () => {
      rejectInstall?.({
        code: "ssh_auth",
        message: "SSH authentication failed",
        hint: "Check the one-time password from your provider.",
        retryable: true,
      });
    });

    const alert = await screen.findByTestId("secure-error");
    expect(alert).toHaveTextContent("SSH authentication failed");
    expect(alert).toHaveTextContent("Check the one-time password from your provider.");
    expect(alert).not.toHaveTextContent(ONE_TIME_PASSWORD);
    expect(document.body.textContent).not.toContain(ONE_TIME_PASSWORD);
    expect(document.body.innerHTML).not.toContain(ONE_TIME_PASSWORD);
    expect(screen.queryByTestId("secure-received")).not.toBeInTheDocument();
    expect(
      useConversations
        .getState()
        .messages.setup?.some((message) => message.card?.kind === "secure-received"),
    ).toBe(false);

    const retryForm = screen.getByRole("form", { name: "Secure server details" });
    expect(within(retryForm).getByLabelText("Server address")).toHaveValue(HOST);
    const passwordInput = within(retryForm).getByLabelText("One-time password from your provider");
    expect(passwordInput).toHaveValue("");

    await user.click(within(alert).getByRole("button", { name: "Enter the password again" }));
    expect(screen.queryByTestId("secure-error")).not.toBeInTheDocument();
    expect(passwordInput).toHaveFocus();

    await user.type(passwordInput, "second-one-time-password");
    expect(
      within(retryForm).getByRole("button", { name: "Connect securely" }),
    ).toBeEnabled();
  });

  it("reuses the confirmed pin when a failed install is retried", async () => {
    const user = userEvent.setup();
    const retryScannedAt = "2026-11-18T09:05:00.000Z";
    const scanFingerprint = vi
      .fn<() => Promise<HostKeyScan>>()
      .mockResolvedValueOnce(scanResult())
      .mockResolvedValueOnce(scanResult(retryScannedAt));
    const pins: HostKeyPin[] = [];
    let attempt = 0;
    const install = vi.fn(async (input: OnboardingInstallInput) => {
      pins.push(input.host_key);
      attempt += 1;
      if (attempt === 1) {
        throw {
          code: "ssh_auth",
          message: "SSH authentication failed",
          hint: null,
          retryable: true,
        };
      }
      return outcomeFor({ state: "disabled" })(input);
    });
    renderCard(fakeBridge({ scan: scanFingerprint, install }));

    await confirmAndInstall(user);
    await screen.findByTestId("secure-error");

    const form = screen.getByRole("form", { name: "Secure server details" });
    await user.type(
      within(form).getByLabelText("One-time password from your provider"),
      "second-one-time-password",
    );
    await user.click(within(form).getByRole("button", { name: "Connect securely" }));
    const retryInstall = await screen.findByRole("button", { name: "Install key" });
    await user.click(screen.getByRole("checkbox", { name: /I verified this fingerprint/ }));
    await user.click(retryInstall);

    expect(await screen.findByTestId("secure-received")).toHaveTextContent(FINGERPRINT);
    expect(pins).toHaveLength(2);
    expect(pins[0]?.sha256).toBe(FINGERPRINT);
    expect(pins[1]?.sha256).toBe(FINGERPRINT);
    expect(pins[0]?.pinned_at).toBe(SCANNED_AT);
    expect(pins[1]?.pinned_at).toBe(pins[0]?.pinned_at);
  });

  it("records the receipt only for installed true, with the host and verified fingerprint", async () => {
    const user = userEvent.setup();
    const install = vi.fn(async (input: OnboardingInstallInput) =>
      outcomeFor({ state: "disabled" })(input),
    );
    renderCard(fakeBridge({ install }));

    await confirmAndInstall(user);

    const receipt = await screen.findByTestId("secure-received");
    expect(receipt).toHaveTextContent(`Key installed for ${HOST}`);
    expect(receipt).toHaveTextContent(FINGERPRINT);
    expect(receipt).not.toHaveTextContent(APP_KEY_FINGERPRINT);
    expect(receipt).toHaveTextContent(/key-only login/);
    expect(screen.getByText(/Password login is now turned off/)).toBeInTheDocument();
    expect(screen.queryByRole("form", { name: "Secure server details" })).not.toBeInTheDocument();

    const stored = JSON.stringify(useConversations.getState().messages);
    expect(stored).toContain(HOST);
    expect(stored).toContain(FINGERPRINT);
    expect(stored).not.toContain(APP_KEY_FINGERPRINT);
    expect(stored).not.toContain(ONE_TIME_PASSWORD);
    expect(stored).not.toContain("root");
  });

  it("refuses to record secure-received when the host reports installed false", async () => {
    const user = userEvent.setup();
    const install = vi.fn(async (input: OnboardingInstallInput) =>
      outcomeFor({ state: "disabled" }, false)(input),
    );
    renderCard(fakeBridge({ install }));

    await confirmAndInstall(user);

    const alert = await screen.findByTestId("secure-error");
    expect(alert).toHaveTextContent(/fresh key-only login did not verify/);
    expect(screen.queryByTestId("secure-received")).not.toBeInTheDocument();
    expect(screen.getByRole("form", { name: "Secure server details" })).toBeInTheDocument();
    expect(screen.getByLabelText("One-time password from your provider")).toHaveValue("");
  });

  const authVariants: Array<{
    name: string;
    auth: PasswordAuthOutcome;
    copy: RegExp;
  }> = [
    {
      name: "disabled",
      auth: { state: "disabled" },
      copy: /Password login is now turned off; the StackSpirit key has a verified key-only login\./,
    },
    {
      name: "not_requested",
      auth: { state: "not_requested" },
      copy: /Password login is still on because you asked to keep it\./,
    },
    {
      name: "not_supported",
      auth: { state: "not_supported", reason: "no_passwordless_sudo" },
      copy: /no passwordless sudo.*\(no_passwordless_sudo\)/,
    },
    {
      name: "failed",
      auth: { state: "failed", detail: "the drop-in was removed again" },
      copy: /the change failed \(the drop-in was removed again\)/,
    },
  ];

  it.each(authVariants)("reports the $name password-auth outcome honestly", async ({ auth, copy }) => {
    const user = userEvent.setup();
    const install = vi.fn(async (input: OnboardingInstallInput) => outcomeFor(auth)(input));
    renderCard(fakeBridge({ install }));

    await confirmAndInstall(user);

    expect(await screen.findByTestId("secure-received")).toHaveTextContent(FINGERPRINT);
    expect(screen.getByText(copy)).toBeInTheDocument();
    expect(document.body.textContent).not.toContain(ONE_TIME_PASSWORD);
  });
});
