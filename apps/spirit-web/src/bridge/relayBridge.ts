/**
 * Adapts a secured `CloudRelayClient` to the read-only Web bridge.
 *
 * `HttpSseBridge` already owns the honest read model, stale notice, polling
 * fallback and route allowlist. This adapter only replaces `fetch` with relay
 * `http_request` frames, so the Web cockpit reuses exactly the same page
 * contracts over `app.stackspirit.io` as it does over a loopback daemon. Board
 * SSE is not tunnelled in v1; the bridge's documented 5 second polling fallback
 * is used instead.
 */

import type { CloudRelayConnector } from "@stackspirit/ui";

import { HttpSseBridge, type FetchLike } from "./HttpSseBridge";
import { webError } from "./errors";

export const RELAY_BRIDGE_ORIGIN = "https://relay.stackspirit.io";

function requestUrl(input: RequestInfo | URL): string {
  if (typeof input === "string") return input;
  if (input instanceof URL) return input.toString();
  return input.url;
}

function headersToRecord(headers: HeadersInit | undefined): Record<string, string> {
  if (!headers) return {};
  const source = new Headers(headers);
  const record: Record<string, string> = {};
  source.forEach((value, key) => {
    record[key] = value;
  });
  return record;
}

/** A `fetch`-shaped function whose transport is one encrypted relay channel. */
export function createRelayFetch(relay: CloudRelayConnector): FetchLike {
  return async (input, init) => {
    let url: URL;
    try {
      url = new URL(requestUrl(input), RELAY_BRIDGE_ORIGIN);
    } catch {
      throw webError("relay_protocol", "The relay request URL is not readable.");
    }
    if (url.origin !== RELAY_BRIDGE_ORIGIN) {
      throw webError(
        "relay_scope_denied",
        "The relay bridge only serves the configured relay origin.",
      );
    }
    const method = (init?.method ?? "GET").toUpperCase();
    if (method !== "GET" && method !== "POST") {
      throw webError("relay_scope_denied", "Only GET and POST cross the relay bridge.");
    }
    let body: string | null = null;
    if (init?.body !== undefined && init.body !== null) {
      if (typeof init.body !== "string") {
        throw webError("invalid_input", "The relay bridge accepts only text request bodies.");
      }
      body = init.body;
    }
    const response = await relay.request(`${url.pathname}${url.search}`, {
      method,
      headers: headersToRecord(init?.headers),
      body,
    });
    const headers = new Headers();
    for (const [name, value] of Object.entries(response.headers)) {
      // Response headers that could establish browser state never enter the page.
      if (name.toLowerCase() === "set-cookie" || name.toLowerCase() === "www-authenticate") {
        continue;
      }
      headers.set(name, value);
    }
    return new Response(response.body ? response.body : null, {
      status: response.status,
      headers,
    });
  };
}

export interface RelayBridgeOptions {
  now?: () => number;
  pollIntervalMs?: number;
  staleBackoffMs?: number;
  readTimeoutMs?: number;
}

/**
 * Builds a fresh `HttpSseBridge` reading the signed-in Home over the relay.
 * Call it only after `CloudRelayClient.connect()` resolved; it makes no request
 * until the Web shell mounts it.
 */
export function createRelayBridge(
  relay: CloudRelayConnector,
  options: RelayBridgeOptions = {},
): HttpSseBridge {
  return new HttpSseBridge({
    baseUrl: RELAY_BRIDGE_ORIGIN,
    pageOrigin: RELAY_BRIDGE_ORIGIN,
    fetchImpl: createRelayFetch(relay),
    // Board SSE is not tunnelled in v1. A throwing factory selects the bridge's
    // documented polling fallback instead of opening a bogus EventSource.
    eventSourceFactory: () => {
      throw new Error("relay board stream uses polling");
    },
    now: options.now,
    pollIntervalMs: options.pollIntervalMs,
    staleBackoffMs: options.staleBackoffMs,
    readTimeoutMs: options.readTimeoutMs,
  });
}
