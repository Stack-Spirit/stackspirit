import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import type { SpiritBridge } from "../../bridge/SpiritBridge";
import { MockBridge } from "../../bridge/MockBridge";
import { resetConversations, upsertMessage } from "../../next/conversations";
import { NextShell } from "../../next/NextShell";
import { resetShellUi } from "../../next/shellUi";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";

const bridges: SpiritBridge[] = [];

async function openSetup(bridge: MockBridge) {
  resetFleetStore();
  resetConversations();
  bridges.push(bridge);
  const user = userEvent.setup();
  render(<NextShell bridge={bridge} />);
  await waitFor(() => expect(useFleetStore.getState().hostsLoaded).toBe(true));
  await user.click(screen.getByRole("button", { name: /Set up with AI/ }));
  await user.type(await screen.findByRole("textbox", { name: "Message" }), "go{Enter}");
  return { user, bridge };
}

afterEach(() => {
  for (const bridge of bridges.splice(0)) bridge.dispose?.();
  resetConversations();
  resetShellUi();
  resetFleetStore();
});

describe("tool activity rows", () => {
  it("collapses completed steps behind the completion header once the reply settles", async () => {
    await openSetup(new MockBridge({ scenario: "chat-tools", hosts: [] }));
    await waitFor(() =>
      expect(screen.getByTestId("session-state")).toHaveTextContent("Ready"),
    );
    // Once the reply is done the completed steps collapse behind the completion header.
    const toggle = await screen.findByTestId("completion-header");
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryAllByTestId("tool-row")).toHaveLength(0);
    await userEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    const settled = screen.getAllByTestId("tool-row");
    expect(settled.map((row) => row.getAttribute("data-status"))).toEqual([
      "completed",
      "completed",
      "error",
    ]);
    // The status is stated in words, not by the dot's colour alone.
    expect(within(settled[2]!).getByText("failed")).toBeInTheDocument();
    expect(within(settled[0]!).getByText("read")).toBeInTheDocument();
    expect(within(settled[0]!).getByText("docs/PLAN.md")).toBeInTheDocument();
    expect(screen.getByText(/Preview reply\./)).toBeInTheDocument();
    await userEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryAllByTestId("tool-row")).toHaveLength(0);
  });
});

describe("message upsert", () => {
  it("a text-only update never erases the rows an earlier update delivered", () => {
    const withTools = {
      id: "m1",
      role: "assistant" as const,
      text: [""],
      created_at: "2026-11-20T00:00:00.000Z",
      origin: "model" as const,
      tools: [{ id: "t1", name: "bash", status: "running" as const }],
    };
    const { tools: _tools, ...shell } = withTools;
    void _tools;
    const merged = upsertMessage([withTools], { ...shell, text: ["Hello"] });
    expect(merged[0]?.tools).toEqual(withTools.tools);
    expect(merged[0]?.text).toEqual(["Hello"]);
  });
});

describe("inline permission card", () => {
  it("pauses the turn on an accessible card and Allow once resolves it", async () => {
    const { user, bridge } = await openSetup(
      new MockBridge({ scenario: "chat-permission", hosts: [] }),
    );
    const card = await screen.findByTestId("permission-card");
    expect(within(card).getByRole("heading", { name: "Permission needed" })).toBeInTheDocument();
    expect(within(card).getByText("/srv/shared/*")).toBeInTheDocument();
    expect(within(card).getByRole("button", { name: "Allow once" })).toBeEnabled();
    expect(within(card).getByRole("button", { name: "Always allow" })).toBeEnabled();
    expect(within(card).getByRole("button", { name: "Reject" })).toBeEnabled();
    expect(card).toHaveClass("nx-permission-card");
    expect(card).toHaveTextContent("Waiting for approval");
    const order = within(card)
      .getAllByRole("button")
      .map((button) => button.textContent);
    expect(order).toEqual(["Reject", "Always allow", "Allow once"]);
    expect(screen.getByTestId("session-state")).toHaveTextContent("Replying…");
    expect((await screen.findAllByTestId("tool-row")).at(-1)).toHaveAttribute(
      "data-status",
      "running",
    );

    await user.click(within(card).getByRole("button", { name: "Allow once" }));
    await waitFor(() => expect(screen.queryByTestId("permission-card")).not.toBeInTheDocument());
    await waitFor(() =>
      expect(screen.getByTestId("session-state")).toHaveTextContent("Ready"),
    );
    expect(screen.getByText(/You allowed the request/)).toBeInTheDocument();
    expect(bridge.permissionAnswers).toEqual([{ id: "per_preview_1", decision: "once" }]);
  });

  it("docks the card above the composer and answers Enter (allow once) and Escape (reject)", async () => {
    const { user, bridge } = await openSetup(
      new MockBridge({ scenario: "chat-permission", hosts: [] }),
    );
    const card = await screen.findByTestId("permission-card");
    expect(card.closest('[data-testid="permission-dock"]')).not.toBeNull();
    expect(card.closest(".nx-chat__inner")).toBeNull();
    const dock = screen.getByTestId("permission-dock");
    const composer = screen.getByRole("textbox", { name: "Message" });
    expect(
      dock.compareDocumentPosition(composer) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    await waitFor(() => expect(card).toHaveFocus());
    await user.keyboard("{Enter}");
    await waitFor(() => expect(screen.queryByTestId("permission-card")).not.toBeInTheDocument());
    expect(bridge.permissionAnswers).toEqual([{ id: "per_preview_1", decision: "once" }]);
  });

  it("Escape rejects the focused permission card", async () => {
    const { user, bridge } = await openSetup(
      new MockBridge({ scenario: "chat-permission", hosts: [] }),
    );
    const card = await screen.findByTestId("permission-card");
    await waitFor(() => expect(card).toHaveFocus());
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByTestId("permission-card")).not.toBeInTheDocument());
    expect(bridge.permissionAnswers).toEqual([{ id: "per_preview_1", decision: "reject" }]);
  });

  it("Reject also resolves the turn and reports the rejection honestly", async () => {
    const { user, bridge } = await openSetup(
      new MockBridge({ scenario: "chat-permission", hosts: [] }),
    );
    await user.click(await screen.findByRole("button", { name: "Reject" }));
    await waitFor(() => expect(screen.queryByTestId("permission-card")).not.toBeInTheDocument());
    expect(await screen.findByText(/You rejected the request/)).toBeInTheDocument();
    expect(bridge.permissionAnswers).toEqual([{ id: "per_preview_1", decision: "reject" }]);
    const toggle = await screen.findByTestId("completion-header");
    await user.click(toggle);
    expect(
      screen.getAllByTestId("tool-row").some((row) => row.getAttribute("data-status") === "error"),
    ).toBe(true);
  });

  it("keeps the card and shows an error when the reply fails", async () => {
    const { user, bridge } = await openSetup(
      new MockBridge({ scenario: "chat-permission", hosts: [] }),
    );
    await screen.findByTestId("permission-card");
    bridge.chat!.replyPermission = () =>
      Promise.reject(new Error("engine unavailable"));
    await user.click(screen.getByRole("button", { name: "Allow once" }));
    expect(await screen.findByTestId("permission-error")).toBeInTheDocument();
    expect(screen.getByTestId("permission-card")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Allow once" })).toBeEnabled();
  });
});
