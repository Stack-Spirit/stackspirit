import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { CloudAccountPanel } from "../../next/CloudAccountPanel";
import {
  configureCloudRuntime,
  resetCloudAccount,
  useCloudAccount,
} from "../../cloud/store";
import { TestCloudClient, TestRelay } from "./fakes";

beforeEach(() => {
  resetCloudAccount();
});

afterEach(() => {
  resetCloudAccount();
});

describe("optional StackSpirit account panel", () => {
  it("shows account-free local mode with no sign-in button and no request", () => {
    render(<CloudAccountPanel />);
    expect(screen.getByTestId("cloud-local-default")).toHaveTextContent(/needs no account/i);
    expect(screen.getByTestId("cloud-local-default")).toHaveTextContent(
      /SSH private keys stay on this device/i,
    );
    expect(screen.queryByTestId("cloud-sign-in")).not.toBeInTheDocument();
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
    expect(screen.queryByText(/Sign in to StackSpirit again/i)).not.toBeInTheDocument();
  });

  it("offers an explicitly optional device sign-in without any password field", async () => {
    const client = new TestCloudClient();
    configureCloudRuntime({ client, relay: new TestRelay() });
    const user = userEvent.setup();
    render(<CloudAccountPanel />);

    expect(screen.getByTestId("cloud-local-default")).toHaveTextContent(
      /Signing in is optional/i,
    );
    await user.click(screen.getByTestId("cloud-sign-in"));
    const code = await screen.findByTestId("cloud-device-code");
    expect(code).toHaveTextContent("ABCD-EFGH");
    expect(screen.getByRole("link", { name: /sign-in page/i })).toHaveAttribute(
      "href",
      "https://stackspirit.io/device?code=ABCD-EFGH",
    );
    expect(document.querySelector('input[type="password"]')).toBeNull();
    expect(document.body.textContent).not.toMatch(/SSH private key field|passphrase/i);

    await user.click(screen.getByTestId("cloud-poll"));
    const account = await screen.findByTestId("cloud-account");
    expect(account).toHaveTextContent("owner@example.com");
    expect(screen.getByTestId("cloud-machine-status-mch_online")).toHaveTextContent("Online");
    expect(screen.getByTestId("cloud-machine-status-mch_offline")).toHaveTextContent("Offline");
    expect(screen.getByTestId("cloud-connect-mch_offline")).toBeDisabled();
    expect(client.pollCalls).toBe(1);
  });

  it("connects an online Home through the relay and shows the encrypted state", async () => {
    const client = new TestCloudClient();
    const relay = new TestRelay();
    configureCloudRuntime({ client, relay });
    const user = userEvent.setup();
    render(<CloudAccountPanel />);
    await user.click(screen.getByTestId("cloud-sign-in"));
    await user.click(await screen.findByTestId("cloud-poll"));
    await screen.findByTestId("cloud-account");

    await user.click(screen.getByTestId("cloud-connect-mch_online"));
    await waitFor(() => {
      expect(useCloudAccount.getState().relayStatus?.state).toBe("secure");
    });
    expect(relay.connectCalls).toEqual([
      { machineId: "mch_online", scopes: ["http.read"] },
    ]);
    expect(screen.getByTestId("cloud-relay-status")).toHaveTextContent(
      /encrypted end to end/i,
    );
    expect(screen.getByTestId("cloud-disconnect")).toBeInTheDocument();

    await user.click(screen.getByTestId("cloud-disconnect"));
    expect(relay.disconnectCalls).toBe(1);
  });

  it("shows a typed sign-in failure and keeps local mode available", async () => {
    const client = new TestCloudClient({
      startError: {
        code: "cloud_unreachable",
        message: "The StackSpirit API could not be reached.",
        hint: "Local mode needs no account.",
        retryable: true,
      },
    });
    configureCloudRuntime({ client, relay: null });
    const user = userEvent.setup();
    render(<CloudAccountPanel />);
    await user.click(screen.getByTestId("cloud-sign-in"));
    expect(await screen.findByTestId("cloud-error")).toHaveTextContent(
      "The StackSpirit API could not be reached.",
    );
    expect(screen.getByTestId("cloud-local-default")).toBeInTheDocument();
  });
});
