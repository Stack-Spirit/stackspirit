/**
 * Typed StackSpirit Account, Cloud Relay and Managed Home contracts.
 *
 * These types mirror the Stage 2 relay design in
 * `runtime/docs/design/relay-and-packaging.md` §6.3–§6.7 and the account-free
 * strategy in `docs/STRATEGY.md` §4 & §6. They are deliberately independent of
 * the local `SpiritBridge`: the cloud client talks only to
 * `api.stackspirit.io` / `relay.stackspirit.io`, while the local host/SSH
 * surface stays entirely on the user's device.
 *
 * Security invariants encoded by the types themselves:
 *
 * - there is no field for an SSH private key, a passphrase, a password or a
 *   provider secret anywhere in a cloud request;
 * - a relay ticket carries only a public P-256 JWK, a public nonce and a scope
 *   list;
 * - account tokens live only in the client instance's memory and are never
 *   part of a view model or an error.
 */

import type { SpiritErrorDto, Timestamp } from "../bridge/wire";

/** The fetch shape the cloud client accepts; tests pass a fully mocked function. */
export type CloudFetch = (
  input: RequestInfo | URL,
  init?: RequestInit,
) => Promise<Response>;

/** Reliable, user-facing cloud error codes. Unknown transport codes stay verbatim. */
export type CloudErrorCode =
  | "cloud_unavailable"
  | "cloud_unreachable"
  | "cloud_timeout"
  | "cloud_auth"
  | "cloud_forbidden"
  | "cloud_not_found"
  | "cloud_conflict"
  | "cloud_rate_limited"
  | "cloud_server"
  | "cloud_parse"
  | "cloud_http"
  | "cloud_machine_offline"
  | "cloud_relay_unavailable"
  | "cloud_crypto_unsupported"
  | "relay_not_connected"
  | "relay_auth"
  | "relay_forbidden"
  | "relay_offline"
  | "relay_superseded"
  | "relay_rate_limited"
  | "relay_ticket_expired"
  | "relay_ticket_replayed"
  | "relay_scope_denied"
  | "relay_frame_too_large"
  | "relay_protocol"
  | "relay_crypto"
  | "relay_timeout"
  | "relay_closed";

/**
 * The one cloud error shape. It is `SpiritErrorDto`-compatible so the existing
 * `toSpiritError`/`ErrorNotice` plumbing can display it unchanged. `status` is
 * the HTTP status when the cloud API answered, or the WebSocket close code when
 * the relay closed the transport.
 */
export interface CloudError extends SpiritErrorDto {
  code: CloudErrorCode | string;
  status: number | null;
}

export function cloudError(
  code: CloudErrorCode | string,
  message: string,
  hint: string | null = null,
  retryable = false,
  status: number | null = null,
): CloudError {
  return { code, message, hint, retryable, status };
}

/** Normalizes anything thrown by a cloud client into a `CloudError`. */
export function asCloudError(value: unknown): CloudError {
  if (
    typeof value === "object" &&
    value !== null &&
    typeof (value as { code?: unknown }).code === "string" &&
    typeof (value as { message?: unknown }).message === "string"
  ) {
    const candidate = value as Partial<CloudError> & { code: string; message: string };
    return {
      code: candidate.code,
      message: candidate.message,
      hint: typeof candidate.hint === "string" ? candidate.hint : null,
      retryable: candidate.retryable === true,
      status: typeof candidate.status === "number" ? candidate.status : null,
    };
  }
  if (value instanceof Error) {
    return cloudError("cloud_unavailable", value.message || "The cloud client failed.");
  }
  return cloudError("cloud_unavailable", "The cloud client failed.");
}

/** The signed-in account profile. No token or credential is ever included. */
export interface CloudAccount {
  id: string;
  email: string;
  name: string | null;
}

export type DevicePollState = "authorized" | "pending" | "slow_down";

/** `POST /v1/auth/device` response. */
export interface DeviceAuthorization {
  device_code: string;
  user_code: string;
  verification_uri: string;
  /** A complete URL that embeds the user code, when the API supplies one. */
  verification_uri_complete: string | null;
  expires_in: number;
  interval: number;
  /** Client-side expiry, derived from `expires_in` when the API does not send one. */
  expires_at: Timestamp;
}

export type MachinePresence = "online" | "offline" | "degraded" | "revoked";

/** `GET /v1/machines` row. Public identity only. */
export interface CloudMachine {
  id: string;
  name: string;
  status: MachinePresence;
  project: string | null;
  last_seen_at: Timestamp | null;
  /** Inline provisioning state when the API includes one; never an SSH key. */
  provisioning: ProvisioningStatus | null;
}

export type RelayScope = "http.read" | "http.write" | "mcp.read" | "mcp.write" | "ssh" | "exec";
export type ClientKind = "web" | "cli" | "desktop" | "mcp";

/** The public half of the browser's ephemeral P-256 ECDH key. */
export interface RelayPublicJwk {
  kty: "EC";
  crv: "P-256";
  x: string;
  y: string;
}

/** `POST /v1/machines/:id/ticket` request body. Public values only. */
export interface RelayTicketRequest {
  kind: ClientKind;
  scopes: RelayScope[];
  client_key: RelayPublicJwk;
  client_nonce: string;
  ttl_secs: number;
}

/** The machine card carried by a minted ticket. */
export interface RelayTicketMachine {
  id: string;
  name: string;
  key_id: string;
  public_key: string;
  project: string | null;
}

/** A short-lived, single-use relay ticket. */
export interface RelayTicket {
  ticket: string;
  expires_at: Timestamp;
  relay_url: string;
  machine: RelayTicketMachine;
}

/** One published managed tier. Prices come from `GET /v1/plans`, never guessed. */
export interface ManagedPlan {
  id: string;
  name: string;
  vcpu: number;
  ram_gb: number;
  disk_gb: number;
  disk_kind: string | null;
  /** Monthly price in euro, already including the +20% StackSpirit markup. */
  monthly_eur: number;
  /** One-time setup fee in euro when the provider charges one (Home 8). */
  setup_fee_eur: number | null;
  currency: "EUR";
  /** The underlying provider spec, shown transparently when reported. */
  provider_spec: string | null;
}

/** The optional off-site encrypted R2 backup add-on. Off by default. */
export interface BackupPlan {
  monthly_eur: number | null;
  recommended: boolean;
  provider: string | null;
}

export interface ManagedPlanCatalog {
  plans: ManagedPlan[];
  backups: BackupPlan;
}

export type ProvisioningPhase = "queued" | "provisioning" | "online" | "failed";

/** Live managed-Home provisioning status. Dates are safe ISO timestamps. */
export interface ProvisioningStatus {
  machine_id: string;
  phase: ProvisioningPhase;
  step: string | null;
  progress_pct: number | null;
  detail: string | null;
  updated_at: Timestamp;
  home_url: string | null;
}

/**
 * The account API surface the UI may call. It is implemented by
 * `StackSpiritCloudClient` and can be replaced by a fake in tests. No method
 * returns an access token, a refresh token or an SSH key.
 */
export interface CloudAccountApi {
  /** True when this client holds an in-memory bearer session. */
  readonly hasSession: boolean;
  startDeviceSignIn(): Promise<DeviceAuthorization>;
  pollDeviceSignIn(deviceCode: string): Promise<DevicePollState>;
  account(): Promise<CloudAccount>;
  listMachines(): Promise<CloudMachine[]>;
  mintRelayTicket(machineId: string, request: RelayTicketRequest): Promise<RelayTicket>;
  getPlans(): Promise<ManagedPlanCatalog>;
  getProvisioning(machineId: string): Promise<ProvisioningStatus>;
  /** Drops the in-memory bearer session; idempotent. */
  clearSession(): void;
}

export type RelayConnectionState =
  | "idle"
  | "connecting"
  | "authenticating"
  | "secure"
  | "reconnecting"
  | "failed"
  | "closed";

export interface RelayConnectionStatus {
  state: RelayConnectionState;
  machine_id: string | null;
  channel_id: string | null;
  attempts: number;
  next_retry_at: Timestamp | null;
  error: CloudError | null;
}

/** A read-only HTTP request over the relay. Writes are denied unless enabled. */
export interface RelayHttpInit {
  method?: "GET" | "POST";
  headers?: Record<string, string>;
  body?: string | null;
}

export interface RelayHttpResponse {
  status: number;
  headers: Record<string, string>;
  body: string;
}

/**
 * The Web/PWA relay transport surface. `CloudRelayClient` implements it over a
 * real WebSocket; the desktop app can reuse the same interface later.
 */
export interface CloudRelayConnector {
  /** Resolves when the encrypted channel is ready; rejects with a `CloudError`. */
  connect(machineId: string, options?: { scopes?: RelayScope[] }): Promise<void>;
  disconnect(): void;
  status(): RelayConnectionStatus;
  subscribe(listener: (status: RelayConnectionStatus) => void): () => void;
  /** One HTTP request through the authenticated, encrypted channel. */
  request(path: string, init?: RelayHttpInit): Promise<RelayHttpResponse>;
}

/** The two injectable cloud surfaces. The Web app wires the real implementations. */
export interface CloudRuntime {
  client: CloudAccountApi;
  relay: CloudRelayConnector | null;
}

export const RELAY_DEFAULT_URL = "wss://relay.stackspirit.io/v1/client";
export const CLOUD_DEFAULT_API_URL = "https://api.stackspirit.io";
export const RELAY_PROTOCOL = "stackspirit-relay/1";
export const REMOTE_PROTOCOL = "stackspirit-remote/1";
