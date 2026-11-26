/**
 * Control-frame and remote-frame types for the StackSpirit relay.
 *
 * Mirrors `runtime/docs/design/relay-and-packaging.md` §6.3 (RelayFrame) and
 * §6.6 (RemoteFrame). Readers reject an unknown frame type or an unsupported
 * `v`, ignore unknown fields (so a future optional field is additive), and fail
 * closed on a missing required field.
 */

import {
  cloudError,
  type ClientKind,
  type CloudError,
  type RelayPublicJwk,
  type RelayScope,
} from "@stackspirit/ui";

export const RELAY_PROTOCOL = "stackspirit-relay/1";
export const REMOTE_PROTOCOL = "stackspirit-remote/1";
export const RELAY_MAX_CONTROL_BYTES = 65_536;
export const RELAY_MAX_DATA_BYTES = 1_048_576;
export const REMOTE_MAX_PLAINTEXT_BYTES = 768 * 1024;
export const RELAY_SUITE = "P-256-HKDF-SHA256-AES-256-GCM";

export type MachineState = "online" | "degraded" | "offline" | "revoked";

export interface MachineCapabilities {
  protocol: string;
  http: boolean;
  mcp: boolean;
  ssh: boolean;
  exec: boolean;
  max_channels: number;
  projects: string[];
}

/** Every control frame in §6.3. Machine-only variants are documented for parity. */
export type RelayFrame =
  | {
      type: "machine.hello";
      v: 1;
      machine_id: string;
      account_id: string;
      key_id: string;
      public_key: string;
      credential: string;
      agent_version: string;
      capabilities: MachineCapabilities;
      sent_at: string;
    }
  | {
      type: "relay.challenge";
      v: 1;
      relay_id: string;
      session_id: string;
      nonce: string;
      server_time: string;
      heartbeat_secs: number;
      max_frame_bytes: number;
      features: string[];
    }
  | { type: "machine.auth"; v: 1; signature: string }
  | {
      type: "relay.welcome";
      v: 1;
      relay_id: string;
      session_id: string;
      machine_id: string;
      account_id: string;
      heartbeat_secs: number;
      max_frame_bytes: number;
      features: string[];
    }
  | { type: "machine.status"; v: 1; state: MachineState; channels: number; sent_at: string }
  | {
      type: "client.hello";
      v: 1;
      machine_id: string;
      ticket: string;
      client_id: string;
      kind: ClientKind;
      protocol: string;
      capabilities: string[];
      client_key: RelayPublicJwk;
      client_nonce: string;
      sent_at: string;
    }
  | {
      type: "client.welcome";
      v: 1;
      relay_id: string;
      session_id: string;
      channel_id: string;
      machine_id: string;
      account_id: string;
      expires_at: string;
      features: string[];
    }
  | {
      type: "channel.open";
      v: 1;
      channel_id: string;
      machine_id: string;
      client_id: string;
      kind: ClientKind;
      ticket: string;
      scopes: RelayScope[];
      client_key: RelayPublicJwk;
      client_nonce: string;
    }
  | {
      type: "channel.accept";
      v: 1;
      channel_id: string;
      machine_key: RelayPublicJwk;
      machine_nonce: string;
      machine_key_id: string;
      machine_signature: string;
      suite: string;
    }
  | {
      type: "channel.reject";
      v: 1;
      channel_id: string;
      code: string;
      message: string;
      retryable: boolean;
    }
  | { type: "channel.close"; v: 1; channel_id: string; code: string; message: string }
  | { type: "superseded"; v: 1; reason: string }
  | { type: "ping"; v: 1; nonce: string }
  | { type: "pong"; v: 1; nonce: string }
  | { type: "error"; v: 1; code: string; message: string; retryable: boolean };

/** Frames a client may receive on `wss://relay.stackspirit.io/v1/client`. */
export type ServerRelayFrame = Extract<
  RelayFrame,
  {
    type:
      | "client.welcome"
      | "channel.accept"
      | "channel.reject"
      | "channel.close"
      | "superseded"
      | "ping"
      | "pong"
      | "error";
  }
>;

/** §6.6 encrypted application frames. */
export type RemoteFrame =
  | {
      type: "hello";
      v: 1;
      client: string;
      machine: string;
      scopes: RelayScope[];
      capabilities: string[];
    }
  | {
      type: "hello_ack";
      v: 1;
      machine: string;
      scopes: RelayScope[];
      capabilities: string[];
    }
  | {
      type: "http_request";
      stream: number;
      method: string;
      path: string;
      headers: Record<string, string>;
      body: string;
    }
  | {
      type: "http_response";
      stream: number;
      status: number;
      headers: Record<string, string>;
    }
  | { type: "http_body"; stream: number; data: string; end: boolean }
  | { type: "http_cancel"; stream: number }
  | {
      type: "mcp_request";
      stream: number;
      project: string | null;
      session: string | null;
      request: unknown;
    }
  | { type: "mcp_response"; stream: number; response: unknown }
  | { type: "mcp_cancel"; stream: number }
  | { type: "tcp_open"; stream: number; target: string }
  | { type: "tcp_data"; stream: number; data: string }
  | { type: "tcp_close"; stream: number }
  | {
      type: "exec_start";
      stream: number;
      argv: string[];
      cwd: string | null;
      pty: boolean;
      cols: number;
      rows: number;
      env: Record<string, string>;
    }
  | { type: "exec_data"; stream: number; data: string; channel: "stdout" | "stderr" | "stdin" }
  | { type: "exec_resize"; stream: number; cols: number; rows: number }
  | { type: "exec_exit"; stream: number; code: number; signal: string | null }
  | {
      type: "stream_error";
      stream: number;
      code: string;
      message: string;
      retryable: boolean;
    }
  | { type: "ping"; nonce: string }
  | { type: "pong"; nonce: string }
  | { type: "close"; code: string; message: string };

export interface RelayTicketClaims {
  iss?: string;
  aud?: string;
  sub?: string;
  acct?: string;
  machine?: string;
  kind?: string;
  scopes?: RelayScope[];
  cnf?: { jkt?: string };
  mkey?: string;
  mkey_id?: string;
  iat?: number;
  exp?: number;
  jti?: string;
}

const CLIENT_SCOPES: ReadonlySet<string> = new Set([
  "http.read",
  "http.write",
  "mcp.read",
  "mcp.write",
  "ssh",
  "exec",
]);

const RELAY_SERVER_TYPES: ReadonlySet<string> = new Set([
  "client.welcome",
  "channel.accept",
  "channel.reject",
  "channel.close",
  "superseded",
  "ping",
  "pong",
  "error",
]);

const CLIENT_KINDS: ReadonlySet<string> = new Set(["web", "cli", "desktop", "mcp"]);

const REMOTE_TYPES: ReadonlySet<string> = new Set([
  "hello",
  "hello_ack",
  "http_request",
  "http_response",
  "http_body",
  "http_cancel",
  "mcp_request",
  "mcp_response",
  "mcp_cancel",
  "tcp_open",
  "tcp_data",
  "tcp_close",
  "exec_start",
  "exec_data",
  "exec_resize",
  "exec_exit",
  "stream_error",
  "ping",
  "pong",
  "close",
]);

function protocolError(message: string): CloudError {
  return cloudError(
    "relay_protocol",
    message,
    "The relay or machine sent a frame this client cannot read.",
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function requireString(value: unknown, field: string): string {
  if (typeof value !== "string" || value.length === 0) {
    throw protocolError(`The relay frame is missing ${field}.`);
  }
  return value;
}

function optionalString(value: unknown, field: string): string | null {
  if (value === null || value === undefined) return null;
  if (typeof value !== "string") throw protocolError(`The relay frame has an unreadable ${field}.`);
  return value;
}

function requireNumber(value: unknown, field: string): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw protocolError(`The relay frame is missing ${field}.`);
  }
  return value;
}

function requireBoolean(value: unknown, field: string): boolean {
  if (typeof value !== "boolean") throw protocolError(`The relay frame is missing ${field}.`);
  return value;
}

function requireArray(value: unknown, field: string): unknown[] {
  if (!Array.isArray(value)) throw protocolError(`The relay frame is missing ${field}.`);
  return value;
}

function requireStringArray(value: unknown, field: string): string[] {
  return requireArray(value, field).map((item, index) => requireString(item, `${field}[${index}]`));
}

function requireScopeArray(value: unknown, field: string): RelayScope[] {
  const scopes = requireStringArray(value, field);
  for (const scope of scopes) {
    if (!CLIENT_SCOPES.has(scope)) throw protocolError(`The relay frame has an unknown scope.`);
  }
  return scopes as RelayScope[];
}

function requireJwk(value: unknown, field: string): RelayPublicJwk {
  if (!isRecord(value) || value.kty !== "EC" || value.crv !== "P-256") {
    throw protocolError(`The relay frame has an unreadable ${field}.`);
  }
  return {
    kty: "EC",
    crv: "P-256",
    x: requireString(value.x, `${field}.x`),
    y: requireString(value.y, `${field}.y`),
  };
}

function requireHeaderMap(value: unknown, field: string): Record<string, string> {
  if (value === null || value === undefined) return {};
  if (!isRecord(value)) throw protocolError(`The relay frame has an unreadable ${field}.`);
  const headers: Record<string, string> = {};
  for (const [name, header] of Object.entries(value)) {
    if (typeof header !== "string") throw protocolError(`The relay frame has an unreadable ${field}.`);
    headers[name] = header;
  }
  return headers;
}

function requireV(value: unknown): 1 {
  if (value !== 1) throw protocolError("The relay frame uses an unsupported protocol version.");
  return 1;
}

/** Parses one text message. An oversized control frame is rejected before parsing. */
export function parseServerRelayFrame(raw: string): ServerRelayFrame {
  if (utf8Bytes(raw).byteLength > RELAY_MAX_CONTROL_BYTES) {
    throw cloudError("relay_frame_too_large", "The relay closed a frame that was too large.");
  }
  let value: unknown;
  try {
    value = JSON.parse(raw) as unknown;
  } catch {
    throw protocolError("The relay sent text that is not a JSON control frame.");
  }
  if (!isRecord(value)) throw protocolError("The relay sent an unreadable control frame.");
  const type = requireString(value.type, "type");
  const v = requireV(value.v);
  if (!RELAY_SERVER_TYPES.has(type)) {
    throw protocolError(`The relay sent an unexpected ${type} frame to a client.`);
  }
  switch (type) {
    case "client.welcome": {
      requireTimestamp(value.expires_at, "client.welcome.expires_at");
      return {
        type: "client.welcome",
        v,
        relay_id: requireString(value.relay_id, "client.welcome.relay_id"),
        session_id: requireString(value.session_id, "client.welcome.session_id"),
        channel_id: requireString(value.channel_id, "client.welcome.channel_id"),
        machine_id: requireString(value.machine_id, "client.welcome.machine_id"),
        account_id: requireString(value.account_id, "client.welcome.account_id"),
        expires_at: requireString(value.expires_at, "client.welcome.expires_at"),
        features: requireStringArray(value.features, "client.welcome.features"),
      };
    }
    case "channel.accept": {
      const suite = requireString(value.suite, "channel.accept.suite");
      if (suite !== RELAY_SUITE) throw protocolError("The machine selected an unsupported cipher suite.");
      return {
        type: "channel.accept",
        v,
        channel_id: requireString(value.channel_id, "channel.accept.channel_id"),
        machine_key: requireJwk(value.machine_key, "channel.accept.machine_key"),
        machine_nonce: requireString(value.machine_nonce, "channel.accept.machine_nonce"),
        machine_key_id: requireString(value.machine_key_id, "channel.accept.machine_key_id"),
        machine_signature: requireString(value.machine_signature, "channel.accept.machine_signature"),
        suite,
      };
    }
    case "channel.reject":
      return {
        type: "channel.reject",
        v,
        channel_id: requireString(value.channel_id, "channel.reject.channel_id"),
        code: requireString(value.code, "channel.reject.code"),
        message: requireString(value.message, "channel.reject.message"),
        retryable: requireBoolean(value.retryable, "channel.reject.retryable"),
      };
    case "channel.close":
      return {
        type: "channel.close",
        v,
        channel_id: requireString(value.channel_id, "channel.close.channel_id"),
        code: requireString(value.code, "channel.close.code"),
        message: requireString(value.message, "channel.close.message"),
      };
    case "superseded":
      return { type: "superseded", v, reason: requireString(value.reason, "superseded.reason") };
    case "ping":
      return { type: "ping", v, nonce: requireString(value.nonce, "ping.nonce") };
    case "pong":
      return { type: "pong", v, nonce: requireString(value.nonce, "pong.nonce") };
    case "error":
      return {
        type: "error",
        v,
        code: requireString(value.code, "error.code"),
        message: requireString(value.message, "error.message"),
        retryable: requireBoolean(value.retryable, "error.retryable"),
      };
    default:
      throw protocolError(`The relay sent an unhandled ${type} frame.`);
  }
}

/** Parses an encrypted application frame after decryption. */
export function parseRemoteFrame(raw: unknown): RemoteFrame {
  let value: unknown = raw;
  if (typeof raw === "string") {
    try {
      value = JSON.parse(raw) as unknown;
    } catch {
      throw protocolError("The machine sent text that is not a JSON remote frame.");
    }
  }
  if (!isRecord(value)) throw protocolError("The machine sent an unreadable remote frame.");
  const type = requireString(value.type, "type");
  if (!REMOTE_TYPES.has(type)) throw protocolError(`The machine sent an unknown ${type} frame.`);
  switch (type) {
    case "hello":
      return {
        type,
        v: requireV(value.v),
        client: requireString(value.client, "hello.client"),
        machine: requireString(value.machine, "hello.machine"),
        scopes: requireScopeArray(value.scopes, "hello.scopes"),
        capabilities: requireStringArray(value.capabilities, "hello.capabilities"),
      };
    case "hello_ack":
      return {
        type,
        v: requireV(value.v),
        machine: requireString(value.machine, "hello_ack.machine"),
        scopes: requireScopeArray(value.scopes, "hello_ack.scopes"),
        capabilities: requireStringArray(value.capabilities, "hello_ack.capabilities"),
      };
    case "http_request":
      return {
        type,
        stream: requireNumber(value.stream, "http_request.stream"),
        method: requireString(value.method, "http_request.method"),
        path: requireString(value.path, "http_request.path"),
        headers: requireHeaderMap(value.headers, "http_request.headers"),
        body: typeof value.body === "string" ? value.body : "",
      };
    case "http_response":
      return {
        type,
        stream: requireNumber(value.stream, "http_response.stream"),
        status: requireNumber(value.status, "http_response.status"),
        headers: requireHeaderMap(value.headers, "http_response.headers"),
      };
    case "http_body":
      return {
        type,
        stream: requireNumber(value.stream, "http_body.stream"),
        data: typeof value.data === "string" ? value.data : "",
        end: requireBoolean(value.end, "http_body.end"),
      };
    case "http_cancel":
      return { type, stream: requireNumber(value.stream, "http_cancel.stream") };
    case "mcp_request":
      return {
        type,
        stream: requireNumber(value.stream, "mcp_request.stream"),
        project: optionalString(value.project, "mcp_request.project"),
        session: optionalString(value.session, "mcp_request.session"),
        request: value.request,
      };
    case "mcp_response":
      return {
        type,
        stream: requireNumber(value.stream, "mcp_response.stream"),
        response: value.response,
      };
    case "mcp_cancel":
      return { type, stream: requireNumber(value.stream, "mcp_cancel.stream") };
    case "tcp_open":
      return {
        type,
        stream: requireNumber(value.stream, "tcp_open.stream"),
        target: requireString(value.target, "tcp_open.target"),
      };
    case "tcp_data":
      return {
        type,
        stream: requireNumber(value.stream, "tcp_data.stream"),
        data: typeof value.data === "string" ? value.data : "",
      };
    case "tcp_close":
      return { type, stream: requireNumber(value.stream, "tcp_close.stream") };
    case "exec_start": {
      const env = requireHeaderMap(value.env, "exec_start.env");
      return {
        type,
        stream: requireNumber(value.stream, "exec_start.stream"),
        argv: requireStringArray(value.argv, "exec_start.argv"),
        cwd: optionalString(value.cwd, "exec_start.cwd"),
        pty: requireBoolean(value.pty, "exec_start.pty"),
        cols: requireNumber(value.cols, "exec_start.cols"),
        rows: requireNumber(value.rows, "exec_start.rows"),
        env,
      };
    }
    case "exec_data": {
      const channel = requireString(value.channel, "exec_data.channel");
      if (channel !== "stdout" && channel !== "stderr" && channel !== "stdin") {
        throw protocolError("The machine sent an unknown exec channel.");
      }
      return {
        type,
        stream: requireNumber(value.stream, "exec_data.stream"),
        data: typeof value.data === "string" ? value.data : "",
        channel,
      };
    }
    case "exec_resize":
      return {
        type,
        stream: requireNumber(value.stream, "exec_resize.stream"),
        cols: requireNumber(value.cols, "exec_resize.cols"),
        rows: requireNumber(value.rows, "exec_resize.rows"),
      };
    case "exec_exit":
      return {
        type,
        stream: requireNumber(value.stream, "exec_exit.stream"),
        code: requireNumber(value.code, "exec_exit.code"),
        signal: optionalString(value.signal, "exec_exit.signal"),
      };
    case "stream_error":
      return {
        type,
        stream: requireNumber(value.stream, "stream_error.stream"),
        code: requireString(value.code, "stream_error.code"),
        message: requireString(value.message, "stream_error.message"),
        retryable: requireBoolean(value.retryable, "stream_error.retryable"),
      };
    case "ping":
      return { type, nonce: requireString(value.nonce, "ping.nonce") };
    case "pong":
      return { type, nonce: requireString(value.nonce, "pong.nonce") };
    case "close":
      return {
        type,
        code: requireString(value.code, "close.code"),
        message: requireString(value.message, "close.message"),
      };
    default:
      throw protocolError(`The machine sent an unhandled ${type} frame.`);
  }
}

/** Decodes the (unverified) JWT payload. The relay verifies the signature; the
 * client only needs the machine key, jti and scope claims to run the E2E
 * handshake, and always trusts only the `mkey` it receives inside its own ticket. */
export function decodeTicketClaims(ticket: string): RelayTicketClaims {
  const parts = ticket.split(".");
  if (parts.length !== 3 || !parts[1]) throw protocolError("The relay ticket is not a JWT.");
  let value: unknown;
  try {
    value = JSON.parse(new TextDecoder().decode(base64UrlToBytes(parts[1]))) as unknown;
  } catch {
    throw protocolError("The relay ticket payload is not readable JSON.");
  }
  if (!isRecord(value)) throw protocolError("The relay ticket payload is not an object.");
  const scopes =
    Array.isArray(value.scopes) && value.scopes.every((scope) => typeof scope === "string" && CLIENT_SCOPES.has(scope))
      ? (value.scopes as RelayScope[])
      : undefined;
  return {
    iss: optionalString(value.iss, "ticket.iss") ?? undefined,
    aud: typeof value.aud === "string" ? value.aud : undefined,
    sub: typeof value.sub === "string" ? value.sub : undefined,
    acct: typeof value.acct === "string" ? value.acct : undefined,
    machine: typeof value.machine === "string" ? value.machine : undefined,
    kind: typeof value.kind === "string" && CLIENT_KINDS.has(value.kind) ? value.kind : undefined,
    scopes,
    cnf: isRecord(value.cnf) && typeof value.cnf.jkt === "string" ? { jkt: value.cnf.jkt } : undefined,
    mkey: typeof value.mkey === "string" ? value.mkey : undefined,
    mkey_id: typeof value.mkey_id === "string" ? value.mkey_id : undefined,
    iat: typeof value.iat === "number" ? value.iat : undefined,
    exp: typeof value.exp === "number" ? value.exp : undefined,
    jti: typeof value.jti === "string" ? value.jti : undefined,
  };
}

/** RFC 7638 canonical JWK JSON for a P-256 public key, no whitespace. */
export function canonicalJwk(jwk: RelayPublicJwk): string {
  return `{"crv":"P-256","kty":"EC","x":${JSON.stringify(jwk.x)},"y":${JSON.stringify(jwk.y)}}`;
}

/** The exact §6.5 handshake transcript, UTF-8, no trailing newline. */
export function relayTranscript(input: {
  channelId: string;
  clientKey: RelayPublicJwk;
  machineKey: RelayPublicJwk;
  clientNonce: string;
  machineNonce: string;
  machineId: string;
  accountId: string;
  ticketJti: string;
}): string {
  return (
    "stackspirit-e2e-v1\n" +
    input.channelId +
    "\n" +
    canonicalJwk(input.clientKey) +
    "\n" +
    canonicalJwk(input.machineKey) +
    "\n" +
    input.clientNonce +
    "\n" +
    input.machineNonce +
    "\n" +
    input.machineId +
    "\n" +
    input.accountId +
    "\n" +
    input.ticketJti
  );
}

/** Maps WebSocket close codes to typed, user-safe errors. */
export function relayErrorForClose(code: number, reason: string): CloudError {
  switch (code) {
    case 1000:
      return cloudError("relay_closed", "The relay closed the connection.");
    case 1009:
      return cloudError(
        "relay_frame_too_large",
        "The relay closed a frame that exceeded the protocol limit.",
        null,
        false,
        code,
      );
    case 1013:
      return cloudError(
        "relay_rate_limited",
        "The relay is temporarily overloaded.",
        "Reconnecting with backoff.",
        true,
        code,
      );
    case 4001:
      return cloudError(
        "relay_auth",
        "The relay rejected this session.",
        "Sign in to StackSpirit again.",
        false,
        code,
      );
    case 4004:
      return cloudError(
        "relay_offline",
        "The Always-On Home is offline.",
        "Start the Home or wait for it to reconnect.",
        true,
        code,
      );
    case 4008:
      return cloudError(
        "relay_superseded",
        "Another StackSpirit client replaced this relay connection.",
        null,
        false,
        code,
      );
    case 4009:
      return cloudError(
        "relay_rate_limited",
        "The relay is rate limiting this account.",
        "Reconnecting with backoff.",
        true,
        code,
      );
    case 4010:
      return cloudError(
        "relay_ticket_expired",
        "The relay ticket expired.",
        "Minting a fresh ticket.",
        true,
        code,
      );
    case 4011:
      return cloudError(
        "relay_scope_denied",
        "The relay denied the requested scope.",
        "Enable the scope on the machine first.",
        false,
        code,
      );
    case 4012:
      return cloudError(
        "relay_crypto",
        "The encrypted relay handshake failed.",
        null,
        false,
        code,
      );
    default:
      return cloudError(
        "relay_closed",
        reason ? `The relay closed the connection: ${reason}` : "The relay closed the connection.",
        null,
        true,
        code,
      );
  }
}

/** Maps an `error`/`channel.reject` code to a typed error. */
export function relayErrorForServerCode(code: string, message: string, retryable: boolean): CloudError {
  const mapped =
    code === "unauthenticated" || code === "forbidden"
      ? "relay_auth"
      : code === "machine_offline"
        ? "relay_offline"
        : code === "ticket_replayed"
          ? "relay_ticket_replayed"
          : code === "ticket_expired"
            ? "relay_ticket_expired"
            : code === "scope_denied"
              ? "relay_scope_denied"
              : code === "rate_limited"
                ? "relay_rate_limited"
                : code === "crypto_error"
                  ? "relay_crypto"
                  : "relay_protocol";
  return cloudError(mapped, message || "The relay rejected the connection.", null, retryable);
}

const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/** Decodes a 26-character ULID into the 16 raw bytes used by a data frame. */
export function ulidToBytes(ulid: string): Uint8Array {
  if (!/^[0-9A-HJKMNP-TV-Z]{26}$/.test(ulid)) {
    throw protocolError("The relay channel id is not a ULID.");
  }
  let value = 0n;
  for (const character of ulid) {
    value = (value << 5n) | BigInt(CROCKFORD.indexOf(character));
  }
  // 26 Crockford characters carry 130 bits; the ULID payload is the low 128.
  value &= (1n << 128n) - 1n;
  const bytes = new Uint8Array(16);
  for (let index = 15; index >= 0; index -= 1) {
    bytes[index] = Number(value & 0xffn);
    value >>= 8n;
  }
  return bytes;
}

export function utf8Bytes(text: string): Uint8Array<ArrayBuffer> {
  return new TextEncoder().encode(text);
}

const BASE64_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const BASE64_LOOKUP = (() => {
  const map = new Map<string, number>();
  for (let index = 0; index < BASE64_ALPHABET.length; index += 1) {
    map.set(BASE64_ALPHABET[index] ?? "", index);
  }
  return map;
})();

/** Standard padded base64, with no browser `btoa` call-stack limits. */
export function bytesToBase64(bytes: Uint8Array): string {
  let output = "";
  let index = 0;
  while (index + 2 < bytes.length) {
    const chunk =
      ((bytes[index] ?? 0) << 16) | ((bytes[index + 1] ?? 0) << 8) | (bytes[index + 2] ?? 0);
    output +=
      (BASE64_ALPHABET[(chunk >> 18) & 63] ?? "") +
      (BASE64_ALPHABET[(chunk >> 12) & 63] ?? "") +
      (BASE64_ALPHABET[(chunk >> 6) & 63] ?? "") +
      (BASE64_ALPHABET[chunk & 63] ?? "");
    index += 3;
  }
  const remaining = bytes.length - index;
  if (remaining === 1) {
    const chunk = (bytes[index] ?? 0) << 16;
    output +=
      (BASE64_ALPHABET[(chunk >> 18) & 63] ?? "") +
      (BASE64_ALPHABET[(chunk >> 12) & 63] ?? "") +
      "==";
  } else if (remaining === 2) {
    const chunk = ((bytes[index] ?? 0) << 16) | ((bytes[index + 1] ?? 0) << 8);
    output +=
      (BASE64_ALPHABET[(chunk >> 18) & 63] ?? "") +
      (BASE64_ALPHABET[(chunk >> 12) & 63] ?? "") +
      (BASE64_ALPHABET[(chunk >> 6) & 63] ?? "") +
      "=";
  }
  return output;
}

export function base64ToBytes(text: string): Uint8Array<ArrayBuffer> {
  const clean = text.replace(/\s+/g, "");
  if (clean.length % 4 !== 0 || /[^A-Za-z0-9+/=]/.test(clean)) {
    throw protocolError("The relay sent unreadable base64 data.");
  }
  const padding = clean.endsWith("==") ? 2 : clean.endsWith("=") ? 1 : 0;
  const bytes = new Uint8Array((clean.length / 4) * 3 - padding);
  let output = 0;
  for (let index = 0; index < clean.length; index += 4) {
    const a = BASE64_LOOKUP.get(clean[index] ?? "") ?? -1;
    const b = BASE64_LOOKUP.get(clean[index + 1] ?? "") ?? -1;
    const c = clean[index + 2] === "=" ? 0 : (BASE64_LOOKUP.get(clean[index + 2] ?? "") ?? -1);
    const d = clean[index + 3] === "=" ? 0 : (BASE64_LOOKUP.get(clean[index + 3] ?? "") ?? -1);
    if (a < 0 || b < 0 || c < 0 || d < 0) throw protocolError("The relay sent unreadable base64 data.");
    const chunk = (a << 18) | (b << 12) | (c << 6) | d;
    if (output < bytes.length) bytes[output++] = (chunk >> 16) & 255;
    if (output < bytes.length) bytes[output++] = (chunk >> 8) & 255;
    if (output < bytes.length) bytes[output++] = chunk & 255;
  }
  return bytes;
}

export function bytesToBase64Url(bytes: Uint8Array): string {
  return bytesToBase64(bytes).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export function base64UrlToBytes(text: string): Uint8Array<ArrayBuffer> {
  const padded = text.replace(/-/g, "+").replace(/_/g, "/");
  return base64ToBytes(padded + "=".repeat((4 - (padded.length % 4)) % 4));
}

export function bytesToHex(bytes: Uint8Array): string {
  let output = "";
  for (const byte of bytes) output += byte.toString(16).padStart(2, "0");
  return output;
}

export function hexToBytes(hex: string): Uint8Array<ArrayBuffer> {
  if (!/^[0-9a-fA-F]*$/.test(hex) || hex.length % 2 !== 0) {
    throw protocolError("The relay sent an unreadable hex key.");
  }
  const bytes = new Uint8Array(hex.length / 2);
  for (let index = 0; index < bytes.length; index += 1) {
    bytes[index] = Number.parseInt(hex.slice(index * 2, index * 2 + 2), 16);
  }
  return bytes;
}

function requireTimestamp(value: unknown, field: string): string {
  const text = requireString(value, field);
  if (Number.isNaN(Date.parse(text))) throw protocolError(`The relay frame has an unreadable ${field}.`);
  return text;
}
