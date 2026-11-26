import type {
  CloudAccount,
  CloudAccountApi,
  CloudMachine,
  CloudRelayConnector,
  DeviceAuthorization,
  ManagedPlanCatalog,
  ProvisioningStatus,
  RelayConnectionStatus,
  RelayHttpResponse,
  RelayScope,
  RelayTicket,
  RelayTicketRequest,
} from "../../cloud/types";

export const TEST_ACCOUNT: CloudAccount = {
  id: "acct_1",
  email: "owner@example.com",
  name: "Owner",
};

export const TEST_MACHINES: CloudMachine[] = [
  {
    id: "mch_online",
    name: "contabo-1",
    status: "online",
    project: "vowp",
    last_seen_at: "2026-11-26T10:00:00.000Z",
    provisioning: null,
  },
  {
    id: "mch_offline",
    name: "home-mini",
    status: "offline",
    project: null,
    last_seen_at: null,
    provisioning: null,
  },
];

export const TEST_PLANS: ManagedPlanCatalog = {
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
    {
      id: "home-12",
      name: "Home 12",
      vcpu: 6,
      ram_gb: 12,
      disk_gb: 200,
      disk_kind: "SSD",
      monthly_eur: 9,
      setup_fee_eur: 0,
      currency: "EUR",
      provider_spec: "Cloud VPS 6",
    },
    {
      id: "home-24",
      name: "Home 24",
      vcpu: 8,
      ram_gb: 24,
      disk_gb: 300,
      disk_kind: "SSD",
      monthly_eur: 17,
      setup_fee_eur: 0,
      currency: "EUR",
      provider_spec: "Cloud VPS 8",
    },
    {
      id: "home-48",
      name: "Home 48",
      vcpu: 12,
      ram_gb: 48,
      disk_gb: 400,
      disk_kind: "SSD",
      monthly_eur: 30,
      setup_fee_eur: 0,
      currency: "EUR",
      provider_spec: "Cloud VPS 12",
    },
    {
      id: "home-64",
      name: "Home 64",
      vcpu: 16,
      ram_gb: 64,
      disk_gb: 500,
      disk_kind: "SSD",
      monthly_eur: 44,
      setup_fee_eur: 0,
      currency: "EUR",
      provider_spec: "Cloud VPS 16",
    },
  ],
  backups: { monthly_eur: 3.5, recommended: true, provider: "cloudflare-r2" },
};

export const TEST_PROVISIONING: ProvisioningStatus = {
  machine_id: "mch_online",
  phase: "provisioning",
  step: "cloud-init",
  progress_pct: 42,
  detail: "Installing StackSpirit",
  updated_at: "2026-11-26T10:01:00.000Z",
  home_url: "https://app.stackspirit.io/machines/mch_online",
};

export interface TestCloudOptions {
  poll?: "authorized" | "pending";
  machines?: CloudMachine[];
  plans?: ManagedPlanCatalog;
  provisioning?: ProvisioningStatus;
  startError?: { code: string; message: string; hint: string | null; retryable: boolean };
}

export class TestCloudClient implements CloudAccountApi {
  hasSession = false;
  startCalls = 0;
  pollCalls = 0;
  machineCalls = 0;
  planCalls = 0;
  provisioningCalls = 0;
  clearCalls = 0;
  ticketCalls: Array<{ machineId: string; request: RelayTicketRequest }> = [];

  constructor(private readonly options: TestCloudOptions = {}) {}

  async startDeviceSignIn(): Promise<DeviceAuthorization> {
    this.startCalls += 1;
    if (this.options.startError) throw this.options.startError;
    return {
      device_code: "device-1",
      user_code: "ABCD-EFGH",
      verification_uri: "https://stackspirit.io/device",
      verification_uri_complete: "https://stackspirit.io/device?code=ABCD-EFGH",
      expires_in: 600,
      interval: 5,
      expires_at: "2026-11-26T10:10:00.000Z",
    };
  }

  async pollDeviceSignIn(): Promise<"authorized" | "pending" | "slow_down"> {
    this.pollCalls += 1;
    if (this.options.poll === "pending") return "pending";
    this.hasSession = true;
    return "authorized";
  }

  async account(): Promise<CloudAccount> {
    return TEST_ACCOUNT;
  }

  async listMachines(): Promise<CloudMachine[]> {
    this.machineCalls += 1;
    return this.options.machines ?? TEST_MACHINES;
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
        key_id: "ed25519:test-machine-key",
        public_key: "test-machine-key",
        project: "vowp",
      },
    };
  }

  async getPlans(): Promise<ManagedPlanCatalog> {
    this.planCalls += 1;
    return this.options.plans ?? TEST_PLANS;
  }

  async getProvisioning(): Promise<ProvisioningStatus> {
    this.provisioningCalls += 1;
    return this.options.provisioning ?? TEST_PROVISIONING;
  }

  clearSession(): void {
    this.clearCalls += 1;
    this.hasSession = false;
  }
}

export class TestRelay implements CloudRelayConnector {
  connectCalls: Array<{ machineId: string; scopes?: RelayScope[] }> = [];
  disconnectCalls = 0;
  requests: Array<{ path: string }> = [];
  current: RelayConnectionStatus = {
    state: "idle",
    machine_id: null,
    channel_id: null,
    attempts: 0,
    next_retry_at: null,
    error: null,
  };
  private readonly listeners = new Set<(status: RelayConnectionStatus) => void>();

  async connect(machineId: string, options?: { scopes?: RelayScope[] }): Promise<void> {
    this.connectCalls.push({ machineId, scopes: options?.scopes });
    this.publish({
      state: "secure",
      machine_id: machineId,
      channel_id: "01JQ8Z6K4W5N7R9T2V3X4Y5Z6A",
      attempts: 1,
      next_retry_at: null,
      error: null,
    });
  }

  disconnect(): void {
    this.disconnectCalls += 1;
    this.publish({ ...this.current, state: "closed", channel_id: null });
  }

  status(): RelayConnectionStatus {
    return this.current;
  }

  subscribe(listener: (status: RelayConnectionStatus) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  async request(path: string): Promise<RelayHttpResponse> {
    this.requests.push({ path });
    return { status: 200, headers: {}, body: "{}" };
  }

  publish(status: RelayConnectionStatus): void {
    this.current = status;
    for (const listener of this.listeners) listener(status);
  }
}
