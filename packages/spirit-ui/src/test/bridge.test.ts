import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { MockBridge } from "../bridge/MockBridge";
import type { ChatBridgeSurface } from "../bridge/SpiritBridge";
import type {
  ChatEvent,
  ChatStartInput,
  HostDraft,
  NewHostProfile,
  SessionId,
  SessionLocation,
} from "../bridge/wire";

const draft: HostDraft = {
  label: "test-vps",
  ssh: { user: "owner", host: "203.0.113.20", port: 22 },
  identity_file: null,
  vowp_home: null,
  default_project: null,
};

const confirmed: NewHostProfile = {
  ...draft,
  host_key: {
    algorithm: "ssh-ed25519",
    sha256: "SHA256:testFingerprint000000000000000000000000000000",
    pinned_at: "2026-11-14T12:00:00.000Z",
  },
};

/** The wire types deliberately have no endpoint URL, credential or machine-path field. */
const FORBIDDEN_WIRE_KEYS = [
  "url",
  "port",
  "token",
  "authorization",
  "password",
  "key_path",
  "identity_file",
  "directory",
  "binary_path",
  "pid",
  "native_session_id",
  "ssh_target",
  "remote_port",
] as const;

function requireChat(bridge: MockBridge): ChatBridgeSurface {
  const chat = bridge.chat;
  if (!chat) throw new Error("MockBridge must expose chat");
  return chat;
}

function startInput(
  session: SessionId,
  location: SessionLocation = "local",
): ChatStartInput {
  return {
    session,
    project: session.startsWith("project:")
      ? session.slice("project:".length)
      : null,
    location,
    resume: true,
  };
}

function collectKeys(value: unknown, into: Set<string>): void {
  if (Array.isArray(value)) {
    for (const item of value) collectKeys(item, into);
    return;
  }
  if (value && typeof value === "object") {
    for (const [key, child] of Object.entries(value)) {
      into.add(key);
      collectKeys(child, into);
    }
  }
}

function assistantMessages(events: ChatEvent[]) {
  return events
    .filter(
      (event): event is Extract<ChatEvent, { kind: "message" }> =>
        event.kind === "message",
    )
    .map((event) => event.message)
    .filter((message) => message.role === "assistant");
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("MockBridge", () => {
  it("rejects an incomplete host profile", async () => {
    const bridge = new MockBridge("empty");
    await expect(bridge.hosts.add({ ...confirmed, label: "" })).rejects.toMatchObject({
      code: "invalid_input",
    });
    await expect(
      bridge.hosts.add({ ...confirmed, ssh: { ...draft.ssh, port: 0 } }),
    ).rejects.toMatchObject({ code: "invalid_input" });
    await expect(
      bridge.hosts.add({ ...confirmed, ssh: { ...draft.ssh, host: "https://vps.example" } }),
    ).rejects.toMatchObject({ code: "invalid_input" });
  });

  it("saves a valid host and scans a deterministic fingerprint", async () => {
    const bridge = new MockBridge("empty");
    const host = await bridge.hosts.add(confirmed);
    expect(host.label).toBe("test-vps");
    expect(host.host_key.sha256).toBe(confirmed.host_key.sha256);
    const scan = await bridge.hosts.scanFingerprint(draft);
    expect(scan.keys[0]?.preferred).toBe(true);
    expect(scan.keys[0]?.algorithm).toBe("ssh-ed25519");
  });

  it("reports the failing probe stage for a bad SSH backend", async () => {
    const bridge = new MockBridge("probe-failure");
    const probe = await bridge.hosts.probe(draft, confirmed.host_key);
    expect(probe.reachable).toBe(false);
    expect(probe.stages.find((stage) => stage.stage === "ssh")).toMatchObject({
      ok: false,
    });
    expect(probe.stages.find((stage) => stage.stage === "snapshot")?.ok).toBe(false);
  });

  it("emits fleet then connection with increasing revisions on connect", async () => {
    const bridge = new MockBridge("fresh");
    const host = (await bridge.hosts.list())[0];
    if (!host) throw new Error("fixture host missing");
    const events: string[] = [];
    const revisions: number[] = [];
    const unsubscribe = bridge.connection.subscribe((event) => {
      events.push(event.kind);
      revisions.push(event.revision);
    });
    const connection = await bridge.connection.connect(host.id);
    expect(connection.state).toBe("connected");
    expect(events.slice(-2)).toEqual(["fleet", "connection"]);
    expect(revisions).toEqual([...revisions].sort((left, right) => left - right));
    unsubscribe();
    await bridge.connection.disconnect(host.id);
    expect((await bridge.connection.status()).state).toBe("disconnected");
  });

  it("carries parse warnings in the raw document", async () => {
    const bridge = new MockBridge("warnings");
    const host = (await bridge.hosts.list())[0];
    if (!host) throw new Error("fixture host missing");
    await bridge.connection.connect(host.id);
    const raw = await bridge.fleet.rawDocument();
    expect(raw?.status_warnings.map((warning) => warning.code)).toContain("unknown_enum");
    expect(raw?.capacity_warnings.map((warning) => warning.code)).toContain("truncated");
  });
});

describe("MockBridge chat", () => {
  it("starts setup locally and re-attaches the existing view", async () => {
    const bridge = new MockBridge("chat-preview");
    const chat = requireChat(bridge);
    const view = await chat.start(startInput("setup", "server"));
    expect(view).toMatchObject({
      session: "setup",
      location: "local",
      state: "ready",
      attached: true,
      streaming_supported: true,
      error: null,
    });
    expect(view.model).toEqual({
      id: "deepseek/deepseek-flash",
      label: "DeepSeek V4.1 Flash",
      provider: "deepseek",
    });
    await expect(chat.attach("setup")).resolves.toMatchObject({
      state: "ready",
      attached: true,
    });
  });

  it("returns the user message and schedules preview-only assistant messages", async () => {
    const bridge = new MockBridge("chat-preview");
    const chat = requireChat(bridge);
    await chat.start(startInput("global", "server"));
    const events: ChatEvent[] = [];
    const unsubscribe = chat.subscribe((event) => events.push(event));

    const receipt = await chat.send({
      session: "global",
      text: "What's running right now?",
    });
    expect(receipt.accepted).toBe(true);
    expect(receipt.message).toMatchObject({ role: "user", origin: "user" });
    expect(receipt.message.text).toEqual(["What's running right now?"]);

    await vi.advanceTimersByTimeAsync(50);
    unsubscribe();

    const assistant = assistantMessages(events);

    expect(assistant).toHaveLength(1);
    expect(assistant[0]?.origin).toBe("preview");
    expect(assistant[0]?.text[0]).toMatch(/^Preview reply\./);
    expect(assistant[0]?.show).toEqual({ kind: "overview" });
    expect(events.some((event) => event.kind === "done")).toBe(true);
    expect(events.at(-1)).toMatchObject({
      kind: "session",
      session: { session: "global", state: "ready" },
    });
  });

  it("streams preview deltas and finishes every assistant message as preview", async () => {
    const bridge = new MockBridge("chat-streaming");
    const chat = requireChat(bridge);
    await chat.start(startInput("project:vowp", "server"));
    const events: ChatEvent[] = [];
    chat.subscribe((event) => events.push(event));

    await chat.send({ session: "project:vowp", text: "How busy is the server?" });
    await vi.advanceTimersByTimeAsync(250);

    expect(events.filter((event) => event.kind === "delta").length).toBeGreaterThan(0);
    const assistant = assistantMessages(events);
    expect(assistant.length).toBeGreaterThan(0);
    for (const message of assistant) {
      expect(message.origin).toBe("preview");
      expect(message.text[0]).toMatch(/^Preview reply\./);
    }
    const final = assistant.at(-1);
    expect(final?.streaming).toBe(false);
    expect(final?.show).toEqual({ kind: "server" });
  });

  it("rejects a second send while streaming and clears pending timers on stop", async () => {
    // chat-reconnect schedules a connecting state and then a completion; stop must
    // clear both before they fire.
    const bridge = new MockBridge("chat-reconnect");
    const chat = requireChat(bridge);
    await chat.start(startInput("setup"));
    const events: ChatEvent[] = [];
    chat.subscribe((event) => events.push(event));

    await chat.send({ session: "setup", text: "hold the turn open" });
    await expect(
      chat.send({ session: "setup", text: "another turn" }),
    ).rejects.toMatchObject({ code: "chat_busy" });
    await expect(
      chat.send({ session: "setup", text: "   " }),
    ).rejects.toMatchObject({ code: "invalid_input" });

    const stopped = await chat.stop("setup");
    expect(stopped).toMatchObject({ state: "stopped", attached: false });
    expect(
      events.some((event) => event.kind === "closed" && event.reason === "stopped"),
    ).toBe(true);

    const eventCount = events.length;
    await vi.advanceTimersByTimeAsync(1_000);
    expect(events).toHaveLength(eventCount);
    bridge.dispose();
  });

  it("clears scheduled preview events on dispose", async () => {
    const bridge = new MockBridge("chat-reconnect");
    const chat = requireChat(bridge);
    await chat.start(startInput("setup"));
    let deliveries = 0;
    chat.subscribe(() => {
      deliveries += 1;
    });
    await chat.send({ session: "setup", text: "hello" });
    const beforeDispose = deliveries;

    bridge.dispose();
    await vi.advanceTimersByTimeAsync(1_000);
    expect(deliveries).toBe(beforeDispose);
  });

  it("pages the in-memory log backward from newest", async () => {
    const bridge = new MockBridge("chat-preview");
    const chat = requireChat(bridge);
    await chat.start(startInput("setup"));
    await chat.send({ session: "setup", text: "hello" });
    await vi.advanceTimersByTimeAsync(50);

    const full = await chat.log("setup");
    expect(full.messages.map((message) => message.role)).toEqual([
      "user",
      "assistant",
    ]);
    expect(full.next).toBeNull();
    expect(full.truncated).toBe(false);

    const newest = await chat.log("setup", { limit: 1 });
    expect(newest.messages.map((message) => message.role)).toEqual(["assistant"]);
    expect(newest.next).not.toBeNull();

    const older = await chat.log("setup", {
      limit: 1,
      cursor: newest.next,
    });
    expect(older.messages.map((message) => message.role)).toEqual(["user"]);
    expect(older.next).toBeNull();

    await expect(chat.log("setup", { limit: 0 })).rejects.toMatchObject({
      code: "invalid_input",
    });
  });

  it("stops delivering events after unsubscribe", async () => {
    const bridge = new MockBridge("chat-preview");
    const chat = requireChat(bridge);
    await chat.start(startInput("setup"));

    let deliveries = 0;
    const unsubscribe = chat.subscribe(() => {
      deliveries += 1;
    });
    unsubscribe();

    await chat.send({ session: "setup", text: "hello" });
    await vi.advanceTimersByTimeAsync(50);
    expect(deliveries).toBe(0);
  });

  it("keeps endpoint URLs, credentials and machine paths out of wire objects", async () => {
    const bridge = new MockBridge("chat-preview");
    const chat = requireChat(bridge);
    const events: ChatEvent[] = [];
    chat.subscribe((event) => events.push(event));

    const view = await chat.start(startInput("setup"));
    const receipt = await chat.send({ session: "setup", text: "What matters?" });
    await vi.advanceTimersByTimeAsync(50);
    const page = await chat.log("setup");

    const keys = new Set<string>();
    collectKeys({ view, receipt, page, events }, keys);
    for (const key of FORBIDDEN_WIRE_KEYS) {
      expect(keys.has(key), key).toBe(false);
    }
  });

  it("reports typed gates instead of inventing a preview reply", async () => {
    const stopped = requireChat(new MockBridge("chat-engine-stopped"));
    await expect(stopped.start(startInput("setup"))).rejects.toMatchObject({
      code: "engine_not_ready",
    });

    const external = requireChat(new MockBridge("chat-external-engine"));
    await expect(external.start(startInput("setup"))).rejects.toMatchObject({
      code: "engine_external",
    });

    const gated = requireChat(new MockBridge("chat-server-gated"));
    await expect(gated.start(startInput("global", "server"))).rejects.toMatchObject({
      code: "chat_gated",
    });

    const lost = requireChat(new MockBridge("chat-session-lost"));
    await expect(lost.attach("global")).rejects.toMatchObject({
      code: "session_lost",
    });
  });
});
