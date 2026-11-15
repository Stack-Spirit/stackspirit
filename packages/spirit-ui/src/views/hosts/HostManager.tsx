import { useEffect, useState } from "react";

import {
  parseHostKeyMismatch,
  toSpiritError,
} from "../../bridge/errors";
import type { HostProfile } from "../../bridge/wire";
import {
  Badge,
  Button,
  EmptyState,
  ErrorState,
  Modal,
  Toast,
  ToastViewport,
} from "../../components";
import {
  useConnection,
  useFleet,
  useFleetStore,
  useHosts,
  useLastError,
  useSelectedHostId,
} from "../../state/fleetStore";
import { AddHostWizard } from "./AddHostWizard";
import { EditHostDialog } from "./EditHostDialog";
import { HostCard } from "./HostCard";
import { RePinDialog } from "./RePinDialog";
import { useConnectionPrompt } from "./useConnectionPrompt";

type Overlay = "add" | "edit" | "repin" | null;

export function HostManager() {
  const hosts = useHosts();
  const hostsLoaded = useFleetStore((state) => state.hostsLoaded);
  const connection = useConnection();
  const fleet = useFleet();
  const selectedHostId = useSelectedHostId();
  const latencyHistory = useFleetStore((state) => state.latencyHistory);
  const lastError = useLastError();
  const loadHosts = useFleetStore((state) => state.loadHosts);
  const selectHost = useFleetStore((state) => state.selectHost);
  const connect = useFleetStore((state) => state.connect);
  const disconnect = useFleetStore((state) => state.disconnect);
  const removeHost = useFleetStore((state) => state.removeHost);
  const clearError = useFleetStore((state) => state.clearError);
  const { requestConnect, connectionPrompt } = useConnectionPrompt();

  const [overlay, setOverlay] = useState<Overlay>(null);
  const [editTarget, setEditTarget] = useState<HostProfile | null>(null);
  const [repinTarget, setRepinTarget] = useState<HostProfile | null>(null);
  const [removeTarget, setRemoveTarget] = useState<HostProfile | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [removing, setRemoving] = useState(false);
  const [mismatchDismissed, setMismatchDismissed] = useState(false);

  useEffect(() => {
    if (!hostsLoaded) void loadHosts();
  }, [hostsLoaded, loadHosts]);

  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(null), 6000);
    return () => window.clearTimeout(timer);
  }, [notice]);

  const activeHost = hosts.find((host) => host.id === connection.host_id) ?? null;
  const selectedHost = hosts.find((host) => host.id === selectedHostId) ?? activeHost;
  const mismatch = lastError ? parseHostKeyMismatch(lastError) : null;
  const showMismatch = Boolean(mismatch) && !mismatchDismissed;

  function edit(host: HostProfile) {
    setEditTarget(host);
    setOverlay("edit");
  }
  function repin(host: HostProfile) {
    setRepinTarget(host);
    setOverlay("repin");
  }
  function saved(message: string) {
    setNotice(message);
  }

  async function confirmRemove() {
    if (!removeTarget) return;
    setRemoving(true);
    try {
      if (connection.host_id === removeTarget.id) {
        await disconnect(removeTarget.id);
      }
      await removeHost(removeTarget.id);
      setNotice(`Removed ${removeTarget.label}. The VPS was not changed.`);
      setRemoveTarget(null);
    } catch (error) {
      setNotice(toSpiritError(error).message);
    } finally {
      setRemoving(false);
    }
  }

  return (
    <div className="host-manager" data-testid="host-manager">
      <div className="host-manager__header">
        <div>
          <h2 className="section-title">Manage hosts</h2>
          <p className="page-subtitle">Add a VPS once, verify it, then connect without a terminal.</p>
        </div>
        <Button variant="primary" onClick={() => setOverlay("add")}>
          Add your VPS
        </Button>
      </div>

      {lastError && !mismatch ? (
        <ErrorState
          error={lastError}
          onRetry={
            lastError.retryable && selectedHost
              ? () => {
                  clearError();
                  void connect(selectedHost.id);
                }
              : undefined
          }
        />
      ) : null}

      {hosts.length === 0 ? (
        <div className="host-empty">
          <EmptyState
            icon="⌁"
            title="Add your VPS"
            body="StackSpirit connects to an existing VOWP home over SSH. You need SSH key access, VOWP installed, and nothing else."
            actionLabel="Add your VPS"
            onAction={() => setOverlay("add")}
          />
          <ul className="inline-checks inline-checks--large" aria-label="What you need">
            <li>
              <span aria-hidden="true">○</span> SSH key or agent
            </li>
            <li>
              <span aria-hidden="true">○</span> VOWP on the host
            </li>
            <li>
              <span aria-hidden="true">○</span> Host key verified
            </li>
          </ul>
        </div>
      ) : (
        <div className="host-manager__list">
          {hosts.map((host) => (
            <HostCard
              key={host.id}
              host={host}
              connection={connection}
              fleet={fleet}
              latencyHistory={latencyHistory}
              selected={host.id === selectedHostId}
              onSelect={() => selectHost(host.id)}
              onConnect={() => requestConnect(host)}
              onDisconnect={() => void disconnect(host.id)}
              onEdit={() => edit(host)}
              onRepin={() => repin(host)}
              onRemove={() => setRemoveTarget(host)}
            />
          ))}
        </div>
      )}

      {overlay === "add" ? (
        <AddHostWizard
          onClose={() => setOverlay(null)}
          onSaved={(label) => saved(`${label} saved. Connect when you are ready.`)}
        />
      ) : null}

      {overlay === "edit" && editTarget ? (
        <EditHostDialog
          host={editTarget}
          onClose={() => {
            setOverlay(null);
            setEditTarget(null);
          }}
          onSaved={() => saved(`${editTarget.label} updated.`)}
        />
      ) : null}

      {overlay === "repin" && repinTarget ? (
        <RePinDialog
          host={repinTarget}
          onClose={() => {
            setOverlay(null);
            setRepinTarget(null);
          }}
          onSaved={() => saved(`${repinTarget.label} re-pinned after verification.`)}
        />
      ) : null}

      <Modal
        open={removeTarget !== null}
        title="Remove host profile?"
        danger
        onClose={() => setRemoveTarget(null)}
        footer={
          <>
            <Button variant="secondary" onClick={() => setRemoveTarget(null)} disabled={removing}>
              Cancel
            </Button>
            <Button variant="danger" loading={removing} onClick={() => void confirmRemove()}>
              Remove host
            </Button>
          </>
        }
      >
        <p>
          Remove <strong>{removeTarget?.label}</strong> ({removeTarget?.ssh.user}@
          {removeTarget?.ssh.host}:{removeTarget?.ssh.port}) from this Mac?
        </p>
        <p className="muted">
          This deletes the local profile and its pinned fingerprint. It never changes the
          VPS.
        </p>
      </Modal>

      <Modal
        open={showMismatch}
        title="Host key changed"
        danger
        onClose={() => {
          setMismatchDismissed(true);
          clearError();
        }}
        footer={
          <>
            <Button
              variant="secondary"
              onClick={() => {
                setMismatchDismissed(true);
                clearError();
              }}
            >
              Do not connect
            </Button>
            {activeHost ? (
              <Button
                variant="primary"
                onClick={() => {
                  setMismatchDismissed(true);
                  clearError();
                  repin(activeHost);
                }}
              >
                Re-pin key…
              </Button>
            ) : null}
          </>
        }
      >
        <p>
          StackSpirit refused to connect because the server offered a different key. Verify
          the new fingerprint out of band before re-pinning.
        </p>
        <dl className="fingerprint-compare">
          <div>
            <dt>Pinned</dt>
            <dd className="mono" data-testid="old-fingerprint">
              {mismatch?.oldSha256}
            </dd>
          </div>
          <div>
            <dt>Server offered</dt>
            <dd className="mono" data-testid="new-fingerprint">
              {mismatch?.newSha256}
            </dd>
          </div>
        </dl>
      </Modal>

      <ToastViewport>
        {notice ? (
          <Toast tone="ok" title={notice} onDismiss={() => setNotice(null)}>
            <span className="muted">
              <Badge tone="ok">local only</Badge>
            </span>
          </Toast>
        ) : null}
      </ToastViewport>

      {connectionPrompt}
    </div>
  );
}
