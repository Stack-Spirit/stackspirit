import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { afterEach, describe, expect, it, vi } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import type { SpiritBridge } from "../../bridge/SpiritBridge";
import type {
  ChatAccess,
  ChatOwnershipView,
  ChatSessionView,
  ChatWriterView,
} from "../../bridge/wire";
import { resetConversations } from "../../next/conversations";
import { NextShell } from "../../next/NextShell";
import { resetShellUi } from "../../next/shellUi";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";

const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"];

const FOREIGN_OWNER: ChatOwnershipView = {
  kind: "opencode",
  label: "OpenCode · macbook",
  holder: "macbook",
  node: "macbook",
  mode: "interactive",
  state: "active",
  activity: "running",
};

const STACKSPIRIT_OWNER: ChatOwnershipView = {
  kind: "desktop_ui",
  label: "StackSpirit · macbook",
  holder: "stackspirit",
  node: "macbook",
  mode: "interactive",
  state: "active",
  activity: "idle",
};

const AVAILABLE_WRITER: ChatWriterView = {
  state: "available",
  can_take_over: true,
  can_release: false,
};

const HELD_WRITER: ChatWriterView = {
  state: "held",
  can_take_over: false,
  can_release: true,
};

interface WriterPatch {
  writer?: ChatWriterView;
  ownership?: ChatOwnershipView | null;
  access?: ChatAccess;
  /** Extra wire fields planted by a test; the UI must never render them. */
  leak?: Record<string, unknown>;
  releaseError?: unknown;
}

const bridges: MockBridge[] = [];

/**
 * Adds writer fields and deterministic take/release results to the preview bridge.
 * It never fabricates a lease outside the test process: the returned held view is the
 * same safe wire shape a real Tauri command returns.
 */
function patchWriter(bridge: MockBridge, patch: WriterPatch) {
  const chat = bridge.chat;
  if (!chat) throw new Error("the mock has no chat surface");
  let latest: ChatSessionView | null = null;

  const decorate = (view: ChatSessionView): ChatSessionView => ({
    ...view,
    ...(patch.writer ? { writer: patch.writer } : {}),
    ...(patch.ownership !== undefined
      ? { ownership: patch.ownership ?? undefined }
      : {}),
    ...(patch.access ? { access: patch.access } : {}),
    ...(patch.leak ?? {}),
  });

  const start = chat.start.bind(chat);
  chat.start = async (input) => {
    const next = decorate(await start(input));
    latest = next;
    return next;
  };
  const attach = chat.attach.bind(chat);
  chat.attach = async (session) => {
    const next = decorate(await attach(session));
    latest = next;
    return next;
  };

  const requireLatest = (): ChatSessionView => {
    if (!latest) throw new Error("chat.start must run before a writer action");
    return latest;
  };

  const takeOver = vi.fn(
    async (_session: string, _replaceHolder?: boolean): Promise<ChatSessionView> => {
      void _session;
      void _replaceHolder;
      const held: ChatSessionView = {
        ...requireLatest(),
        state: "ready",
        attached: true,
        error: null,
        ownership: STACKSPIRIT_OWNER,
        access: "owner",
        writer: HELD_WRITER,
        ...(patch.leak ?? {}),
      };
      latest = held;
      return held;
    },
  );
  chat.takeOver = takeOver;

  const releaseWriter = vi.fn(async (_session: string): Promise<ChatSessionView> => {
    void _session;
    if (patch.releaseError) throw patch.releaseError;
    const available: ChatSessionView = {
      ...requireLatest(),
      state: "ready",
      attached: true,
      error: null,
      ownership: FOREIGN_OWNER,
      access: "read_only",
      writer: AVAILABLE_WRITER,
    };
    latest = available;
    return available;
  });
  chat.releaseWriter = releaseWriter;

  return { takeOver, releaseWriter };
}

async function openGlobal(patch: WriterPatch = {}) {
  resetFleetStore();
  resetConversations();
  const bridge = new MockBridge("busy");
  const writer = patchWriter(bridge, patch);
  bridges.push(bridge);
  const user = userEvent.setup();
  const result = render(<NextShell bridge={bridge as SpiritBridge} />);
  await waitFor(() => expect(useFleetStore.getState().hostsLoaded).toBe(true));
  await user.click(screen.getByTestId("connect-prompt-connect"));
  await waitFor(() =>
    expect(useFleetStore.getState().connection.state).toBe("connected"),
  );
  await screen.findByRole("heading", { name: "Global orchestrator", level: 1 });
  return { bridge, user, ...writer, ...result };
}

async function expectNoWcagViolations() {
  const results = await axe.run(document.body, {
    runOnly: { type: "tag", values: WCAG_TAGS },
  });
  expect(results.violations).toEqual([]);
}

afterEach(() => {
  for (const bridge of bridges.splice(0)) bridge.dispose?.();
  resetConversations();
  resetShellUi();
  resetFleetStore();
  delete document.documentElement.dataset.ui;
  delete document.documentElement.dataset.theme;
});

describe("chat writer takeover", () => {
  it("takes over an eligible session, names the holder, then releases", async () => {
    const { user, takeOver, releaseWriter } = await openGlobal({
      writer: AVAILABLE_WRITER,
      ownership: FOREIGN_OWNER,
      access: "read_only",
    });

    const notice = await screen.findByTestId("readonly-notice");
    expect(within(notice).queryByRole("button")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Message" })).toBeDisabled();

    await user.click(await screen.findByTestId("writer-take-over"));
    const dialog = await screen.findByRole("alertdialog", {
      name: "Take over this session?",
    });
    expect(dialog).toHaveTextContent(
      "OpenCode · macbook is writing in this session. (Running) Taking over makes the other app read-only.",
    );
    expect(dialog).toHaveTextContent(
      "A turn is running. It may be interrupted after a few seconds.",
    );
    expect(dialog).toHaveTextContent(
      "A window that connects to the orchestrator directly may keep working until it notices.",
    );
    expect(dialog).not.toHaveTextContent(/fence|stopped/i);

    await user.click(within(dialog).getByRole("button", { name: "Take over" }));

    expect(await screen.findByTestId("writer-release")).toBeInTheDocument();
    expect(takeOver).toHaveBeenCalledWith("global", true);
    expect(screen.getByTestId("session-writer")).toHaveTextContent("StackSpirit");
    expect(screen.getByTestId("writer-status")).toHaveTextContent(
      "You are writing here.",
    );
    expect(screen.queryByTestId("readonly-notice")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Message" })).toBeEnabled();

    await user.click(screen.getByTestId("writer-release"));

    expect(await screen.findByTestId("writer-take-over")).toBeInTheDocument();
    expect(releaseWriter).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("session-writer")).not.toBeInTheDocument();
    expect(screen.getByTestId("readonly-notice")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Message" })).toBeDisabled();
  });

  it("attaches without the replace flag when no one is writing", async () => {
    const { user, takeOver } = await openGlobal({
      writer: AVAILABLE_WRITER,
      ownership: null,
      access: "read_only",
    });

    await user.click(await screen.findByTestId("writer-take-over"));
    const dialog = await screen.findByRole("alertdialog", {
      name: "Take over this session?",
    });
    expect(dialog).toHaveTextContent(
      "No one is writing in this session. Taking over makes this window the writer.",
    );
    await user.click(within(dialog).getByRole("button", { name: "Take over" }));

    await waitFor(() => expect(takeOver).toHaveBeenCalledWith("global", false));
    expect(await screen.findByTestId("writer-release")).toBeInTheDocument();
  });

  it("shows the node-hosted reason and never offers a takeover action", async () => {
    await openGlobal({
      writer: {
        state: "unavailable",
        can_take_over: true,
        can_release: false,
        reason: "node_hosted",
      },
      ownership: null,
      access: "read_only",
    });

    const reason = await screen.findByTestId("writer-reason");
    expect(reason).toHaveTextContent("runs on a node");
    expect(reason).toHaveTextContent("home-hosted orchestrator");
    expect(screen.queryByTestId("writer-take-over")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Take over" })).not.toBeInTheDocument();

    const notice = screen.getByTestId("readonly-notice");
    expect(within(notice).queryByRole("button")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Message" })).toBeDisabled();
  });

  it("shows an unavailable reason with no action", async () => {
    await openGlobal({
      writer: {
        state: "unavailable",
        can_take_over: false,
        can_release: false,
        reason: "vowp_too_old",
      },
      ownership: null,
      access: "read_only",
    });

    expect(await screen.findByTestId("writer-reason")).toHaveTextContent(
      "newer VOWP",
    );
    expect(screen.queryByTestId("writer-take-over")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Take over" })).not.toBeInTheDocument();
  });

  it("shows the not-configured reason with no action when the app is not on the home", async () => {
    await openGlobal({
      writer: {
        state: "unavailable",
        can_take_over: false,
        can_release: false,
        reason: "not_home",
      },
      ownership: null,
      access: "read_only",
    });

    expect(await screen.findByTestId("writer-reason")).toHaveTextContent(
      "not configured for takeover here",
    );
    expect(screen.queryByTestId("writer-take-over")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Take over" })).not.toBeInTheDocument();
  });

  it("traps Tab, cancels with Cancel and Escape, and returns focus to Take over", async () => {
    const { user } = await openGlobal({
      writer: AVAILABLE_WRITER,
      ownership: FOREIGN_OWNER,
      access: "read_only",
    });
    const trigger = await screen.findByTestId("writer-take-over");

    await user.click(trigger);
    let dialog = await screen.findByRole("alertdialog", {
      name: "Take over this session?",
    });
    const cancel = within(dialog).getByRole("button", { name: "Cancel" });
    const confirm = within(dialog).getByRole("button", { name: "Take over" });
    expect(cancel).toHaveFocus();

    await user.tab({ shift: true });
    expect(confirm).toHaveFocus();
    await user.tab();
    expect(cancel).toHaveFocus();

    await user.click(cancel);
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();

    await user.click(trigger);
    dialog = await screen.findByRole("alertdialog", {
      name: "Take over this session?",
    });
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
  });

  it("keeps the held writer when release fails with a typed error", async () => {
    const { user, releaseWriter } = await openGlobal({
      writer: HELD_WRITER,
      ownership: STACKSPIRIT_OWNER,
      access: "owner",
      releaseError: {
        code: "chat_writer_lost",
        message: "raw lease text that must not render",
        hint: "raw hint that must not render",
        retryable: false,
      },
    });

    await screen.findByTestId("writer-release");
    await user.click(screen.getByTestId("writer-release"));

    const error = await screen.findByTestId("writer-error");
    expect(error).toHaveAttribute("data-code", "chat_writer_lost");
    expect(error).toHaveTextContent("Another writer took this session over.");
    expect(error).not.toHaveTextContent("raw lease text");
    expect(error).not.toHaveTextContent("raw hint");
    expect(releaseWriter).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("session-writer")).toBeInTheDocument();
    expect(screen.getByTestId("writer-release")).toBeInTheDocument();
  });

  it("never renders lease identifiers, URLs, ports or paths", async () => {
    const leak = {
      token: "canary-lease-token-9f3",
      surface_id: "canary-surface-id-9f3",
      url: "canary-url-9f3",
      port: "canary-port-9f3",
      native_session_id: "canary-native-session-9f3",
      epoch: "canary-epoch-9f3",
      expires_at: "canary-expiry-9f3",
      path: "canary-path-9f3",
    };
    const { user } = await openGlobal({
      writer: AVAILABLE_WRITER,
      ownership: FOREIGN_OWNER,
      access: "read_only",
      leak,
    });

    const markup = () => document.documentElement.outerHTML;
    const expectNoLeaks = () => {
      for (const value of Object.values(leak)) {
        expect(markup()).not.toContain(value);
      }
      expect(markup()).not.toContain("127.0.0.1");
      expect(markup()).not.toContain("http://");
      expect(markup()).not.toContain("https://");
      expect(markup()).not.toContain("Authorization");
      expect(markup()).not.toContain("Bearer");
    };

    await screen.findByTestId("writer-take-over");
    expectNoLeaks();

    await user.click(screen.getByTestId("writer-take-over"));
    await screen.findByRole("alertdialog", { name: "Take over this session?" });
    expectNoLeaks();

    await user.click(
      within(screen.getByRole("alertdialog")).getByRole("button", {
        name: "Take over",
      }),
    );
    await screen.findByTestId("writer-release");
    expectNoLeaks();
  });

  for (const theme of ["light", "dark"] as const) {
    it(`meets WCAG 2.2 AA for the action, dialog and held state in ${theme}`, async () => {
      document.documentElement.dataset.theme = theme;
      const { user } = await openGlobal({
        writer: AVAILABLE_WRITER,
        ownership: FOREIGN_OWNER,
        access: "read_only",
      });

      await screen.findByTestId("writer-take-over");
      await expectNoWcagViolations();

      await user.click(screen.getByTestId("writer-take-over"));
      await screen.findByRole("alertdialog", { name: "Take over this session?" });
      await expectNoWcagViolations();

      await user.click(
        within(screen.getByRole("alertdialog")).getByRole("button", {
          name: "Take over",
        }),
      );
      await screen.findByTestId("writer-release");
      await expectNoWcagViolations();
    });
  }
});
