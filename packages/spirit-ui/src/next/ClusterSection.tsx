import { useState } from "react";

import type { ClusterHostHealthView } from "../bridge/wire";
import { toSpiritError, type SpiritError } from "../bridge/errors";
import { useFleetStore, useHosts } from "../state/fleetStore";
import { ClusterErrorAlert } from "./ClusterError";
import { useCluster, useClusterError, useClusterStatus, useClusterStore } from "./clusterStore";
import {
  agentStateLabel,
  clusterPooled,
  formatInterval,
  formatTimestamp,
  healthRowLabel,
  hostLabel,
  promotionLabel,
} from "./cluster";

export interface ClusterSectionProps {
  onFormCluster: () => void;
  onPromote: (target: string | null) => void;
}

/**
 * The Server page's cluster block: Home + every worker with its honest state and slots,
 * the pooled numbers read from `ClusterHealth`, the backup policy, and the
 * confirmation-gated promotion entry point.
 *
 * With fewer than two saved servers this renders nothing, so the single-server page is
 * unchanged. It never computes or caches a pooled total itself.
 */
export function ClusterSection({ onFormCluster, onPromote }: ClusterSectionProps) {
  const hosts = useHosts();
  const cluster = useCluster();
  const clusterAvailable = useFleetStore((state) => Boolean(state.bridge?.cluster));
  const status = useClusterStatus();
  const commandError = useClusterError();
  const loadCluster = useClusterStore((state) => state.loadCluster);
  const removeWorker = useClusterStore((state) => state.removeClusterWorker);
  const [removing, setRemoving] = useState<string | null>(null);
  const [removingBusy, setRemovingBusy] = useState(false);
  const [removeError, setRemoveError] = useState<SpiritError | null>(null);

  const config = cluster?.config ?? null;
  if (!config) {
    if (!clusterAvailable || hosts.length < 2) return null;
    return (
      <div className="nx-block nx-cluster nx-cluster--invite" role="region" aria-label="Cluster">
        <div className="nx-block__head">
          <h2>Cluster</h2>
        </div>
        <p className="nx-cluster__note">
          {hosts.length} servers are saved. Form a cluster to pool their workers and keep a
          scheduled backup on a Worker/Backup server.
        </p>
        <div className="nx-cluster__actions">
          <button type="button" className="nx-btn nx-btn--sm" onClick={onFormCluster}>
            Form a cluster
          </button>
        </div>
      </div>
    );
  }

  const health = cluster?.health ?? null;
  const pooled = health ? clusterPooled(health) : null;
  const homeRow = health?.hosts.find((row) => row.role === "home") ?? null;
  const homeName = hostLabel(hosts, config.home, homeRow?.label ?? "Home");
  const backupWorkers = config.workers.filter((worker) => worker.role === "backup");
  const restored = promotionLabel(hosts, cluster?.promotion ?? null);

  async function confirmRemove(hostId: string) {
    setRemovingBusy(true);
    setRemoveError(null);
    try {
      await removeWorker(hostId);
      setRemoving(null);
    } catch (caught) {
      setRemoveError(toSpiritError(caught));
    } finally {
      setRemovingBusy(false);
    }
  }

  return (
    <div
      className="nx-block nx-cluster"
      role="region"
      aria-label="Cluster"
      data-testid="cluster-section"
    >
      <div className="nx-block__head">
        <h2>Cluster</h2>
        <span className="nx-chip nx-chip--cluster nx-chip--tiny">cluster</span>
        <span className="nx-cluster__head-actions">
          <button type="button" className="nx-btn nx-btn--quiet nx-btn--sm" onClick={onFormCluster}>
            Form again
          </button>
          <button type="button" className="nx-btn nx-btn--sm" onClick={() => onPromote(null)}>
            Promote backup to Home
          </button>
        </span>
      </div>

      {commandError ? <ClusterErrorAlert error={commandError} /> : null}
      {removeError ? <ClusterErrorAlert error={removeError} /> : null}

      {pooled && health ? (
        <div className="nx-cluster__summary" data-testid="cluster-pooled">
          <div className="nx-cluster__number" aria-hidden="true">
            {pooled.busy}
            <span>/{pooled.pooled}</span>
          </div>
          <div className="nx-cluster__summary-text">
            <span className="nx-cluster__summary-title">
              {pooled.busy} of {pooled.pooled} pooled workers busy
            </span>
            <span className="nx-cluster__breakdown">
              Home {pooled.homeTarget} + {pooled.onlineWorkers} online worker
              {pooled.onlineWorkers === 1 ? "" : "s"} ({pooled.workerSlots} slots) ={" "}
              {pooled.pooled} workers
            </span>
            {pooled.offline.length > 0 ? (
              <span className="nx-cluster__offline">
                {pooled.offline.length} worker{pooled.offline.length === 1 ? "" : "s"} offline and
                not counted: {pooled.offline.map((row) => healthRowLabel(hosts, row)).join(", ")}.
              </span>
            ) : null}
            <span className="nx-cluster__source">
              {pooled.source === "vowp_cluster"
                ? "VOWP reported the pooled totals."
                : "Pooled totals computed from Home and the visible worker nodes."}
            </span>
          </div>
        </div>
      ) : status === "loading" || status === "idle" ? (
        <p className="nx-cluster__note" role="status">
          Reading cluster health from Home…
        </p>
      ) : null}

      {cluster?.health_error ? (
        <ClusterErrorAlert error={cluster.health_error} />
      ) : null}
      {cluster?.health_error ? (
        <div className="nx-cluster__actions">
          <button type="button" className="nx-btn nx-btn--quiet nx-btn--sm" onClick={() => void loadCluster()}>
            Retry health read
          </button>
        </div>
      ) : null}

      {health && health.warnings.length > 0 ? (
        <ul className="nx-cluster__warnings" aria-label="Cluster warnings">
          {health.warnings.map((warning) => (
            <li key={warning}>{warning}</li>
          ))}
        </ul>
      ) : null}

      <ul className="nx-cluster__hosts" aria-label="Cluster servers">
        <li className="nx-cluster__host">
          <span
            className={`nx-dot ${homeRow?.agent_state === "enrolled" ? "nx-dot--ok" : "nx-dot--warn"}`}
            aria-hidden="true"
          />
          <span className="nx-cluster__host-text">
            <span className="nx-cluster__host-title">
              Home · {homeName}
              <span className="nx-chip nx-chip--cluster nx-chip--tiny">home</span>
            </span>
            <span className="nx-cluster__meta">
              {homeRow ? agentStateLabel(homeRow.agent_state) : "state unknown"}
              {homeRow?.node_name ? ` · node ${homeRow.node_name}` : ""}
              {homeRow ? ` · ${homeRow.slots_busy}/${homeRow.slots_max} slots` : ""}
            </span>
            {homeRow?.last_error ? (
              <span className="nx-cluster__host-error">{homeRow.last_error}</span>
            ) : null}
          </span>
        </li>
        {config.workers.map((worker) => {
          const row =
            health?.hosts.find((candidate) => candidate.host === worker.host) ?? null;
          const label = hostLabel(hosts, worker.host, worker.node_name);
          return (
            <li key={worker.host} className="nx-cluster__host">
              <span className={`nx-dot ${dotClass(row)}`} aria-hidden="true" />
              <span className="nx-cluster__host-text">
                <span className="nx-cluster__host-title">
                  {label}
                  <span className="nx-chip nx-chip--cluster nx-chip--tiny">
                    {worker.role === "backup" ? "worker + backup" : "worker"}
                  </span>
                </span>
                <span className="nx-cluster__meta">
                  {row ? agentStateLabel(row.agent_state) : "not read yet"} · node{" "}
                  {worker.node_name}
                  {row ? ` · ${row.slots_busy}/${row.slots_max} slots` : ""}
                  {row && row.mapped_projects.length > 0
                    ? ` · ${row.mapped_projects.length} project${row.mapped_projects.length === 1 ? "" : "s"}`
                    : ""}
                </span>
                {worker.role === "backup" ? (
                  <span className="nx-cluster__meta">
                    Backup freshness: at most {formatInterval(config.backup.every_secs)} old at
                    promotion.
                  </span>
                ) : null}
                {row?.last_error ? (
                  <span className="nx-cluster__host-error">{row.last_error}</span>
                ) : null}
              </span>
              <span className="nx-cluster__host-actions">
                {worker.role === "backup" ? (
                  <button
                    type="button"
                    className="nx-btn nx-btn--quiet nx-btn--sm"
                    onClick={() => onPromote(worker.host)}
                  >
                    Promote
                  </button>
                ) : null}
                {removing === worker.host ? (
                  <>
                    <span className="nx-cluster__meta">Remove from the saved cluster?</span>
                    <button
                      type="button"
                      className="nx-btn nx-btn--sm"
                      disabled={removingBusy}
                      onClick={() => void confirmRemove(worker.host)}
                    >
                      {removingBusy ? "Removing…" : "Remove"}
                    </button>
                    <button
                      type="button"
                      className="nx-btn nx-btn--quiet nx-btn--sm"
                      disabled={removingBusy}
                      onClick={() => setRemoving(null)}
                    >
                      Keep
                    </button>
                  </>
                ) : (
                  <button
                    type="button"
                    className="nx-btn nx-btn--quiet nx-btn--sm"
                    onClick={() => {
                      setRemoveError(null);
                      setRemoving(worker.host);
                    }}
                  >
                    Remove
                  </button>
                )}
              </span>
            </li>
          );
        })}
      </ul>
      <p className="nx-cluster__note">
        Removing a worker only changes StackSpirit&apos;s saved cluster. Its VOWP enrollment and
        Home&apos;s backup schedule stay until you revoke them in VOWP.
      </p>

      <div className="nx-cluster__backup">
        <span className="nx-form__label">Backups</span>
        <p>
          Every {formatInterval(config.backup.every_secs)} · keep {config.backup.keep} ·{" "}
          <strong>freshness ≤ {formatInterval(config.backup.every_secs)}</strong> at promotion.
        </p>
        <p className="nx-cluster__note">
          Home registers the schedule in VOWP
          {backupWorkers.length > 0
            ? ` and writes to ${backupWorkers
                .map((worker) => hostLabel(hosts, worker.host, worker.node_name))
                .join(", ")}`
            : ""}
          . The destination itself is not shown or stored by StackSpirit.
        </p>
        {restored && cluster?.promotion ? (
          <p data-testid="cluster-promotion">
            Last promotion restored <strong>{restored}</strong> (node{" "}
            <code>{cluster.promotion.node_name}</code>) from a backup made{" "}
            {formatTimestamp(cluster.promotion.backup_at)}.
          </p>
        ) : null}
      </div>
    </div>
  );
}

/** The dot tone for one worker row. */
function dotClass(row: ClusterHostHealthView | null): string {
  if (!row) return "";
  if (row.agent_state === "enrolled") return "nx-dot--ok";
  if (row.agent_state === "pending") return "nx-dot--busy";
  return "nx-dot--warn";
}
