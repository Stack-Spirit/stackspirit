import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  configureCloudRuntime,
  resetCloudAccount,
  useCloudAccount,
} from "./store";
import type {
  CloudAccount,
  CloudAccountApi,
  CloudMachine,
  CloudRelayConnector,
  ManagedPlanCatalog,
  ProvisioningStatus,
  RelayConnectionStatus,
  RelayHttpResponse,
  RelayScope,
  RelayTicket,
  RelayTicketRequest,
} from "./types";

const ACCOUNT: CloudAccount = { id: "acct_1", email: "owner@example.com", name: "Owner" };
const MACHINE_ONLINE: CloudMachine = {
  id: "mch_online",
  name: "contabo-1",
  status: "online",
  project: "vowp",
  last_seen_at: "2026-11-26T10:00:00.000Z",
  provisioning: null,
};
const MACHINE_OFFLINE: CloudMachine = {
  id: "mch_offline",
  name: "home-mini",
  status: "offline",
  project: null,
  last_seen_at: "2026-11-25T10:00:00.000Z",
  provisioning: null,
};
const CATALOG: ManagedPlanCatalog = {
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
      currency: "EUR",
      provider_spec: "Cloud VPS 4",
    },
  ],
  backups: { monthly_eur: 3.5, recommended: true, provider: "cloudflare-r2" },
};
const PROVISIONING: ProvisioningStatus = {
  machine_id: "mch_online",
  phase: "provisioning",
  step: "cloud-init",
  progress_pct: 40,
  detail: null,
  updated_at: "2026-11-26T10:01:00.000Z",
  home_url: null,
};

class FakeClient implements CloudAccountApi {
  hasSession = false;
  startCalls = 0;
  pollCalls = 0;
  machinesCalls = 0;
  ticketCalls: Array<{ machineId: string; request: RelayTicketRequest }> = [];
  clearCalls = 0;
  constructor(
    private readonly options: {
      poll?: "authorized" | "pending";
      machines?: CloudMachine[];
    } = {},
  ) {}

  async startDeviceSignIn() {
    this.startCalls += 1;
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
    this.pollCalls += 1;
    if (this.options.poll === "pending") return "pending" as const;
    this.hasSession = true;
    return "authorized" as const;
  }

  async account() {
    return ACCOUNT;
  }

  async listMachines() {
    this.machinesCalls += 1;
    return this.options.machines ?? [MACHINE_ONLINE, MACHINE_OFFLINE];
  }

  async mintRelayTicket(machineId: string, request: RelayTicketRequest): Promise<RelayTicket> {
    this.ticketCalls.push({ machineId, request });
    return {
      ticket: "ticket",
      expires_at: "2026-11-26T10:05:00.000Z",
      relay_url: "wss://relay.stackspirit.io/v1/client",
      machine: {
        id: machineId,
        name: "contabo-1",
        key_id: "ed25519:ab",
        public_key: "ab",
        project: "vowp",
      },
    };
  }

  async getPlans() {
    return CATALOG;
  }

  async getProvisioning() {
    return PROVISIONING;
  }

  clearSession() {
    this.clearCalls += 1;
    this.hasSession = false;
  }
}

class FakeRelay implements CloudRelayConnector {
  connectCalls: Array<{ machineId: string; scopes?: RelayScope[] }> = [];
  disconnectCalls = 0;
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
    this.connectCalls.push({ machineId, scopes: options?.scopes });
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
    this.disconnectCalls += 1;
    this.current = {
      ...this.current,
      state: "closed",
      channel_id: null,
    };
    for (const listener of this.listeners) listener(this.current);
  }

  status(): RelayConnectionStatus {
    return this.current;
  }

  subscribe(listener: (status: RelayConnectionStatus) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  async request(): Promise<RelayHttpResponse> {
    throw new Error("not used in this test");
  }
}

beforeEach(() => {
  resetCloudAccount();
});

afterEach(() => {
  resetCloudAccount();
  vi.restoreAllMocks();
});

describe("cloud account store", () => {
  it("is account-free by default and makes no cloud call", async () => {
    const client = new FakeClient();
    const relay = new FakeRelay();
    const accountSpy = vi.spyOn(client, "account");
    const machinesSpy = vi.spyOn(client, "listMachines");
    const plansSpy = vi.spyOn(client, "getPlans");
    const relaySpy = vi.spyOn(relay, "connect");
    configureCloudRuntime({ client, relay });

    const state = useCloudAccount.getState();
    expect(state.phase).toBe("local");
    expect(state.account).toBeNull();
    expect(state.device).toBeNull();
    expect(state.machines).toEqual([]);
    expect(state.backupsEnabled).toBe(false);
    expect(client.startCalls).toBe(0);
    expect(accountSpy).not.toHaveBeenCalled();
    expect(machinesSpy).not.toHaveBeenCalled();
    expect(plansSpy).not.toHaveBeenCalled();
    expect(relaySpy).not.toHaveBeenCalled();
  });

  it("signs in with a device code, lists machines and selects the online Home", async () => {
    const client = new FakeClient();
    configureCloudRuntime({ client, relay: new FakeRelay() });

    await useCloudAccount.getState().startSignIn();
    expect(useCloudAccount.getState().phase).toBe("device_pending");
    expect(useCloudAccount.getState().device?.user_code).toBe("ABCD-EFGH");

    await useCloudAccount.getState().pollSignIn();
    const state = useCloudAccount.getState();
    expect(state.phase).toBe("signed_in");
    expect(state.account).toEqual(ACCOUNT);
    expect(state.machines.map((machine) => machine.id)).toEqual([
      "mch_online",
      "mch_offline",
    ]);
    expect(state.selectedMachineId).toBe("mch_online");
    expect(state.machinesStatus).toBe("ready");
    expect(state.device).toBeNull();
  });

  it("keeps a pending device poll calm and does not fake a session", async () => {
    const client = new FakeClient({ poll: "pending" });
    configureCloudRuntime({ client, relay: new FakeRelay() });
    await useCloudAccount.getState().startSignIn();
    await useCloudAccount.getState().pollSignIn();
    const state = useCloudAccount.getState();
    expect(state.phase).toBe("device_pending");
    expect(state.account).toBeNull();
    expect(state.machines).toEqual([]);
    expect(state.notice).toMatch(/Waiting for you to approve/);
    expect(client.hasSession).toBe(false);
  });

  it("connects a selected online Home through the relay with http.read only", async () => {
    const client = new FakeClient();
    const relay = new FakeRelay();
    configureCloudRuntime({ client, relay });
    await useCloudAccount.getState().startSignIn();
    await useCloudAccount.getState().pollSignIn();

    await useCloudAccount.getState().connectMachine("mch_online");
    expect(relay.connectCalls).toEqual([
      { machineId: "mch_online", scopes: ["http.read"] },
    ]);
    const state = useCloudAccount.getState();
    expect(state.relayStatus?.state).toBe("secure");
    expect(state.connectingMachineId).toBeNull();
    expect(state.error).toBeNull();
    expect(JSON.stringify(state)).not.toMatch(/ssh_private|private_key|passphrase/);
  });

  it("refuses an offline Home before opening a relay socket", async () => {
    const client = new FakeClient();
    const relay = new FakeRelay();
    configureCloudRuntime({ client, relay });
    await useCloudAccount.getState().startSignIn();
    await useCloudAccount.getState().pollSignIn();
    useCloudAccount.getState().selectMachine("mch_offline");

    await useCloudAccount.getState().connectMachine("mch_offline");
    expect(relay.connectCalls).toHaveLength(0);
    expect(useCloudAccount.getState().error).toMatchObject({ code: "cloud_machine_offline" });
  });

  it("reports a missing relay transport instead of pretending to connect", async () => {
    const client = new FakeClient();
    configureCloudRuntime({ client, relay: null });
    await useCloudAccount.getState().startSignIn();
    await useCloudAccount.getState().pollSignIn();
    await useCloudAccount.getState().connectMachine("mch_online");
    expect(useCloudAccount.getState().error).toMatchObject({
      code: "cloud_relay_unavailable",
    });
  });

  it("loads plans while keeping backups off by default, then toggles locally", async () => {
    const client = new FakeClient();
    configureCloudRuntime({ client, relay: new FakeRelay() });
    await useCloudAccount.getState().loadPlans();
    expect(useCloudAccount.getState().plans?.plans[0]?.name).toBe("Home 8");
    expect(useCloudAccount.getState().backupsEnabled).toBe(false);
    useCloudAccount.getState().setBackupsEnabled(true);
    expect(useCloudAccount.getState().backupsEnabled).toBe(true);
  });

  it("refreshes live provisioning and treats a missing record as normal", async () => {
    const client = new FakeClient();
    configureCloudRuntime({ client, relay: new FakeRelay() });
    await useCloudAccount.getState().startSignIn();
    await useCloudAccount.getState().pollSignIn();
    await useCloudAccount.getState().refreshProvisioning("mch_online");
    expect(useCloudAccount.getState().provisioning).toEqual(PROVISIONING);

    vi.spyOn(client, "getProvisioning").mockRejectedValueOnce({
      code: "cloud_not_found",
      message: "not found",
      hint: null,
      retryable: false,
      status: 404,
    });
    await useCloudAccount.getState().refreshProvisioning("mch_online");
    expect(useCloudAccount.getState().provisioning).toBeNull();
    expect(useCloudAccount.getState().provisioningStatus).toBe("ready");
    expect(useCloudAccount.getState().notice).toMatch(/No provisioning record/);
  });

  it("signs out, disconnects the relay and returns to local mode", async () => {
    const client = new FakeClient();
    const relay = new FakeRelay();
    configureCloudRuntime({ client, relay });
    await useCloudAccount.getState().startSignIn();
    await useCloudAccount.getState().pollSignIn();
    await useCloudAccount.getState().connectMachine("mch_online");

    useCloudAccount.getState().signOut();
    const state = useCloudAccount.getState();
    expect(state.phase).toBe("local");
    expect(state.account).toBeNull();
    expect(state.machines).toEqual([]);
    expect(state.relayStatus?.state).toBe("closed");
    expect(client.clearCalls).toBe(1);
    expect(relay.disconnectCalls).toBe(1);
    expect(state.client).toBe(client);
  });
});
