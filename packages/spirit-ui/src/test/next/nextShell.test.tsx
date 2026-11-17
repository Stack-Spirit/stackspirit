import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { MockBridge, type MockScenario } from "../../bridge/MockBridge";
import { useConversations } from "../../next/conversations";
import { NextShell } from "../../next/NextShell";
import { resetShellUi, useShellUi } from "../../next/shellUi";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";

async function renderNext(scenario: MockScenario) {
  resetFleetStore();
  const bridge = new MockBridge(scenario);
  const result = render(<NextShell bridge={bridge} />);
  await waitFor(() => expect(useFleetStore.getState().hostsLoaded).toBe(true));
  return { bridge, ...result };
}

async function renderConnected() {
  const user = userEvent.setup();
  const rendered = await renderNext("busy");
  await user.click(screen.getByTestId("connect-prompt-connect"));
  await waitFor(() => expect(useFleetStore.getState().connection.state).toBe("connected"));
  return { user, ...rendered };
}

afterEach(() => {
  useConversations.setState({ messages: {}, location: {} });
  resetShellUi();
  delete document.documentElement.dataset.ui;
  delete document.documentElement.dataset.theme;
});

describe("NextShell", () => {
  it("shows the full-page welcome when no server is saved", async () => {
    await renderNext("empty");
    expect(screen.getByTestId("next-welcome")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "No always-on server connected" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Add server/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Set up with AI/ })).toBeInTheDocument();
    expect(document.documentElement.dataset.ui).toBe("next");
  });

  it("keeps a typed password out of the conversation", async () => {
    const user = userEvent.setup();
    await renderNext("empty");
    await user.click(screen.getByRole("button", { name: /Set up with AI/ }));
    expect(screen.getByTestId("session-location")).toHaveTextContent("Local");

    await user.click(screen.getByRole("button", { name: "I have a new VPS" }));
    const form = screen.getByRole("form", { name: "Secure server details" });
    await user.type(within(form).getByLabelText("Server address"), "203.0.113.50");
    await user.type(within(form).getByLabelText("One-time password from your provider"), "hunter2-secret");
    await user.click(within(form).getByRole("button", { name: "Connect securely" }));

    const stored = JSON.stringify(useConversations.getState().messages);
    expect(stored).toContain("203.0.113.50");
    expect(stored).not.toContain("hunter2-secret");
    expect(screen.queryByDisplayValue("hunter2-secret")).not.toBeInTheDocument();
    expect(screen.getByText(/Server details received for 203.0.113.50/)).toBeInTheDocument();
  });

  it("offers one Connect action for a saved server and opens the global orchestrator", async () => {
    await renderNext("busy");
    expect(screen.getByRole("heading", { name: "staging-vps is not connected" })).toBeInTheDocument();

    const user = userEvent.setup();
    await user.click(screen.getByTestId("connect-prompt-connect"));
    await waitFor(() => expect(useFleetStore.getState().connection.state).toBe("connected"));

    expect(await screen.findByRole("heading", { name: "Global orchestrator", level: 1 })).toBeInTheDocument();
    expect(screen.getByTestId("session-location")).toHaveTextContent("Server · staging-vps");
    expect(screen.getByTestId("health-card")).toHaveTextContent("staging-vps");
  });

  it("keeps the details panel closed until it is toggled", async () => {
    const { user } = await renderConnected();
    const panel = screen.getByTestId("details-panel");
    expect(panel).toHaveAttribute("aria-hidden", "true");
    expect(within(panel).queryByText("Running now")).not.toBeInTheDocument();

    await user.click(screen.getByTestId("panel-toggle"));
    expect(panel).toHaveAttribute("aria-hidden", "false");
    expect(within(panel).getByText(/Running now/)).toBeInTheDocument();

    await user.click(within(panel).getByRole("button", { name: "Close details panel" }));
    expect(panel).toHaveAttribute("aria-hidden", "true");
  });

  it("opens the details panel when an agent reply points at something", async () => {
    const { user } = await renderConnected();
    await user.click(screen.getByRole("button", { name: "How busy is the server?" }));
    expect(useShellUi.getState().panel).toEqual({ kind: "server" });
    expect(within(screen.getByTestId("details-panel")).getByText(/workers active/)).toBeInTheDocument();
  });

  it("opens the full server page from the health card", async () => {
    const { user } = await renderConnected();
    await user.click(screen.getByTestId("health-card"));
    expect(screen.getByRole("heading", { name: "staging-vps", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Server structure" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Projects" })).toBeInTheDocument();
  });

  it("shows a project's workers as a full page and opens a job in the panel", async () => {
    const { user } = await renderConnected();
    const sidebar = screen.getByRole("navigation", { name: "Sessions" });
    await user.click(within(sidebar).getByRole("button", { name: /^vowp/ }));
    await user.click(within(sidebar).getByRole("button", { name: /^Workers/ }));

    expect(screen.getByRole("heading", { name: "vowp · Workers", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "vowp structure" })).toBeInTheDocument();

    const rows = screen.getByRole("region", { name: "Jobs" });
    const row = within(rows).getByRole("button", { name: /^ss-2-desktop-shell/ });
    await user.click(row);
    expect(row).toHaveAttribute("aria-pressed", "true");
    const panel = screen.getByTestId("details-panel");
    expect(panel).toHaveAttribute("aria-hidden", "false");
    expect(within(panel).getByRole("heading", { name: "Job" })).toBeInTheDocument();
    expect(within(panel).getByRole("heading", { name: "ss-2-desktop-shell" })).toBeInTheDocument();
  });

  it("switches a project session between the server and this Mac", async () => {
    const { user } = await renderConnected();
    const sidebar = screen.getByRole("navigation", { name: "Sessions" });
    await user.click(within(sidebar).getByRole("button", { name: /^vowp/ }));
    await user.click(within(sidebar).getByRole("button", { name: "Orchestrator" }));
    expect(screen.getByRole("heading", { name: "vowp", level: 1 })).toBeInTheDocument();
    expect(screen.getByTestId("session-location")).toHaveTextContent("Server");

    await user.click(screen.getByRole("button", { name: /Run on this Mac/ }));
    expect(screen.getByTestId("session-location")).toHaveTextContent("Local · this Mac");
    await user.click(screen.getByRole("button", { name: /Run on server/ }));
    expect(screen.getByTestId("session-location")).toHaveTextContent("Server · staging-vps");
  });

  it("lets a project collapse while you are inside it", async () => {
    const { user } = await renderConnected();
    const sidebar = screen.getByRole("navigation", { name: "Sessions" });
    const project = within(sidebar).getByRole("button", { name: /^vowp/ });
    await user.click(project);
    await user.click(within(sidebar).getByRole("button", { name: "Orchestrator" }));
    expect(project).toHaveAttribute("aria-expanded", "true");

    await user.click(project);
    expect(project).toHaveAttribute("aria-expanded", "false");
    expect(within(sidebar).queryByRole("button", { name: "Orchestrator" })).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "vowp", level: 1 })).toBeInTheDocument();
  });

  it("shows one load number and the active workers on the server card", async () => {
    await renderConnected();
    const card = screen.getByTestId("health-card");
    expect(card).toHaveTextContent(/Load\s*\d+%/);
    expect(card).toHaveTextContent(/4\/8 workers/);
    expect(card).toHaveTextContent(/GiB free/);
  });

  it("keeps settings to one line that opens three options", async () => {
    const { user } = await renderConnected();
    await user.click(screen.getByTestId("settings-trigger"));
    const menu = screen.getByRole("menu", { name: "Settings" });
    expect(within(menu).getByRole("menuitem", { name: "Add server" })).toBeInTheDocument();
    expect(within(menu).getByRole("group", { name: "Theme" })).toBeInTheDocument();
    await user.click(within(menu).getByRole("menuitemradio", { name: "Dark" }));
    expect(document.documentElement.dataset.theme).toBe("dark");

    await user.click(within(menu).getByRole("menuitem", { name: "All settings" }));
    expect(screen.getByRole("heading", { name: "Settings", level: 1 })).toBeInTheDocument();
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("adds a project from a Git repository through the global orchestrator", async () => {
    const { user } = await renderConnected();
    await user.click(screen.getByRole("button", { name: "Add project" }));
    const dialog = screen.getByRole("dialog", { name: "Add a project" });
    await user.type(within(dialog).getByLabelText("Repository URL"), "https://github.com/acme/billing-api.git");
    expect(within(dialog).getByLabelText("Project name")).toHaveValue("billing-api");

    await user.click(within(dialog).getByRole("button", { name: "Create project" }));
    expect(screen.queryByRole("dialog", { name: "Add a project" })).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Global orchestrator", level: 1 })).toBeInTheDocument();
    expect(screen.getByText(/Create the project "billing-api"/)).toBeInTheDocument();
  });

  it("rejects a project name that already exists", async () => {
    const { user } = await renderConnected();
    await user.click(screen.getByRole("button", { name: "Add project" }));
    const dialog = screen.getByRole("dialog", { name: "Add a project" });
    await user.type(within(dialog).getByLabelText("Repository URL"), "https://github.com/acme/vowp.git");
    expect(within(dialog).getByText(/already exists/)).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Create project" })).toBeDisabled();
  });
});
