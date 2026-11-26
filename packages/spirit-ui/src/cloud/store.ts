/**
 * Account/relay state for the optional StackSpirit cloud surface.
 *
 * The default is deliberately boring: `phase: "local"` with no client request,
 * no timer and no login prompt. A host wires the optional runtime with
 * `configureCloudRuntime`; nothing connects until the owner explicitly signs in
 * and chooses a machine. Account-free local/SSH mode therefore stays the
 * zero-friction default required by `docs/STRATEGY.md` §4.
 *
 * The store never receives or holds an access token. The client keeps its bearer
 * session in memory; this store only sees the account profile, machine presence,
 * plans and provisioning metadata.
 */

import { create } from "zustand";

import {
  asCloudError,
  cloudError,
  type CloudAccount,
  type CloudAccountApi,
  type CloudError,
  type CloudMachine,
  type CloudRelayConnector,
  type DeviceAuthorization,
  type ManagedPlanCatalog,
  type ProvisioningStatus,
  type RelayConnectionStatus,
  type RelayScope,
} from "./types";

export type CloudPhase = "local" | "signed_out" | "device_pending" | "signed_in";
export type LoadStatus = "idle" | "loading" | "ready" | "error";

const DEFAULT_SCOPES: RelayScope[] = ["http.read"];

export interface CloudAccountState {
  client: CloudAccountApi | null;
  relay: CloudRelayConnector | null;
  relayUnsubscribe: (() => void) | null;
  phase: CloudPhase;
  account: CloudAccount | null;
  device: DeviceAuthorization | null;
  deviceBusy: boolean;
  machines: CloudMachine[];
  machinesStatus: LoadStatus;
  machinesError: CloudError | null;
  selectedMachineId: string | null;
  connectingMachineId: string | null;
  relayStatus: RelayConnectionStatus | null;
  plans: ManagedPlanCatalog | null;
  plansStatus: LoadStatus;
  plansError: CloudError | null;
  /** Off by default, independent of any server-supplied default flag. */
  backupsEnabled: boolean;
  provisioning: ProvisioningStatus | null;
  provisioningStatus: LoadStatus;
  provisioningError: CloudError | null;
  error: CloudError | null;
  notice: string | null;

  /** Runtime injection; called once by the host, never inside a render. */
  configure(runtime: { client: CloudAccountApi; relay: CloudRelayConnector | null }): void;
  startSignIn(): Promise<void>;
  pollSignIn(): Promise<void>;
  cancelSignIn(): void;
  signOut(): void;
  refreshMachines(): Promise<void>;
  selectMachine(machineId: string): void;
  connectMachine(machineId: string): Promise<void>;
  disconnectMachine(): void;
  loadPlans(): Promise<void>;
  setBackupsEnabled(enabled: boolean): void;
  refreshProvisioning(machineId: string): Promise<void>;
  clearError(): void;
}

function initialFields() {
  return {
    client: null as CloudAccountApi | null,
    relay: null as CloudRelayConnector | null,
    relayUnsubscribe: null as (() => void) | null,
    phase: "local" as CloudPhase,
    account: null as CloudAccount | null,
    device: null as DeviceAuthorization | null,
    deviceBusy: false,
    machines: [] as CloudMachine[],
    machinesStatus: "idle" as LoadStatus,
    machinesError: null as CloudError | null,
    selectedMachineId: null as string | null,
    connectingMachineId: null as string | null,
    relayStatus: null as RelayConnectionStatus | null,
    plans: null as ManagedPlanCatalog | null,
    plansStatus: "idle" as LoadStatus,
    plansError: null as CloudError | null,
    backupsEnabled: false,
    provisioning: null as ProvisioningStatus | null,
    provisioningStatus: "idle" as LoadStatus,
    provisioningError: null as CloudError | null,
    error: null as CloudError | null,
    notice: null as string | null,
  };
}

/** Drops every read model and unsubscribes a configured relay. Used by tests. */
export function resetCloudAccount(): void {
  const current = useCloudAccount.getState();
  current.relayUnsubscribe?.();
  useCloudAccount.setState(initialFields());
}

export function createCloudAccountStore() {
  return create<CloudAccountState>((set, get) => ({
    ...initialFields(),

    configure(runtime) {
      get().relayUnsubscribe?.();
      const unsubscribe = runtime.relay
        ? runtime.relay.subscribe((status) => {
            useCloudAccount.setState({ relayStatus: status });
          })
        : null;
      set({
        client: runtime.client,
        relay: runtime.relay,
        relayUnsubscribe: unsubscribe,
        relayStatus: runtime.relay ? runtime.relay.status() : null,
      });
    },

    async startSignIn() {
      const client = get().client;
      if (!client) {
        set({
          error: cloudError(
            "cloud_unavailable",
            "StackSpirit Account is not available in this build.",
            "Local server mode needs no account.",
          ),
        });
        return;
      }
      set({ deviceBusy: true, error: null, notice: null });
      try {
        const device = await client.startDeviceSignIn();
        set({
          device,
          phase: "device_pending",
          deviceBusy: false,
          notice: null,
        });
      } catch (error) {
        set({ deviceBusy: false, error: asCloudError(error) });
      }
    },

    async pollSignIn() {
      const client = get().client;
      const device = get().device;
      if (!client || !device) return;
      set({ deviceBusy: true, error: null });
      try {
        const state = await client.pollDeviceSignIn(device.device_code);
        if (state !== "authorized") {
          set({
            deviceBusy: false,
            notice:
              state === "slow_down"
                ? "StackSpirit asked to slow down. Wait a moment, then check again."
                : "Waiting for you to approve the sign-in.",
          });
          return;
        }
        set({ device: null, deviceBusy: false, phase: "signed_in" });
        let account: CloudAccount | null = null;
        let machines: CloudMachine[] = [];
        let failures: CloudError | null = null;
        try {
          account = await client.account();
        } catch (error) {
          failures = asCloudError(error);
        }
        try {
          machines = await client.listMachines();
        } catch (error) {
          failures = failures ?? asCloudError(error);
        }
        const selected = selectFirstMachine(get().selectedMachineId, machines);
        set({
          account,
          machines,
          machinesStatus: failures ? "error" : "ready",
          machinesError: failures,
          selectedMachineId: selected,
          provisioning: machineProvisioning(selected, machines),
          provisioningStatus: "idle",
          error: failures,
          notice: failures ? null : "Signed in. Local mode still works without an account.",
        });
      } catch (error) {
        set({ deviceBusy: false, error: asCloudError(error) });
      }
    },

    cancelSignIn() {
      set({ device: null, deviceBusy: false, phase: get().account ? "signed_in" : "local", error: null, notice: null });
    },

    signOut() {
      const { relay, client } = get();
      relay?.disconnect();
      client?.clearSession();
      set({
        ...initialFields(),
        client,
        relay,
        relayUnsubscribe: get().relayUnsubscribe,
        relayStatus: relay ? relay.status() : null,
        plans: get().plans,
        plansStatus: get().plansStatus,
        plansError: get().plansError,
        notice: "Signed out. Local mode is unchanged.",
      });
    },

    async refreshMachines() {
      const { client, phase } = get();
      if (!client || phase !== "signed_in") return;
      set({ machinesStatus: "loading", machinesError: null });
      try {
        const machines = await client.listMachines();
        const selected = selectFirstMachine(get().selectedMachineId, machines);
        const inline = machineProvisioning(selected, machines);
        const existing = get().provisioning;
        set({
          machines,
          machinesStatus: "ready",
          machinesError: null,
          selectedMachineId: selected,
          provisioning:
            inline ?? (existing && existing.machine_id === selected ? existing : null),
        });
      } catch (error) {
        const mapped = asCloudError(error);
        set({
          machinesStatus: "error",
          machinesError: mapped,
          error: mapped.code === "cloud_auth" ? mapped : get().error,
        });
        if (mapped.code === "cloud_auth") {
          set({ phase: "signed_out", account: null, selectedMachineId: null });
        }
      }
    },

    selectMachine(machineId) {
      const machine = get().machines.find((candidate) => candidate.id === machineId) ?? null;
      set({
        selectedMachineId: machine ? machine.id : null,
        provisioning: machine?.provisioning ?? null,
        provisioningStatus: "idle",
        provisioningError: null,
      });
    },

    async connectMachine(machineId) {
      const { relay, machines, relayStatus } = get();
      if (!relay) {
        set({
          error: cloudError(
            "cloud_relay_unavailable",
            "The Cloud Relay transport is not available in this build.",
            "Use the StackSpirit Web app or a local server connection.",
          ),
        });
        return;
      }
      const machine = machines.find((candidate) => candidate.id === machineId);
      if (!machine) {
        set({ error: cloudError("cloud_not_found", "That Always-On Home is not in your account.") });
        return;
      }
      if (machine.status !== "online") {
        set({
          error: cloudError(
            "cloud_machine_offline",
            `${machine.name} is ${machine.status}.`,
            "Start the Home or wait for relay.stackspirit.io to see it online.",
            true,
          ),
        });
        return;
      }
      if (relayStatus?.state === "secure" && relayStatus.machine_id === machine.id) return;
      set({ connectingMachineId: machine.id, error: null, notice: null });
      try {
        await relay.connect(machine.id, { scopes: DEFAULT_SCOPES });
        set({
          connectingMachineId: null,
          relayStatus: relay.status(),
          notice: `Connected to ${machine.name} through the encrypted relay.`,
        });
      } catch (error) {
        set({
          connectingMachineId: null,
          relayStatus: relay.status(),
          error: asCloudError(error),
        });
      }
    },

    disconnectMachine() {
      const relay = get().relay;
      relay?.disconnect();
      set({
        connectingMachineId: null,
        relayStatus: relay ? relay.status() : null,
        notice: "Relay connection closed.",
      });
    },

    async loadPlans() {
      const client = get().client;
      if (!client) {
        set({
          plansStatus: "error",
          plansError: cloudError(
            "cloud_unavailable",
            "Managed Home plans are not available in this build.",
            "Local server mode needs no account or plan.",
          ),
        });
        return;
      }
      set({ plansStatus: "loading", plansError: null });
      try {
        const plans = await client.getPlans();
        set({ plans, plansStatus: "ready", plansError: null });
      } catch (error) {
        set({ plansStatus: "error", plansError: asCloudError(error) });
      }
    },

    setBackupsEnabled(enabled) {
      // The toggle is local selection state only; it never enables a server-side
      // backup service by itself and it starts off regardless of the API default.
      set({ backupsEnabled: enabled });
    },

    async refreshProvisioning(machineId) {
      const { client, phase } = get();
      if (!client || phase !== "signed_in") return;
      set({ provisioningStatus: "loading", provisioningError: null });
      try {
        const provisioning = await client.getProvisioning(machineId);
        set({ provisioning, provisioningStatus: "ready", provisioningError: null });
      } catch (error) {
        const mapped = asCloudError(error);
        if (mapped.code === "cloud_not_found") {
          set({
            provisioning: null,
            provisioningStatus: "ready",
            provisioningError: null,
            notice: "No provisioning record for this Always-On Home yet.",
          });
          return;
        }
        set({ provisioningStatus: "error", provisioningError: mapped });
      }
    },

    clearError() {
      set({ error: null, machinesError: null, plansError: null, provisioningError: null });
    },
  }));
}

function selectFirstMachine(
  current: string | null,
  machines: CloudMachine[],
): string | null {
  if (current && machines.some((machine) => machine.id === current)) return current;
  return machines.find((machine) => machine.status === "online")?.id ?? machines[0]?.id ?? null;
}

function machineProvisioning(
  machineId: string | null,
  machines: CloudMachine[],
): ProvisioningStatus | null {
  if (!machineId) return null;
  return machines.find((machine) => machine.id === machineId)?.provisioning ?? null;
}

export const useCloudAccount = createCloudAccountStore();

/** Injects the host's cloud implementations. Makes no network request itself. */
export function configureCloudRuntime(runtime: {
  client: CloudAccountApi;
  relay: CloudRelayConnector | null;
}): void {
  useCloudAccount.getState().configure(runtime);
}
