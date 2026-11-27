/**
 * The Web/PWA StackSpirit Account client.
 *
 * It implements the shared `CloudAccountApi` the store already consumes and adds
 * the browser sign-in endpoints used by the account-first landing screen:
 * Google/GitHub OAuth starts, email sign-up with a six-digit verification code,
 * email/password login, and one-time machine enrollment tokens.
 *
 * The bearer session lives in `session.ts` memory only. No request body, result,
 * error or view model carries it, and no key, password or SSH credential is ever
 * sent to the cloud API.
 */

import {
  CLOUD_DEFAULT_API_URL,
  cloudError,
  type CloudAccount,
  type CloudAccountApi,
  type CloudError,
  type CloudFetch,
  type CloudMachine,
  type DeviceAuthorization,
  type DevicePollState,
  type ManagedPlan,
  type ManagedPlanCatalog,
  type ProvisioningPhase,
  type ProvisioningStatus,
  type RelayPublicJwk,
  type RelayScope,
  type RelayTicket,
  type RelayTicketRequest,
} from "@stackspirit/ui";

import { getSessionToken, setSessionToken } from "./session";

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

export type WebAuthProvider = "google" | "github";

export interface EmailSignUpResult {
  email: string;
  /** True when the API asked for the six-digit verification code. */
  verificationRequired: boolean;
  /** True when the response already established a session. */
  signedIn: boolean;
}

export interface EmailVerifyResult {
  signedIn: boolean;
}

export interface EnrollmentToken {
  token: string;
  expires_at: string;
}

/**
 * The account surface the Web sign-in screen needs. `WebCloudClient` is the real
 * implementation; tests supply the same shape with a fake fetch.
 */
export interface WebAuthClient extends CloudAccountApi {
  oauthStartUrl(provider: WebAuthProvider): string;
  signUp(email: string, password: string): Promise<EmailSignUpResult>;
  verifyEmail(email: string, code: string): Promise<EmailVerifyResult>;
  login(email: string, password: string): Promise<void>;
  createEnrollmentToken(name: string, ttlSecs?: number): Promise<EnrollmentToken>;
}

export interface WebCloudClientOptions {
  /** The public API base. Production default: `https://api.stackspirit.io`. */
  apiBaseUrl?: string;
  fetchImpl?: CloudFetch;
  now?: () => number;
  timeoutMs?: number;
  deviceClientId?: string;
}

interface RequestOptions {
  method: "GET" | "POST";
  path: string;
  body?: unknown;
  authenticated?: boolean;
}

interface PlanUsdFields {
  monthly_usd: number | null;
  setup_fee_usd: number | null;
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

function optionalAmount(value: unknown): number | null {
  if (typeof value === "number" && Number.isFinite(value)) return Math.max(0, value);
  if (typeof value === "string" && value.trim() !== "") {
    const parsed = Number(value);
    if (Number.isFinite(parsed)) return Math.max(0, parsed);
  }
  return null;
}

function nestedAmount(body: Record<string, unknown>, key: string): number | null {
  const nested = body[key];
  if (!isRecord(nested)) return null;
  return optionalAmount(nested.usd ?? nested.USD);
}

function nestedEurAmount(body: Record<string, unknown>, key: string): number | null {
  const nested = body[key];
  if (!isRecord(nested)) return null;
  return optionalAmount(nested.eur ?? nested.EUR);
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
  const progress = optionalAmount(value.progress_pct);
  return {
    machine_id: requireString(value.machine_id, "provisioning.machine_id"),
    phase: phase as ProvisioningPhase,
    step: optionalString(value.step, "provisioning.step"),
    progress_pct:
      progress === null ? null : Math.max(0, Math.min(100, Math.round(progress))),
    detail:
      value.detail === null || value.detail === undefined
        ? null
        : safeDetail(requireString(value.detail, "provisioning.detail")),
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
    status: status as CloudMachine["status"],
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
  if (
    !/^wss:\/\//.test(relayUrl) &&
    !/^ws:\/\/(?:127\.0\.0\.1|localhost)(?::\d+)?\//.test(relayUrl)
  ) {
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
  const monthlyEur =
    optionalAmount(value.monthly_eur) ??
    nestedEurAmount(value, "prices") ??
    nestedEurAmount(value, "monthly");
  if (monthlyEur === null) {
    throw cloudError("cloud_parse", `The cloud API returned an unreadable plan at ${index}.`);
  }
  const plan: ManagedPlan & PlanUsdFields = {
    id: requireString(value.id, `plans[${index}].id`),
    name: requireString(value.name, `plans[${index}].name`),
    vcpu: Math.max(0, requireInteger(value.vcpu, `plans[${index}].vcpu`)),
    ram_gb: Math.max(0, requireInteger(value.ram_gb, `plans[${index}].ram_gb`)),
    disk_gb: Math.max(0, requireInteger(value.disk_gb, `plans[${index}].disk_gb`)),
    disk_kind: optionalString(value.disk_kind, `plans[${index}].disk_kind`),
    monthly_eur: monthlyEur,
    setup_fee_eur:
      optionalAmount(value.setup_fee_eur) ??
      nestedEurAmount(value, "setup_fee") ??
      nestedEurAmount(value, "setup"),
    currency: "EUR",
    provider_spec: optionalString(value.provider_spec, `plans[${index}].provider_spec`),
    monthly_usd:
      optionalAmount(value.monthly_usd) ??
      nestedAmount(value, "prices") ??
      nestedAmount(value, "monthly"),
    setup_fee_usd:
      optionalAmount(value.setup_fee_usd) ??
      nestedAmount(value, "setup_fee") ??
      nestedAmount(value, "setup"),
  };
  return plan;
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
  const backupRaw = isRecord(value) && isRecord(value.backups) ? value.backups : {};
  const backups = {
    monthly_eur: optionalAmount(backupRaw.monthly_eur),
    recommended:
      backupRaw.recommended === undefined
        ? true
        : requireBoolean(backupRaw.recommended, "backups.recommended"),
    provider: optionalString(backupRaw.provider, "backups.provider"),
    monthly_usd:
      optionalAmount(backupRaw.monthly_usd) ?? nestedAmount(backupRaw, "prices"),
  };
  return { plans, backups };
}

function statusError(status: number, message: string): CloudError {
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

function readAccessToken(body: unknown): string | null {
  if (!isRecord(body)) return null;
  const candidates: unknown[] = [
    body.access_token,
    body.token,
    isRecord(body.session) ? body.session.access_token : null,
    isRecord(body.session) ? body.session.token : null,
    isRecord(body.auth) ? body.auth.access_token : null,
    isRecord(body.auth) ? body.auth.token : null,
  ];
  for (const candidate of candidates) {
    if (typeof candidate === "string" && candidate.length > 0 && !/\s/.test(candidate)) {
      return candidate;
    }
  }
  return null;
}

/** The production client used by the Web/PWA sign-in and Always-On Homes screen. */
export class WebCloudClient implements WebAuthClient {
  private readonly apiBaseUrl: string;
  private readonly fetchImpl: CloudFetch;
  private readonly now: () => number;
  private readonly timeoutMs: number;
  private readonly deviceClientId: string;

  constructor(options: WebCloudClientOptions = {}) {
    this.apiBaseUrl = validateApiBase(options.apiBaseUrl ?? CLOUD_DEFAULT_API_URL);
    this.fetchImpl = options.fetchImpl ?? ((input, init) => fetch(input, init));
    this.now = options.now ?? (() => Date.now());
    this.timeoutMs = options.timeoutMs ?? REQUEST_TIMEOUT_MS;
    this.deviceClientId = options.deviceClientId ?? "stackspirit-web";
  }

  get hasSession(): boolean {
    return getSessionToken() !== null;
  }

  clearSession(): void {
    setSessionToken(null);
  }

  oauthStartUrl(provider: WebAuthProvider): string {
    return `${this.apiBaseUrl}/v1/auth/${provider}/start`;
  }

  async signUp(email: string, password: string): Promise<EmailSignUpResult> {
    const address = requireEmail(email);
    const body = await this.requestJson({
      method: "POST",
      path: "/v1/auth/signup",
      body: { email: address, password },
      authenticated: false,
    });
    const signedIn = this.absorbToken(body);
    return { email: address, verificationRequired: !signedIn, signedIn };
  }

  async verifyEmail(email: string, code: string): Promise<EmailVerifyResult> {
    const address = requireEmail(email);
    const value = code.trim();
    if (!/^\d{6}$/.test(value)) {
      throw cloudError("invalid_input", "Enter the 6-digit code from your email.");
    }
    const body = await this.requestJson({
      method: "POST",
      path: "/v1/auth/verify-email",
      body: { email: address, code: value },
      authenticated: false,
    });
    return { signedIn: this.absorbToken(body) };
  }

  async login(email: string, password: string): Promise<void> {
    const address = requireEmail(email);
    const body = await this.requestJson({
      method: "POST",
      path: "/v1/auth/login",
      body: { email: address, password },
      authenticated: false,
    });
    if (!this.absorbToken(body)) {
      throw cloudError(
        "cloud_auth",
        "The sign-in response did not include a session.",
        "Try again or use Google or GitHub.",
      );
    }
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
      const error = isRecord(body.value)
        ? optionalString(body.value.error, "device.error")
        : null;
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
    if (!this.absorbToken(body.value)) {
      throw cloudError("cloud_parse", "The cloud API returned an unreadable device token.");
    }
    return "authorized";
  }

  async account(): Promise<CloudAccount> {
    return parseAccount(
      await this.requestJson({ method: "GET", path: "/v1/me", authenticated: true }),
    );
  }

  async listMachines(): Promise<CloudMachine[]> {
    return parseMachines(
      await this.requestJson({ method: "GET", path: "/v1/machines", authenticated: true }),
    );
  }

  async mintRelayTicket(
    machineId: string,
    request: RelayTicketRequest,
  ): Promise<RelayTicket> {
    const id = validateMachineId(machineId);
    const scopes: RelayScope[] = request.scopes.filter((scope) => SCOPES.has(scope));
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
      const status = isRecord(error) && typeof error.status === "number" ? error.status : null;
      if (status !== 404) throw error;
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

  async createEnrollmentToken(name: string, ttlSecs = 900): Promise<EnrollmentToken> {
    const machineName = name.trim();
    if (!machineName) {
      throw cloudError("invalid_input", "Give the machine a name first.");
    }
    const body = await this.requestJson({
      method: "POST",
      path: "/v1/machines/enroll-tokens",
      body: { name: machineName, ttl_secs: Math.max(60, Math.min(3600, Math.round(ttlSecs))) },
      authenticated: true,
    });
    if (!isRecord(body)) {
      throw cloudError("cloud_parse", "The cloud API returned an unreadable enrollment token.");
    }
    const tokenBody = isRecord(body.enrollment) ? body.enrollment : body;
    return {
      token: requireString(tokenBody.token, "enrollment.token"),
      expires_at: requireTimestamp(tokenBody.expires_at, "enrollment.expires_at"),
    };
  }

  private absorbToken(body: unknown): boolean {
    const token = readAccessToken(body);
    if (!token) return false;
    setSessionToken(token);
    return true;
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
      const token = getSessionToken();
      if (!token) throw cloudError("cloud_auth", "Sign in to StackSpirit first.");
      headers["Authorization"] = `Bearer ${token}`;
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

function requireEmail(value: string): string {
  const email = value.trim();
  if (!email || !email.includes("@")) {
    throw cloudError("invalid_input", "Enter a valid email address.");
  }
  return email;
}
