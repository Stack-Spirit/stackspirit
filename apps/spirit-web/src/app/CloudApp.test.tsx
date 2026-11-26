import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import {
  configureCloudRuntime,
  resetCloudAccount,
  type CloudAccountApi,
  type CloudRelayConnector,
  type RelayConnectionStatus,
  type RelayHttpResponse,
  type RelayScope,
} from "@stackspirit/ui";

import {
  BOARD,
  HEALTH,
  INBOX,
  JOB_DETAIL,
  JOB_GATES,
  JOB_RUNS,
  JOBS,
  SESSIONS,
} from "../test/fakes";
import { CloudApp } from "./CloudApp";
import type { WebCloudRuntime } from "../cloud/runtime";
import { RELAY_BRIDGE_ORIGIN } from "../bridge/relayBridge";

const MACHINE = {
  id: "mch_online",
  name: "contabo-1",
  status: "online" as const,
  project: "vowp",
  last_seen_at: "2026-11-26T10:00:00.000Z",
  provisioning: null,
};

class FakeClient implements CloudAccountApi {
  hasSession = false;
  async startDeviceSignIn() {
    return {
      device_code: "device-1",
      user_code: "ABCD-EFGH",
      verification_uri: "https://stackspirit.io/device",
      verification_uri_complete: null,
      expires_in: 600,
      interval: 5,
      expires_at: "2026-11-26T10:10:00.000Z",
    };
  }
  async pollDeviceSignIn() {
    this.hasSession = true;
    return "authorized" as const;
  }
  async account() {
    return { id: "acct_1", email: "owner@example.com", name: "Owner" };
  }
  async listMachines() {
    return [MACHINE];
  }
  async mintRelayTicket(machineId: string) {
    return {
      ticket: "ticket",
      expires_at: "2026-11-26T10:05:00.000Z",
      relay_url: "wss://relay.stackspirit.io/v1/client",
      machine: {
        id: machineId,
        name: MACHINE.name,
        key_id: "ed25519:test-machine-key",
        public_key: "test-machine-key",
        project: "vowp",
      },
    };
  }
  async getPlans() {
    return {
      plans: [
        {
          id: "home-8",
          name: "Home 8",
          vcpu: 4,
          ram_gb: 8,
          disk_gb: 100,
          disk_kind: "SSD",
          monthly_eur: 6.6,
          setup_fee_eur: 9.9,
          currency: "EUR" as const,
          provider_spec: "Cloud VPS 4",
        },
      ],
      backups: { monthly_eur: 3.5, recommended: true, provider: "cloudflare-r2" },
    };
  }
  async getProvisioning() {
    return {
      machine_id: MACHINE.id,
      phase: "provisioning" as const,
      step: "cloud-init",
      progress_pct: 40,
      detail: null,
      updated_at: "2026-11-26T10:01:00.000Z",
      home_url: null,
    };
  }
  clearSession(): void {
    this.hasSession = false;
  }
}

class FakeRelay implements CloudRelayConnector {
  current: RelayConnectionStatus = {
    state: "idle",
    machine_id: null,
    channel_id: null,
    attempts: 0,
    next_retry_at: null,
    error: null,
  };
  private listeners = new Set<(status: RelayConnectionStatus) => void>();

  async connect(machineId: string, options?: { scopes?: RelayScope[] }): Promise<void> {
    void options;
    this.current = {
      state: "secure",
      machine_id: machineId,
      channel_id: "01JQ8Z6K4W5N7R9T2V3X4Y5Z6A",
      attempts: 1,
      next_retry_at: null,
      error: null,
    };
    for (const listener of this.listeners) listener(this.current);
  }

  disconnect(): void {
    this.current = { ...this.current, state: "closed", channel_id: null };
    for (const listener of this.listeners) listener(this.current);
  }

  status(): RelayConnectionStatus {
    return this.current;
  }

  subscribe(listener: (status: RelayConnectionStatus) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  async request(path: string): Promise<RelayHttpResponse> {
    const url = new URL(path, RELAY_BRIDGE_ORIGIN);
    if (url.pathname === "/api/v1/health") return ok(HEALTH);
    if (url.pathname === "/api/v1/board") return ok(BOARD);
    if (url.pathname === "/api/v1/jobs") return ok(JOBS);
    if (url.pathname === "/api/v1/sessions") return ok(SESSIONS);
    if (url.pathname === "/api/v1/inbox") return ok(INBOX);
    if (url.pathname.startsWith("/api/v1/jobs/")) {
      if (url.pathname.endsWith("/runs")) return ok(JOB_RUNS);
      if (url.pathname.endsWith("/gates")) return ok(JOB_GATES);
      return ok(JOB_DETAIL);
    }
    return {
      status: 404,
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ message: "not found" }),
    };
  }
}

function ok(body: unknown): RelayHttpResponse {
  return {
    status: 200,
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  };
}

beforeEach(() => {
  resetCloudAccount();
  window.history.replaceState(null, "", "/cloud");
});

afterEach(() => {
  resetCloudAccount();
});

describe("Web Always-On Homes route", () => {
  it("stays optional and offers the local daemon path", async () => {
    const client = new FakeClient();
    const relay = new FakeRelay();
    configureCloudRuntime({ client, relay });
    render(<CloudApp runtime={makeRuntime(client, relay)} />);

    expect(screen.getByTestId("cloud-app")).toBeInTheDocument();
    expect(screen.getByTestId("cloud-local-default")).toHaveTextContent(/needs no account/i);
    expect(screen.getByRole("link", { name: /Continue with a local daemon/i })).toHaveAttribute(
      "href",
      "/",
    );
    // Public plans are visible without signing in.
    expect(await screen.findByTestId("managed-plans")).toHaveTextContent("Home 8");
  });

  it("signs in, connects an online Home and mounts the existing read-only cockpit", async () => {
    const client = new FakeClient();
    const relay = new FakeRelay();
    configureCloudRuntime({ client, relay });
    const user = userEvent.setup();
    render(<CloudApp runtime={makeRuntime(client, relay)} />);

    await user.click(screen.getByTestId("cloud-sign-in"));
    await user.click(await screen.findByTestId("cloud-poll"));
    await screen.findByTestId("cloud-account");
    await user.click(screen.getByTestId("cloud-connect-mch_online"));

    expect(await screen.findByTestId("cloud-live")).toBeInTheDocument();
    expect(await screen.findByTestId("web-lanes", {}, { timeout: 5_000 })).toBeInTheDocument();
    expect(screen.getByText(/Cloud Relay · contabo-1/)).toBeInTheDocument();

    await user.click(screen.getByTestId("cloud-live-disconnect"));
    await waitFor(() => {
      expect(screen.getByTestId("cloud-app")).toBeInTheDocument();
    });
  });
});

function makeRuntime(client: FakeClient, relay: FakeRelay): WebCloudRuntime {
  return {
    client,
    relay,
    dispose() {
      relay.disconnect();
      resetCloudAccount();
      client.clearSession();
    },
  };
}
