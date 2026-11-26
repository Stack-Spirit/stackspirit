import {
  StackSpiritCloudClient,
  configureCloudRuntime,
  resetCloudAccount,
  type CloudAccountApi,
  type CloudFetch,
  type CloudRelayConnector,
} from "@stackspirit/ui";

import { CloudRelayClient, type RelaySocketFactory } from "../bridge/CloudRelayClient";
import type { RelayCryptoProvider } from "../bridge/relayCrypto";

export interface WebCloudRuntimeOptions {
  apiBaseUrl?: string;
  fetchImpl?: CloudFetch;
  socketFactory?: RelaySocketFactory;
  crypto?: RelayCryptoProvider;
  now?: () => number;
  random?: () => number;
  clientId?: string;
}

export interface WebCloudRuntime {
  client: CloudAccountApi;
  relay: CloudRelayConnector;
  dispose(): void;
}

/**
 * Wires the real account API and relay transport into the shared cloud store.
 * Construction is inert: no account request and no WebSocket happens until the
 * owner clicks Sign in or Connect.
 */
export function createWebCloudRuntime(options: WebCloudRuntimeOptions = {}): WebCloudRuntime {
  const client = new StackSpiritCloudClient({
    apiBaseUrl: options.apiBaseUrl,
    fetchImpl: options.fetchImpl,
    now: options.now,
  });
  const relay = new CloudRelayClient({
    ticketProvider: (input) =>
      client.mintRelayTicket(input.machineId, {
        kind: input.kind,
        scopes: input.scopes,
        client_key: input.clientKey,
        client_nonce: input.clientNonce,
        ttl_secs: input.ttlSecs,
      }),
    socketFactory: options.socketFactory,
    crypto: options.crypto,
    now: options.now,
    random: options.random,
    clientId: options.clientId,
  });
  configureCloudRuntime({ client, relay });
  return {
    client,
    relay,
    dispose() {
      relay.dispose();
      resetCloudAccount();
      client.clearSession();
    },
  };
}
