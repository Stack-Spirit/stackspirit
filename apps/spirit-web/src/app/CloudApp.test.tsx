import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import {
  configureCloudRuntime,
  resetCloudAccount,
  type CloudAccount,
  type CloudMachine,
  type CloudRelayConnector,
  type DeviceAuthorization,
  type DevicePollState,
  type ManagedPlanCatalog,
  type ProvisioningStatus,
  type RelayConnectionStatus,
  type RelayHttpResponse,
  type RelayScope,
  type RelayTicket,
  type RelayTicketRequest,
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
import type { WebAuthClient } from "../auth/client";
import { CloudApp } from "./CloudApp";
import type { WebCloudRuntime } from "../cloud/runtime";
import { RELAY_BRIDGE_ORIGIN } from "../bridge/relayBridge";

const ACCOUNT: CloudAccount = {
  id: "acct_1",
  email: "owner@example.com",
  name: "Owner",
};

const MACHINE: CloudMachine = {
  id: "mch_online",
  name: "contabo-1",
  status: "online",
  project: "vowp",
  last_seen_at: "2026-11-26T10:00:00.000Z",
  provisioning: null,
};

class FakeClient implements WebAuthClient {
  hasSession = false;

  constructor(private readonly machines: CloudMachine[] = [MACHINE]) {}

  oauthStartUrl(provider: "google" | "github"): string {
    return `https://api.stackspirit.io/v1/auth/${provider}/start`;
  }

  async signUp(email: string): Promise<{
    email: string;
    verificationRequired: boolean;
    signedIn: boolean;
  }> {
    this.hasSession = true;
    return { email, verificationRequired: false, signedIn: true };
  }

  async verifyEmail(): Promise<{ signedIn: boolean }> {
    this.hasSession = true;
    return { signedIn: true };
  }

  async login(): Promise<void> {
    this.hasSession = true;
  }

  async createEnrollmentToken(): Promise<{ token: string; expires_at: string }> {
    return {
      token: "enrollment-token-value",
      expires_at: "2026-11-26T10:15:00.000Z",
    };
  }

  async startDeviceSignIn(): Promise<DeviceAuthorization> {
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

  async pollDeviceSignIn(): Promise<DevicePollState> {
    this.hasSession = true;
    return "authorized";
  }

  async account(): Promise<CloudAccount> {
    return ACCOUNT;
  }

  async listMachines(): Promise<CloudMachine[]> {
    return this.machines;
  }

  async mintRelayTicket(machineId: string, _request: RelayTicketRequest): Promise<RelayTicket> {
    void _request;
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

  async getPlans(): Promise<ManagedPlanCatalog> {
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

  async getProvisioning(): Promise<ProvisioningStatus> {
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

function makeRuntime(client: FakeClient, relay: FakeRelay): WebCloudRuntime {
  return {
    client,
    auth: client,
    relay,
    dispose() {
      relay.disconnect();
      resetCloudAccount();
      client.clearSession();
    },
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
  it("puts Google, GitHub and email sign-in first and hides the local daemon detail", async () => {
    const client = new FakeClient();
    const relay = new FakeRelay();
    configureCloudRuntime({ client, relay });
    const user = userEvent.setup();
    render(<CloudApp runtime={makeRuntime(client, relay)} />);

    expect(screen.getByTestId("cloud-app")).toBeInTheDocument();
    expect(screen.getByTestId("auth-google")).toHaveAttribute(
      "href",
      "https://api.stackspirit.io/v1/auth/google/start",
    );
    expect(screen.getByTestId("auth-github")).toHaveAttribute(
      "href",
      "https://api.stackspirit.io/v1/auth/github/start",
    );
    expect(screen.getByTestId("auth-mode-sign-up")).toBeInTheDocument();
    expect(screen.queryByTestId("local-daemon-details")).not.toBeInTheDocument();

    await user.click(screen.getByTestId("local-daemon-toggle"));
    expect(screen.getByTestId("local-daemon-details")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /Continue with a local daemon/i })).toHaveAttribute(
      "href",
      "/",
    );

    // Plans stay behind the explicit "See plans" disclosure until asked for.
    expect(screen.queryByTestId("managed-plans")).not.toBeInTheDocument();
    await user.click(screen.getByTestId("managed-homes-expand"));
    expect(await screen.findByTestId("managed-plans")).toHaveTextContent("Home 8");
  });

  it("signs in with email and offers the two post-signup onboarding paths", async () => {
    const client = new FakeClient([]);
    const relay = new FakeRelay();
    configureCloudRuntime({ client, relay });
    const user = userEvent.setup();
    render(<CloudApp runtime={makeRuntime(client, relay)} />);

    await user.type(screen.getByLabelText("Email"), "owner@example.com");
    await user.type(screen.getByLabelText("Password"), "correct-horse-battery");
    await user.click(screen.getByTestId("auth-submit"));

    const onboarding = await screen.findByTestId("web-onboarding");
    expect(onboarding).toHaveTextContent(/Connect your existing Always-On Machine/i);
    expect(onboarding).toHaveTextContent(/Need an Always-On Machine/i);

    await user.type(screen.getByLabelText("Machine name"), "contabo-existing");
    await user.click(screen.getByTestId("onboard-create-token"));
    expect(await screen.findByTestId("web-enroll-token")).toHaveTextContent(
      "enrollment-token-value",
    );

    await user.click(screen.getByTestId("onboard-open-plans"));
    expect(await screen.findByTestId("managed-plans")).toHaveTextContent("Home 8");
  });

  it("accepts the #auth_token callback, stores the session and scrubs the URL", async () => {
    const client = new FakeClient();
    const relay = new FakeRelay();
    configureCloudRuntime({ client, relay });
    window.history.replaceState(null, "", "/cloud#auth_token=auth-token-value");
    render(<CloudApp runtime={makeRuntime(client, relay)} />);

    expect(await screen.findByTestId("cloud-account")).toHaveTextContent(
      "owner@example.com",
    );
    expect(window.location.pathname).toBe("/cloud");
    expect(window.location.hash).toBe("");
  });

  it("connects an online Home and mounts the existing read-only cockpit", async () => {
    const client = new FakeClient();
    const relay = new FakeRelay();
    configureCloudRuntime({ client, relay });
    window.history.replaceState(null, "", "/cloud#auth_token=auth-token-value");
    const user = userEvent.setup();
    render(<CloudApp runtime={makeRuntime(client, relay)} />);

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
