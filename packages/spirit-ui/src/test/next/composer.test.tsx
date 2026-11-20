import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { SpiritBridge } from "../../bridge/SpiritBridge";
import type { ChatUsageView } from "../../bridge/wire";
import { MockBridge } from "../../bridge/MockBridge";
import { ContextMeter } from "../../next/ContextMeter";
import { resetConversations, useConversations } from "../../next/conversations";
import { NextShell } from "../../next/NextShell";
import { resetShellUi } from "../../next/shellUi";
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

async function openSetup(bridge: SpiritBridge = new MockBridge("empty")) {
  const user = userEvent.setup();
  const rendered = await renderWithBridge(bridge);
  await user.click(screen.getByRole("button", { name: /Set up with AI/ }));
  return { user, ...rendered };
}

afterEach(() => {
  for (const bridge of bridges.splice(0)) bridge.dispose?.();
  resetConversations();
  resetShellUi();
  resetFleetStore();
  delete document.documentElement.dataset.ui;
  delete document.documentElement.dataset.theme;
});

function usage(overrides: Partial<ChatUsageView> = {}): ChatUsageView {
  return {
    context_tokens_used: 23_600,
    context_limit: 128_000,
    input_tokens: 12_800,
    output_tokens: 2_400,
    cache_read_tokens: 8_400,
    turns: 3,
    cost_usd: 0,
    ...overrides,
  };
}

describe("Meadow composer", () => {
  it("shows the empty hero without a password wall and opens the secure card on demand", async () => {
    const { user } = await openSetup();

    expect(screen.getByRole("heading", { name: "StackSpirit", level: 2 })).toBeInTheDocument();
    expect(screen.queryByRole("form", { name: "Secure server details" })).not.toBeInTheDocument();
    expect(screen.queryByLabelText("One-time password from your provider")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Message" })).toBeEnabled();

    const model = await screen.findByTestId("composer-model");
    expect(model).toHaveTextContent("OpenCode free · DeepSeek V4.1 Flash Free");

    await user.click(screen.getByTestId("composer-connect-server"));
    expect(
      await screen.findByRole("form", { name: "Secure server details" }),
    ).toBeInTheDocument();
  });

  it("keeps the same composer node and focus when the hero moves to the docked layout", async () => {
    const { user } = await openSetup();
    const composer = screen.getByRole("textbox", { name: "Message" });
    await user.type(composer, "hold this composer{Enter}");

    expect(await screen.findByText(/Preview reply\./)).toBeInTheDocument();
    const docked = screen.getByRole("textbox", { name: "Message" });
    expect(docked).toBe(composer);
    await waitFor(() => expect(document.activeElement).toBe(docked));
    expect(screen.queryByRole("heading", { name: "StackSpirit", level: 2 })).not.toBeInTheDocument();
  });

  it("restores focus to the Message field when a slow turn returns to ready", async () => {
    const { user } = await openSetup(new MockBridge({ scenario: "chat-slow", hosts: [] }));
    const composer = screen.getByRole("textbox", { name: "Message" });
    await user.type(composer, "hold the turn open{Enter}");

    await waitFor(() =>
      expect(screen.getByTestId("session-state")).toHaveTextContent("Replying…"),
    );
    // A disabled control cannot keep focus in a browser (Chromium is covered by
    // the desktop e2e). The field must not remount or forget that it owned focus,
    // so it is returnable once the session is ready again.
    expect(composer).toBeDisabled();

    await user.click(screen.getByRole("button", { name: "Stop" }));
    await waitFor(() =>
      expect(screen.getByTestId("session-state")).toHaveTextContent("Ready"),
    );
    expect(composer).toBeEnabled();
    expect(screen.queryByRole("button", { name: "Start" })).not.toBeInTheDocument();
    await waitFor(() => expect(document.activeElement).toBe(composer));

    // Stop is a turn control: the same composer accepts the next message right away.
    await user.type(composer, "continue with another model{Enter}");
    expect(await screen.findByText(/Preview reply\./)).toBeInTheDocument();
  });

  it("keeps the included free OpenCode model active and switches it inline", async () => {
    const { user, bridge } = await openSetup();
    const model = await screen.findByTestId("composer-model");
    expect(model).toHaveTextContent("OpenCode free · DeepSeek V4.1 Flash Free");
    expect(useConversationsView()?.model?.tier).toBe("included");

    const setModel = vi.spyOn(bridge.chat!, "setModel");
    await user.click(model);
    const menu = screen.getByRole("menu", { name: "Choose a model" });
    expect(within(menu).getByRole("menuitemradio", { name: /Big Pickle/ })).toBeInTheDocument();
    expect(
      within(menu).getByRole("menuitemradio", { name: /BYOK · DeepSeek V4.1 Flash/ }),
    ).toBeInTheDocument();

    await user.click(within(menu).getByRole("menuitemradio", { name: /Big Pickle/ }));
    await waitFor(() =>
      expect(setModel).toHaveBeenCalledWith("setup", "opencode/big-pickle"),
    );
    await waitFor(() =>
      expect(screen.getByTestId("composer-model")).toHaveTextContent(
        "OpenCode free · Big Pickle",
      ),
    );
  });

  it("renders the context ring, exact popover and compact stats pills", async () => {
    const { user } = await openSetup();

    const meter = await screen.findByTestId("context-meter");
    expect(meter).toHaveTextContent("18%");
    expect(meter).toHaveAttribute("aria-label", "Context 18% used");
    expect(screen.getByTestId("usage-pill-turns")).toHaveTextContent("3 turns");
    expect(screen.getByTestId("usage-pill-tokens")).toHaveTextContent("15.2k tokens");
    expect(screen.getByTestId("usage-pill-cache")).toHaveTextContent("cache 8.4k");

    await user.click(meter);
    const popover = screen.getByTestId("context-popover");
    expect(popover).toHaveTextContent("23,600 / 128,000 tokens");
    expect(popover).toHaveTextContent("12,800 tokens");
    expect(popover).toHaveTextContent("2,400 tokens");
    expect(popover).toHaveTextContent("8,400 tokens");
    expect(screen.getByRole("group", { name: "Context usage" })).toBeInTheDocument();

    await user.keyboard("{Escape}");
    expect(screen.queryByTestId("context-popover")).not.toBeInTheDocument();
  });

  it("hides the context meter until usage reports used tokens and a limit", () => {
    const { container, rerender } = render(<ContextMeter usage={null} />);
    expect(container).toBeEmptyDOMElement();

    rerender(<ContextMeter usage={usage({ context_limit: null })} />);
    expect(container).toBeEmptyDOMElement();

    rerender(<ContextMeter usage={usage({ context_tokens_used: null })} />);
    expect(container).toBeEmptyDOMElement();

    rerender(<ContextMeter usage={usage()} />);
    expect(screen.getByTestId("context-meter")).toHaveTextContent("18%");
  });

  it("shows the honest no-model state with a Settings → Engine link", async () => {
    const base = new MockBridge("empty");
    if (base.chat) {
      const start = base.chat.start;
      base.chat.start = async (input) => ({
        ...(await start(input)),
        model: null,
        available_models: [],
      });
    }

    resetFleetStore();
    resetConversations();
    bridges.push(base);
    render(<NextShell bridge={base} />);
    await waitFor(() => expect(useFleetStore.getState().hostsLoaded).toBe(true));

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: /Set up with AI/ }));

    const setup = await screen.findByTestId("composer-setup-model");
    expect(setup).toHaveTextContent("Set up a model");
    expect(screen.queryByTestId("composer-model")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Message" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();

    await user.click(setup);
    expect(screen.getByRole("heading", { name: "Settings", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Hide advanced settings" })).toBeInTheDocument();
  });
});

function useConversationsView() {
  // Read the same store the shell reads without adding a hook to the test body.
  return useConversations.getState().views.setup ?? null;
}
