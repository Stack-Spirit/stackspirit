import { beforeEach, describe, expect, it, vi } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import type { ChatBridgeSurface } from "../../bridge/SpiritBridge";
import type { ChatSessionView } from "../../bridge/wire";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
const { listen } = vi.hoisted(() => ({ listen: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));

import { TauriBridge } from "../../../../../apps/spirit-desktop/src/bridge/TauriBridge";

/**
 * A realistic held view. It is deliberately the whole safe wire shape: the command
 * boundary must not add a lease token, surface id, URL, port, native session id, epoch
 * or path to any part of it.
 */
const HELD_VIEW: ChatSessionView = {
  session: "global",
  location: "local",
  state: "ready",
  attached: true,
  streaming_supported: true,
  model: null,
  usage: null,
  started_at: "2026-11-28T09:00:00.000Z",
  last_event_at: "2026-11-28T09:00:01.000Z",
  error: null,
  ownership: {
    kind: "desktop_ui",
    label: "StackSpirit · macbook",
    holder: "stackspirit",
    node: "macbook",
    mode: "interactive",
    state: "active",
    activity: "idle",
  },
  access: "owner",
  writer: {
    state: "held",
    can_take_over: false,
    can_release: true,
  },
};

const AVAILABLE_VIEW: ChatSessionView = {
  ...HELD_VIEW,
  ownership: undefined,
  access: "read_only",
  writer: {
    state: "available",
    can_take_over: true,
    can_release: false,
  },
};

/** Structural leak checks; bare words like "port" occur inside "supported". */
const FORBIDDEN_LEAKS = [
  '"token"',
  '"surface_id"',
  '"url"',
  '"port":',
  '"native_session_id"',
  '"epoch"',
  '"expires_at"',
  '"path"',
  "http://",
  "https://",
  "127.0.0.1",
  "/Users/",
  "/home/",
  "Authorization",
];

function requireChat(bridge: TauriBridge): ChatBridgeSurface {
  const chat = bridge.chat;
  if (!chat) throw new Error("TauriBridge must expose chat");
  return chat;
}

async function captureError(action: () => Promise<unknown>): Promise<unknown> {
  try {
    await action();
  } catch (error) {
    return error;
  }
  throw new Error("expected the call to fail");
}

/** A bridge surface with no writer methods, like the read-only Web bridge. */
function readOnlyChat(): ChatBridgeSurface {
  const unavailable = async (): Promise<never> => {
    throw new Error("the read-only Web bridge has no chat writer");
  };
  return {
    start: unavailable,
    attach: unavailable,
    send: unavailable,
    stop: unavailable,
    log: unavailable,
    subscribe: () => () => {},
  };
}

beforeEach(() => {
  invoke.mockReset();
  listen.mockReset();
});

describe("TauriBridge chat writer commands", () => {
  it("maps both actions to the exact command and argument shape", async () => {
    const chat = requireChat(new TauriBridge());

    invoke.mockResolvedValueOnce(AVAILABLE_VIEW);
    await expect(chat.takeOver?.("global")).resolves.toEqual(AVAILABLE_VIEW);
    expect(invoke).toHaveBeenLastCalledWith("chat_take_over", {
      session: "global",
      takeover: false,
    });

    invoke.mockResolvedValueOnce(HELD_VIEW);
    await expect(chat.takeOver?.("global", true)).resolves.toEqual(HELD_VIEW);
    expect(invoke).toHaveBeenLastCalledWith("chat_take_over", {
      session: "global",
      takeover: true,
    });

    invoke.mockResolvedValueOnce(AVAILABLE_VIEW);
    await expect(chat.releaseWriter?.("global")).resolves.toEqual(AVAILABLE_VIEW);
    expect(invoke).toHaveBeenLastCalledWith("chat_release_writer", {
      session: "global",
    });

    expect(invoke.mock.calls.map(([command]) => command)).toEqual([
      "chat_take_over",
      "chat_take_over",
      "chat_release_writer",
    ]);
  });

  it("keeps every serialized writer result free of lease identity", () => {
    const rendered = JSON.stringify(HELD_VIEW);
    for (const forbidden of FORBIDDEN_LEAKS) {
      expect(rendered).not.toContain(forbidden);
    }

    expect(Object.keys(HELD_VIEW.writer ?? {}).sort()).toEqual([
      "can_release",
      "can_take_over",
      "state",
    ]);
    expect(JSON.parse(rendered).writer).toEqual({
      state: "held",
      can_take_over: false,
      can_release: true,
    });
  });

  it("passes a typed host refusal through without inventing lease identity", async () => {
    const chat = requireChat(new TauriBridge());
    const failure = {
      code: "gate",
      message:
        "chat_takeover_unavailable: VOWP reports a node-hosted orchestrator; only a home-hosted orchestrator can be attached",
      hint: "Wait for VOWP node-side attach or run the app on the VOWP home.",
      retryable: false,
    };
    invoke.mockRejectedValueOnce(failure);

    const error = await captureError(() => chat.takeOver?.("global") ?? Promise.reject());
    expect(error).toEqual(failure);
    const rendered = JSON.stringify(error);
    for (const forbidden of FORBIDDEN_LEAKS) {
      expect(rendered).not.toContain(forbidden);
    }
  });

  it("forwards release twice so the core's idempotent no-op is observable", async () => {
    const chat = requireChat(new TauriBridge());
    invoke.mockResolvedValue(AVAILABLE_VIEW);

    const first = await chat.releaseWriter?.("global");
    const second = await chat.releaseWriter?.("global");

    expect(first).toEqual(AVAILABLE_VIEW);
    expect(second).toEqual(AVAILABLE_VIEW);
    expect(invoke.mock.calls).toEqual([
      ["chat_release_writer", { session: "global" }],
      ["chat_release_writer", { session: "global" }],
    ]);
  });
});

describe("preview chat writer honesty", () => {
  it("never claims a real lease and rejects takeover with a typed preview gate", async () => {
    const bridge = new MockBridge("chat-preview");
    const chat = bridge.chat;
    if (!chat) throw new Error("MockBridge must expose chat");

    const started = await chat.start({
      session: "global",
      project: null,
      location: "local",
      resume: true,
    });
    expect(started.writer).toBeUndefined();
    expect(started.access).toBeUndefined();

    const error = await captureError(() => chat.takeOver?.("global") ?? Promise.reject());
    expect(error).toMatchObject({ code: "preview_unavailable" });
    expect(JSON.stringify(error)).not.toContain("held");

    const after = await chat.attach("global");
    expect(after.writer).toBeUndefined();
    expect(after.access).toBeUndefined();

    bridge.dispose();
  });

  it("makes release an idempotent no-op and never fabricates write access", async () => {
    const bridge = new MockBridge("chat-preview");
    const chat = bridge.chat;
    if (!chat) throw new Error("MockBridge must expose chat");

    await chat.start({
      session: "global",
      project: null,
      location: "local",
      resume: true,
    });
    const first = await chat.releaseWriter?.("global");
    const second = await chat.releaseWriter?.("global");

    expect(first?.writer).toBeUndefined();
    expect(second?.writer).toBeUndefined();
    expect(second?.access).toBeUndefined();
    expect(second).toEqual(first);

    bridge.dispose();
  });

  it("rejects setup takeover with the same typed gate as the host", async () => {
    const bridge = new MockBridge("chat-preview");
    const chat = bridge.chat;
    if (!chat) throw new Error("MockBridge must expose chat");

    await chat.start({
      session: "setup",
      project: null,
      location: "local",
      resume: true,
    });
    const error = await captureError(() => chat.takeOver?.("setup") ?? Promise.reject());
    expect(error).toMatchObject({ code: "gate" });
    expect(String((error as { message?: unknown }).message)).toContain(
      "harness_not_wired",
    );

    bridge.dispose();
  });
});

describe("SpiritBridge writer capability", () => {
  it("keeps both methods optional for the read-only Web bridge", () => {
    const chat = readOnlyChat();
    expect(chat.takeOver).toBeUndefined();
    expect(chat.releaseWriter).toBeUndefined();
  });
});
