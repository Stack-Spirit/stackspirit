import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import type { SpiritBridge } from "../../bridge/SpiritBridge";
import { MockBridge } from "../../bridge/MockBridge";
import { resetConversations, useConversations } from "../../next/conversations";
import { NextShell } from "../../next/NextShell";
import { resetShellUi } from "../../next/shellUi";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";

const bridges: SpiritBridge[] = [];

async function openFreeLimit() {
  resetFleetStore();
  resetConversations();
  const bridge = new MockBridge({ scenario: "chat-free-limit", hosts: [] });
  bridges.push(bridge);
  const user = userEvent.setup();
  render(<NextShell bridge={bridge} />);
  await waitFor(() => expect(useFleetStore.getState().hostsLoaded).toBe(true));
  await user.click(screen.getByRole("button", { name: /Set up with AI/ }));
  const composer = await screen.findByRole("textbox", { name: "Message" });
  await user.type(composer, "explain the server{Enter}");
  const notice = await screen.findByTestId("retry-notice");
  return { user, bridge, composer, notice };
}

afterEach(() => {
  for (const bridge of bridges.splice(0)) bridge.dispose?.();
  resetConversations();
  resetShellUi();
  resetFleetStore();
});

describe("free-limit retry recovery", () => {
  it("shows the calm free-limit notice with a countdown instead of Replying…", async () => {
    await openFreeLimit();
    const notice = screen.getByTestId("retry-notice");
    expect(within(notice).getByRole("heading", { name: "Free limit reached" })).toBeInTheDocument();
    expect(within(notice).getByText(/usage limit/i)).toBeInTheDocument();
    expect(within(notice).getByText(/Retrying in \d+s/)).toBeInTheDocument();
    expect(within(notice).getByRole("button", { name: "Switch model" })).toBeEnabled();
    expect(
      within(notice).getByRole("button", { name: "Connect a provider" }),
    ).toBeEnabled();
    expect(within(notice).getByRole("button", { name: "Stop retrying" })).toBeEnabled();
    expect(screen.getByTestId("session-state")).toHaveTextContent("Retrying…");
  });

  it("Switch model focuses the composer picker and selects another model", async () => {
    const { user } = await openFreeLimit();
    await user.click(screen.getByRole("button", { name: "Switch model" }));

    const menu = await screen.findByTestId("model-picker-popover");
    await waitFor(() => expect(screen.getByTestId("composer-model-search")).toHaveFocus());
    expect(
      within(menu).getByRole("option", { name: /Big Pickle/ }),
    ).toBeInTheDocument();
    expect(
      within(menu).getByRole("option", { name: /DeepSeek V4.1 Flash(?! Free)/ }),
    ).toBeInTheDocument();

    await user.click(within(menu).getByRole("option", { name: /Big Pickle/ }));
    await waitFor(() =>
      expect(screen.getByTestId("composer-model")).toHaveTextContent(
        "OpenCode free · Big Pickle",
      ),
    );
    expect(screen.getByTestId("retry-notice")).toBeInTheDocument();
    expect(useConversations.getState().views.setup?.model?.id).toBe("opencode/big-pickle");
  });

  it("Connect a provider opens Settings with the Engine advanced section", async () => {
    const { user } = await openFreeLimit();
    await user.click(screen.getByRole("button", { name: "Connect a provider" }));
    expect(await screen.findByRole("heading", { name: "Settings", level: 1 })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Hide advanced settings" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "StackSpirit Engine", level: 2 }),
    ).toBeInTheDocument();
  });

  it("Stop retrying returns to Ready and the next send is accepted", async () => {
    const { user, composer } = await openFreeLimit();
    await user.click(screen.getByRole("button", { name: "Stop retrying" }));

    await waitFor(() => expect(screen.queryByTestId("retry-notice")).not.toBeInTheDocument());
    expect(screen.getByTestId("session-state")).toHaveTextContent("Ready");
    expect(composer).toBeEnabled();
    expect(screen.queryByRole("button", { name: "Start" })).not.toBeInTheDocument();

    await user.type(composer, "continue with another model{Enter}");
    expect(await screen.findByText(/Preview reply\./)).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByTestId("session-state")).toHaveTextContent("Ready"),
    );
  });
});
