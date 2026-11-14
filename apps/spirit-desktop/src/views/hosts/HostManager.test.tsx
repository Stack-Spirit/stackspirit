import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { renderHostManager } from "../../test/render";

async function openWizard(scenario: "empty" | "probe-failure" = "empty") {
  await renderHostManager(scenario);
  const user = userEvent.setup();
  const addButtons = screen.getAllByRole("button", { name: "Add your VPS" });
  await user.click(addButtons[0] as HTMLElement);
  const dialog = await screen.findByRole("dialog", { name: "Add your VPS" });
  return { user, dialog };
}

async function fillAddress(
  user: ReturnType<typeof userEvent.setup>,
  dialog: HTMLElement,
  values = { label: "test-vps", host: "203.0.113.20", user: "owner" },
) {
  const label = within(dialog).getByLabelText(/^Label/);
  const host = within(dialog).getByLabelText(/^Host/);
  const sshUser = within(dialog).getByLabelText(/^User/);
  await user.type(label, values.label);
  await user.type(host, values.host);
  await user.clear(sshUser);
  await user.type(sshUser, values.user);
}

async function confirmFingerprint(user: ReturnType<typeof userEvent.setup>, dialog: HTMLElement) {
  const checkbox = await within(dialog).findByRole("checkbox", {
    name: /I verified this fingerprint/,
  });
  const trust = within(dialog).getByRole("button", { name: "Trust and continue" });
  expect(trust).toBeDisabled();
  await user.click(checkbox);
  await waitFor(() => expect(trust).toBeEnabled());
  await user.click(trust);
}

describe("Host Manager", () => {
  it("requires label, host, user and a valid port before continuing", async () => {
    const { user, dialog } = await openWizard();
    const sshUser = within(dialog).getByLabelText(/^User/);
    await user.clear(sshUser);
    await user.click(within(dialog).getByRole("button", { name: "Next: verify host key" }));
    expect(await within(dialog).findByText("Label must be 1–64 characters.")).toBeInTheDocument();
    expect(within(dialog).getByText("Enter a DNS name or IP address.")).toBeInTheDocument();
    expect(
      within(dialog).getByText("Enter the SSH user."),
    ).toBeInTheDocument();

    const port = within(dialog).getByLabelText(/^Port/) as HTMLInputElement;
    await user.clear(port);
    await user.type(port, "70000");
    await user.click(within(dialog).getByRole("button", { name: "Next: verify host key" }));
    expect(
      await within(dialog).findByText("Port must be between 1 and 65535."),
    ).toBeInTheDocument();
  });

  it("requires an explicit fingerprint confirmation before trust", async () => {
    const { user, dialog } = await openWizard();
    await fillAddress(user, dialog);
    await user.click(within(dialog).getByRole("button", { name: "Next: verify host key" }));
    const checkbox = await within(dialog).findByRole("checkbox", {
      name: /I verified this fingerprint/,
    });
    const trust = within(dialog).getByRole("button", { name: "Trust and continue" });
    expect(checkbox).not.toBeChecked();
    expect(trust).toBeDisabled();
    await user.click(checkbox);
    await waitFor(() => expect(trust).toBeEnabled());
  });

  it("shows the failing probe stage and refuses to save", async () => {
    const { user, dialog } = await openWizard("probe-failure");
    await fillAddress(user, dialog);
    await user.click(within(dialog).getByRole("button", { name: "Next: verify host key" }));
    await confirmFingerprint(user, dialog);
    const sshStage = await within(dialog).findByTestId("probe-stage-ssh");
    expect(sshStage).toHaveTextContent("Failed");
    expect(sshStage).toHaveTextContent("Permission denied");
    expect(within(dialog).getByRole("button", { name: "Save host" })).toBeDisabled();
  });

  it("saves a probed host and shows it in the list", async () => {
    const { user, dialog } = await openWizard();
    await fillAddress(user, dialog);
    await user.click(within(dialog).getByRole("button", { name: "Next: verify host key" }));
    await confirmFingerprint(user, dialog);
    const save = within(dialog).getByRole("button", { name: "Save host" });
    await waitFor(() => expect(save).toBeEnabled());
    await user.click(save);
    expect(await screen.findByText("test-vps")).toBeInTheDocument();
    expect(screen.queryByRole("dialog", { name: "Add your VPS" })).not.toBeInTheDocument();
  });

  it("connects then disconnects a saved host", async () => {
    const { bridge } = await renderHostManager("fresh");
    const user = userEvent.setup();
    const connect = await screen.findByRole("button", { name: "Connect to staging-vps" });
    await user.click(connect);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Disconnect" })).toBeInTheDocument(),
    );
    expect((await bridge.connection.status()).state).toBe("connected");

    await user.click(screen.getByRole("button", { name: "Disconnect" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Connect to staging-vps" })).toBeInTheDocument(),
    );
    expect((await bridge.connection.status()).state).toBe("disconnected");
  });

  it("opens the host-key mismatch dialog with both fingerprints", async () => {
    await renderHostManager("host-key-mismatch");
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Connect to staging-vps" }));
    const dialog = await screen.findByRole("dialog", { name: "Host key changed" });
    expect(within(dialog).getByTestId("old-fingerprint")).toHaveTextContent("SHA256:oldPin");
    expect(within(dialog).getByTestId("new-fingerprint")).toHaveTextContent("SHA256:newPin");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "Host key changed" })).not.toBeInTheDocument();
  });

  it("shows the empty state with the three prerequisite checks", async () => {
    await renderHostManager("empty");
    expect(screen.getAllByText("Add your VPS").length).toBeGreaterThan(0);
    const checks = screen.getByRole("list", { name: "What you need" });
    expect(within(checks).getByText(/SSH key or agent/)).toBeInTheDocument();
    expect(within(checks).getByText(/VOWP on the host/)).toBeInTheDocument();
    expect(within(checks).getByText(/Host key verified/)).toBeInTheDocument();
  });
});
