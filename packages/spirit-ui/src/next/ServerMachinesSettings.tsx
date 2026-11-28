import { useState } from "react";
import { create } from "zustand";

import { useCloudAccount } from "../cloud/store";
import type { CloudMachine, RelayConnectionStatus } from "../cloud/types";
import type { HostProfile } from "../bridge/wire";
import { useConnection, useHosts, useSelectedHostId } from "../state/fleetStore";
import { EngineView } from "../views/engine/EngineView";
import { HostManager } from "../views/hosts/HostManager";
import { SecretStore } from "./SecretStore";
import { ServerSecurityCard } from "./ServerSecurity";

export interface ServerMachinesSettingsProps {
  /** Opens the Account rail item, where sign-in and managed plans live. */
  onOpenAccount: () => void;
}

/**
 * Enrolls a connected SSH machine on the signed-in StackSpirit Account. The host wires the
 * real implementation (VOWP relay enrollment over the verified SSH session); it receives
 * only the saved host id, never a key or relay secret, and resolves when enrollment is done.
 */
export type MachineEnroller = (hostId: string) => Promise<void>;

interface MachineSyncState {
  enroller: MachineEnroller | null;
  /** Host ids enrolled during this session, until the account machine list catches up. */
  enrolledHostIds: string[];
  busyHostId: string | null;
  error: { hostId: string; message: string } | null;
  configure(enroller: MachineEnroller | null): void;
  sync(hostId: string): Promise<void>;
  reset(): void;
}

export const useMachineSync = create<MachineSyncState>((set, get) => ({
  enroller: null,
  enrolledHostIds: [],
  busyHostId: null,
  error: null,
  configure(enroller) {
    set({ enroller });
  },
  async sync(hostId) {
    const { enroller, busyHostId } = get();
    if (!enroller || busyHostId) return;
    set({ busyHostId: hostId, error: null });
    try {
      await enroller(hostId);
      set((state) => ({
        busyHostId: null,
        enrolledHostIds: [...state.enrolledHostIds, hostId],
      }));
      void useCloudAccount.getState().refreshMachines();
    } catch {
      set({
        busyHostId: null,
        error: { hostId, message: "Sync did not finish. Nothing was changed; try again." },
      });
    }
  },
  reset() {
    set({ enroller: null, enrolledHostIds: [], busyHostId: null, error: null });
  },
}));

export type MachineSyncStatus = "synced" | "local";

/** A saved machine is Account-synced only while signed in and enrolled on that Account. */
export function machineSyncStatus(
  host: HostProfile,
  signedIn: boolean,
  machines: CloudMachine[],
  enrolledHostIds: string[],
): MachineSyncStatus {
  if (!signedIn) return "local";
  if (enrolledHostIds.includes(host.id)) return "synced";
  const names = [host.label, host.ssh.host].map((name) => name.trim().toLowerCase());
  return machines.some((machine) => names.includes(machine.name.trim().toLowerCase()))
    ? "synced"
    : "local";
}

export function useMachineSyncStatus(host: HostProfile | null): MachineSyncStatus | null {
  const phase = useCloudAccount((state) => state.phase);
  const machines = useCloudAccount((state) => state.machines);
  const enrolled = useMachineSync((state) => state.enrolledHostIds);
  if (!host) return null;
  return machineSyncStatus(host, phase === "signed_in", machines, enrolled);
}

/** Crisp, subtle badge: Account-synced (encrypted relay) or Local only (this Mac). */
export function MachineSyncBadge({
  status,
  testId,
}: {
  status: MachineSyncStatus;
  testId?: string;
}) {
  const synced = status === "synced";
  return (
    <span
      className={`nx-chip nx-chip--tiny ${synced ? "nx-chip--server" : "nx-chip--muted"}`}
      title={
        synced
          ? "Enrolled on your StackSpirit Account; reachable from Web, phone and remote MCP over the encrypted relay."
          : "Saved on this Mac only."
      }
      data-testid={testId}
      data-sync-status={status}
    >
      {synced ? "Account-synced" : "Local only"}
    </span>
  );
}

/** 1-click sync for a Local-only machine, or a plain reason it cannot be synced yet. */
export function SyncToAccountAction({
  host,
  connected,
}: {
  host: HostProfile;
  connected: boolean;
}) {
  const status = useMachineSyncStatus(host);
  const enroller = useMachineSync((state) => state.enroller);
  const busy = useMachineSync((state) => state.busyHostId === host.id);
  const anyBusy = useMachineSync((state) => state.busyHostId !== null);
  const error = useMachineSync((state) =>
    state.error?.hostId === host.id ? state.error.message : null,
  );
  const sync = useMachineSync((state) => state.sync);
  const signedIn = useCloudAccount((state) => state.phase === "signed_in");
  if (!signedIn || status !== "local") return null;
  const reason = !enroller
    ? "Account sync is not available in this build yet."
    : !connected
      ? "Connect this machine over SSH first. It can't be synced while offline or disconnected."
      : null;
  return (
    <div className="nx-settings__sync" data-testid={`sync-action-${host.id}`}>
      <button
        type="button"
        className="nx-btn nx-btn--sm"
        disabled={reason !== null || anyBusy}
        onClick={() => void sync(host.id)}
        data-testid={`sync-to-account-${host.id}`}
      >
        {busy ? "Syncing…" : "Sync to Account"}
      </button>
      {reason ? (
        <small className="nx-form__hint" data-testid={`sync-reason-${host.id}`}>
          {reason}
        </small>
      ) : null}
      {error ? (
        <small className="nx-form__hint" role="alert" data-testid={`sync-error-${host.id}`}>
          {error}
        </small>
      ) : null}
    </div>
  );
}

function connectionLabel(state: string): string {
  switch (state) {
    case "connected":
      return "Connected";
    case "stale":
      return "Stale";
    case "connecting":
      return "Connecting…";
    case "disconnecting":
      return "Disconnecting…";
    case "failed":
      return "Failed";
    default:
      return "Not connected";
  }
}

function stateTone(state: string): string {
  if (state === "connected") return "nx-chip--server";
  if (state === "stale" || state === "connecting" || state === "disconnecting") {
    return "nx-chip--warn";
  }
  if (state === "failed") return "nx-chip--danger";
  return "nx-chip--muted";
}

function relayTone(status: RelayConnectionStatus | null): string {
  if (status?.state === "secure") return "nx-chip--server";
  if (
    status?.state === "connecting" ||
    status?.state === "authenticating" ||
    status?.state === "reconnecting"
  ) {
    return "nx-chip--warn";
  }
  if (status?.state === "failed") return "nx-chip--danger";
  return "nx-chip--muted";
}

function relayLabel(status: RelayConnectionStatus | null): string {
  if (!status) return "Not configured";
  switch (status.state) {
    case "connecting":
      return "Connecting…";
    case "authenticating":
      return "Verifying channel…";
    case "secure":
      return "Encrypted";
    case "reconnecting":
      return "Reconnecting…";
    case "failed":
      return "Failed";
    case "closed":
      return "Closed";
    case "idle":
      return "Idle";
  }
}

/**
 * Settings → Server & Machines.
 *
 * This is the operational rail item: the saved Home server and its SSH/relay state, the
 * optional Always-On Machine relay setup, the off-site backup status, and the existing
 * server-secrets and password-login controls. Engine/gate diagnostics stay folded away
 * so the everyday controls are not buried under a wall of cards.
 */
export function ServerMachinesSettings({ onOpenAccount }: ServerMachinesSettingsProps) {
  const hosts = useHosts();
  const connection = useConnection();
  const selectedHostId = useSelectedHostId();
  const phase = useCloudAccount((state) => state.phase);
  const machines = useCloudAccount((state) => state.machines);
  const selectedMachineId = useCloudAccount((state) => state.selectedMachineId);
  const connectingMachineId = useCloudAccount((state) => state.connectingMachineId);
  const relayStatus = useCloudAccount((state) => state.relayStatus);
  const backupsEnabled = useCloudAccount((state) => state.backupsEnabled);
  const provisioning = useCloudAccount((state) => state.provisioning);
  const selectMachine = useCloudAccount((state) => state.selectMachine);
  const connectMachine = useCloudAccount((state) => state.connectMachine);
  const disconnectMachine = useCloudAccount((state) => state.disconnectMachine);
  const [engineOpen, setEngineOpen] = useState(false);

  const activeHost =
    hosts.find((host) => host.id === connection.host_id) ??
    hosts.find((host) => host.id === selectedHostId) ??
    hosts[0] ??
    null;
  const activeState = connection.host_id === activeHost?.id ? connection.state : "disconnected";
  const signedIn = phase === "signed_in";
  const activeSync = useMachineSyncStatus(activeHost);
  const selectedMachine = machines.find((machine) => machine.id === selectedMachineId) ?? null;
  const secureMachineId =
    relayStatus?.state === "secure" ? relayStatus.machine_id : null;

  return (
    <div className="nx-settings-section" data-testid="settings-server">
      <header className="nx-settings-section__head">
        <h3>Server &amp; Machines</h3>
        <p>
          Home over SSH, the optional encrypted relay, and the machines that stay on for
          you. Nothing here opens a manual tunnel.
        </p>
      </header>

      <section className="nx-settings__card" aria-labelledby="settings-home-heading">
        <div className="nx-block__head">
          <h4 id="settings-home-heading">Home server</h4>
          <span className={`nx-chip ${stateTone(activeState)}`}>{connectionLabel(activeState)}</span>
        </div>
        {activeHost ? (
          <>
            <div className="nx-settings__row">
              <span>
                <strong>{activeHost.label}</strong>
                <small>
                  SSH {activeHost.ssh.user}@{activeHost.ssh.host}
                  {activeHost.ssh.port !== 22 ? `:${activeHost.ssh.port}` : ""}
                </small>
              </span>
              <span className="nx-settings__actions">
                <MachineSyncBadge
                  status={activeSync ?? "local"}
                  testId="settings-home-sync-badge"
                />
                <span className={`nx-chip ${connection.host_id === activeHost.id ? stateTone(activeState) : "nx-chip--muted"}`}>
                  {connection.host_id === activeHost.id ? connectionLabel(activeState) : "Saved"}
                </span>
              </span>
            </div>
            <SyncToAccountAction
              host={activeHost}
              connected={activeState === "connected" || activeState === "stale"}
            />
            <div className="nx-settings__row">
              <span>
                Relay
                <small>
                  {selectedMachine
                    ? `Encrypted channel to ${selectedMachine.name}`
                    : "Sign in to StackSpirit to use the encrypted relay."}
                </small>
              </span>
              <span className={`nx-chip ${relayTone(relayStatus)}`}>{relayLabel(relayStatus)}</span>
            </div>
            <div className="nx-settings__row">
              <span>
                Off-site encrypted backups
                <small>
                  Optional R2 backups for a Managed Home; off by default. Managed from
                  the Account rail item.
                </small>
              </span>
              <span className={`nx-chip ${backupsEnabled ? "nx-chip--server" : "nx-chip--muted"}`}>
                {backupsEnabled ? "Enabled" : "Off · default"}
              </span>
            </div>
          </>
        ) : (
          <p className="nx-form__hint">
            No Home server is saved yet. Add one below; local chat still works without it.
          </p>
        )}
      </section>

      <section className="nx-settings__card" aria-labelledby="settings-machines-heading">
        <div className="nx-block__head">
          <div>
            <h4 id="settings-machines-heading">Always-On Machines</h4>
            <p className="nx-form__hint">
              Connect a synced machine through StackSpirit's encrypted relay. The desktop
              host keeps the channel open; there is no ssh -L command to paste.
            </p>
          </div>
          <button
            type="button"
            className="nx-btn nx-btn--quiet nx-btn--sm"
            onClick={onOpenAccount}
            data-testid="settings-open-account"
          >
            {signedIn ? "Manage account" : "Sign in"}
          </button>
        </div>
        {!signedIn ? (
          <p className="nx-form__hint" data-testid="settings-machines-signed-out">
            Sign in from Account to load synced Always-On Machines. Local and SSH servers
            need no account.
          </p>
        ) : machines.length === 0 ? (
          <p className="nx-form__hint" data-testid="settings-machines-empty">
            No Always-On Machines are enrolled on this account yet.
          </p>
        ) : (
          <ul className="nx-settings__machines" aria-label="Always-On Machines">
            {machines.map((machine) => (
              <MachineRow
                key={machine.id}
                machine={machine}
                selected={selectedMachineId === machine.id}
                connecting={connectingMachineId === machine.id}
                secure={secureMachineId === machine.id}
                relayState={relayStatus?.state ?? null}
                onSelect={() => selectMachine(machine.id)}
                onConnect={() => void connectMachine(machine.id)}
                onDisconnect={disconnectMachine}
              />
            ))}
          </ul>
        )}
        {provisioning ? (
          <p className="nx-form__hint" role="status" data-testid="settings-provisioning">
            Provisioning {selectedMachine?.name ?? "the selected Home"}:{" "}
            {provisioning.step ?? provisioning.phase}
            {provisioning.progress_pct !== null ? ` · ${Math.round(provisioning.progress_pct)}%` : ""}
          </p>
        ) : null}
      </section>

      <section className="nx-settings__card" aria-labelledby="settings-ssh-heading">
        <div className="nx-block__head">
          <div>
            <h4 id="settings-ssh-heading">Saved servers &amp; SSH</h4>
            <p className="nx-form__hint">
              Add, edit, re-pin or remove a Home server. StackSpirit keeps its own private
              known-hosts entry and never shows a key value here.
            </p>
          </div>
        </div>
        <HostManager />
        <ServerSecurityCard />
      </section>

      <section className="nx-settings__card" aria-label="Server secrets">
        <SecretStore />
      </section>

      <section className="nx-settings__card nx-settings-disclosure" data-testid="settings-engine-advanced">
        <button
          type="button"
          className="nx-settings-disclosure__toggle"
          aria-expanded={engineOpen}
          onClick={() => setEngineOpen((open) => !open)}
        >
          Engine &amp; capability checks
        </button>
        {engineOpen ? (
          <>
            <p className="nx-form__hint">
              The bundled engine, external harness scan, D6 local-orchestrator evidence and
              the ASIF capability gates. These stay folded because they are diagnostics, not
              everyday settings.
            </p>
            <EngineView />
          </>
        ) : (
          <p className="nx-form__hint">
            Diagnostics for the engine, external harness scan and capability gates. Open only
            when you need to check a gate.
          </p>
        )}
      </section>
    </div>
  );
}

function MachineRow({
  machine,
  selected,
  connecting,
  secure,
  relayState,
  onSelect,
  onConnect,
  onDisconnect,
}: {
  machine: CloudMachine;
  selected: boolean;
  connecting: boolean;
  secure: boolean;
  relayState: RelayConnectionStatus["state"] | null;
  onSelect: () => void;
  onConnect: () => void;
  onDisconnect: () => void;
}) {
  const online = machine.status === "online";
  return (
    <li
      className={`nx-settings__machine${selected ? " is-selected" : ""}`}
      data-testid={`settings-machine-${machine.id}`}
    >
      <button
        type="button"
        className="nx-settings__machine-main"
        aria-pressed={selected}
        onClick={onSelect}
      >
        <span className={`nx-dot ${online ? "nx-dot--ok" : "nx-dot--warn"}`} aria-hidden="true" />
        <span>{machine.name}</span>
        <span className={`nx-chip ${online ? "nx-chip--server" : "nx-chip--muted"}`}>
          {online ? "Online" : machine.status}
        </span>
      </button>
      <div className="nx-settings__actions">
        {secure ? (
          <>
            <span className="nx-chip nx-chip--server">Encrypted</span>
            <button
              type="button"
              className="nx-btn nx-btn--quiet nx-btn--sm"
              onClick={onDisconnect}
              data-testid={`settings-machine-disconnect-${machine.id}`}
            >
              Disconnect
            </button>
          </>
        ) : (
          <button
            type="button"
            className="nx-btn nx-btn--sm"
            disabled={
              !online ||
              connecting ||
              (relayState !== null &&
                relayState !== "failed" &&
                relayState !== "closed" &&
                relayState !== "idle")
            }
            onClick={onConnect}
            data-testid={`settings-machine-connect-${machine.id}`}
          >
            {connecting ? "Connecting…" : online ? "Connect via relay" : "Offline"}
          </button>
        )}
      </div>
    </li>
  );
}
