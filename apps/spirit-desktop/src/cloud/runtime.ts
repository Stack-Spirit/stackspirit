import {
  StackSpiritCloudClient,
  configureCloudRuntime,
  resetCloudAccount,
  type CloudAccountApi,
  type CloudFetch,
} from "@stackspirit/ui";

export interface DesktopCloudRuntimeOptions {
  /** Override the public API base; defaults to `https://api.stackspirit.io`. */
  apiBaseUrl?: string;
  /** Injectable for tests. Production uses the webview's `fetch`. */
  fetchImpl?: CloudFetch;
  /** Injectable clock for device-code expiry in tests. */
  now?: () => number;
  /** Device-flow client id; defaults to the shared UI client id. */
  deviceClientId?: string;
}

export interface DesktopCloudRuntime {
  client: CloudAccountApi;
  /** Drops in-memory state and the bearer session; never makes a request. */
  dispose(): void;
}

/**
 * Wires the optional StackSpirit Account client into the shared cloud store.
 *
 * Construction is inert: no account request and no socket happens until the
 * owner clicks Sign in or opens the plan catalogue. Account-free local and SSH
 * mode therefore behaves exactly as before. The relay connector is null in this
 * build: Desktop lists and signs in to Always-On Homes here, while the relayed
 * cockpit remains the Web transport until the desktop relay adapter lands.
 */
export function createDesktopCloudRuntime(
  options: DesktopCloudRuntimeOptions = {},
): DesktopCloudRuntime {
  const client = new StackSpiritCloudClient({
    apiBaseUrl: options.apiBaseUrl,
    fetchImpl: options.fetchImpl,
    now: options.now,
    deviceClientId: options.deviceClientId,
  });
  configureCloudRuntime({ client, relay: null });
  return {
    client,
    dispose() {
      resetCloudAccount();
      client.clearSession();
    },
  };
}
