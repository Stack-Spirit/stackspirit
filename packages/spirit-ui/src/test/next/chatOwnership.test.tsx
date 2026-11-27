import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { afterEach, describe, expect, it, vi } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import type { SpiritBridge } from "../../bridge/SpiritBridge";
import type { ChatAccess, ChatOwnershipView, ChatSessionView } from "../../bridge/wire";
import { resetConversations } from "../../next/conversations";
import { NextShell } from "../../next/NextShell";
import { resetShellUi } from "../../next/shellUi";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";

const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"];

const OPENCODE_OWNER: ChatOwnershipView = {
  kind: "opencode",
  label: "OpenCode · macbook",
  holder: "macbook",
  node: "macbook",
  mode: "interactive",
  state: "active",
  activity: "running",
};

interface ChatOwnershipPatch {
  ownership?: ChatOwnershipView;
  access?: ChatAccess;
}

const bridges: MockBridge[] = [];

/** Adds the ss-152 ownership fields to the mock's chat views without touching it. */
function patchOwnership(bridge: MockBridge, patch: ChatOwnershipPatch): void {
  const chat = bridge.chat;
  if (!chat) throw new Error("the mock has no chat surface");
  const withPatch = (view: ChatSessionView): ChatSessionView => ({
    ...view,
    ...(patch.ownership ? { ownership: patch.ownership } : {}),
    ...(patch.access ? { access: patch.access } : {}),
  });
  const start = chat.start.bind(chat);
  chat.start = async (input) => withPatch(await start(input));
  const attach = chat.attach.bind(chat);
  chat.attach = async (session) => withPatch(await attach(session));
}

async function openGlobal(patch: ChatOwnershipPatch = {}) {
  resetFleetStore();
  resetConversations();
  const bridge = new MockBridge("busy");
  patchOwnership(bridge, patch);
  bridges.push(bridge);
  const user = userEvent.setup();
  const result = render(<NextShell bridge={bridge as SpiritBridge} />);
  await waitFor(() => expect(useFleetStore.getState().hostsLoaded).toBe(true));
  await user.click(screen.getByTestId("connect-prompt-connect"));
  await waitFor(() =>
    expect(useFleetStore.getState().connection.state).toBe("connected"),
  );
  await screen.findByRole("heading", { name: "Global orchestrator", level: 1 });
  return { bridge, user, ...result };
}

afterEach(() => {
  for (const bridge of bridges.splice(0)) bridge.dispose?.();
  resetConversations();
  resetShellUi();
  resetFleetStore();
});

describe("chat session ownership", () => {
  it("shows the owner chip and keeps the composer enabled when this window owns the session", async () => {
    const { bridge, user } = await openGlobal({
      ownership: OPENCODE_OWNER,
      access: "owner",
    });

    const chip = await screen.findByTestId("session-ownership");
    expect(chip).toHaveTextContent("OpenCode · macbook");
    expect(chip).toHaveAttribute(
      "title",
      "Session owner: OpenCode · macbook · Running",
    );
    expect(screen.queryByTestId("readonly-notice")).not.toBeInTheDocument();

    const box = screen.getByRole("textbox", { name: "Message" });
    expect(box).toBeEnabled();
    expect(box).toHaveAttribute("placeholder", "Message the global orchestrator");

    const send = vi.spyOn(bridge.chat!, "send");
    await user.type(box, "hello{Enter}");
    await waitFor(() => expect(send).toHaveBeenCalledTimes(1));
  });

  it("shows the read-only notice, disables text and Send, and never submits on Enter", async () => {
    const { bridge } = await openGlobal({
      ownership: OPENCODE_OWNER,
      access: "read_only",
    });

    const notice = await screen.findByTestId("readonly-notice");
    expect(notice).toHaveTextContent(
      "OpenCode · macbook is writing in this session. This window is read-only.",
    );
    expect(notice).toHaveTextContent(
      "Close the other window or end the CLI session to release it.",
    );
    expect(within(notice).queryByRole("button")).not.toBeInTheDocument();
    expect(screen.getByTestId("session-ownership")).toHaveTextContent(
      "OpenCode · macbook",
    );

    const box = screen.getByRole("textbox", { name: "Message" });
    expect(box).toBeDisabled();
    expect(box).toHaveAttribute(
      "placeholder",
      "Read-only · OpenCode · macbook is writing here",
    );
    const sendButton = screen.getByRole("button", { name: "Send" });
    expect(sendButton).toBeDisabled();

    const send = vi.spyOn(bridge.chat!, "send");
    fireEvent.change(box, { target: { value: "hello" } });
    fireEvent.keyDown(box, { key: "Enter", code: "Enter" });
    fireEvent.click(sendButton);
    expect(send).not.toHaveBeenCalled();
  });

  it("behaves exactly as today when the bridge omits ownership and access", async () => {
    const { user } = await openGlobal();

    expect(screen.queryByTestId("session-ownership")).not.toBeInTheDocument();
    expect(screen.queryByTestId("readonly-notice")).not.toBeInTheDocument();

    const box = screen.getByRole("textbox", { name: "Message" });
    expect(box).toBeEnabled();
    expect(box).toHaveAttribute("placeholder", "Message the global orchestrator");
    await user.type(box, "hello");
    expect(screen.getByRole("button", { name: "Send" })).toBeEnabled();
  });

  it("has no WCAG 2.2 AA violations when this window owns the session", async () => {
    await openGlobal({ ownership: OPENCODE_OWNER, access: "owner" });
    await screen.findByTestId("session-ownership");
    const results = await axe.run(document.body, {
      runOnly: { type: "tag", values: WCAG_TAGS },
    });
    expect(results.violations).toEqual([]);
  });

  it("has no WCAG 2.2 AA violations when the session is read-only", async () => {
    await openGlobal({ ownership: OPENCODE_OWNER, access: "read_only" });
    await screen.findByTestId("readonly-notice");
    const results = await axe.run(document.body, {
      runOnly: { type: "tag", values: WCAG_TAGS },
    });
    expect(results.violations).toEqual([]);
  });
});
