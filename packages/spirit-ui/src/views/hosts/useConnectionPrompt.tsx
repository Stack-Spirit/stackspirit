import { useState, type ReactElement } from "react";

import { Button, Modal } from "../../components";
import type { HostProfile } from "../../bridge/wire";
import { useConnection, useFleetStore, useHosts } from "../../state/fleetStore";

/**
 * Only one host may be connected at a time. Selecting another always asks before
 * disconnecting the current one.
 */
export function useConnectionPrompt(): {
  requestConnect: (host: HostProfile) => void;
  connectionPrompt: ReactElement;
} {
  const connection = useConnection();
  const hosts = useHosts();
  const connect = useFleetStore((state) => state.connect);
  const disconnect = useFleetStore((state) => state.disconnect);
  const [pending, setPending] = useState<HostProfile | null>(null);
  const [busy, setBusy] = useState(false);

  const activeHost = hosts.find((host) => host.id === connection.host_id) ?? null;

  function requestConnect(host: HostProfile) {
    const transition =
      connection.state === "connected" ||
      connection.state === "connecting" ||
      connection.state === "stale";
    if (transition && activeHost && activeHost.id !== host.id) {
      setPending(host);
      return;
    }
    void connect(host.id);
  }

  async function confirmSwitch() {
    if (!pending) return;
    setBusy(true);
    try {
      if (activeHost && activeHost.id !== pending.id) {
        await disconnect(activeHost.id);
      }
      await connect(pending.id);
    } finally {
      setBusy(false);
      setPending(null);
    }
  }

  return {
    requestConnect,
    connectionPrompt: (
      <Modal
        open={pending !== null}
        title="Disconnect and switch host?"
        onClose={() => setPending(null)}
        footer={
          <>
            <Button variant="secondary" onClick={() => setPending(null)}>
              Keep current host
            </Button>
            <Button variant="primary" loading={busy} onClick={() => void confirmSwitch()}>
              Disconnect and connect
            </Button>
          </>
        }
      >
        <p>
          Disconnect from <strong>{activeHost?.label ?? "the current host"}</strong> and connect
          to <strong>{pending?.label ?? "the selected host"}</strong>?
        </p>
        <p className="muted">
          Polling stops immediately. The last fleet view stays visible but dimmed.
        </p>
      </Modal>
    ),
  };
}
