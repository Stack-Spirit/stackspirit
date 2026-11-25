import type { SpiritErrorDto } from "./wire";

/**
 * Stable error codes from `spirit-core::error::SpiritErrorCode`. Copy is fixed by
 * `docs/PLAN.md` §5.5; never replace a primary message with raw remote stderr.
 */
export type SpiritErrorCode =
  | "invalid_input"
  | "not_found"
  | "conflict"
  | "refused"
  | "limit"
  | "auth"
  | "host_not_found"
  | "host_key_mismatch"
  | "ssh_auth"
  | "ssh_unavailable"
  | "remote"
  | "remote_timeout"
  | "vowp_missing"
  | "vowp_incompatible"
  | "vowp_command"
  | "parse"
  | "keychain"
  | "engine"
  | "gate"
  | "io"
  | "internal";

export interface SpiritError extends SpiritErrorDto {
  code: SpiritErrorCode | string;
}

const KNOWN_CODES = new Set<string>([
  "invalid_input",
  "not_found",
  "conflict",
  "refused",
  "limit",
  "auth",
  "host_not_found",
  "host_key_mismatch",
  "ssh_auth",
  "ssh_unavailable",
  "remote",
  "remote_timeout",
  "vowp_missing",
  "vowp_incompatible",
  "vowp_command",
  "parse",
  "keychain",
  "engine",
  "gate",
  "io",
  "internal",
]);

export function isSpiritError(value: unknown): value is SpiritError {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.code === "string" &&
    typeof candidate.message === "string" &&
    typeof candidate.retryable === "boolean"
  );
}

/** Normalizes anything thrown by Tauri or a mock into the bridge error shape. */
export function toSpiritError(value: unknown): SpiritError {
  if (isSpiritError(value)) {
    return {
      code: value.code,
      message: value.message,
      hint: typeof value.hint === "string" ? value.hint : null,
      retryable: value.retryable,
    };
  }
  if (value instanceof Error) {
    return {
      code: "internal",
      message: value.message || "Unexpected desktop error",
      hint: null,
      retryable: false,
    };
  }
  if (typeof value === "string") {
    return { code: "internal", message: value, hint: null, retryable: false };
  }
  return {
    code: "internal",
    message: "Unexpected desktop error",
    hint: null,
    retryable: false,
  };
}

export function isKnownCode(code: string): code is SpiritErrorCode {
  return KNOWN_CODES.has(code);
}

/** Short, fixed UI copy for one error code. */
export function errorTitle(error: SpiritErrorDto): string {
  switch (error.code) {
    case "invalid_input":
      return "Check the highlighted fields";
    case "not_found":
      return "That item no longer exists";
    case "conflict":
      return "The host changed before the action finished";
    case "refused":
      return "StackSpirit refused the action";
    case "limit":
      return "A provider limit is blocking this work";
    case "auth":
      return "Provider authentication failed";
    case "host_not_found":
      return "Host profile not found";
    case "host_key_mismatch":
      return "Host key changed";
    case "ssh_auth":
      return "SSH authentication failed";
    case "ssh_unavailable":
      return "SSH is not available";
    case "remote":
      return "The remote command failed";
    case "remote_timeout":
      return "The host did not answer in time";
    case "vowp_missing":
      return "StackSpirit is not installed on this host";
    case "vowp_incompatible":
      return "This host runs an incompatible StackSpirit version";
    case "vowp_command":
      return "StackSpirit rejected the command";
    case "parse":
      return "The host returned a document StackSpirit could not read";
    case "keychain":
      return "The local keychain failed";
    case "engine":
      return "StackSpirit needs attention";
    case "gate":
      return "A capability gate is not ready";
    case "io":
      return "A local file operation failed";
    default:
      return "Unexpected desktop error";
  }
}

export interface HostKeyMismatch {
  oldSha256: string;
  newSha256: string;
}

/**
 * Extracts the two fingerprints from a core mismatch error. Core keeps the message
 * safe: `host key changed: pinned SHA256:…, server offered SHA256:…`.
 */
export function parseHostKeyMismatch(error: SpiritErrorDto): HostKeyMismatch | null {
  if (error.code !== "host_key_mismatch") return null;
  const match = /pinned\s+(SHA256:[^\s,]+),\s+server offered\s+(SHA256:[^\s,]+)/i.exec(
    error.message,
  );
  if (!match || !match[1] || !match[2]) return null;
  return { oldSha256: match[1], newSha256: match[2] };
}
