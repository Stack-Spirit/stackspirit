import { afterEach, describe, expect, it } from "vitest";

import type { SpiritBridge } from "../../bridge/SpiritBridge";
import type {
  ChatEvent,
  ChatLogPage,
  ChatSendReceipt,
  ChatSessionView,
} from "../../bridge/wire";
import {
  initializeConversations,
  resetConversations,
  upsertMessage,
  useConversations,
  type ChatMessageView,
} from "../../next/conversations";

const SESSION = "setup" as const;
const T0 = "2026-11-20T10:00:00.000Z";

function at(offsetMs: number): string {
  return new Date(Date.parse(T0) + offsetMs).toISOString();
}

function message(
  overrides: Partial<ChatMessageView> & Pick<ChatMessageView, "id">,
): ChatMessageView {
  return {
    role: "user",
    text: ["hello"],
    created_at: T0,
    origin: "user",
    ...overrides,
  };
}

function chatView(): ChatSessionView {
  return {
    session: SESSION,
    location: "local",
    state: "ready",
    attached: true,
    streaming_supported: true,
    model: null,
    started_at: T0,
    last_event_at: T0,
    error: null,
  };
}

function fakeBridge(
  receipt: ChatSendReceipt,
  page: ChatLogPage = { messages: [], next: null, truncated: false },
): { bridge: SpiritBridge; emit: (event: ChatEvent) => void } {
  let listener: ((event: ChatEvent) => void) | null = null;
  const chat = {
    start: async () => chatView(),
    attach: async () => chatView(),
    send: async () => receipt,
    stop: async () => chatView(),
    log: async () => page,
    subscribe: (next: (event: ChatEvent) => void) => {
      listener = next;
      return () => {
        listener = null;
      };
    },
  };
  return {
    bridge: { kind: "mock", preview: false, chat } as unknown as SpiritBridge,
    emit: (event) => listener?.(event),
  };
}

afterEach(() => {
  resetConversations();
});

describe("optimistic user-turn reconciliation", () => {
  it("merges the native message into an exact-id receipt and clears the marker", () => {
    const receipt = message({ id: "msg_01", optimistic: true, streaming: true });
    const event = message({ id: "msg_01", streaming: false, created_at: at(1_200) });

    const merged = upsertMessage([receipt], event);

    expect(merged).toHaveLength(1);
    expect(merged[0]).toMatchObject({
      id: "msg_01",
      streaming: false,
      created_at: at(1_200),
    });
    expect(merged[0]?.optimistic).toBeUndefined();
  });

  it("replaces an optimistic bubble when the native message has a different id", () => {
    const receipt = message({ id: "msg_01", optimistic: true, created_at: at(500) });
    const native = message({ id: "msg_02", created_at: at(1_200) });

    const merged = upsertMessage([receipt], native);

    expect(merged).toHaveLength(1);
    expect(merged[0]).toMatchObject({ id: "msg_02", created_at: at(1_200) });
    expect(merged[0]?.optimistic).toBeUndefined();
  });

  it("merges a later receipt into the native message without a duplicate", () => {
    const native = message({ id: "msg_02", created_at: at(1_000) });
    const receipt = message({ id: "msg_01", optimistic: true, created_at: at(1_300) });

    const merged = upsertMessage([native], receipt);

    expect(merged).toHaveLength(1);
    expect(merged[0]?.id).toBe("msg_02");
    expect(merged[0]?.optimistic).toBeUndefined();
  });

  it("keeps repeated identical user turns separate", () => {
    const first = message({ id: "msg_01", created_at: at(0) });
    const later = message({ id: "msg_02", created_at: at(6_000) });
    const nearby = message({ id: "msg_03", created_at: at(1_000) });

    expect(upsertMessage([first], later)).toHaveLength(2);
    expect(upsertMessage([first], nearby)).toHaveLength(2);

    const optimisticA = message({ id: "msg_04", optimistic: true, created_at: at(0) });
    const optimisticB = message({ id: "msg_05", optimistic: true, created_at: at(1_000) });
    expect(upsertMessage([optimisticA], optimisticB)).toHaveLength(2);
  });

  it("keeps an unconfirmed receipt as one bubble when the same id arrives", () => {
    const receipt = message({ id: "msg_01", optimistic: true });

    const alone = upsertMessage([], receipt);
    expect(alone).toHaveLength(1);
    expect(alone[0]?.optimistic).toBe(true);

    const confirmed = upsertMessage(alone, message({ id: "msg_01" }));
    expect(confirmed).toHaveLength(1);
    expect(confirmed[0]?.optimistic).toBeUndefined();
  });
});

describe("conversation store optimistic receipts", () => {
  it("marks only the send receipt optimistic and clears it when the native event arrives", async () => {
    const receipt: ChatSendReceipt = {
      session: SESSION,
      accepted: true,
      message: {
        id: "msg_01",
        role: "user",
        text: ["hello"],
        created_at: T0,
        origin: "user",
      },
    };
    const { bridge, emit } = fakeBridge(receipt);
    initializeConversations(bridge);

    await useConversations.getState().open(SESSION, null);
    await useConversations.getState().send(SESSION, "hello");

    const afterSend = useConversations.getState().messages[SESSION] ?? [];
    expect(afterSend).toHaveLength(1);
    expect(afterSend[0]?.id).toBe("msg_01");
    expect(afterSend[0]?.optimistic).toBe(true);

    emit({
      kind: "message",
      revision: 2,
      session: SESSION,
      message: { ...receipt.message, created_at: at(900) },
    });

    const afterEvent = useConversations.getState().messages[SESSION] ?? [];
    expect(afterEvent).toHaveLength(1);
    expect(afterEvent[0]?.id).toBe("msg_01");
    expect(afterEvent[0]?.created_at).toBe(at(900));
    expect(afterEvent[0]?.optimistic).toBeUndefined();
  });
});
