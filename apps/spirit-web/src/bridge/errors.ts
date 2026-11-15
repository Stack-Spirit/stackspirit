import { errorTitle, toSpiritError, type SpiritError } from "@stackspirit/ui";

/** Error codes this Web bridge can emit in addition to the shared core codes. */
export type WebErrorCode =
  | "daemon_unreachable"
  | "daemon_incompatible"
  | "writes_disabled"
  | "desktop_only"
  | "no_route";

const WEB_TITLES: Record<WebErrorCode, string> = {
  daemon_unreachable: "The daemon did not answer",
  daemon_incompatible: "This daemon cannot serve the Web shell",
  writes_disabled: "Writes are disabled on this daemon",
  desktop_only: "This action belongs to the desktop app",
  no_route: "This daemon has no such route",
};

export function isWebErrorCode(code: string): code is WebErrorCode {
  return Object.prototype.hasOwnProperty.call(WEB_TITLES, code);
}

/** Fixed title for a Web error; falls back to the shared core title table. */
export function webErrorTitle(error: SpiritError): string {
  if (isWebErrorCode(error.code)) return WEB_TITLES[error.code];
  return errorTitle(error);
}

export function asSpiritError(value: unknown): SpiritError {
  return toSpiritError(value);
}

export function webError(
  code: WebErrorCode | string,
  message: string,
  hint: string | null = null,
  retryable = false,
): SpiritError {
  return { code, message, hint, retryable };
}
