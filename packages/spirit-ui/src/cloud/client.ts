/**
 * The typed StackSpirit Account HTTP client.
 *
 * Every call is explicit and opt-in: constructing the client makes no request,
 * and the account-free local mode never constructs one in the first place. The
 * bearer session (when a device flow completes) is kept only in this instance's
 * memory: it is never written to localStorage, sessionStorage, a URL, a log or
 * an error, and no view model exposes it.
 *
 * The client sends no SSH private key, passphrase, password, provider secret or
 * relay private key. The only key material it ever transmits is the public
 * half of the browser's ephemeral P-256 ECDH key when minting a relay ticket.
 */

import {
  CLOUD_DEFAULT_API_URL,
  asCloudError,
  cloudError,
  type BackupPlan,
  type CloudAccount,
  type CloudAccountApi,
  type CloudMachine,
  type CloudFetch,
  type DeviceAuthorization,
  type DevicePollState,
  type MachinePresence,
  type ManagedPlan,
  type ManagedPlanCatalog,
  type ProvisioningPhase,
  type ProvisioningStatus,
  type RelayPublicJwk,
  type RelayTicket,
  type RelayTicketRequest,
} from "./types";

const REQUEST_TIMEOUT_MS = 15_000;
const MAX_BODY_CHARS = 1_000_000;
const SAFE_MACHINE_ID = /^[A-Za-z0-9_-]{1,128}$/;
const SCOPES: ReadonlySet<string> = new Set([
  "http.read",
  "http.write",
  "mcp.read",
  "mcp.write",
  "ssh",
  "exec",
]);
const PRESENCE: ReadonlySet<string> = new Set(["online", "offline", "degraded", "revoked"]);
const PROVISIONING_PHASES: ReadonlySet<string> = new Set([
  "queued",
  "provisioning",
  "online",
  "failed",
]);

export interface StackSpiritCloudClientOptions {
  /** The public API base. Production default: `https://api.stackspirit.io`. */
  apiBaseUrl?: string;
  fetchImpl?: CloudFetch;
  now?: () => number;
  timeoutMs?: number;
  /** Overridable for contract tests; production uses the StackSpirit client id. */
  deviceClientId?: string;
}

interface RequestOptions {
  method: "GET" | "POST";
  path: string;
  body?: unknown;
  authenticated?: boolean;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function requireString(value: unknown, field: string): string {
  if (typeof value !== "string" || value.length === 0) {
    throw cloudError("cloud_parse", `The cloud API returned an unreadable ${field}.`);
  }
  return value;
}

function optionalString(value: unknown, field: string): string | null {
  if (value === null || value === undefined) return null;
  if (typeof value !== "string") {
    throw cloudError("cloud_parse", `The cloud API returned an unreadable ${field}.`);
  }
  return value;
}

function requireNumber(value: unknown, field: string): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw cloudError("cloud_parse", `The cloud API returned an unreadable ${field}.`);
  }
  return value;
}

function requireInteger(value: unknown, field: string): number {
  const number = requireNumber(value, field);
  if (!Number.isInteger(number)) {
    throw cloudError("cloud_parse", `The cloud API returned an unreadable ${field}.`);
  }
  return number;
}

function optionalNumber(value: unknown, field: string): number | null {
  if (value === null || value === undefined) return null;
  return requireNumber(value, field);
}

function requireTimestamp(value: unknown, field: string): string {
  const text = requireString(value, field);
  if (Number.isNaN(Date.parse(text))) {
    throw cloudError("cloud_parse", `The cloud API returned an unreadable ${field}.`);
  }
  return text;
}

function optionalTimestamp(value: unknown, field: string): string | null {
  if (value === null || value === undefined) return null;
  return requireTimestamp(value, field);
}

function requireBoolean(value: unknown, field: string): boolean {
  if (typeof value !== "boolean") {
    throw cloudError("cloud_parse", `The cloud API returned an unreadable ${field}.`);
  }
  return value;
}

function safeText(value: string, max = 300): string {
  let clean = "";
  for (const character of value) {
    const code = character.codePointAt(0) ?? 0;
    clean += code < 0x20 || code === 0x7f ? " " : character;
  }
  const trimmed = clean.replace(/\s+/g, " ").trim();
  return trimmed.length > max ? `${trimmed.slice(0, max)}…` : trimmed;
}

/** Keeps local absolute paths out of provisioning details shown in the browser. */
function safeDetail(value: string): string {
  return safeText(value).replace(
    /(^|[\s("'=])((?:\/home\/|\/Users\/|\/root\/|\/tmp\/|\/var\/|\/etc\/|\/opt\/|\/srv\/)[^\s"'`)]*)/g,
    (_match, prefix: string) => `${prefix}[path]`,
  );
}

function validateApiBase(raw: string): string {
  const trimmed = raw.trim().replace(/\/+$/, "");
  let url: URL;
  try {
    url = new URL(trimmed);
  } catch {
    throw cloudError("cloud_unavailable", "The StackSpirit API address is not a valid URL.");
  }
  const local =
    url.hostname === "127.0.0.1" || url.hostname === "localhost" || url.hostname === "::1";
  if (url.protocol !== "https:" && !(url.protocol === "http:" && local)) {
    throw cloudError("cloud_unavailable", "The StackSpirit API must use https.");
  }
  if (url.username || url.password || url.search || url.hash) {
    throw cloudError(
      "cloud_unavailable",
      "The StackSpirit API address cannot contain credentials, a query or a fragment.",
    );
  }
  return trimmed;
}

function validateMachineId(machineId: string): string {
  const trimmed = machineId.trim();
  if (!SAFE_MACHINE_ID.test(trimmed)) {
    throw cloudError("invalid_input", "A machine id must be a safe StackSpirit identifier.");
  }
  return trimmed;
}

function parseAccount(value: unknown): CloudAccount {
  const body = isRecord(value) && isRecord(value.account) ? value.account : value;
  if (!isRecord(body)) {
    throw cloudError("cloud_parse", "The cloud API returned an unreadable account.");
  }
  return {
    id: requireString(body.id, "account.id"),
    email: requireString(body.email, "account.email"),
    name: optionalString(body.name, "account.name"),
  };
}

function parseProvisioning(value: unknown): ProvisioningStatus {
  if (!isRecord(value)) {
    throw cloudError("cloud_parse", "The cloud API returned an unreadable provisioning status.");
  }
  const phase = requireString(value.phase, "provisioning.phase");
  if (!PROVISIONING_PHASES.has(phase)) {
    throw cloudError("cloud_parse", "The cloud API returned an unknown provisioning phase.");
  }
  const progress = optionalNumber(value.progress_pct, "provisioning.progress_pct");
  return {
    machine_id: requireString(value.machine_id, "provisioning.machine_id"),
    phase: phase as ProvisioningPhase,
    step: optionalString(value.step, "provisioning.step"),
    progress_pct:
      progress === null ? null : Math.max(0, Math.min(100, Math.round(progress))),
    detail: value.detail === null || value.detail === undefined ? null : safeDetail(requireString(value.detail, "provisioning.detail")),
    updated_at: requireTimestamp(value.updated_at, "provisioning.updated_at"),
    home_url: optionalString(value.home_url, "provisioning.home_url"),
  };
}

function parseMachine(value: unknown, index: number): CloudMachine {
  if (!isRecord(value)) {
    throw cloudError("cloud_parse", `The cloud API returned an unreadable machine at ${index}.`);
  }
  const status = requireString(value.status, `machines[${index}].status`);
  if (!PRESENCE.has(status)) {
    throw cloudError("cloud_parse", `The cloud API returned an unknown machine presence at ${index}.`);
  }
  return {
    id: requireString(value.id, `machines[${index}].id`),
    name: requireString(value.name, `machines[${index}].name`),
    status: status as MachinePresence,
    project: optionalString(value.project, `machines[${index}].project`),
    last_seen_at: optionalTimestamp(value.last_seen_at, `machines[${index}].last_seen_at`),
    provisioning:
      value.provisioning === null || value.provisioning === undefined
        ? null
        : parseProvisioning(value.provisioning),
  };
}

function parseMachines(value: unknown): CloudMachine[] {
  const items = Array.isArray(value)
    ? value
    : isRecord(value) && Array.isArray(value.machines)
      ? value.machines
      : null;
  if (!items) {
    throw cloudError("cloud_parse", "The cloud API returned an unreadable machine list.");
  }
  return items.map(parseMachine);
}

function parseJwk(value: unknown): RelayPublicJwk {
  if (!isRecord(value)) {
    throw cloudError("cloud_parse", "The cloud API returned an unreadable relay client key.");
  }
  if (value.kty !== "EC" || value.crv !== "P-256") {
    throw cloudError("cloud_parse", "A relay client key must be a P-256 EC JWK.");
  }
  return {
    kty: "EC",
    crv: "P-256",
    x: requireString(value.x, "client_key.x"),
    y: requireString(value.y, "client_key.y"),
  };
}

function parseRelayTicket(value: unknown): RelayTicket {
  if (!isRecord(value)) {
    throw cloudError("cloud_parse", "The cloud API returned an unreadable relay ticket.");
  }
  if (!isRecord(value.machine)) {
    throw cloudError("cloud_parse", "The cloud API returned a relay ticket without a machine.");
  }
  const relayUrl = requireString(value.relay_url, "ticket.relay_url");
  if (!/^wss:\/\//.test(relayUrl) && !/^ws:\/\/(?:127\.0\.0\.1|localhost)(?::\d+)?\//.test(relayUrl)) {
    throw cloudError("cloud_parse", "The relay ticket returned an unsafe relay URL.");
  }
  return {
    ticket: requireString(value.ticket, "ticket.ticket"),
    expires_at: requireTimestamp(value.expires_at, "ticket.expires_at"),
    relay_url: relayUrl,
    machine: {
      id: requireString(value.machine.id, "ticket.machine.id"),
      name: requireString(value.machine.name, "ticket.machine.name"),
      key_id: requireString(value.machine.key_id, "ticket.machine.key_id"),
      public_key: requireString(value.machine.public_key, "ticket.machine.public_key"),
      project: optionalString(value.machine.project, "ticket.machine.project"),
    },
  };
}

function parsePlan(value: unknown, index: number): ManagedPlan {
  if (!isRecord(value)) {
    throw cloudError("cloud_parse", `The cloud API returned an unreadable plan at ${index}.`);
  }
  const currency = optionalString(value.currency, `plans[${index}].currency`) ?? "EUR";
  if (currency !== "EUR") {
    throw cloudError("cloud_parse", "Only euro plans are supported in this build.");
  }
  return {
    id: requireString(value.id, `plans[${index}].id`),
    name: requireString(value.name, `plans[${index}].name`),
    vcpu: Math.max(0, requireInteger(value.vcpu, `plans[${index}].vcpu`)),
    ram_gb: Math.max(0, requireInteger(value.ram_gb, `plans[${index}].ram_gb`)),
    disk_gb: Math.max(0, requireInteger(value.disk_gb, `plans[${index}].disk_gb`)),
    disk_kind: optionalString(value.disk_kind, `plans[${index}].disk_kind`),
    monthly_eur: Math.max(0, requireNumber(value.monthly_eur, `plans[${index}].monthly_eur`)),
    setup_fee_eur: (() => {
      const fee = optionalNumber(value.setup_fee_eur, `plans[${index}].setup_fee_eur`);
      return fee === null ? null : Math.max(0, fee);
    })(),
    currency: "EUR",
    provider_spec: optionalString(value.provider_spec, `plans[${index}].provider_spec`),
  };
}

function parseBackups(value: unknown): BackupPlan {
  if (value === null || value === undefined) {
    return { monthly_eur: null, recommended: true, provider: "cloudflare-r2" };
  }
  if (!isRecord(value)) {
    throw cloudError("cloud_parse", "The cloud API returned an unreadable backup add-on.");
  }
  return {
    monthly_eur: optionalNumber(value.monthly_eur, "backups.monthly_eur"),
    recommended: value.recommended === undefined ? true : requireBoolean(value.recommended, "backups.recommended"),
    provider: optionalString(value.provider, "backups.provider"),
  };
}

function parseCatalog(value: unknown): ManagedPlanCatalog {
  const items = Array.isArray(value)
    ? value
    : isRecord(value) && Array.isArray(value.plans)
      ? value.plans
      : null;
  if (!items) {
    throw cloudError("cloud_parse", "The cloud API returned an unreadable plan catalog.");
  }
  const plans = items
    .map(parsePlan)
    .sort((left, right) => left.monthly_eur - right.monthly_eur || left.ram_gb - right.ram_gb);
  const backups = isRecord(value) ? parseBackups(value.backups) : parseBackups(null);
  return { plans, backups };
}

function statusError(status: number, message: string): ReturnType<typeof cloudError> {
  switch (status) {
    case 400:
      return cloudError("invalid_input", message || "The cloud API rejected the request.", null, false, status);
    case 401:
      return cloudError(
        "cloud_auth",
        "Sign in to StackSpirit again.",
        "The account session expired or was revoked.",
        false,
        status,
      );
    case 403:
      return cloudError("cloud_forbidden", message || "This account cannot use that machine.", null, false, status);
    case 404:
      return cloudError("cloud_not_found", message || "The cloud API could not find that item.", null, false, status);
    case 409:
      return cloudError("cloud_conflict", message || "The machine changed; refresh and retry.", null, false, status);
    case 429:
      return cloudError(
        "cloud_rate_limited",
        message || "The cloud API is rate limiting this account.",
        "Wait a moment and retry.",
        true,
        status,
      );
    default:
      if (status >= 500) {
        return cloudError(
          "cloud_server",
          message || `The cloud API answered with HTTP ${status}.`,
          "Retry shortly.",
          true,
          status,
        );
      }
      return cloudError("cloud_http", message || `The cloud API answered with HTTP ${status}.`, null, false, status);
  }
}

function apiMessage(body: unknown): string {
  if (isRecord(body)) {
    if (typeof body.message === "string") return safeText(body.message);
    if (isRecord(body.error) && typeof body.error.message === "string") {
      return safeText(body.error.message);
    }
  }
  return "";
}

/** The production client used by the Web/PWA and available to the desktop shell. */
export class StackSpiritCloudClient implements CloudAccountApi {
  private readonly apiBaseUrl: string;
  private readonly fetchImpl: CloudFetch;
  private readonly now: () => number;
  private readonly timeoutMs: number;
  private readonly deviceClientId: string;
  private accessToken: string | null = null;

  constructor(options: StackSpiritCloudClientOptions = {}) {
    this.apiBaseUrl = validateApiBase(options.apiBaseUrl ?? CLOUD_DEFAULT_API_URL);
    this.fetchImpl = options.fetchImpl ?? ((input, init) => fetch(input, init));
    this.now = options.now ?? (() => Date.now());
    this.timeoutMs = options.timeoutMs ?? REQUEST_TIMEOUT_MS;
    this.deviceClientId = options.deviceClientId ?? "stackspirit-ui";
  }

  /** True when a device flow completed in this instance. No token is exposed. */
  get hasSession(): boolean {
    return this.accessToken !== null;
  }

  clearSession(): void {
    this.accessToken = null;
  }

  async startDeviceSignIn(): Promise<DeviceAuthorization> {
    const body = await this.requestJson({
      method: "POST",
      path: "/v1/auth/device",
      body: { client_id: this.deviceClientId, kind: "web" },
      authenticated: false,
    });
    if (!isRecord(body)) {
      throw cloudError("cloud_parse", "The cloud API returned an unreadable device authorization.");
    }
    const expires = requireInteger(body.expires_in, "device.expires_in");
    const interval = Math.max(1, requireInteger(body.interval, "device.interval"));
    const issuedAt = this.now();
    return {
      device_code: requireString(body.device_code, "device.device_code"),
      user_code: requireString(body.user_code, "device.user_code"),
      verification_uri: requireString(body.verification_uri, "device.verification_uri"),
      verification_uri_complete: optionalString(
        body.verification_uri_complete,
        "device.verification_uri_complete",
      ),
      expires_in: Math.max(0, expires),
      interval,
      expires_at: new Date(issuedAt + Math.max(0, expires) * 1000).toISOString(),
    };
  }

  async pollDeviceSignIn(deviceCode: string): Promise<DevicePollState> {
    const body = await this.requestRaw({
      method: "POST",
      path: "/v1/auth/device/token",
      body: { device_code: deviceCode },
      authenticated: false,
    });
    if (!body.ok) {
      const error = isRecord(body.value) ? optionalString(body.value.error, "device.error") : null;
      if (body.status === 400 || body.status === 428) {
        if (error === "authorization_pending" || error === null) return "pending";
        if (error === "slow_down") return "slow_down";
        if (error === "expired_token") {
          throw cloudError(
            "cloud_auth",
            "The sign-in code expired. Start again.",
            null,
            false,
            body.status,
          );
        }
        if (error === "access_denied") {
          throw cloudError("cloud_auth", "The sign-in was declined.", null, false, body.status);
        }
      }
      throw statusError(body.status, apiMessage(body.value));
    }
    if (!isRecord(body.value)) {
      throw cloudError("cloud_parse", "The cloud API returned an unreadable device token.");
    }
    const token = requireString(body.value.access_token, "device.access_token");
    if (/\s/.test(token)) {
      throw cloudError("cloud_parse", "The cloud API returned an unreadable access token.");
    }
    this.accessToken = token;
    return "authorized";
  }

  async account(): Promise<CloudAccount> {
    return parseAccount(
      await this.requestJson({
        method: "GET",
        path: "/v1/me",
        authenticated: true,
      }),
    );
  }

  async listMachines(): Promise<CloudMachine[]> {
    return parseMachines(
      await this.requestJson({
        method: "GET",
        path: "/v1/machines",
        authenticated: true,
      }),
    );
  }

  async mintRelayTicket(
    machineId: string,
    request: RelayTicketRequest,
  ): Promise<RelayTicket> {
    const id = validateMachineId(machineId);
    const scopes = request.scopes.filter((scope) => SCOPES.has(scope));
    if (scopes.length === 0) {
      throw cloudError("invalid_input", "A relay ticket needs at least one scope.");
    }
    if (request.scopes.length !== scopes.length) {
      throw cloudError("invalid_input", "The relay ticket requested an unknown scope.");
    }
    const body = {
      kind: request.kind,
      scopes,
      client_key: parseJwk(request.client_key),
      client_nonce: requireString(request.client_nonce, "ticket.client_nonce"),
      ttl_secs: Math.max(30, Math.min(300, Math.round(request.ttl_secs))),
    };
    // Deliberately explicit: this body contains public key material only. There is
    // no SSH private key, passphrase, password or provider secret field to send.
    try {
      return parseRelayTicket(
        await this.requestJson({
          method: "POST",
          path: `/v1/machines/${encodeURIComponent(id)}/ticket`,
          body,
          authenticated: true,
        }),
      );
    } catch (error) {
      // The relay design also documents an account-wide `POST /v1/relay-tickets`;
      // use it only when the machine-scoped route is absent (a 404), never as a
      // silent retry after an auth, scope or server failure.
      if (asCloudError(error).status !== 404) throw error;
      return parseRelayTicket(
        await this.requestJson({
          method: "POST",
          path: "/v1/relay-tickets",
          body: { ...body, machine_id: id },
          authenticated: true,
        }),
      );
    }
  }

  async getPlans(): Promise<ManagedPlanCatalog> {
    // Public pricing: no bearer token is attached, so prices stay visible to an
    // account-free visitor without creating a login wall.
    return parseCatalog(
      await this.requestJson({ method: "GET", path: "/v1/plans", authenticated: false }),
    );
  }

  async getProvisioning(machineId: string): Promise<ProvisioningStatus> {
    const id = validateMachineId(machineId);
    return parseProvisioning(
      await this.requestJson({
        method: "GET",
        path: `/v1/machines/${encodeURIComponent(id)}/provisioning`,
        authenticated: true,
      }),
    );
  }

  private url(path: string): string {
    return `${this.apiBaseUrl}${path}`;
  }

  private async requestJson(options: RequestOptions): Promise<unknown> {
    const response = await this.requestRaw(options);
    if (!response.ok) throw statusError(response.status, apiMessage(response.value));
    return response.value;
  }

  private async requestRaw(options: RequestOptions): Promise<{
    ok: boolean;
    status: number;
    value: unknown;
  }> {
    const headers: Record<string, string> = { Accept: "application/json" };
    if (options.body !== undefined) headers["Content-Type"] = "application/json";
    if (options.authenticated) {
      if (!this.accessToken) {
        throw cloudError("cloud_auth", "Sign in to StackSpirit first.");
      }
      headers["Authorization"] = `Bearer ${this.accessToken}`;
    }
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), this.timeoutMs);
    let response: Response;
    try {
      response = await this.fetchImpl(this.url(options.path), {
        method: options.method,
        headers,
        body: options.body === undefined ? undefined : JSON.stringify(options.body),
        signal: controller.signal,
        credentials: "omit",
        cache: "no-store",
        redirect: "error",
      });
    } catch {
      if (controller.signal.aborted) {
        throw cloudError(
          "cloud_timeout",
          "The StackSpirit API did not answer in time.",
          "Retry when the audience is reachable.",
          true,
        );
      }
      throw cloudError(
        "cloud_unreachable",
        "The StackSpirit API could not be reached.",
        "Check the network and retry. Local server mode needs no account.",
        true,
      );
    } finally {
      clearTimeout(timeout);
    }

    const text = await response.text();
    if (text.length > MAX_BODY_CHARS) {
      throw cloudError("cloud_parse", "The cloud API returned an oversized document.");
    }
    let value: unknown = null;
    if (text.trim()) {
      try {
        value = JSON.parse(text) as unknown;
      } catch {
        if (response.ok) {
          throw cloudError("cloud_parse", "The cloud API returned an unreadable document.");
        }
      }
    }
    return { ok: response.ok, status: response.status, value };
  }
}
