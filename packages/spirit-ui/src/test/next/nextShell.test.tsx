import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { SpiritBridge } from "../../bridge/SpiritBridge";
import type { HostProfile } from "../../bridge/wire";
import { MockBridge, type MockScenario } from "../../bridge/MockBridge";
import { resetConversations, useConversations } from "../../next/conversations";
import { NextShell } from "../../next/NextShell";
import { resetShellUi, useShellUi } from "../../next/shellUi";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";

const bridges: SpiritBridge[] = [];

async function renderWithBridge(bridge: SpiritBridge) {
  resetFleetStore();
  resetConversations();
  bridges.push(bridge);
  const result = render(<NextShell bridge={bridge} />);
  await waitFor(() => expect(useFleetStore.getState().hostsLoaded).toBe(true));
  return { bridge, ...result };
}

async function renderNext(scenario: MockScenario) {
  return renderWithBridge(new MockBridge(scenario));
}

async function renderConnected(scenario: MockScenario = "busy") {
  const user = userEvent.setup();
  const rendered = await renderNext(scenario);
  await user.click(screen.getByTestId("connect-prompt-connect"));
  await waitFor(() => expect(useFleetStore.getState().connection.state).toBe("connected"));
  return { user, ...rendered };
}

function savedHost(): HostProfile {
  return {
    id: "host-saved",
    label: "staging-vps",
    ssh: { user: "owner", host: "203.0.113.10", port: 2222 },
    identity_file: "/home/owner/.ssh/id_ed25519",
    vowp_home: null,
    default_project: null,
    host_key: {
      algorithm: "ssh-ed25519",
      sha256: "SHA256:nextShellSavedHost000000000000000000000000",
      pinned_at: "2026-11-18T09:00:00.000Z",
    },
    created_at: "2026-11-18T09:00:00.000Z",
    updated_at: "2026-11-18T09:00:00.000Z",
    last_connected_at: null,
  };
}

afterEach(() => {
  for (const bridge of bridges.splice(0)) bridge.dispose?.();
  resetConversations();
  resetShellUi();
  resetFleetStore();
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

  it("opens a calm empty hero and sends setup chat through bridge.chat", async () => {
    const user = userEvent.setup();
    const { bridge } = await renderNext("empty");
    await user.click(screen.getByRole("button", { name: /Set up with AI/ }));

    expect(screen.getByTestId("session-location")).toHaveTextContent("Local");
    expect(screen.getByRole("heading", { name: "StackSpirit", level: 2 })).toBeInTheDocument();
    // No password wall: the secure card waits for an explicit Connect server.
    expect(screen.queryByRole("form", { name: "Secure server details" })).not.toBeInTheDocument();
    expect(screen.queryByLabelText("One-time password from your provider")).not.toBeInTheDocument();

    const send = vi.spyOn(bridge.chat!, "send");
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "I have a new VPS" })).toBeEnabled(),
    );

    await user.click(screen.getByRole("button", { name: "I have a new VPS" }));
    await waitFor(() =>
      expect(send).toHaveBeenCalledWith({ session: "setup", text: "I have a new VPS" }),
    );
    expect(await screen.findByText(/Preview reply\./)).toBeInTheDocument();
    expect(screen.getByTestId("preview-tag")).toHaveTextContent("Preview");

    // The secure card is still app-owned, not a model reply, and only appears on ask.
    await user.click(screen.getByRole("button", { name: /Connect server/ }));
    expect(
      await screen.findByRole("form", { name: "Secure server details" }),
    ).toBeInTheDocument();
    const setupMessages = useConversations.getState().messages.setup ?? [];
    expect(setupMessages[0]?.card).toEqual({ kind: "secure-server" });
    expect(setupMessages[0]?.origin).toBe("app");
    expect(screen.queryByTestId("saved-server-card")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add another server" })).not.toBeInTheDocument();
  });

  it("chats from a saved offline server without connecting or showing the password card", async () => {
    const user = userEvent.setup();
    const bridge = new MockBridge({ scenario: "busy", hosts: [savedHost()] });
    await renderWithBridge(bridge);
    const connect = vi.spyOn(bridge.connection, "connect");

    expect(
      screen.getByRole("button", { name: "Chat with the setup assistant" }),
    ).toBeInTheDocument();
    await user.click(screen.getByTestId("connect-prompt-chat"));

    expect(
      await screen.findByRole("heading", { name: "Setup assistant", level: 1 }),
    ).toBeInTheDocument();
    expect(useFleetStore.getState().connection.state).toBe("disconnected");
    expect(connect).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(screen.getByTestId("session-location")).toHaveTextContent("Local · this Mac"),
    );

    const card = screen.getByTestId("saved-server-card");
    expect(card).toHaveTextContent("staging-vps");
    expect(card).toHaveTextContent("203.0.113.10");
    expect(card).toHaveTextContent("owner");
    expect(card).toHaveTextContent("2222");
    expect(card).toHaveTextContent("id_ed25519");
    expect(card).not.toHaveTextContent("/home/owner");
    expect(screen.queryByRole("form", { name: "Secure server details" })).not.toBeInTheDocument();
    expect(screen.queryByText("Secure server details")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Message" })).toBeEnabled();
  });

  it("opens the same saved-server setup chat from the sidebar without a password card", async () => {
    const user = userEvent.setup();
    const bridge = new MockBridge({ scenario: "busy", hosts: [savedHost()] });
    await renderWithBridge(bridge);
    const connect = vi.spyOn(bridge.connection, "connect");

    await user.click(
      within(screen.getByRole("navigation", { name: "Sessions" })).getByRole("button", {
        name: /Setup assistant/,
      }),
    );

    expect(
      await screen.findByRole("heading", { name: "Setup assistant", level: 1 }),
    ).toBeInTheDocument();
    expect(connect).not.toHaveBeenCalled();
    expect(screen.getByTestId("saved-server-card")).toHaveTextContent("staging-vps");
    expect(screen.queryByRole("form", { name: "Secure server details" })).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Message" })).toBeEnabled();
  });

  it("shows the new-server password card only when the owner asks to add a server", async () => {
    const user = userEvent.setup();
    await renderNext("busy");
    await user.click(screen.getByTestId("connect-prompt-chat"));
    expect(await screen.findByTestId("saved-server-card")).toBeInTheDocument();
    expect(screen.queryByRole("form", { name: "Secure server details" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Add another server" }));

    expect(
      await screen.findByRole("form", { name: "Secure server details" }),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("One-time password from your provider")).toBeInTheDocument();
    expect(screen.getByTestId("saved-server-card")).toBeInTheDocument();
  });

  it("keeps the honest engine error in a saved-server setup chat and never fakes a reply", async () => {
    const user = userEvent.setup();
    await renderNext("chat-engine-stopped");
    await user.click(screen.getByTestId("connect-prompt-chat"));

    const error = await screen.findByTestId("chat-error");
    expect(error).toHaveTextContent("The bundled StackSpirit Engine is stopped.");
    expect(error).toHaveTextContent("Settings → Engine");
    expect(screen.queryByText(/Preview reply\./)).not.toBeInTheDocument();
    expect(screen.queryByRole("form", { name: "Secure server details" })).not.toBeInTheDocument();
    expect(screen.getByTestId("saved-server-card")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Message" })).toBeDisabled();
  });

  it("keeps a typed password out of the conversation", async () => {
    const user = userEvent.setup();
    await renderNext("empty");
    await user.click(screen.getByRole("button", { name: /Set up with AI/ }));
    expect(screen.getByTestId("session-location")).toHaveTextContent("Local");
    expect(screen.queryByRole("form", { name: "Secure server details" })).not.toBeInTheDocument();

    await user.click(screen.getByTestId("composer-connect-server"));
    const form = await screen.findByRole("form", { name: "Secure server details" });
    await user.type(within(form).getByLabelText("Server address"), "203.0.113.50");
    await user.type(within(form).getByLabelText("One-time password from your provider"), "hunter2-secret");
    await user.click(within(form).getByRole("button", { name: "Connect securely" }));

    const stored = JSON.stringify(useConversations.getState().messages);
    expect(stored).toContain("203.0.113.50");
    expect(stored).not.toContain("hunter2-secret");
    expect(screen.queryByDisplayValue("hunter2-secret")).not.toBeInTheDocument();
    expect(screen.getByText(/Server details received for 203.0.113.50/)).toBeInTheDocument();
  });

  it("shows chat_unavailable for a real bridge with no chat and no preview reply", async () => {
    const base = new MockBridge("busy");
    const bridge = { ...base, preview: false } as SpiritBridge;
    delete (bridge as { chat?: unknown }).chat;

    const user = userEvent.setup();
    await renderWithBridge(bridge);
    await user.click(screen.getByTestId("connect-prompt-connect"));
    await waitFor(() => expect(useFleetStore.getState().connection.state).toBe("connected"));

    const error = await screen.findByTestId("chat-error");
    expect(error).toHaveTextContent("chat_unavailable");
    expect(error).toHaveTextContent("Chat is not available in this client.");
    expect(screen.queryByText(/Preview reply\./)).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Message" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "How busy is the server?" })).toBeDisabled();
  });

  it("never renders a preview reply when a real bridge returns one", async () => {
    // Keep the mock streaming chat surface but pretend it is a real transport. Preview
    // messages and deltas with `bridge.preview === false` are a contract violation; the
    // renderer must drop them (including their details-panel hint).
    const base = new MockBridge("chat-streaming");
    const bridge = { ...base, preview: false } as SpiritBridge;

    const user = userEvent.setup();
    await renderWithBridge(bridge);
    await user.click(screen.getByTestId("connect-prompt-connect"));
    await waitFor(() => expect(useFleetStore.getState().connection.state).toBe("connected"));

    await user.click(await screen.findByRole("button", { name: "How busy is the server?" }));
    // The preview reply arrives (and is dropped) before the turn completes. Wait for
    // the completed turn, then prove none of the preview text reached the DOM.
    await waitFor(() =>
      expect(screen.getByTestId("session-state")).toHaveTextContent("Ready"),
    );
    expect(screen.queryByText(/Preview reply\./)).not.toBeInTheDocument();
    expect(screen.queryByTestId("preview-tag")).not.toBeInTheDocument();
    expect(useShellUi.getState().panel).toBeNull();
  });

  it("opens the global orchestrator after Connect on Local, the only location that is live", async () => {
    await renderNext("busy");
    expect(screen.getByRole("heading", { name: "staging-vps is not connected" })).toBeInTheDocument();

    const user = userEvent.setup();
    await user.click(screen.getByTestId("connect-prompt-connect"));
    await waitFor(() => expect(useFleetStore.getState().connection.state).toBe("connected"));

    expect(await screen.findByRole("heading", { name: "Global orchestrator", level: 1 })).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByTestId("session-location")).toHaveTextContent("Local · this Mac"),
    );
    expect(screen.getByTestId("health-card")).toHaveTextContent("staging-vps");
  });

  it("uses the location the bridge reports and has no label-only switch", async () => {
    const base = new MockBridge("busy");
    if (base.chat) {
      const originalStart = base.chat.start;
      base.chat.start = async (input) => ({
        ...(await originalStart(input)),
        // Simulate VOWP reporting a local orchestrator for a global session.
        location: "local",
      });
    }

    const user = userEvent.setup();
    await renderWithBridge(base);
    await user.click(screen.getByTestId("connect-prompt-connect"));
    await waitFor(() => expect(useFleetStore.getState().connection.state).toBe("connected"));

    await waitFor(() =>
      expect(screen.getByTestId("session-location")).toHaveTextContent("Local · this Mac"),
    );
    expect(screen.queryByRole("button", { name: /Run on this Mac/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Run on server/ })).not.toBeInTheDocument();
  });

  it("disables the composer while streaming and sends Stop to bridge.chat", async () => {
    const { user, bridge } = await renderConnected("chat-slow");
    const composer = screen.getByRole("textbox", { name: "Message" });
    await user.type(composer, "hold the turn open");
    await user.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() => expect(screen.getByTestId("session-state")).toHaveTextContent("Replying…"));
    expect(composer).toBeDisabled();
    expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();

    const stop = vi.spyOn(bridge.chat!, "stop");
    await user.click(screen.getByRole("button", { name: "Stop" }));
    await waitFor(() => expect(stop).toHaveBeenCalledWith("global"));
    await waitFor(() => expect(screen.getByTestId("session-state")).toHaveTextContent("Ready"));
    expect(composer).toBeEnabled();
    expect(screen.queryByRole("button", { name: "Start" })).not.toBeInTheDocument();
  });

  it("shows a typed chat error and retries through Reconnect", async () => {
    const { user } = await renderConnected("chat-error");
    await user.type(screen.getByRole("textbox", { name: "Message" }), "hello");
    await user.click(screen.getByRole("button", { name: "Send" }));

    const error = await screen.findByTestId("chat-error");
    expect(error).toHaveTextContent("The preview orchestrator reported a safe error.");
    expect(within(error).getByRole("button", { name: "Reconnect" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Message" })).toBeDisabled();

    await user.click(within(error).getByRole("button", { name: "Reconnect" }));
    await waitFor(() => expect(screen.queryByTestId("chat-error")).not.toBeInTheDocument());
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "Message" })).toBeEnabled(),
    );
  });

  it("shows the exact VOWP gate for server chat and never a fake reply", async () => {
    await renderConnected("chat-server-gated");
    const error = await screen.findByTestId("chat-error");
    expect(error).toHaveTextContent(
      "Server chat needs VOWP 0.5's `vowp orchestrator endpoint`.",
    );
    expect(error).toHaveAttribute("data-gate", "true");
    expect(within(error).queryByRole("button", { name: "Reconnect" })).not.toBeInTheDocument();
    expect(screen.queryByText(/Preview reply\./)).not.toBeInTheDocument();
    expect(screen.getByTestId("session-location")).toHaveTextContent("Not connected");
    expect(screen.getByRole("textbox", { name: "Message" })).toBeDisabled();
    // The user can still open the secure card in setup; this is the global path.
    expect(await screen.findByRole("button", { name: "How busy is the server?" })).toBeDisabled();
  });

  it("offers a one-click Start for a project with no local orchestrator, never Connect a provider", async () => {
    const { user } = await renderConnected("chat-project-not-configured");
    const sidebar = screen.getByRole("navigation", { name: "Sessions" });
    await user.click(within(sidebar).getByRole("button", { name: /^vowp/ }));
    await user.click(within(sidebar).getByRole("button", { name: /^Orchestrator/ }));

    const card = await screen.findByTestId("start-orchestrator");
    expect(card).toHaveTextContent("doesn’t have a local orchestrator running on this Mac yet");
    expect(screen.queryByText("A capability gate is not ready")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Connect a provider" })).not.toBeInTheDocument();

    await user.click(within(card).getByRole("button", { name: "Create & Start vowp orchestrator" }));
    await waitFor(() => expect(screen.queryByTestId("start-orchestrator")).not.toBeInTheDocument());
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "Message" })).toBeEnabled(),
    );
    expect(screen.queryByTestId("chat-error")).not.toBeInTheDocument();
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

  it("opens the details panel when a preview reply points at something", async () => {
    const { user } = await renderConnected();
    await user.click(screen.getByRole("button", { name: "How busy is the server?" }));
    await waitFor(() => expect(useShellUi.getState().panel).toEqual({ kind: "server" }));
    expect(within(screen.getByTestId("details-panel")).getByText(/workers active/)).toBeInTheDocument();
    expect(await screen.findByText(/Preview reply\./)).toBeInTheDocument();
    expect(screen.getByTestId("preview-tag")).toBeInTheDocument();
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

  async function openAddProject(user: ReturnType<typeof userEvent.setup>) {
    await user.click(screen.getByRole("button", { name: "Add project" }));
    return screen.findByRole("dialog", { name: "Add a project" });
  }

  it("has no branch field and says VOWP clones the default branch", async () => {
    const { user } = await renderConnected();
    const dialog = await openAddProject(user);
    expect(within(dialog).queryByLabelText(/branch/i)).not.toBeInTheDocument();
    expect(within(dialog).getByText("VOWP clones the repository's default branch.")).toBeInTheDocument();
  });

  it("disables the Local choice when no local agent is connected", async () => {
    const { user } = await renderConnected("busy");
    const dialog = await openAddProject(user);
    const box = within(dialog).getByRole("checkbox", { name: /Create the chat on this Mac/ });
    expect(box).toBeDisabled();
    expect(box).not.toBeChecked();
    expect(within(dialog).getByText(/No local agent is connected/)).toBeInTheDocument();
  });

  it("checks Local by default, creates the project, moves the chat and opens it", async () => {
    const { user, bridge } = await renderConnected("owner-project");
    const dialog = await openAddProject(user);
    const box = within(dialog).getByRole("checkbox", { name: /Create the chat on this Mac/ });
    expect(box).toBeChecked();
    expect(box).toBeEnabled();

    await user.type(within(dialog).getByLabelText("Repository URL"), "https://github.com/acme/billing-api.git");
    await user.click(within(dialog).getByRole("button", { name: "Create project" }));

    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Add a project" })).not.toBeInTheDocument());
    const mock = bridge as MockBridge;
    expect(mock.recordedProjectAdds()).toEqual([
      { name: "billing-api", source: { kind: "git", url: "https://github.com/acme/billing-api.git" }, init: true, orchestrator: "chat" },
    ]);
    expect(mock.orchestratorMoves).toEqual([
      { name: "billing-api-chat", target: { kind: "node", node: expect.any(String) } },
    ]);
    const fleet = await bridge.fleet.get();
    expect(fleet?.projects.map((project) => project.name)).toContain("billing-api");
    expect(fleet?.orchestrators.find((row) => row.name === "billing-api-chat")?.node).not.toBeNull();
    await waitFor(() => expect(screen.getAllByText(/billing-api/).length).toBeGreaterThan(0));
    expect(screen.queryByTestId("project-notice")).not.toBeInTheDocument();
  });

  it("keeps the dialog open and shows the message when the add fails", async () => {
    const { user, bridge } = await renderConnected("owner-project");
    const dialog = await openAddProject(user);
    await user.click(within(dialog).getByRole("button", { name: "Folder on the server" }));
    await user.type(within(dialog).getByLabelText(/Folder path on/), "~/code/relative-ok");
    await user.clear(within(dialog).getByLabelText("Project name"));
    await user.type(within(dialog).getByLabelText("Project name"), "folder-proj");
    vi.spyOn(bridge.projects!, "add").mockRejectedValueOnce({ code: "invalid_input", message: "project folder: use an absolute path" });
    await user.click(within(dialog).getByRole("button", { name: "Create project" }));

    expect(await within(dialog).findByRole("alert")).toHaveTextContent("project folder: use an absolute path");
    expect(screen.getByRole("dialog", { name: "Add a project" })).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Create project" })).toBeEnabled();
  });

  it("still opens the project and reports Home when the Local move fails", async () => {
    const { user, bridge } = await renderConnected("owner-project");
    vi.spyOn(bridge.localOrchestrators!, "orchestratorMove").mockRejectedValueOnce({ code: "gate", message: "agent offline" });
    const dialog = await openAddProject(user);
    await user.type(within(dialog).getByLabelText("Repository URL"), "https://github.com/acme/ledger.git");
    await user.click(within(dialog).getByRole("button", { name: "Create project" }));

    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Add a project" })).not.toBeInTheDocument());
    expect(await screen.findByTestId("project-notice")).toHaveTextContent(/ledger was created.*stayed on Home/);
    const fleet = await bridge.fleet.get();
    expect(fleet?.projects.map((project) => project.name)).toContain("ledger");
  });

  it("rejects a project name that already exists", async () => {
    const { user } = await renderConnected();
    const dialog = await openAddProject(user);
    await user.type(within(dialog).getByLabelText("Repository URL"), "https://github.com/acme/vowp.git");
    expect(within(dialog).getByText(/already exists/)).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Create project" })).toBeDisabled();
  });
});
