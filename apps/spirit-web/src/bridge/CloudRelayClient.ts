/**
 * The StackSpirit Cloud Relay client for the Web/PWA.
 *
 * It implements the typed client half of `runtime/docs/design/relay-and-packaging.md`
 * §6.3–§6.7:
 *
 * 1. mint a short-lived, scoped ticket with the public ephemeral P-256 key;
 * 2. open `wss://relay.stackspirit.io/v1/client` with the
 *    `stackspirit-relay/1` subprotocol and send `client.hello`;
 * 3. verify the machine's Ed25519 signature and derive the E2E channel;
 * 4. send an encrypted `hello` / await `hello_ack`;
 * 5. issue `http_request` frames and surface typed failures;
 * 6. reconnect with exponential backoff, minting a fresh ticket every attempt.
 *
 * Tests inject a fake socket, a fake ticket provider and a fake crypto provider;
 * no live network call is made anywhere in the unit suite.
 */

import {
  cloudError,
  type ClientKind,
  type CloudError,
  type CloudRelayConnector,
  type RelayConnectionStatus,
  type RelayHttpInit,
  type RelayHttpResponse,
  type RelayPublicJwk,
  type RelayScope,
  type RelayTicket,
} from "@stackspirit/ui";

import {
  RELAY_MAX_CONTROL_BYTES,
  RELAY_MAX_DATA_BYTES,
  REMOTE_MAX_PLAINTEXT_BYTES,
  REMOTE_PROTOCOL,
  RELAY_PROTOCOL,
  bytesToBase64,
  base64ToBytes,
  decodeTicketClaims,
  parseRemoteFrame,
  parseServerRelayFrame,
  relayErrorForClose,
  relayErrorForServerCode,
  utf8Bytes,
  type RemoteFrame,
  type ServerRelayFrame,
} from "./relayProtocol";
import {
  createWebCryptoProvider,
  type RelayCryptoProvider,
  type RelayHandshake,
  type RelaySecureChannel,
} from "./relayCrypto";

export interface RelaySocketLike {
  binaryType?: string;
  readonly readyState: number;
  onopen: ((event: Event) => void) | null;
  onmessage: ((event: MessageEvent) => void) | null;
  onerror: ((event: Event) => void) | null;
  onclose: ((event: CloseEvent) => void) | null;
  send(data: string | ArrayBufferLike | Blob | ArrayBufferView): void;
  close(code?: number, reason?: string): void;
}

export type RelaySocketFactory = (url: string, protocols: string[]) => RelaySocketLike;

export interface RelayTicketProviderInput {
  machineId: string;
  kind: ClientKind;
  scopes: RelayScope[];
  clientKey: RelayPublicJwk;
  clientNonce: string;
  ttlSecs: number;
}

export type RelayTicketProvider = (input: RelayTicketProviderInput) => Promise<RelayTicket>;

export interface CloudRelayClientOptions {
  ticketProvider: RelayTicketProvider;
  socketFactory?: RelaySocketFactory;
  crypto?: RelayCryptoProvider;
  now?: () => number;
  /** Deterministic jitter hook for tests; production uses `Math.random`. */
  random?: () => number;
  reconnectBaseMs?: number;
  reconnectMaxMs?: number;
  /** `Infinity` by default, matching the connector's backoff-to-60s rule. */
  maxReconnectAttempts?: number;
  handshakeTimeoutMs?: number;
  requestTimeoutMs?: number;
  heartbeatIntervalMs?: number;
  heartbeatTimeoutMs?: number;
  clientId?: string;
  /** Read-only by default (§6.6 requires an explicit machine-side write grant). */
  allowWrites?: boolean;
}

interface PendingRequest {
  resolve: (response: RelayHttpResponse) => void;
  reject: (error: CloudError) => void;
  timer: ReturnType<typeof setTimeout>;
  status: number | null;
  headers: Record<string, string>;
  chunks: Uint8Array[];
  bytes: number;
}

interface ConnectWaiter {
  resolve: () => void;
  reject: (error: CloudError) => void;
}

const DEFAULT_RECONNECT_BASE_MS = 1_000;
const DEFAULT_RECONNECT_MAX_MS = 60_000;
const DEFAULT_HANDSHAKE_TIMEOUT_MS = 5_000;
const DEFAULT_REQUEST_TIMEOUT_MS = 20_000;
const DEFAULT_HEARTBEAT_INTERVAL_MS = 20_000;
const DEFAULT_HEARTBEAT_TIMEOUT_MS = 60_000;
const MAX_QUERY_CHARS = 2_048;
const MAX_RESPONSE_BYTES = 8 * 1024 * 1024;
const ALLOWED_REQUEST_HEADERS = new Set(["accept", "content-type", "last-event-id", "if-none-match"]);
const FORBIDDEN_QUERY_KEYS = new Set(["token", "key", "secret", "auth", "code"]);

function defaultSocketFactory(url: string, protocols: string[]): RelaySocketLike {
  if (typeof WebSocket === "undefined") {
    throw cloudError(
      "cloud_relay_unavailable",
      "This environment cannot open a WebSocket.",
      "Use a current browser. Local server mode needs no relay.",
    );
  }
  const socket = new WebSocket(url, protocols);
  socket.binaryType = "arraybuffer";
  return socket as unknown as RelaySocketLike;
}

function iso(ms: number): string {
  return new Date(ms).toISOString();
}

function cloneStatus(status: RelayConnectionStatus): RelayConnectionStatus {
  return {
    ...status,
    error: status.error ? { ...status.error } : null,
  };
}

function initialStatus(): RelayConnectionStatus {
  return {
    state: "idle",
    machine_id: null,
    channel_id: null,
    attempts: 0,
    next_retry_at: null,
    error: null,
  };
}

function validateRequestPath(path: string): string {
  if (
    !path.startsWith("/api/v1/") ||
    path.includes("..") ||
    path.includes("//") ||
    path.includes("\\") ||
    path.includes("\0") ||
    path.includes("\r") ||
    path.includes("\n") ||
    path.includes("#") ||
    /%(?:2f|5c)/i.test(path)
  ) {
    throw cloudError("relay_scope_denied", "The relay request path is not an /api/v1/ path.");
  }
  const queryIndex = path.indexOf("?");
  const query = queryIndex === -1 ? "" : path.slice(queryIndex + 1);
  if (query.length > MAX_QUERY_CHARS) {
    throw cloudError("relay_scope_denied", "The relay request query is too long.");
  }
  if (query) {
    for (const [key] of new URLSearchParams(query)) {
      if (FORBIDDEN_QUERY_KEYS.has(key.toLowerCase())) {
        throw cloudError(
          "relay_scope_denied",
          "The relay request query names a credential field; credentials never travel in a URL.",
        );
      }
    }
  }
  return path;
}

function validateHeaders(headers: Record<string, string> | undefined): Record<string, string> {
  if (!headers) return {};
  const allowed: Record<string, string> = {};
  for (const [name, value] of Object.entries(headers)) {
    const lower = name.toLowerCase();
    if (!ALLOWED_REQUEST_HEADERS.has(lower)) {
      throw cloudError(
        "relay_scope_denied",
        `The relay request header ${name} is not allowed.`,
        "Only Accept, Content-Type, Last-Event-ID and If-None-Match may cross the relay.",
      );
    }
    allowed[name] = value;
  }
  return allowed;
}

async function binaryFrame(data: unknown): Promise<ArrayBuffer> {
  if (typeof data === "object" && data !== null) {
    // Cross-realm ArrayBuffers (jsdom/Node, a Web Worker) still carry the tag.
    if (Object.prototype.toString.call(data) === "[object ArrayBuffer]") {
      return (data as ArrayBuffer).slice(0);
    }
    if (typeof Blob !== "undefined" && data instanceof Blob) {
      return data.arrayBuffer();
    }
    if (ArrayBuffer.isView(data)) {
      const view = data as ArrayBufferView;
      const copy = new Uint8Array(view.byteLength);
      copy.set(new Uint8Array(view.buffer, view.byteOffset, view.byteLength));
      return copy.buffer;
    }
  }
  throw cloudError("relay_protocol", "The relay sent an unreadable binary frame.");
}

/** The production relay transport. Construction makes no socket and no request. */
export class CloudRelayClient implements CloudRelayConnector {
  private readonly ticketProvider: RelayTicketProvider;
  private readonly socketFactory: RelaySocketFactory;
  private readonly crypto: RelayCryptoProvider;
  private readonly now: () => number;
  private readonly random: () => number;
  private readonly reconnectBaseMs: number;
  private readonly reconnectMaxMs: number;
  private readonly maxReconnectAttempts: number;
  private readonly handshakeTimeoutMs: number;
  private readonly requestTimeoutMs: number;
  private readonly heartbeatIntervalMs: number;
  private readonly heartbeatTimeoutMs: number;
  private readonly clientId: string;
  private readonly allowWrites: boolean;

  private statusValue: RelayConnectionStatus = initialStatus();
  private readonly listeners = new Set<(status: RelayConnectionStatus) => void>();
  private disposed = false;
  private manualClose = false;
  private hadSecure = false;
  private reconnectAttempts = 0;
  private attempts = 0;
  private machineId: string | null = null;
  private scopes: RelayScope[] = ["http.read"];

  private socket: RelaySocketLike | null = null;
  private handshake: RelayHandshake | null = null;
  private ticket: RelayTicket | null = null;
  private claimsJti: string | null = null;
  private machinePublicKeyHex: string | null = null;
  private expectedMachineKeyId: string | null = null;
  private accountId: string | null = null;
  private welcomeChannelId: string | null = null;
  private channel: RelaySecureChannel | null = null;
  private lastFrameAt = 0;

  private handshakeTimer: ReturnType<typeof setTimeout> | null = null;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private heartbeatTimer: ReturnType<typeof setInterval> | null = null;
  private connectWaiter: ConnectWaiter | null = null;
  private connectPromise: Promise<void> | null = null;
  private pending = new Map<number, PendingRequest>();
  private nextStream = 0;

  constructor(options: CloudRelayClientOptions) {
    this.ticketProvider = options.ticketProvider;
    this.socketFactory = options.socketFactory ?? defaultSocketFactory;
    this.crypto = options.crypto ?? createWebCryptoProvider();
    this.now = options.now ?? (() => Date.now());
    this.random = options.random ?? (() => Math.random());
    this.reconnectBaseMs = options.reconnectBaseMs ?? DEFAULT_RECONNECT_BASE_MS;
    this.reconnectMaxMs = options.reconnectMaxMs ?? DEFAULT_RECONNECT_MAX_MS;
    this.maxReconnectAttempts = options.maxReconnectAttempts ?? Number.POSITIVE_INFINITY;
    this.handshakeTimeoutMs = options.handshakeTimeoutMs ?? DEFAULT_HANDSHAKE_TIMEOUT_MS;
    this.requestTimeoutMs = options.requestTimeoutMs ?? DEFAULT_REQUEST_TIMEOUT_MS;
    this.heartbeatIntervalMs = options.heartbeatIntervalMs ?? DEFAULT_HEARTBEAT_INTERVAL_MS;
    this.heartbeatTimeoutMs = options.heartbeatTimeoutMs ?? DEFAULT_HEARTBEAT_TIMEOUT_MS;
    this.clientId = options.clientId ?? makeClientId();
    this.allowWrites = options.allowWrites ?? false;
  }

  status(): RelayConnectionStatus {
    return cloneStatus(this.statusValue);
  }

  subscribe(listener: (status: RelayConnectionStatus) => void): () => void {
    if (this.disposed) return () => undefined;
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  connect(machineId: string, options?: { scopes?: RelayScope[] }): Promise<void> {
    if (this.disposed) {
      return Promise.reject(cloudError("relay_closed", "The relay client is disposed."));
    }
    if (
      this.statusValue.state === "secure" &&
      this.statusValue.machine_id === machineId
    ) {
      return Promise.resolve();
    }
    if (this.connectPromise) return this.connectPromise;
    this.machineId = machineId;
    this.scopes = options?.scopes && options.scopes.length > 0 ? [...options.scopes] : ["http.read"];
    this.manualClose = false;
    this.hadSecure = false;
    this.reconnectAttempts = 0;
    this.attempts = 0;
    this.clearTimers();
    this.closeSocketSilently();
    this.rejectPending(
      cloudError("relay_closed", "The relay connection was replaced before it completed."),
    );
    this.setStatus({
      state: "connecting",
      machine_id: machineId,
      channel_id: null,
      attempts: 0,
      next_retry_at: null,
      error: null,
    });
    const promise = new Promise<void>((resolve, reject) => {
      this.connectWaiter = { resolve, reject };
    }).finally(() => {
      this.connectPromise = null;
    });
    this.connectPromise = promise;
    void this.openAttempt("initial");
    return promise;
  }

  disconnect(): void {
    this.manualClose = true;
    this.clearTimers();
    this.closeSocketSilently();
    this.channel = null;
    this.welcomeChannelId = null;
    this.rejectPending(cloudError("relay_closed", "The relay connection was closed."));
    if (this.connectWaiter) {
      this.connectWaiter.reject(cloudError("relay_closed", "The relay connection was cancelled."));
      this.connectWaiter = null;
    }
    this.setStatus({
      state: "closed",
      machine_id: this.machineId,
      channel_id: null,
      attempts: this.attempts,
      next_retry_at: null,
      error: null,
    });
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposeInternal();
  }

  private disposeInternal(): void {
    this.disposed = true;
    this.manualClose = true;
    this.clearTimers();
    this.closeSocketSilently();
    this.channel = null;
    this.rejectPending(cloudError("relay_closed", "The relay client was disposed."));
    if (this.connectWaiter) {
      this.connectWaiter.reject(cloudError("relay_closed", "The relay client was disposed."));
      this.connectWaiter = null;
    }
    this.setStatus({ state: "closed", next_retry_at: null });
    this.listeners.clear();
  }

  async request(path: string, init: RelayHttpInit = {}): Promise<RelayHttpResponse> {
    if (this.disposed) throw cloudError("relay_closed", "The relay client is disposed.");
    if (this.statusValue.state !== "secure" || !this.channel || !this.socket) {
      throw cloudError(
        "relay_not_connected",
        "The relay channel is not secure yet.",
        "Connect to an Always-On Home first.",
        true,
      );
    }
    const safePath = validateRequestPath(path);
    const method = init.method ?? "GET";
    if (method !== "GET" && method !== "POST") {
      throw cloudError("invalid_input", "Only GET and POST cross the relay.");
    }
    if (method === "POST" && !this.allowWrites) {
      throw cloudError(
        "relay_scope_denied",
        "This relay client is read-only.",
        "Enable the machine's write scope and this client's write mode explicitly.",
      );
    }
    const headers = validateHeaders(init.headers);
    const body = init.body ?? "";
    if (method === "GET" && body.length > 0) {
      throw cloudError("invalid_input", "A relay GET request cannot carry a body.");
    }
    if (body.length > REMOTE_MAX_PLAINTEXT_BYTES / 2) {
      throw cloudError("invalid_input", "The relay request body is too large.");
    }
    const stream = this.allocateStream();
    const response = new Promise<RelayHttpResponse>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(stream);
        void this.sendRemote({ type: "http_cancel", stream }).catch(() => undefined);
        reject(
          cloudError(
            "relay_timeout",
            "The Always-On Home did not answer in time.",
            "Retry the read; the channel is still open.",
            true,
          ),
        );
      }, this.requestTimeoutMs);
      this.pending.set(stream, {
        resolve,
        reject,
        timer,
        status: null,
        headers: {},
        chunks: [],
        bytes: 0,
      });
    });
    try {
      await this.sendRemote({
        type: "http_request",
        stream,
        method,
        path: safePath,
        headers,
        body: body ? bytesToBase64(utf8Bytes(body)) : "",
      });
    } catch (error) {
      const pending = this.pending.get(stream);
      if (pending) {
        clearTimeout(pending.timer);
        this.pending.delete(stream);
      }
      throw errorToCloudError(error, "The relay request could not be sent.");
    }
    return response;
  }

  private allocateStream(): number {
    this.nextStream = this.nextStream >= 2_147_483_647 ? 1 : this.nextStream + 1;
    if (this.pending.has(this.nextStream)) {
      throw cloudError("relay_rate_limited", "Every relay stream is busy.", null, true);
    }
    return this.nextStream;
  }

  private async openAttempt(mode: "initial" | "reconnect"): Promise<void> {
    if (this.disposed || this.manualClose || !this.machineId) return;
    this.attempts += 1;
    this.clearAttemptTimers();
    this.closeSocketSilently();
    this.handshake = null;
    this.ticket = null;
    this.claimsJti = null;
    this.machinePublicKeyHex = null;
    this.expectedMachineKeyId = null;
    this.accountId = null;
    this.welcomeChannelId = null;
    this.channel = null;
    this.setStatus({
      state: mode === "initial" && this.attempts === 1 ? "connecting" : "reconnecting",
      machine_id: this.machineId,
      channel_id: null,
      attempts: this.attempts,
      next_retry_at: null,
      error: null,
    });
    try {
      const handshake = await this.crypto.createHandshake();
      this.handshake = handshake;
      const ticket = await this.ticketProvider({
        machineId: this.machineId,
        kind: "web",
        scopes: this.scopes,
        clientKey: handshake.clientKey,
        clientNonce: handshake.clientNonce,
        ttlSecs: 300,
      });
      this.ticket = ticket;
      if (!isSafeRelayUrl(ticket.relay_url)) {
        throw cloudError("relay_protocol", "The relay ticket returned an unsafe relay URL.");
      }
      const claims = decodeTicketClaims(ticket.ticket);
      if (!claims.jti || !claims.mkey) {
        throw cloudError(
          "relay_protocol",
          "The relay ticket is missing its machine key or ticket id.",
        );
      }
      if (claims.machine && claims.machine !== ticket.machine.id) {
        throw cloudError("relay_protocol", "The relay ticket is for a different machine.");
      }
      if (claims.mkey_id && claims.mkey_id !== ticket.machine.key_id) {
        throw cloudError("relay_protocol", "The relay ticket machine key id does not match.");
      }
      if (claims.exp !== undefined && claims.exp * 1000 <= this.now()) {
        throw cloudError(
          "relay_ticket_expired",
          "The relay ticket was already expired.",
          "Minting a fresh ticket.",
          true,
        );
      }
      this.claimsJti = claims.jti;
      this.machinePublicKeyHex = claims.mkey;
      this.expectedMachineKeyId = claims.mkey_id ?? ticket.machine.key_id;
      const socket = this.socketFactory(ticket.relay_url, [RELAY_PROTOCOL]);
      if (this.disposed || this.manualClose) {
        try {
          socket.close(1000, "client cancelled");
        } catch {
          // A socket that never opened is already closed.
        }
        return;
      }
      this.socket = socket;
      this.bindSocket(socket);
      this.handshakeTimer = setTimeout(() => {
        void this.handleAttemptFailure(
          cloudError(
            "relay_timeout",
            "The relay or machine did not finish the handshake in 5 seconds.",
            "Reconnecting.",
            true,
          ),
          mode,
        );
      }, this.handshakeTimeoutMs);
    } catch (error) {
      await this.handleAttemptFailure(
        errorToCloudError(error, "The relay connection could not start."),
        mode,
      );
    }
  }

  private bindSocket(socket: RelaySocketLike): void {
    socket.onopen = () => {
      if (socket !== this.socket || !this.handshake || !this.ticket || !this.machineId) return;
      this.setStatus({ state: "authenticating", error: null });
      try {
        socket.send(
          JSON.stringify({
            type: "client.hello",
            v: 1,
            machine_id: this.machineId,
            ticket: this.ticket.ticket,
            client_id: this.clientId,
            kind: "web",
            protocol: REMOTE_PROTOCOL,
            capabilities: [],
            client_key: this.handshake.clientKey,
            client_nonce: this.handshake.clientNonce,
            sent_at: iso(this.now()),
          }),
        );
      } catch {
        void this.handleAttemptFailure(
          cloudError("relay_closed", "The relay socket rejected the client hello.", null, true),
          this.hadSecure ? "reconnect" : "initial",
        );
      }
    };
    socket.onmessage = (event: MessageEvent) => {
      if (socket !== this.socket) return;
      void this.handleSocketMessage(event.data);
    };
    socket.onerror = () => {
      // Browsers deliver no detail here. The close event carries the code; the
      // handshake timer covers a socket that stops responding without closing.
    };
    socket.onclose = (event: CloseEvent) => {
      this.handleSocketClose(socket, event);
    };
  }

  private async handleSocketMessage(data: unknown): Promise<void> {
    if (this.disposed || this.manualClose) return;
    this.lastFrameAt = this.now();
    if (typeof data === "string") {
      let frame: ServerRelayFrame;
      try {
        frame = parseServerRelayFrame(data);
      } catch (error) {
        this.fail(errorToCloudError(error, "The relay sent an unreadable frame."));
        return;
      }
      await this.handleControl(frame);
      return;
    }
    let frameBytes: ArrayBuffer;
    try {
      frameBytes = await binaryFrame(data);
    } catch (error) {
      this.fail(errorToCloudError(error, "The relay sent an unreadable binary frame."));
      return;
    }
    if (frameBytes.byteLength > RELAY_MAX_DATA_BYTES) {
      this.fail(cloudError("relay_frame_too_large", "The relay sent an oversized data frame."));
      return;
    }
    if (!this.channel) {
      this.fail(cloudError("relay_protocol", "The relay sent channel data before the handshake."));
      return;
    }
    const channel = this.channel;
    let plaintext: string;
    try {
      plaintext = await channel.openFrame(frameBytes);
    } catch (error) {
      this.fail(errorToCloudError(error, "The encrypted relay frame failed."));
      return;
    }
    if (this.disposed || this.manualClose || this.channel !== channel) return;
    let remote: RemoteFrame;
    try {
      remote = parseRemoteFrame(plaintext);
    } catch (error) {
      this.fail(errorToCloudError(error, "The machine sent an unreadable remote frame."));
      return;
    }
    await this.handleRemote(remote);
  }

  private async handleControl(frame: ServerRelayFrame): Promise<void> {
    switch (frame.type) {
      case "client.welcome": {
        if (this.welcomeChannelId !== null) {
          this.fail(cloudError("relay_protocol", "The relay sent a second client welcome."));
          return;
        }
        if (this.machineId && frame.machine_id !== this.machineId) {
          this.fail(cloudError("relay_protocol", "The relay welcomed a different machine."));
          return;
        }
        this.welcomeChannelId = frame.channel_id;
        this.accountId = frame.account_id;
        this.setStatus({ channel_id: frame.channel_id });
        return;
      }
      case "channel.accept": {
        await this.acceptChannel(frame);
        return;
      }
      case "channel.reject": {
        const error = relayErrorForServerCode(frame.code, frame.message, frame.retryable);
        if (error.retryable && this.hadSecure) this.scheduleReconnect(error);
        else this.fail(error);
        return;
      }
      case "channel.close": {
        const error = cloudError(
          frame.code === "machine_offline" ? "relay_offline" : "relay_closed",
          frame.message || "The relay closed the channel.",
          null,
          frame.code !== "closed",
        );
        await this.handleAttemptFailure(error, this.hadSecure ? "reconnect" : "initial");
        return;
      }
      case "superseded": {
        this.fail(relayErrorForClose(4008, frame.reason));
        return;
      }
      case "ping": {
        this.sendControl({ type: "pong", v: 1, nonce: frame.nonce });
        return;
      }
      case "pong":
        return;
      case "error": {
        const error = relayErrorForServerCode(frame.code, frame.message, frame.retryable);
        if (error.retryable && this.hadSecure) {
          await this.handleAttemptFailure(error, "reconnect");
        } else {
          this.fail(error);
        }
        return;
      }
    }
  }

  private async acceptChannel(
    frame: Extract<ServerRelayFrame, { type: "channel.accept" }>,
  ): Promise<void> {
    const handshake = this.handshake;
    const ticket = this.ticket;
    const channelId = this.welcomeChannelId;
    const accountId = this.accountId;
    const claimsJti = this.claimsJti;
    const machinePublicKeyHex = this.machinePublicKeyHex;
    const expectedMachineKeyId = this.expectedMachineKeyId;
    if (!handshake || !ticket || !channelId || !accountId || !claimsJti || !machinePublicKeyHex || !expectedMachineKeyId) {
      this.fail(cloudError("relay_protocol", "The relay accepted a channel before the client handshake."));
      return;
    }
    if (frame.channel_id !== channelId) {
      this.fail(cloudError("relay_protocol", "The machine accepted a different channel."));
      return;
    }
    let channel: RelaySecureChannel;
    try {
      channel = await handshake.complete({
        channelId,
        machineKey: frame.machine_key,
        machineNonce: frame.machine_nonce,
        machineKeyId: frame.machine_key_id,
        machineSignature: frame.machine_signature,
        machineId: ticket.machine.id,
        accountId,
        ticketJti: claimsJti,
        machinePublicKeyHex,
        expectedMachineKeyId,
      });
    } catch (error) {
      this.fail(errorToCloudError(error, "The encrypted relay handshake failed."));
      return;
    }
    if (this.disposed || this.manualClose || this.statusValue.state === "failed") return;
    this.channel = channel;
    this.setStatus({ state: "authenticating", channel_id: channelId });
    try {
      await this.sendRemote({
        type: "hello",
        v: 1,
        client: this.clientId,
        machine: ticket.machine.id,
        scopes: this.scopes,
        capabilities: ["http"],
      });
    } catch (error) {
      this.fail(errorToCloudError(error, "The encrypted client hello could not be sent."));
    }
  }

  private async handleRemote(frame: RemoteFrame): Promise<void> {
    switch (frame.type) {
      case "hello_ack": {
        if (this.statusValue.state === "secure") return;
        if (this.ticket && frame.machine !== this.ticket.machine.id) {
          this.fail(cloudError("relay_protocol", "The machine acknowledged a different machine id."));
          return;
        }
        const missing = this.scopes.filter((scope) => !frame.scopes.includes(scope));
        if (missing.length > 0) {
          this.fail(
            cloudError(
              "relay_scope_denied",
              "The machine did not grant the requested relay scope.",
              `Missing: ${missing.join(", ")}.`,
            ),
          );
          return;
        }
        this.hadSecure = true;
        this.reconnectAttempts = 0;
        this.clearHandshakeTimer();
        this.startHeartbeat();
        this.setStatus({
          state: "secure",
          error: null,
          next_retry_at: null,
          channel_id: this.welcomeChannelId,
        });
        if (this.connectWaiter) {
          this.connectWaiter.resolve();
          this.connectWaiter = null;
        }
        return;
      }
      case "http_response": {
        const pending = this.pending.get(frame.stream);
        if (!pending) return;
        pending.status = frame.status;
        pending.headers = { ...frame.headers };
        return;
      }
      case "http_body": {
        const pending = this.pending.get(frame.stream);
        if (!pending) return;
        let chunk: Uint8Array;
        try {
          chunk = frame.data ? base64ToBytes(frame.data) : new Uint8Array();
        } catch (error) {
          this.rejectRequest(frame.stream, errorToCloudError(error, "The machine sent unreadable HTTP data."));
          return;
        }
        pending.bytes += chunk.byteLength;
        if (pending.bytes > MAX_RESPONSE_BYTES) {
          this.rejectRequest(
            frame.stream,
            cloudError("relay_frame_too_large", "The relay HTTP response exceeded 8 MiB."),
          );
          void this.sendRemote({ type: "http_cancel", stream: frame.stream }).catch(() => undefined);
          return;
        }
        pending.chunks.push(chunk);
        if (frame.end) this.resolveRequest(frame.stream);
        return;
      }
      case "stream_error": {
        const pending = this.pending.get(frame.stream);
        if (!pending) return;
        this.rejectRequest(
          frame.stream,
          cloudError(
            "relay_protocol",
            frame.message || "The machine returned a stream error.",
            null,
            frame.retryable,
          ),
        );
        return;
      }
      case "ping": {
        void this.sendRemote({ type: "pong", nonce: frame.nonce }).catch(() => undefined);
        return;
      }
      case "pong":
        return;
      case "close": {
        this.fail(
          cloudError("relay_closed", frame.message || "The machine closed the relay channel."),
        );
        return;
      }
      default:
        // Other remote frame families (MCP/TCP/exec) are not used by this
        // read-only Web bridge. A known frame is ignored rather than misread.
        return;
    }
  }

  private resolveRequest(stream: number): void {
    const pending = this.pending.get(stream);
    if (!pending) return;
    this.pending.delete(stream);
    clearTimeout(pending.timer);
    if (pending.status === null) {
      pending.reject(cloudError("relay_protocol", "The machine ended a response without a status."));
      return;
    }
    const total = pending.chunks.reduce((sum, chunk) => sum + chunk.byteLength, 0);
    const merged = new Uint8Array(total);
    let offset = 0;
    for (const chunk of pending.chunks) {
      merged.set(chunk, offset);
      offset += chunk.byteLength;
    }
    pending.resolve({
      status: pending.status,
      headers: pending.headers,
      body: new TextDecoder().decode(merged),
    });
  }

  private rejectRequest(stream: number, error: CloudError): void {
    const pending = this.pending.get(stream);
    if (!pending) return;
    this.pending.delete(stream);
    clearTimeout(pending.timer);
    pending.reject(error);
  }

  private rejectPending(error: CloudError): void {
    for (const stream of [...this.pending.keys()]) this.rejectRequest(stream, error);
  }

  private async sendRemote(frame: RemoteFrame): Promise<void> {
    const socket = this.socket;
    const channel = this.channel;
    if (!socket || !channel) {
      throw cloudError("relay_not_connected", "The relay channel is not open.");
    }
    const plaintext = JSON.stringify(frame);
    if (utf8Bytes(plaintext).byteLength > REMOTE_MAX_PLAINTEXT_BYTES) {
      throw cloudError("relay_protocol", "A remote frame exceeds the 768 KiB plaintext limit.");
    }
    const sealed = await channel.sealFrame(plaintext);
    if (this.socket !== socket) {
      throw cloudError("relay_closed", "The relay socket changed before the frame was sent.");
    }
    try {
      socket.send(sealed);
    } catch {
      throw cloudError("relay_closed", "The relay socket rejected a frame.", null, true);
    }
  }

  private sendControl(frame: Extract<ServerRelayFrame, { type: "pong" }>): void {
    const socket = this.socket;
    if (!socket) return;
    const text = JSON.stringify(frame);
    if (utf8Bytes(text).byteLength > RELAY_MAX_CONTROL_BYTES) return;
    try {
      socket.send(text);
    } catch {
      // A failed pong is a dead socket; the close handler will reconnect.
    }
  }

  private handleSocketClose(socket: RelaySocketLike, event: CloseEvent): void {
    if (socket !== this.socket) return;
    this.socket = null;
    this.channel = null;
    this.clearHandshakeTimer();
    this.stopHeartbeat();
    this.rejectPending(
      cloudError(
        "relay_closed",
        "The relay closed before the response finished.",
        null,
        true,
      ),
    );
    if (this.disposed || this.manualClose) {
      this.setStatus({ state: "closed", channel_id: null, next_retry_at: null });
      return;
    }
    const error = relayErrorForClose(event.code, event.reason);
    if (this.hadSecure && error.retryable) {
      this.scheduleReconnect(error);
      return;
    }
    if (this.hadSecure && (event.code === 1000 || event.code === 4008)) {
      this.fail(error);
      return;
    }
    if (!this.hadSecure) {
      this.fail(error);
      return;
    }
    this.fail(error);
  }

  private async handleAttemptFailure(
    error: CloudError,
    mode: "initial" | "reconnect",
  ): Promise<void> {
    if (this.disposed || this.manualClose) return;
    this.clearAttemptTimers();
    this.closeSocketSilently();
    this.channel = null;
    this.welcomeChannelId = null;
    if (mode === "initial") {
      this.fail(error);
      return;
    }
    if (error.retryable) {
      this.scheduleReconnect(error);
    } else {
      this.fail(error);
    }
  }

  private scheduleReconnect(error: CloudError): void {
    if (this.disposed || this.manualClose) return;
    if (this.reconnectTimer !== null) return;
    if (this.reconnectAttempts >= this.maxReconnectAttempts) {
      this.fail(
        cloudError(
          "relay_closed",
          "The relay could not be reached after repeated attempts.",
          "Retry when the Always-On Home is online.",
          true,
          error.status,
        ),
      );
      return;
    }
    this.reconnectAttempts += 1;
    const backoff = Math.min(
      this.reconnectBaseMs * 2 ** (this.reconnectAttempts - 1),
      this.reconnectMaxMs,
    );
    const jitter = Math.floor(this.random() * 250);
    const delay = backoff + jitter;
    this.setStatus({
      state: "reconnecting",
      next_retry_at: iso(this.now() + delay),
      error,
      channel_id: null,
    });
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      void this.openAttempt("reconnect");
    }, delay);
  }

  private fail(error: CloudError): void {
    this.clearTimers();
    this.closeSocketSilently();
    this.channel = null;
    this.rejectPending(
      cloudError("relay_closed", "The relay channel closed before the response finished.", null, true),
    );
    if (this.connectWaiter) {
      this.connectWaiter.reject(error);
      this.connectWaiter = null;
    }
    this.setStatus({
      state: "failed",
      channel_id: null,
      next_retry_at: null,
      error,
    });
  }

  private startHeartbeat(): void {
    this.stopHeartbeat();
    this.lastFrameAt = this.now();
    this.heartbeatTimer = setInterval(() => {
      if (this.disposed || this.manualClose) return;
      if (this.now() - this.lastFrameAt > this.heartbeatTimeoutMs) {
        void this.handleAttemptFailure(
          cloudError(
            "relay_timeout",
            "The relay stopped answering heartbeats.",
            "Reconnecting when the Home is online.",
            true,
          ),
          "reconnect",
        );
      }
    }, this.heartbeatIntervalMs);
  }

  private stopHeartbeat(): void {
    if (this.heartbeatTimer !== null) {
      clearInterval(this.heartbeatTimer);
      this.heartbeatTimer = null;
    }
  }

  private clearHandshakeTimer(): void {
    if (this.handshakeTimer !== null) {
      clearTimeout(this.handshakeTimer);
      this.handshakeTimer = null;
    }
  }

  private clearAttemptTimers(): void {
    this.clearHandshakeTimer();
    this.stopHeartbeat();
  }

  private clearTimers(): void {
    this.clearAttemptTimers();
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
  }

  private closeSocketSilently(): void {
    const socket = this.socket;
    this.socket = null;
    if (!socket) return;
    socket.onopen = null;
    socket.onmessage = null;
    socket.onerror = null;
    socket.onclose = null;
    try {
      socket.close(1000, "client closed");
    } catch {
      // A socket that never opened is already closed.
    }
  }

  private setStatus(patch: Partial<RelayConnectionStatus>): void {
    const next: RelayConnectionStatus = {
      ...this.statusValue,
      ...patch,
      error: patch.error === undefined ? this.statusValue.error : patch.error,
    };
    this.statusValue = next;
    const snapshot = cloneStatus(next);
    for (const listener of this.listeners) listener(snapshot);
  }
}

function isSafeRelayUrl(value: string): boolean {
  try {
    const url = new URL(value);
    if (url.username || url.password || url.hash) return false;
    if (url.protocol === "wss:") return true;
    if (url.protocol === "ws:") {
      return url.hostname === "127.0.0.1" || url.hostname === "localhost" || url.hostname === "[::1]";
    }
    return false;
  } catch {
    return false;
  }
}

function makeClientId(): string {
  const bytes = new Uint8Array(8);
  if (globalThis.crypto?.getRandomValues) globalThis.crypto.getRandomValues(bytes);
  else for (let index = 0; index < bytes.length; index += 1) bytes[index] = Math.floor(Math.random() * 256);
  return `cl_${Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("")}`;
}

function errorToCloudError(value: unknown, fallback: string): CloudError {
  if (
    typeof value === "object" &&
    value !== null &&
    typeof (value as { code?: unknown }).code === "string" &&
    typeof (value as { message?: unknown }).message === "string"
  ) {
    const candidate = value as Partial<CloudError> & { code: string; message: string };
    return cloudError(
      candidate.code,
      candidate.message,
      typeof candidate.hint === "string" ? candidate.hint : null,
      candidate.retryable === true,
      typeof candidate.status === "number" ? candidate.status : null,
    );
  }
  return cloudError("relay_closed", fallback, null, false);
}
