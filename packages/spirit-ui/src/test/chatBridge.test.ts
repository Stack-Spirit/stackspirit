import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { MockBridge } from "../bridge/MockBridge";
import type { ChatBridgeSurface } from "../bridge/SpiritBridge";
import type {
  ChatEvent,
  ChatLogPage,
  ChatSendReceipt,
  ChatSessionView,
  ChatStartInput,
  SessionId,
} from "../bridge/wire";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
const { listen } = vi.hoisted(() => ({ listen: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));

import { TauriBridge } from "../../../../apps/spirit-desktop/src/bridge/TauriBridge";

const START: ChatStartInput = {
  session: "setup",
  project: null,
  location: "local",
  resume: true,
};

const VIEW: ChatSessionView = {
  session: "setup",
  location: "local",
  state: "ready",
  attached: true,
  streaming_supported: false,
  model: null,
  started_at: "2026-11-18T10:00:00.000Z",
  last_event_at: "2026-11-18T10:00:00.000Z",
  error: null,
};

const RECEIPT: ChatSendReceipt = {
  session: "setup",
  message: {
    id: "local:01",
    role: "user",
    text: ["Hello"],
    created_at: "2026-11-18T10:00:00.000Z",
    origin: "user",
    streaming: false,
  },
  accepted: true,
};

const PAGE: ChatLogPage = {
  messages: [RECEIPT.message],
  next: null,
  truncated: false,
};

const EVENT: ChatEvent = {
  kind: "message",
  revision: 7,
  session: "setup",
  message: RECEIPT.message,
};

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

describe("TauriBridge chat commands", () => {
  beforeEach(() => {
    invoke.mockReset();
    listen.mockReset();
  });

  it("maps every bridge method to the exact command and argument shape", async () => {
    const chat = requireChat(new TauriBridge());

    invoke.mockResolvedValueOnce(VIEW);
    await expect(chat.start(START)).resolves.toEqual(VIEW);
    expect(invoke).toHaveBeenLastCalledWith("chat_start", { input: START });

    invoke.mockResolvedValueOnce(VIEW);
    await expect(chat.attach("setup")).resolves.toEqual(VIEW);
    expect(invoke).toHaveBeenLastCalledWith("chat_attach", { session: "setup" });

    invoke.mockResolvedValueOnce(RECEIPT);
    const input = { session: "setup" as SessionId, text: "Hello" };
    await expect(chat.send(input)).resolves.toEqual(RECEIPT);
    expect(invoke).toHaveBeenLastCalledWith("chat_send", { input });

    invoke.mockResolvedValueOnce(VIEW);
    await expect(chat.stop("setup")).resolves.toEqual(VIEW);
    expect(invoke).toHaveBeenLastCalledWith("chat_stop", { session: "setup" });

    invoke.mockResolvedValueOnce(PAGE);
    await expect(chat.log("setup")).resolves.toEqual(PAGE);
    expect(invoke).toHaveBeenLastCalledWith("chat_log", {
      session: "setup",
      query: null,
    });

    invoke.mockResolvedValueOnce(PAGE);
    await expect(chat.log("setup", { cursor: "10", limit: 20 })).resolves.toEqual(PAGE);
    expect(invoke).toHaveBeenLastCalledWith("chat_log", {
      session: "setup",
      query: { cursor: "10", limit: 20 },
    });

    expect(invoke.mock.calls.map(([command]) => command)).toEqual([
      "chat_start",
      "chat_attach",
      "chat_send",
      "chat_stop",
      "chat_log",
      "chat_log",
    ]);
  });

  it("subscribes to spirit://chat, forwards the payload unchanged and cleans up once", async () => {
    const chat = requireChat(new TauriBridge());
    const listener = vi.fn();
    const unlisten = vi.fn();
    listen.mockResolvedValueOnce(unlisten);

    const unsubscribe = chat.subscribe(listener);
    await vi.waitFor(() => {
      expect(listen).toHaveBeenCalledTimes(1);
    });

    expect(listen).toHaveBeenCalledWith("spirit://chat", expect.any(Function));
    const handler = listen.mock.calls[0]?.[1] as (event: { payload: ChatEvent }) => void;
    handler({ payload: EVENT });
    expect(listener).toHaveBeenCalledWith(EVENT);
    expect(listener.mock.calls[0]?.[0]).toBe(EVENT);

    unsubscribe();
    unsubscribe();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("stops a subscription that resolves after the caller unmounted", async () => {
    const chat = requireChat(new TauriBridge());
    const unlisten = vi.fn();
    let resolveListen: ((stop: () => void) => void) | undefined;
    listen.mockImplementationOnce(
      () =>
        new Promise<() => void>((resolve) => {
          resolveListen = resolve;
        }),
    );

    const unsubscribe = chat.subscribe(vi.fn());
    unsubscribe();
    resolveListen?.(unlisten);
    await vi.waitFor(() => {
      expect(unlisten).toHaveBeenCalledTimes(1);
    });
  });

  it("rejects blank text with the normal typed error and makes no invoke", async () => {
    const chat = requireChat(new TauriBridge());

    for (const text of ["", "   ", "\t\n"]) {
      const error = await captureError(() => chat.send({ session: "setup", text }));
      expect(error).toMatchObject({ code: "invalid_input" });
      expect(String((error as { message?: unknown }).message)).toContain(
        "invalid_text",
      );
    }

    expect(invoke).not.toHaveBeenCalled();
  });

  it("passes a typed host failure through without adding a URL, token or path", async () => {
    const chat = requireChat(new TauriBridge());
    const failure = {
      code: "engine",
      message: "engine_not_ready: the StackSpirit Engine is not ready",
      hint: "Restart StackSpirit Engine in Settings → Advanced and retry.",
      retryable: true,
    };
    invoke.mockRejectedValueOnce(failure);

    const error = await captureError(() => chat.start(START));

    expect(error).toEqual(failure);
    const rendered = JSON.stringify(error);
    for (const forbidden of [
      "http://",
      "https://",
      "127.0.0.1",
      "authorization",
      "token=",
      "/Users/",
      "/home/",
    ]) {
      expect(rendered).not.toContain(forbidden);
    }
  });
});

describe("MockBridge chat preview honesty", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("still exposes chat and only produces preview-origin assistant text", async () => {
    const bridge = new MockBridge("chat-preview");
    const chat = bridge.chat;
    if (!chat) throw new Error("MockBridge must expose chat");
    const events: ChatEvent[] = [];
    chat.subscribe((event) => events.push(event));

    await chat.start({ ...START, resume: true });
    await chat.send({ session: "setup", text: "What matters?" });
    await vi.advanceTimersByTimeAsync(50);

    const assistant = events.flatMap((event) =>
      event.kind === "message" && event.message.role === "assistant"
        ? [event.message]
        : [],
    );
    expect(assistant.length).toBeGreaterThan(0);
    for (const message of assistant) {
      expect(message.origin).toBe("preview");
      expect(message.text[0]).toMatch(/^Preview reply\./);
    }
    expect(events.every((event) => event.kind !== "error")).toBe(true);
    bridge.dispose();
  });
});
