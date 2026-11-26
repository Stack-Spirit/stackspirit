import { useCloudAccount } from "../cloud/store";
import type { CloudMachine, RelayConnectionStatus } from "../cloud/types";
import { Button } from "../components/Button";
import { machineLabel, presenceLabel, presenceTone } from "./cloudFormat";

function safeSignInHref(device: {
  verification_uri: string;
  verification_uri_complete: string | null;
}): string | null {
  const candidate = device.verification_uri_complete ?? device.verification_uri;
  try {
    const url = new URL(candidate);
    if (url.protocol === "https:") return url.toString();
    if (
      url.protocol === "http:" &&
      (url.hostname === "127.0.0.1" || url.hostname === "localhost" || url.hostname === "[::1]")
    ) {
      return url.toString();
    }
  } catch {
    return null;
  }
  return null;
}

function relayCopy(status: RelayConnectionStatus | null): string | null {
  if (!status) return null;
  switch (status.state) {
    case "connecting":
      return "Opening the StackSpirit relay connection…";
    case "authenticating":
      return "Relay connected; verifying the encrypted channel…";
    case "secure":
      return "Secure relay channel ready. Prompts, jobs and file contents stay encrypted end to end.";
    case "reconnecting":
      return status.next_retry_at
        ? `Relay lost. Reconnecting automatically (attempt ${status.attempts}).`
        : "Relay lost. Reconnecting automatically…";
    case "failed":
      return null;
    case "closed":
      return "Relay connection closed.";
    case "idle":
      return null;
  }
}

/**
 * Optional StackSpirit Account sign-in and Always-On Home selector.
 *
 * This is never a gate: with no runtime configured, or before sign-in, the
 * panel says that local server mode needs no account and leaves the rest of the
 * app untouched. The device-code flow is the only sign-in path; no password,
 * token or SSH key is ever requested by the renderer.
 */
export function CloudAccountPanel() {
  const client = useCloudAccount((state) => state.client);
  const phase = useCloudAccount((state) => state.phase);
  const account = useCloudAccount((state) => state.account);
  const device = useCloudAccount((state) => state.device);
  const deviceBusy = useCloudAccount((state) => state.deviceBusy);
  const machines = useCloudAccount((state) => state.machines);
  const machinesStatus = useCloudAccount((state) => state.machinesStatus);
  const machinesError = useCloudAccount((state) => state.machinesError);
  const selectedMachineId = useCloudAccount((state) => state.selectedMachineId);
  const connectingMachineId = useCloudAccount((state) => state.connectingMachineId);
  const relayStatus = useCloudAccount((state) => state.relayStatus);
  const error = useCloudAccount((state) => state.error);
  const notice = useCloudAccount((state) => state.notice);
  const startSignIn = useCloudAccount((state) => state.startSignIn);
  const pollSignIn = useCloudAccount((state) => state.pollSignIn);
  const cancelSignIn = useCloudAccount((state) => state.cancelSignIn);
  const signOut = useCloudAccount((state) => state.signOut);
  const refreshMachines = useCloudAccount((state) => state.refreshMachines);
  const selectMachine = useCloudAccount((state) => state.selectMachine);
  const connectMachine = useCloudAccount((state) => state.connectMachine);
  const disconnectMachine = useCloudAccount((state) => state.disconnectMachine);
  const clearError = useCloudAccount((state) => state.clearError);

  const signInHref = device ? safeSignInHref(device) : null;
  const secureMachineId =
    relayStatus?.state === "secure" ? relayStatus.machine_id : null;

  return (
    <section className="nx-cloud" data-testid="cloud-account-panel" aria-label="StackSpirit Account">
      {phase !== "signed_in" ? (
        <div className="nx-cloud__local" data-testid="cloud-local-default">
          <div className="nx-cloud__title">
            <div>
              <h3>Local mode · no account</h3>
              <p>
                StackSpirit works fully with a local or SSH server and needs no account.
                Server details and SSH private keys stay on this device.
              </p>
            </div>
            <span className="nx-chip nx-chip--server">Default</span>
          </div>

          {phase === "device_pending" && device ? (
            <div className="nx-cloud__device" data-testid="cloud-device">
              <p>
                Open{" "}
                {signInHref ? (
                  <a
                    className="nx-link"
                    href={signInHref}
                    target="_blank"
                    rel="noreferrer noopener"
                  >
                    the StackSpirit sign-in page
                  </a>
                ) : (
                  "the StackSpirit sign-in page"
                )}{" "}
                and enter this one-time code:
              </p>
              <p className="nx-cloud__code mono" data-testid="cloud-device-code">
                {device.user_code}
              </p>
              <p className="nx-cloud__meta">
                The code expires in about {Math.max(1, Math.round(device.expires_in / 60))} minutes.
                Nothing is sent to your server.
              </p>
              <div className="nx-cloud__actions">
                <Button
                  size="sm"
                  variant="primary"
                  onClick={() => void pollSignIn()}
                  loading={deviceBusy}
                  data-testid="cloud-poll"
                >
                  I approved it — check again
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={cancelSignIn}
                  disabled={deviceBusy}
                  data-testid="cloud-cancel"
                >
                  Cancel
                </Button>
              </div>
            </div>
          ) : (
            <div className="nx-cloud__actions">
              {client ? (
                <Button
                  size="sm"
                  variant="primary"
                  onClick={() => void startSignIn()}
                  loading={deviceBusy}
                  data-testid="cloud-sign-in"
                >
                  Sign in to StackSpirit
                </Button>
              ) : null}
              <span className="nx-cloud__meta">
                {client
                  ? "Signing in is optional. It is needed only to reach an Always-On Home from this browser."
                  : "Account sign-in is available in the hosted StackSpirit Web app; this build keeps local mode as-is."}
              </span>
            </div>
          )}
        </div>
      ) : (
        <div className="nx-cloud__account" data-testid="cloud-account">
          <div className="nx-cloud__title">
            <div>
              <h3>{account?.name ?? account?.email ?? "StackSpirit Account"}</h3>
              <p>
                {account?.email ?? "Signed in"}
                <span className="nx-chip nx-chip--server">Signed in</span>
              </p>
            </div>
            <div className="nx-cloud__actions">
              <Button
                size="sm"
                variant="secondary"
                onClick={() => void refreshMachines()}
                loading={machinesStatus === "loading"}
                data-testid="cloud-refresh-machines"
              >
                Refresh
              </Button>
              <Button size="sm" variant="ghost" onClick={signOut} data-testid="cloud-sign-out">
                Sign out
              </Button>
            </div>
          </div>

          <div className="nx-cloud__machines" data-testid="cloud-machines" aria-label="Always-On Homes">
            {machinesStatus === "loading" && machines.length === 0 ? (
              <p className="nx-cloud__meta" role="status">
                Reading your Always-On Homes…
              </p>
            ) : machines.length === 0 ? (
              <p className="nx-cloud__empty">
                No Always-On Homes are enrolled on this account yet. Local and SSH servers
                remain available without one.
              </p>
            ) : (
              <ul className="nx-cloud__machine-list">
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
          </div>

          {machinesError && machinesError !== error ? (
            <p className="nx-cloud__error" role="alert" data-testid="cloud-machines-error">
              {machinesError.message}
              {machinesError.hint ? <span> {machinesError.hint}</span> : null}
            </p>
          ) : null}

          {relayCopy(relayStatus) ? (
            <p
              className={
                relayStatus?.state === "secure"
                  ? "nx-cloud__relay nx-cloud__relay--ok"
                  : "nx-cloud__relay"
              }
              data-testid="cloud-relay-status"
              role="status"
            >
              {relayCopy(relayStatus)}
            </p>
          ) : null}
          {relayStatus?.state === "failed" && relayStatus.error ? (
            <p className="nx-cloud__error" role="alert" data-testid="cloud-relay-error">
              {relayStatus.error.message}
              {relayStatus.error.hint ? <span> {relayStatus.error.hint}</span> : null}
            </p>
          ) : null}
        </div>
      )}

      {notice ? (
        <p className="nx-cloud__notice" role="status" data-testid="cloud-notice">
          {notice}
        </p>
      ) : null}
      {error ? (
        <p className="nx-cloud__error" role="alert" data-testid="cloud-error">
          {error.message}
          {error.hint ? <span> {error.hint}</span> : null}
          <button type="button" className="nx-link nx-link--muted" onClick={clearError}>
            Dismiss
          </button>
        </p>
      ) : null}
    </section>
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
  const tone = presenceTone(machine.status);
  const chipTone = tone === "ok" ? "server" : tone;
  return (
    <li
      className={`nx-cloud__machine${selected ? " nx-cloud__machine--selected" : ""}`}
      data-testid={`cloud-machine-${machine.id}`}
      data-status={machine.status}
    >
      <button
        type="button"
        className="nx-cloud__machine-main"
        aria-pressed={selected}
        onClick={onSelect}
      >
        <span className={`nx-cloud__presence nx-cloud__presence--${tone}`} aria-hidden="true" />
        <span className="nx-cloud__machine-name">{machineLabel(machine)}</span>
        <span
          className={`nx-chip nx-chip--${chipTone}`}
          data-testid={`cloud-machine-status-${machine.id}`}
        >
          {presenceLabel(machine.status)}
        </span>
      </button>
      <div className="nx-cloud__machine-actions">
        {secure ? (
          <>
            <span className="nx-chip nx-chip--server">Encrypted</span>
            <button
              type="button"
              className="nx-btn nx-btn--quiet nx-btn--sm"
              onClick={onDisconnect}
              data-testid="cloud-disconnect"
            >
              Disconnect
            </button>
          </>
        ) : (
          <button
            type="button"
            className="nx-btn nx-btn--sm"
            disabled={!online || connecting || (relayState !== null && relayState !== "failed" && relayState !== "closed" && relayState !== "idle")}
            onClick={onConnect}
            data-testid={`cloud-connect-${machine.id}`}
            title={online ? undefined : `${machine.name} is ${presenceLabel(machine.status)}`}
          >
            {connecting ? "Connecting…" : online ? "Connect via relay" : "Offline"}
          </button>
        )}
      </div>
    </li>
  );
}
