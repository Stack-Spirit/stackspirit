import { useId, useState, type FormEvent } from "react";

import type {
  ClusterConfigView,
  ClusterFormedView,
  HostProfile,
} from "../bridge/wire";
import { toSpiritError, type SpiritError } from "../bridge/errors";
import { Modal } from "../components";
import { useCapacity, useCluster, useConnection, useFleet, useFleetStore } from "../state/fleetStore";
import { ClusterErrorAlert } from "./ClusterError";
import {
  clusterDraftReady,
  formatInterval,
  hostLabel,
  suggestNodeName,
  uniqueNodeName,
  validateClusterDraft,
  type ClusterFormDraft,
  type ClusterWorkerDraft,
} from "./cluster";

export interface FormClusterDialogProps {
  hosts: HostProfile[];
  /** The saved cluster, so a re-form starts from the current shape. */
  initial: ClusterConfigView | null;
  onClose: () => void;
}

const DAY = 86_400;

/**
 * Form a cluster over verified SSH: one Home, one or more Worker/Backup servers, Home's
 * backup policy and the owner's explicit confirmation.
 *
 * The dialog never guesses pooled capacity. It shows which servers will join and the
 * Home target when that is the connected server; the real pooled total is read from
 * `ClusterHealth` after the form succeeds. Typed failures stay inline so the owner can
 * fix one worker without retyping the form.
 */
export function FormClusterDialog({ hosts, initial, onClose }: FormClusterDialogProps) {
  const formCluster = useFleetStore((state) => state.formCluster);
  const fleet = useFleet();
  const connection = useConnection();
  const capacity = useCapacity();
  const cluster = useCluster();
  const initialHome =
    initial?.home && hosts.some((host) => host.id === initial.home)
      ? initial.home
      : (hosts[0]?.id ?? "");
  const [homeId, setHomeId] = useState(initialHome);
  const [workers, setWorkers] = useState<ClusterWorkerDraft[]>(() =>
    (initial?.workers ?? [])
      .filter(
        (worker) =>
          worker.host !== initialHome && hosts.some((host) => host.id === worker.host),
      )
      .map((worker) => ({
        hostId: worker.host,
        nodeName: worker.node_name,
        role: worker.role,
      })),
  );
  const [everySecs, setEverySecs] = useState(initial?.backup.every_secs ?? DAY);
  const [keep, setKeep] = useState(initial?.backup.keep ?? 7);
  const [backupTarget, setBackupTarget] = useState("");
  const [projectName, setProjectName] = useState("");
  const [projectTouched, setProjectTouched] = useState(false);
  const [projectRoot, setProjectRoot] = useState("");
  const [workerRoots, setWorkerRoots] = useState<Record<string, string>>({});
  const [confirmed, setConfirmed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<SpiritError | null>(null);
  const [formed, setFormed] = useState<ClusterFormedView | null>(null);
  const ids = { backup: useId(), project: useId(), every: useId(), keep: useId(), root: useId() };

  const suggestedProject =
    hosts.find((host) => host.id === homeId)?.default_project ??
    fleet?.projects[0]?.name ??
    "";
  const effectiveProject = projectTouched ? projectName : projectName || suggestedProject;

  const draft: ClusterFormDraft = {
    homeId,
    workers,
    workerRoots,
    everySecs,
    keep,
    projectName: effectiveProject,
    projectRoot,
    backupTarget,
    confirmed,
  };
  const errors = validateClusterDraft(draft, hosts);
  const ready = clusterDraftReady(draft, errors);
  const selectedHome = hosts.find((host) => host.id === homeId) ?? null;
  const candidates = hosts.filter((host) => host.id !== homeId);
  const homeTarget =
    connection.host_id === homeId && capacity ? capacity.target.target_workers : null;
  const current = cluster?.health;

  function selectHome(id: string) {
    setHomeId(id);
    setWorkers((currentWorkers) => currentWorkers.filter((worker) => worker.hostId !== id));
    setWorkerRoots((roots) => {
      if (!(id in roots)) return roots;
      const next = { ...roots };
      delete next[id];
      return next;
    });
  }

  function toggleWorker(host: HostProfile, checked: boolean) {
    if (!checked) {
      setWorkerRoots((roots) => {
        if (!(host.id in roots)) return roots;
        const next = { ...roots };
        delete next[host.id];
        return next;
      });
    }
    setWorkers((currentWorkers) => {
      if (!checked) return currentWorkers.filter((worker) => worker.hostId !== host.id);
      if (currentWorkers.some((worker) => worker.hostId === host.id)) return currentWorkers;
      const used = currentWorkers.map((worker) => worker.nodeName);
      const nodeName = uniqueNodeName(
        suggestNodeName(host.label, `worker-${currentWorkers.length + 1}`),
        used,
      );
      return [...currentWorkers, { hostId: host.id, nodeName, role: "worker" }];
    });
  }

  function patchWorker(hostId: string, patch: Partial<ClusterWorkerDraft>) {
    setWorkers((currentWorkers) =>
      currentWorkers.map((worker) =>
        worker.hostId === hostId ? { ...worker, ...patch } : worker,
      ),
    );
  }

  async function submit(event?: FormEvent) {
    event?.preventDefault();
    if (!ready || busy) return;
    setBusy(true);
    setError(null);
    try {
      const result = await formCluster({
        home: homeId,
        workers: workers.map((worker) => ({
          host: worker.hostId,
          node_name: worker.nodeName.trim(),
          role: worker.role,
        })),
        backup: { every_secs: everySecs, keep },
        home_project: { name: effectiveProject.trim(), root: projectRoot.trim() },
        worker_projects: workers.flatMap((worker) => {
          const root = workerRoots[worker.hostId]?.trim() ?? "";
          if (root.length === 0) return [];
          return [
            {
              worker: worker.hostId,
              project: effectiveProject.trim(),
              root,
              worktree_root: null,
              allow: [],
            },
          ];
        }),
        backup_target: backupTarget.trim(),
        confirmed: true,
      });
      setFormed(result);
    } catch (caught) {
      setError(toSpiritError(caught));
    } finally {
      setBusy(false);
    }
  }

  if (formed) {
    return (
      <Modal
        open
        title="Cluster formed"
        onClose={onClose}
        footer={
          <button type="button" className="nx-btn" onClick={onClose}>
            Done
          </button>
        }
      >
        <div className="nx-form" data-testid="cluster-formed">
          <p className="nx-form__lede">
            <strong>{hostLabel(hosts, formed.config.home)}</strong> is Home.{" "}
            {formed.config.workers.length} Worker/Backup server
            {formed.config.workers.length === 1 ? "" : "s"} are enrolled.
          </p>
          <p>
            Pooled compute is {formed.offload_config_changed ? "now enabled" : "already enabled"} on
            Home. Backups are registered as{" "}
            <code>{formed.backup_schedule}</code> every {formatInterval(formed.config.backup.every_secs)}.
          </p>
          <p className="nx-cluster__note">
            The real pooled target is read from ClusterHealth while this dialog closes. It is
            never copied from the form's placeholder numbers.
          </p>
        </div>
      </Modal>
    );
  }

  return (
    <Modal
      open
      title="Form a cluster"
      onClose={onClose}
      footer={
        <>
          <button type="button" className="nx-btn nx-btn--quiet" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button
            type="button"
            className="nx-btn"
            disabled={!ready || busy}
            aria-busy={busy || undefined}
            onClick={() => void submit()}
          >
            {busy ? "Forming…" : "Form cluster"}
          </button>
        </>
      }
    >
      <form className="nx-form" aria-label="Form a cluster" onSubmit={(event) => void submit(event)}>
        <p className="nx-form__lede">
          A cluster has exactly one Home — the source of truth for the orchestrator, project
          databases and session logs — and one or more workers that run jobs. Workers stay
          independent until this form succeeds; nothing changes before the confirmation below.
        </p>

        {error ? <ClusterErrorAlert error={error} /> : null}

        <label className="nx-field">
          Home server
          <select aria-label="Home server" value={homeId} onChange={(event) => selectHome(event.target.value)}>
            {hosts.map((host) => (
              <option key={host.id} value={host.id}>
                {host.label}
              </option>
            ))}
          </select>
        </label>
        {errors.home ? <span className="nx-form__error">{errors.home}</span> : null}

        <div className="nx-form__row">
          <span className="nx-form__label">Worker / Backup servers</span>
          {candidates.length === 0 ? (
            <p className="nx-cluster__note">Save another server to add a worker.</p>
          ) : (
            <ul className="nx-cluster__picklist">
              {candidates.map((host) => {
                const worker = workers.find((item) => item.hostId === host.id);
                return (
                  <li key={host.id} className="nx-cluster__pick">
                    <label className="checkbox-row">
                      <input
                        type="checkbox"
                        checked={Boolean(worker)}
                        aria-label={`Use ${host.label} as a worker`}
                        onChange={(event) => toggleWorker(host, event.target.checked)}
                      />
                      <span>
                        <strong>{host.label}</strong>
                        <span className="nx-cluster__meta">
                          {host.ssh.user}@{host.ssh.host}
                        </span>
                      </span>
                    </label>
                    {worker ? (
                      <div className="nx-cluster__pick-detail">
                        <label className="nx-field">
                          Node name
                          <input
                            value={worker.nodeName}
                            aria-label={`Node name for ${host.label}`}
                            aria-invalid={Boolean(errors.nodeNames[host.id])}
                            spellCheck={false}
                            onChange={(event) =>
                              patchWorker(host.id, { nodeName: event.target.value })
                            }
                          />
                        </label>
                        <label className="nx-field">
                          Role
                          <select
                            value={worker.role}
                            aria-label={`Role for ${host.label}`}
                            onChange={(event) =>
                              patchWorker(host.id, {
                                role: event.target.value === "backup" ? "backup" : "worker",
                              })
                            }
                          >
                            <option value="worker">Worker</option>
                            <option value="backup">Worker + backup</option>
                          </select>
                        </label>
                        <label className="nx-field nx-cluster__pick-path">
                          Project checkout on {host.label} (optional)
                          <input
                            value={workerRoots[host.id] ?? ""}
                            aria-label={`Project checkout on ${host.label}`}
                            placeholder="/srv/code/project"
                            spellCheck={false}
                            aria-invalid={Boolean(errors.workerRoots[host.id])}
                            onChange={(event) =>
                              setWorkerRoots((roots) => ({
                                ...roots,
                                [host.id]: event.target.value,
                              }))
                            }
                          />
                        </label>
                        {errors.nodeNames[host.id] ? (
                          <span className="nx-form__error">{errors.nodeNames[host.id]}</span>
                        ) : null}
                        {errors.workerRoots[host.id] ? (
                          <span className="nx-form__error">{errors.workerRoots[host.id]}</span>
                        ) : null}
                      </div>
                    ) : null}
                  </li>
                );
              })}
            </ul>
          )}
          {errors.workers ? <span className="nx-form__error">{errors.workers}</span> : null}
          <p className="nx-cluster__note">
            Optional: add the project checkout on each worker so VOWP can run jobs there. Leave it
            empty to keep that worker&apos;s existing VOWP path maps untouched.
          </p>
        </div>

        <div className="nx-cluster__preview">
          <span className="nx-form__label">Pooled target</span>
          <p>
            {selectedHome ? <strong>{selectedHome.label}</strong> : "Home"} +{" "}
            <strong>{workers.length}</strong> worker server{workers.length === 1 ? "" : "s"}
          </p>
          <p className="nx-cluster__note">
            Pooled capacity is Home&apos;s target plus each worker&apos;s VOWP target. StackSpirit reads
            the real total from ClusterHealth after the servers are enrolled and never assumes a
            slot count here.
          </p>
          {homeTarget != null ? (
            <p className="nx-cluster__note">
              The connected Home currently targets {homeTarget} workers.
            </p>
          ) : null}
          {initial && current ? (
            <p className="nx-cluster__note">
              Current cluster: Home {current.home_target_workers} + {current.online_workers} online
              workers ({current.worker_slots} slots) = {current.pooled_target_workers} workers.
            </p>
          ) : null}
        </div>

        <div className="nx-form__grid">
          <label className="nx-field" htmlFor={ids.every}>
            Back up every
            <select
              id={ids.every}
              value={everySecs}
              onChange={(event) => setEverySecs(Number(event.target.value))}
            >
              <option value={3600}>Hour</option>
              <option value={21_600}>6 hours</option>
              <option value={43_200}>12 hours</option>
              <option value={86_400}>Day</option>
              <option value={604_800}>Week</option>
            </select>
          </label>
          <label className="nx-field" htmlFor={ids.keep}>
            Keep
            <input
              id={ids.keep}
              type="number"
              min={1}
              max={1000}
              value={keep}
              aria-invalid={Boolean(errors.keep)}
              onChange={(event) => setKeep(Number(event.target.value))}
            />
          </label>
        </div>
        {errors.every ? <span className="nx-form__error">{errors.every}</span> : null}
        {errors.keep ? <span className="nx-form__error">{errors.keep}</span> : null}

        <div className="nx-form__grid">
          <label className="nx-field" htmlFor={ids.project}>
            Project name on Home
            <input
              id={ids.project}
              value={effectiveProject}
              placeholder="stackspirit"
              spellCheck={false}
              aria-invalid={Boolean(errors.projectName)}
              onChange={(event) => {
                setProjectTouched(true);
                setProjectName(event.target.value);
              }}
            />
          </label>
          <label className="nx-field" htmlFor={ids.root}>
            Project checkout on Home
            <input
              id={ids.root}
              value={projectRoot}
              placeholder="/srv/code/project"
              spellCheck={false}
              aria-invalid={Boolean(errors.projectRoot)}
              onChange={(event) => setProjectRoot(event.target.value)}
            />
          </label>
        </div>
        {errors.projectName ? <span className="nx-form__error">{errors.projectName}</span> : null}
        {errors.projectRoot ? <span className="nx-form__error">{errors.projectRoot}</span> : null}

        <label className="nx-field" htmlFor={ids.backup}>
          Backup destination
          <input
            id={ids.backup}
            value={backupTarget}
            placeholder="backup-host:/srv/vowp-backups"
            spellCheck={false}
            autoComplete="off"
            aria-invalid={Boolean(errors.backupTarget)}
            onChange={(event) => setBackupTarget(event.target.value)}
          />
        </label>
        {errors.backupTarget ? (
          <span className="nx-form__error">{errors.backupTarget}</span>
        ) : (
          <p className="nx-cluster__note">
            A path or rsync target for <code>vowp backup --to</code>. Use the server&apos;s SSH key;
            never put a password or token here.
          </p>
        )}

        <label className="checkbox-row nx-cluster__confirm">
          <input
            type="checkbox"
            checked={confirmed}
            aria-label="Confirm cluster changes"
            onChange={(event) => setConfirmed(event.target.checked)}
          />
          <span>
            I understand StackSpirit will enroll each worker, edit Home&apos;s <code>vowp.toml</code>{" "}
            and register a backup schedule over SSH. This cannot be undone by cancelling.
          </span>
        </label>

        {busy ? (
          <p className="nx-cluster__note" role="status">
            Working over SSH. Every worker is preflighted before the first mutation; this can take a
            minute per server.
          </p>
        ) : null}
      </form>
    </Modal>
  );
}
