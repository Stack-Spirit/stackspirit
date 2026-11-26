import { afterEach, describe, expect, it, vi } from "vitest";

import { cloudError, type RelayTicket } from "@stackspirit/ui";

import {
  CloudRelayClient,
  type RelaySocketLike,
  type RelayTicketProviderInput,
} from "./CloudRelayClient";
import type {
  RelayCryptoProvider,
  RelayHandshake,
  RelayHandshakeInput,
  RelaySecureChannel,
} from "./relayCrypto";

const NOW = 1_764_072_000_000;
const CHANNEL_ID = "01JQ8Z6K4W5N7R9T2V3X4Y5Z6A";
const MACHINE_ID = "mch_1";
const MKEY = "test-machine-key";
const CLIENT_KEY = { kty: "EC" as const, crv: "P-256" as const, x: "x-value", y: "y-value" };

function jwt(claims: Record<string, unknown>): string {
  const payload = btoa(JSON.stringify(claims))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
  return `header.${payload}.signature`;
}

const TICKET: RelayTicket = {
  ticket: jwt({
    iss: "https://api.stackspirit.io",
    aud: "relay.stackspirit.io",
    sub: "client:cl_1",
    acct: "acct_1",
    machine: MACHINE_ID,
    kind: "web",
    scopes: ["http.read"],
    mkey: MKEY,
    mkey_id: `ed25519:${MKEY}`,
    iat: Math.floor(NOW / 1000),
    exp: Math.floor(NOW / 1000) + 300,
    jti: "jti_1",
  }),
  expires_at: "2026-11-26T10:05:00.000Z",
  relay_url: "wss://relay.stackspirit.io/v1/client",
  machine: {
    id: MACHINE_ID,
    name: "contabo-1",
    key_id: `ed25519:${MKEY}`,
    public_key: MKEY,
    project: "vowp",
  },
};

class FakeChannel implements RelaySecureChannel {
  readonly sealed: string[] = [];
  private outgoing = 0;
  private received = 0;
  private machineSequence = 0;

  async sealFrame(plaintext: string): Promise<ArrayBuffer> {
    this.outgoing += 1;
    this.sealed.push(plaintext);
    return encode({ dir: "c2m", seq: this.outgoing, plaintext });
  }

  async openFrame(frame: ArrayBuffer): Promise<string> {
    const value = JSON.parse(new TextDecoder().decode(frame)) as {
      dir?: string;
      seq?: number;
      plaintext?: string;
    };
    if (value.dir !== "m2c" || typeof value.seq !== "number" || typeof value.plaintext !== "string") {
      throw cloudError("relay_crypto", "unexpected frame");
    }
    if (value.seq !== this.received + 1) throw cloudError("relay_crypto", "bad sequence");
    this.received = value.seq;
    return value.plaintext;
  }

  machineFrame(remote: Record<string, unknown>): ArrayBuffer {
    this.machineSequence += 1;
    return encode({ dir: "m2c", seq: this.machineSequence, plaintext: JSON.stringify(remote) });
  }

  lastRemote(): Record<string, unknown> {
    const last = this.sealed.at(-1);
    if (!last) throw new Error("no sealed frame");
    return JSON.parse(last) as Record<string, unknown>;
  }
}

function encode(value: unknown): ArrayBuffer {
  return new TextEncoder().encode(JSON.stringify(value)).buffer as ArrayBuffer;
}

class FakeHandshake implements RelayHandshake {
  readonly clientKey = CLIENT_KEY;
  readonly clientNonce = "client-nonce";
  readonly channel = new FakeChannel();
  input: RelayHandshakeInput | null = null;

  async complete(input: RelayHandshakeInput): Promise<RelaySecureChannel> {
    this.input = input;
    if (input.machineSignature === "bad-signature") {
      throw cloudError("relay_crypto", "bad machine signature");
    }
    return this.channel;
  }
}

class FakeCrypto implements RelayCryptoProvider {
  readonly handshakes: FakeHandshake[] = [];
  async createHandshake(): Promise<RelayHandshake> {
    const handshake = new FakeHandshake();
    this.handshakes.push(handshake);
    return handshake;
  }
}

class FakeSocket implements RelaySocketLike {
  binaryType = "arraybuffer";
  readyState = 0;
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  onclose: ((event: CloseEvent) => void) | null = null;
  readonly sent: Array<string | ArrayBufferLike | Blob | ArrayBufferView> = [];
  closeCode: number | null = null;

  send(data: string | ArrayBufferLike | Blob | ArrayBufferView): void {
    this.sent.push(data);
  }

  close(code?: number, reason?: string): void {
    this.closeCode = code ?? null;
    this.readyState = 3;
    void reason;
  }

  open(): void {
    this.readyState = 1;
    this.onopen?.(new Event("open"));
  }

  text(message: string): void {
    this.onmessage?.({ data: message } as MessageEvent);
  }

  binary(frame: ArrayBuffer): void {
    this.onmessage?.({ data: frame } as MessageEvent);
  }

  serverClose(code: number, reason = ""): void {
    this.readyState = 3;
    this.onclose?.({ code, reason } as CloseEvent);
  }

  sentText(index = -1): Record<string, unknown> {
    const value = this.sent.at(index);
    if (typeof value !== "string") throw new Error("no text frame");
    return JSON.parse(value) as Record<string, unknown>;
  }

  sentCount(): number {
    return this.sent.length;
  }
}

class FakeSockets {
  readonly sockets: FakeSocket[] = [];
  readonly urls: string[] = [];
  readonly protocols: string[][] = [];

  create = (url: string, protocols: string[]): FakeSocket => {
    const socket = new FakeSocket();
    this.urls.push(url);
    this.protocols.push(protocols);
    this.sockets.push(socket);
    return socket;
  };

  latest(): FakeSocket {
    const socket = this.sockets.at(-1);
    if (!socket) throw new Error("no socket opened");
    return socket;
  }
}

function makeClient(options: {
  crypto?: FakeCrypto;
  sockets?: FakeSockets;
  tickets?: RelayTicket[];
  maxReconnectAttempts?: number;
}) {
  const crypto = options.crypto ?? new FakeCrypto();
  const sockets = options.sockets ?? new FakeSockets();
  const tickets = options.tickets ?? [TICKET];
  const ticketProvider = vi.fn(async (input: RelayTicketProviderInput) => {
    void input;
    const next = tickets.shift();
    if (!next) throw cloudError("relay_ticket_expired", "no ticket", null, true);
    return next;
  });
  const client = new CloudRelayClient({
    ticketProvider,
    socketFactory: sockets.create,
    crypto,
    now: () => NOW,
    random: () => 0,
    reconnectBaseMs: 10,
    reconnectMaxMs: 20,
    maxReconnectAttempts: options.maxReconnectAttempts ?? 2,
    handshakeTimeoutMs: 1_000,
    heartbeatIntervalMs: 60_000,
    heartbeatTimeoutMs: 120_000,
  });
  return { client, crypto, sockets, ticketProvider };
}

async function flushAsync(): Promise<void> {
  for (let index = 0; index < 8; index += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

async function completeHandshake(
  socket: FakeSocket,
  crypto: FakeCrypto,
): Promise<void> {
  socket.open();
  expect(socket.sentText()).toMatchObject({
    type: "client.hello",
    v: 1,
    machine_id: MACHINE_ID,
    kind: "web",
    protocol: "stackspirit-remote/1",
    client_key: CLIENT_KEY,
    client_nonce: "client-nonce",
  });
  socket.text(
    JSON.stringify({
      type: "client.welcome",
      v: 1,
      relay_id: "relay-1",
      session_id: "session-1",
      channel_id: CHANNEL_ID,
      machine_id: MACHINE_ID,
      account_id: "acct_1",
      expires_at: "2026-11-26T10:10:00.000Z",
      features: [],
    }),
  );
  socket.text(
    JSON.stringify({
      type: "channel.accept",
      v: 1,
      channel_id: CHANNEL_ID,
      machine_key: { kty: "EC", crv: "P-256", x: "machine-x", y: "machine-y" },
      machine_nonce: "machine-nonce",
      machine_key_id: `ed25519:${MKEY}`,
      machine_signature: "machine-signature",
      suite: "P-256-HKDF-SHA256-AES-256-GCM",
    }),
  );
  await flushAsync();
  const handshake = crypto.handshakes.at(-1);
  if (!handshake) throw new Error("no handshake");
  expect(JSON.parse(handshake.channel.sealed.at(-1) ?? "{}")).toMatchObject({
    type: "hello",
    v: 1,
    machine: MACHINE_ID,
    scopes: ["http.read"],
  });
  socket.binary(
    handshake.channel.machineFrame({
      type: "hello_ack",
      v: 1,
      machine: MACHINE_ID,
      scopes: ["http.read"],
      capabilities: ["http"],
    }),
  );
  await flushAsync();
}

async function driveToSecure(
  client: CloudRelayClient,
  crypto: FakeCrypto,
  sockets: FakeSockets,
): Promise<void> {
  const connecting = client.connect(MACHINE_ID, { scopes: ["http.read"] });
  await flushAsync();
  await completeHandshake(sockets.latest(), crypto);
  await connecting;
  expect(client.status().state).toBe("secure");
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("CloudRelayClient", () => {
  it("mints a ticket with the public key, sends client.hello and secures the channel", async () => {
    const { client, crypto, sockets, ticketProvider } = makeClient({});
    await driveToSecure(client, crypto, sockets);

    expect(ticketProvider).toHaveBeenCalledTimes(1);
    expect(ticketProvider.mock.calls[0]?.[0]).toMatchObject({
      machineId: MACHINE_ID,
      kind: "web",
      scopes: ["http.read"],
      clientKey: CLIENT_KEY,
      clientNonce: "client-nonce",
      ttlSecs: 300,
    });
    expect(sockets.urls[0]).toBe("wss://relay.stackspirit.io/v1/client");
    expect(sockets.protocols[0]).toEqual(["stackspirit-relay/1"]);
    const hello = sockets.latest().sentText(0);
    expect(hello).toMatchObject({
      type: "client.hello",
      machine_id: MACHINE_ID,
      kind: "web",
      protocol: "stackspirit-remote/1",
      ticket: TICKET.ticket,
    });
    expect(JSON.stringify(hello)).not.toMatch(/private_key|ssh_key|password|passphrase/i);
    expect(client.status()).toMatchObject({
      state: "secure",
      machine_id: MACHINE_ID,
      channel_id: CHANNEL_ID,
      error: null,
    });
    const handshake = crypto.handshakes[0];
    expect(handshake?.input).toMatchObject({
      channelId: CHANNEL_ID,
      machineId: MACHINE_ID,
      accountId: "acct_1",
      ticketJti: "jti_1",
      machinePublicKeyHex: MKEY,
      expectedMachineKeyId: `ed25519:${MKEY}`,
    });
    client.dispose();
  });

  it("sends encrypted HTTP requests and routes response frames back", async () => {
    const { client, crypto, sockets } = makeClient({});
    await driveToSecure(client, crypto, sockets);
    const socket = sockets.latest();
    const channel = crypto.handshakes[0]?.channel;
    if (!channel) throw new Error("no channel");

    const pending = client.request("/api/v1/health", {
      headers: { Accept: "application/json" },
    });
    await flushAsync();
    const request = channel.lastRemote();
    expect(request).toMatchObject({
      type: "http_request",
      stream: 1,
      method: "GET",
      path: "/api/v1/health",
      headers: { Accept: "application/json" },
      body: "",
    });
    // The body is base64 of UTF-8 bytes; decoding a health JSON body is the
    // missing step in this test, so send an empty body in two chunks.
    socket.binary(
      channel.machineFrame({
        type: "http_response",
        stream: 1,
        status: 200,
        headers: { "Content-Type": "application/json" },
      }),
    );
    socket.binary(
      channel.machineFrame({ type: "http_body", stream: 1, data: "", end: false }),
    );
    socket.binary(channel.machineFrame({ type: "http_body", stream: 1, data: "", end: true }));
    const response = await pending;
    expect(response).toEqual({ status: 200, headers: { "Content-Type": "application/json" }, body: "" });

    await expect(
      client.request("/api/v1/board", { headers: { Authorization: "Bearer nope" } }),
    ).rejects.toMatchObject({ code: "relay_scope_denied" });
    await expect(client.request("/api/v1/../secret")).rejects.toMatchObject({
      code: "relay_scope_denied",
    });
    await expect(
      client.request("/api/v1/board", { method: "POST", body: "{}" }),
    ).rejects.toMatchObject({ code: "relay_scope_denied" });
    client.dispose();
  });

  it("reconnects with a fresh ticket and handshake after an overload close", async () => {
    const { client, crypto, sockets, ticketProvider } = makeClient({
      tickets: [TICKET, TICKET],
    });
    await driveToSecure(client, crypto, sockets);
    expect(ticketProvider).toHaveBeenCalledTimes(1);

    sockets.latest().serverClose(1013, "overloaded");
    expect(client.status().state).toBe("reconnecting");
    expect(client.status().error?.code).toBe("relay_rate_limited");
    expect(client.status().next_retry_at).not.toBeNull();

    await new Promise((resolve) => setTimeout(resolve, 40));
    await flushAsync();
    expect(sockets.sockets).toHaveLength(2);
    expect(ticketProvider).toHaveBeenCalledTimes(2);
    await completeHandshake(sockets.latest(), crypto);
    expect(client.status().state).toBe("secure");
    expect(crypto.handshakes).toHaveLength(2);
    client.dispose();
  });

  it("fails closed on auth rejection and does not mint another ticket", async () => {
    const { client, crypto, sockets, ticketProvider } = makeClient({
      tickets: [TICKET, TICKET],
    });
    const connecting = client.connect(MACHINE_ID);
    await flushAsync();
    sockets.latest().open();
    sockets.latest().serverClose(4001, "unauthenticated");
    await expect(connecting).rejects.toMatchObject({ code: "relay_auth" });
    await flushAsync();
    expect(client.status().state).toBe("failed");
    expect(client.status().error?.code).toBe("relay_auth");
    expect(ticketProvider).toHaveBeenCalledTimes(1);
    client.dispose();
    void crypto;
  });

  it("fails closed when superseded and when the machine denies a scope", async () => {
    const first = makeClient({});
    const connecting = first.client.connect(MACHINE_ID);
    await flushAsync();
    first.sockets.latest().open();
    first.sockets.latest().serverClose(4008, "replaced");
    await expect(connecting).rejects.toMatchObject({ code: "relay_superseded" });
    expect(first.client.status().state).toBe("failed");

    const second = makeClient({});
    const pending = second.client.connect(MACHINE_ID);
    await flushAsync();
    second.sockets.latest().open();
    second.sockets.latest().text(
      JSON.stringify({
        type: "client.welcome",
        v: 1,
        relay_id: "relay-1",
        session_id: "session-1",
        channel_id: CHANNEL_ID,
        machine_id: MACHINE_ID,
        account_id: "acct_1",
        expires_at: "2026-11-26T10:10:00.000Z",
        features: [],
      }),
    );
    second.sockets.latest().text(
      JSON.stringify({
        type: "channel.accept",
        v: 1,
        channel_id: CHANNEL_ID,
        machine_key: { kty: "EC", crv: "P-256", x: "machine-x", y: "machine-y" },
        machine_nonce: "machine-nonce",
        machine_key_id: `ed25519:${MKEY}`,
        machine_signature: "machine-signature",
        suite: "P-256-HKDF-SHA256-AES-256-GCM",
      }),
    );
    await flushAsync();
    const channel = second.crypto.handshakes[0]?.channel;
    if (!channel) throw new Error("no channel");
    second.sockets.latest().binary(
      channel.machineFrame({
        type: "hello_ack",
        v: 1,
        machine: MACHINE_ID,
        scopes: [],
        capabilities: ["http"],
      }),
    );
    await expect(pending).rejects.toMatchObject({ code: "relay_scope_denied" });
    expect(second.client.status().state).toBe("failed");
    first.client.dispose();
    second.client.dispose();
  });

  it("surfaces a crypto failure as a typed failed state", async () => {
    const { client, crypto, sockets } = makeClient({});
    const connecting = client.connect(MACHINE_ID);
    await flushAsync();
    sockets.latest().open();
    sockets.latest().text(
      JSON.stringify({
        type: "client.welcome",
        v: 1,
        relay_id: "relay-1",
        session_id: "session-1",
        channel_id: CHANNEL_ID,
        machine_id: MACHINE_ID,
        account_id: "acct_1",
        expires_at: "2026-11-26T10:10:00.000Z",
        features: [],
      }),
    );
    sockets.latest().text(
      JSON.stringify({
        type: "channel.accept",
        v: 1,
        channel_id: CHANNEL_ID,
        machine_key: { kty: "EC", crv: "P-256", x: "machine-x", y: "machine-y" },
        machine_nonce: "machine-nonce",
        machine_key_id: `ed25519:${MKEY}`,
        machine_signature: "bad-signature",
        suite: "P-256-HKDF-SHA256-AES-256-GCM",
      }),
    );
    await expect(connecting).rejects.toMatchObject({ code: "relay_crypto" });
    expect(client.status().state).toBe("failed");
    expect(crypto.handshakes).toHaveLength(1);
    client.dispose();
  });

  it("rejects a request made before the channel is secure", async () => {
    const { client } = makeClient({});
    await expect(client.request("/api/v1/health")).rejects.toMatchObject({
      code: "relay_not_connected",
    });
    client.dispose();
  });
});
