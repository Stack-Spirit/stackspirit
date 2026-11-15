import {
  MOCK_SCENARIOS,
  type MockScenario,
  type SpiritBridge,
  type SpiritErrorDto,
} from "@stackspirit/ui";

import { parseBaseUrl, resolveBaseUrl, sameOriginError } from "./baseUrl";
import { webError } from "./errors";
import { HttpSseBridge } from "./HttpSseBridge";
import { WebMockBridge } from "./WebMockBridge";

export interface BridgeBootstrap {
  bridge: SpiritBridge | null;
  /** The app has no daemon address yet; render the setup panel. */
  setup: boolean;
  error: SpiritErrorDto | null;
  baseUrl: string | null;
}

export interface BridgeEnvironment {
  mock: string | undefined;
  scenario: string | undefined;
  daemonUrl: string | undefined;
}

export interface BootstrapLocation {
  search: string;
  pathname: string;
  hash: string;
  origin: string;
}

function readEnvironment(): BridgeEnvironment {
  return {
    mock: import.meta.env.VITE_SPIRIT_MOCK,
    scenario: import.meta.env.VITE_SPIRIT_MOCK_SCENARIO,
    daemonUrl: import.meta.env.VITE_VOWP_DAEMON_URL,
  };
}

function browserLocation(): BootstrapLocation {
  return {
    search: window.location.search,
    pathname: window.location.pathname,
    hash: window.location.hash,
    origin: window.location.origin,
  };
}

function safeStorage(): Storage | null {
  try {
    return window.sessionStorage;
  } catch {
    return null;
  }
}

function scenarioFromName(name: string | undefined): MockScenario {
  if (name === "web-read-only") return "fresh";
  if (name === "web-stale") return "stale-cache";
  if (name === "web-recovering") return "fresh";
  if (name && (MOCK_SCENARIOS as readonly string[]).includes(name)) {
    return name as MockScenario;
  }
  return "fresh";
}

/**
 * The Web bridge factory. Preview is opt-in with `VITE_SPIRIT_MOCK=1`; otherwise the app
 * needs an explicit daemon base URL. A live failure never falls back to mock data.
 */
export function createBridge(
  environment: BridgeEnvironment = readEnvironment(),
  location: BootstrapLocation = browserLocation(),
): BridgeBootstrap {
  if (environment.mock === "1") {
    return {
      bridge: new WebMockBridge(scenarioFromName(environment.scenario)),
      setup: false,
      error: null,
      baseUrl: null,
    };
  }

  const resolved = resolveBaseUrl({
    search: location.search,
    envUrl: environment.daemonUrl,
    storage: safeStorage(),
    history: window.history,
    location: {
      pathname: location.pathname,
      hash: location.hash,
      origin: location.origin,
    },
  });
  if (resolved.error) {
    return { bridge: null, setup: true, error: resolved.error, baseUrl: null };
  }
  if (!resolved.baseUrl) {
    return { bridge: null, setup: true, error: null, baseUrl: null };
  }
  const sameOrigin = new URL(resolved.baseUrl).origin === location.origin;
  if (!sameOrigin) {
    return {
      bridge: null,
      setup: false,
      error: sameOriginError(),
      baseUrl: resolved.baseUrl,
    };
  }
  try {
    return {
      bridge: new HttpSseBridge({
        baseUrl: resolved.baseUrl,
        pageOrigin: location.origin,
      }),
      setup: false,
      error: null,
      baseUrl: resolved.baseUrl,
    };
  } catch (error) {
    return {
      bridge: null,
      setup: false,
      error:
        typeof error === "object" && error !== null && "code" in error
          ? (error as SpiritErrorDto)
          : webError("daemon_incompatible", "The Web bridge could not start."),
      baseUrl: resolved.baseUrl,
    };
  }
}

/** Used by the setup panel; the app reloads with a `?daemon=` value the bridge removes. */
export function setupRedirectUrl(baseUrl: string, location: BootstrapLocation): string {
  const parsed = parseBaseUrl(baseUrl);
  if (!parsed.baseUrl) {
    throw parsed.error ?? webError("invalid_input", "Enter a valid daemon address.");
  }
  const params = new URLSearchParams(location.search.startsWith("?") ? location.search.slice(1) : location.search);
  params.set("daemon", parsed.baseUrl);
  const query = params.toString();
  return `${location.pathname}${query ? `?${query}` : ""}${location.hash}`;
}
