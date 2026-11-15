import type { SpiritErrorDto } from "@stackspirit/ui";

import { SAME_ORIGIN_REQUIRED } from "../copy";
import { webError } from "./errors";

export const DAEMON_STORAGE_KEY = "spirit.daemon.base";

export interface BaseUrlResolution {
  baseUrl: string | null;
  source: "query" | "storage" | "env" | "none";
  error: SpiritErrorDto | null;
}

export interface BaseUrlOptions {
  /** `window.location.search`, including the leading `?`. */
  search?: string;
  /** `VITE_VOWP_DAEMON_URL` at build time. */
  envUrl?: string | undefined;
  /** Session storage for the daemon address; omit in tests that do not have a DOM. */
  storage?: Pick<Storage, "getItem" | "setItem" | "removeItem"> | null;
  /** `window.history`; omit in tests that do not have a DOM. */
  history?: Pick<History, "replaceState"> | null;
  /** Defaults to `window.location`. */
  location?: Pick<Location, "pathname" | "hash" | "origin"> | null;
}

function invalidBase(message: string): SpiritErrorDto {
  return webError(
    "invalid_input",
    message,
    "Use an absolute http:// or https:// URL with a host and no userinfo, query, or fragment.",
  );
}

/**
 * Validates and normalizes a user-supplied daemon base URL.
 *
 * The daemon has no fixed port. The bridge never guesses one and never falls back to an
 * origin. A path prefix is allowed for a reverse proxy mounted under `/vowp`.
 */
export function parseBaseUrl(raw: string | null | undefined): {
  baseUrl: string | null;
  error: SpiritErrorDto | null;
} {
  const value = raw?.trim() ?? "";
  if (!value) {
    return {
      baseUrl: null,
      error: invalidBase("Enter the daemon address. The Web app never guesses a port."),
    };
  }
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return {
      baseUrl: null,
      error: invalidBase("The daemon address must be an absolute http(s) URL."),
    };
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    return {
      baseUrl: null,
      error: invalidBase("The daemon address must use http or https."),
    };
  }
  if (!url.hostname) {
    return {
      baseUrl: null,
      error: invalidBase("The daemon address must include a host."),
    };
  }
  if (url.username || url.password) {
    return {
      baseUrl: null,
      error: invalidBase("The daemon address must not contain userinfo."),
    };
  }
  if (url.search || url.hash) {
    return {
      baseUrl: null,
      error: invalidBase("The daemon address must not contain a query string or fragment."),
    };
  }
  const path = url.pathname === "/" ? "" : url.pathname.replace(/\/+$/, "");
  return { baseUrl: `${url.origin}${path}`, error: null };
}

/** True when the API base and the page share an origin. A path prefix is fine. */
export function isSameOrigin(baseUrl: string, pageOrigin: string): boolean {
  try {
    return new URL(baseUrl).origin === pageOrigin;
  } catch {
    return false;
  }
}

export function sameOriginError(): SpiritErrorDto {
  return webError("daemon_incompatible", SAME_ORIGIN_REQUIRED);
}

function removeDaemonQuery(
  search: string,
  location: Pick<Location, "pathname" | "hash"> | null,
  history: Pick<History, "replaceState"> | null,
): void {
  if (!history || !location) return;
  const params = new URLSearchParams(search.startsWith("?") ? search.slice(1) : search);
  if (!params.has("daemon")) return;
  params.delete("daemon");
  const query = params.toString();
  history.replaceState(
    null,
    "",
    `${location.pathname}${query ? `?${query}` : ""}${location.hash}`,
  );
}

/**
 * Resolves the base URL in contract order: `?daemon=`, the address saved from a previous
 * `?daemon=` visit, then `VITE_VOWP_DAEMON_URL`. A malformed value is an error; it never
 * falls through to a different source.
 */
export function resolveBaseUrl(options: BaseUrlOptions = {}): BaseUrlResolution {
  const search = options.search ?? "";
  const storage = options.storage ?? null;
  const history = options.history ?? null;
  const location = options.location ?? null;
  const params = new URLSearchParams(search.startsWith("?") ? search.slice(1) : search);
  const queryValue = params.get("daemon");

  if (queryValue !== null) {
    const parsed = parseBaseUrl(queryValue);
    if (!parsed.baseUrl) {
      return { baseUrl: null, source: "query", error: parsed.error };
    }
    storage?.setItem(DAEMON_STORAGE_KEY, parsed.baseUrl);
    removeDaemonQuery(search, location, history);
    return { baseUrl: parsed.baseUrl, source: "query", error: null };
  }

  const stored = storage?.getItem(DAEMON_STORAGE_KEY) ?? null;
  if (stored) {
    const parsed = parseBaseUrl(stored);
    if (!parsed.baseUrl) {
      storage?.removeItem(DAEMON_STORAGE_KEY);
      return { baseUrl: null, source: "storage", error: parsed.error };
    }
    return { baseUrl: parsed.baseUrl, source: "storage", error: null };
  }

  if (options.envUrl && options.envUrl.trim()) {
    const parsed = parseBaseUrl(options.envUrl);
    if (!parsed.baseUrl) {
      return { baseUrl: null, source: "env", error: parsed.error };
    }
    return { baseUrl: parsed.baseUrl, source: "env", error: null };
  }

  return { baseUrl: null, source: "none", error: null };
}
