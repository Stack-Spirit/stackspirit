import { useId, useState, type FormEvent } from "react";

import type { ClusterPromoteOutcome, HostProfile } from "../bridge/wire";
import { toSpiritError, type SpiritError } from "../bridge/errors";
import { Modal } from "../components";
import { ClusterErrorAlert } from "./ClusterError";
import {
  PROMOTE_CONFIRMATION,
  formatInterval,
  formatTimestamp,
  hostLabel,
  isAbsoluteRemotePath,
} from "./cluster";
import { useCluster, useClusterStore } from "./clusterStore";

export interface PromoteBackupDialogProps {
  hosts: HostProfile[];
  /** A worker preselected from the cluster row, when one was clicked. */
  initialTarget: string | null;
  onClose: () => void;
  /** Connects the shell to the restored Home after the owner asks for it. */
  onConnect: (hostId: string) => void;
}

/**
 * Promote one worker's backup to Home.
 *
 * This is the explicit failover action: the owner types the exact confirmation phrase
 * and the host passes it to `ClusterService::promote`, which drains Home first and
 * refuses rather than start a second Home. The result names the restored Home but never
 * a directory; StackSpirit does not silently switch the active server.
 */
export function PromoteBackupDialog({
  hosts,
  initialTarget,
  onClose,
  onConnect,
}: PromoteBackupDialogProps) {
  const cluster = useCluster();
  const promote = useClusterStore((state) => state.promoteClusterBackup);
  const workers = cluster?.config?.workers ?? [];
  const preferred =
    (initialTarget && workers.some((worker) => worker.host === initialTarget)
      ? initialTarget
      : null) ??
    workers.find((worker) => worker.role === "backup")?.host ??
    workers[0]?.host ??
    "";
  const [target, setTarget] = useState(preferred);
  const [backupDir, setBackupDir] = useState("");
  const [newHome, setNewHome] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<SpiritError | null>(null);
  const [outcome, setOutcome] = useState<ClusterPromoteOutcome | null>(null);
  const ids = { target: useId(), backup: useId(), home: useId(), phrase: useId() };

  const policy = cluster?.config?.backup ?? null;
  const targetProfile = hosts.find((host) => host.id === target) ?? null;
  const targetNode = workers.find((worker) => worker.host === target)?.node_name ?? null;
  const backupPathOk = isAbsoluteRemotePath(backupDir);
  const newHomeOk = newHome.trim().length === 0 || isAbsoluteRemotePath(newHome);
  const phraseOk = confirmation.trim() === PROMOTE_CONFIRMATION;
  const ready = target.length > 0 && backupPathOk && newHomeOk && phraseOk && !busy;

  async function submit(event?: FormEvent) {
    event?.preventDefault();
    if (!ready) return;
    setBusy(true);
    setError(null);
    try {
      const result = await promote({
        target,
        backup_dir: backupDir.trim(),
        new_home: newHome.trim() === "" ? null : newHome.trim(),
        confirmation: confirmation.trim(),
      });
      setOutcome(result);
    } catch (caught) {
      setError(toSpiritError(caught));
    } finally {
      setBusy(false);
    }
  }

  if (outcome) {
    const label = hostLabel(hosts, outcome.new_home, outcome.node_name);
    return (
      <Modal
        open
        title="Backup promoted"
        onClose={onClose}
        footer={
          <>
            <button type="button" className="nx-btn nx-btn--quiet" onClick={onClose}>
              Close
            </button>
            <button
              type="button"
              className="nx-btn"
              onClick={() => {
                onConnect(outcome.new_home);
                onClose();
              }}
            >
              Connect to {label}
            </button>
          </>
        }
      >
        <div className="nx-form" data-testid="promote-outcome">
          <p className="nx-form__lede">
            StackSpirit restored the backup and started the supervisor on{" "}
            <strong>{label}</strong>.
          </p>
          <dl className="nx-cluster__facts">
            <div>
              <dt>Restored Home</dt>
              <dd>{label}</dd>
            </div>
            <div>
              <dt>StackSpirit node</dt>
              <dd className="mono">{outcome.node_name}</dd>
            </div>
            <div>
              <dt>Backup from</dt>
              <dd>{formatTimestamp(outcome.backup_at)}</dd>
            </div>
            <div>
              <dt>Promoted at</dt>
              <dd>{formatTimestamp(outcome.promoted_at)}</dd>
            </div>
          </dl>
          <p className="nx-cluster__note">
            The active server has not been switched. Connect to {label} when you are ready;
            old Home credentials and the saved cluster shape stay untouched.
          </p>
          {newHome.trim() !== "" ? (
            <p className="nx-cluster__note">
              This promotion used a fresh StackSpirit home. Update {label}&apos;s StackSpirit home in Settings
              before connecting, or StackSpirit will read the old directory.
            </p>
          ) : null}
          <p className="nx-cluster__note">
            StackSpirit never backs up secrets. Re-enter server secret values with{" "}
            <code>vowp secret set</code> on the restored Home.
          </p>
        </div>
      </Modal>
    );
  }

  return (
    <Modal
      open
      title="Promote backup to Home"
      onClose={onClose}
      footer={
        <>
          <button type="button" className="nx-btn nx-btn--quiet" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button
            type="button"
            className="nx-btn"
            disabled={!ready}
            aria-busy={busy || undefined}
            onClick={() => void submit()}
          >
            {busy ? "Promoting…" : "Promote backup"}
          </button>
        </>
      }
    >
      <form
        className="nx-form"
        aria-label="Promote backup to Home"
        onSubmit={(event) => void submit(event)}
      >
        <p className="nx-form__lede">
          This is failover, not a passive switch. StackSpirit drains the old Home first, restores
          the chosen backup on a worker and starts it as the new Home.
        </p>

        {error ? <ClusterErrorAlert error={error} /> : null}

        <label className="nx-field" htmlFor={ids.target}>
          Backup source
          <select
            id={ids.target}
            value={target}
            disabled={busy}
            onChange={(event) => setTarget(event.target.value)}
          >
            {workers.map((worker) => (
              <option key={worker.host} value={worker.host}>
                {hostLabel(hosts, worker.host, worker.node_name)}
                {worker.role === "backup" ? " (worker + backup)" : ""}
              </option>
            ))}
          </select>
        </label>
        {targetNode ? (
          <p className="nx-cluster__note">
            StackSpirit node <code>{targetNode}</code> on {targetProfile?.label ?? "the selected server"}.
          </p>
        ) : null}

        <label className="nx-field" htmlFor={ids.backup}>
          Backup directory on the target
          <input
            id={ids.backup}
            value={backupDir}
            disabled={busy}
            placeholder="/srv/vowp-backups/cluster-backup"
            spellCheck={false}
            autoComplete="off"
            aria-invalid={backupDir.length > 0 && !backupPathOk}
            onChange={(event) => setBackupDir(event.target.value)}
          />
        </label>
        {backupDir.length > 0 && !backupPathOk ? (
          <span className="nx-form__error">
            Enter the absolute backup directory StackSpirit wrote on the target.
          </span>
        ) : null}

        <label className="nx-field" htmlFor={ids.home}>
          New StackSpirit home on the target (optional)
          <input
            id={ids.home}
            value={newHome}
            disabled={busy}
            placeholder="Uses the target profile's StackSpirit home"
            spellCheck={false}
            autoComplete="off"
            aria-invalid={!newHomeOk}
            onChange={(event) => setNewHome(event.target.value)}
          />
        </label>
        {!newHomeOk ? (
          <span className="nx-form__error">
            Use an absolute path for the fresh home, or leave it empty.
          </span>
        ) : null}

        {policy ? (
          <p className="nx-cluster__note">
            Backup policy: every {formatInterval(policy.every_secs)}, keep {policy.keep}. StackSpirit
            refuses a backup older than {formatInterval(policy.every_secs)} before it drains
            Home.
          </p>
        ) : null}

        <div className="nx-cluster__warning">
          <strong>Home must be reachable for the drain.</strong> If StackSpirit cannot confirm that the old
          Home stopped admitting work, promotion refuses instead of risking two live Homes. Secrets
          are not restored.
        </div>

        <label className="nx-field" htmlFor={ids.phrase}>
          Type {PROMOTE_CONFIRMATION} to confirm
          <input
            id={ids.phrase}
            value={confirmation}
            disabled={busy}
            spellCheck={false}
            autoComplete="off"
            aria-invalid={confirmation.length > 0 && !phraseOk}
            onChange={(event) => setConfirmation(event.target.value)}
          />
        </label>
        {confirmation.length > 0 && !phraseOk ? (
          <span className="nx-form__error">The confirmation phrase does not match exactly.</span>
        ) : null}

        {busy ? (
          <p className="nx-cluster__note" role="status">
            Draining Home and restoring the backup. This can take up to 30 minutes if runs are
            still finishing; StackSpirit shows the result when StackSpirit returns.
          </p>
        ) : null}
      </form>
    </Modal>
  );
}
