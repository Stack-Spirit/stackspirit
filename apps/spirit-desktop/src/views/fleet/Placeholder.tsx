import { Badge, Button, EmptyState, Panel } from "../../components";
import { formatAge, formatClock } from "../../domain/format";
import { supervisorLabel, supervisorTone } from "../../domain/connection";
import {
  useConnection,
  useDataDimmed,
  useFleet,
  useFleetStore,
  useProjectFilter,
} from "../../state/fleetStore";

const LANE_LABELS: Array<[string, string]> = [
  ["running", "Running"],
  ["ready", "Ready"],
  ["review_fix", "Review & Fix"],
  ["parked_blocked", "Parked & Blocked"],
  ["done", "Done"],
];

export function FleetPlaceholder({ onOpenHosts }: { onOpenHosts: () => void }) {
  const fleet = useFleet();
  const connection = useConnection();
  const dimmed = useDataDimmed();
  const projectFilter = useProjectFilter();
  const refresh = useFleetStore((state) => state.refresh);

  if (!fleet) {
    return (
      <div className="view-stack" data-testid="fleet-placeholder">
        <EmptyState
          icon="◈"
          title="No fleet snapshot yet"
          body={
            connection.state === "connecting"
              ? "Connecting to the host and reading its first VOWP snapshot…"
              : "Connect a verified VOWP host to see its workers, capacity and job lanes."
          }
          actionLabel="Open Hosts"
          onAction={onOpenHosts}
        />
        <Panel title="Fleet & Worker Canvas">
          <p className="muted">
            The visual canvas, worker ring and job lanes arrive with the Fleet Canvas job
            (D3). This shell already carries the connected view models and revisions.
          </p>
        </Panel>
      </div>
    );
  }

  const projects = projectFilter
    ? fleet.projects.filter((project) => project.name === projectFilter)
    : fleet.projects;
  const openInbox = fleet.inbox.filter((item) => item.state === "open").length;

  return (
    <div className="view-stack" data-testid="fleet-placeholder">
      <Panel
        className={dimmed ? "data-dimmed" : undefined}
        title={
          <span>
            Fleet · <span className="mono">{fleet.home}</span>
          </span>
        }
        actions={
          <Button
            variant="secondary"
            size="sm"
            onClick={() => void refresh()}
            disabled={connection.state !== "connected"}
          >
            Refresh
          </Button>
        }
      >
        <div className="fleet-summary">
          <div className="fleet-summary__root">
            <span className="eyebrow">VPS root</span>
            <strong className="mono">{fleet.home}</strong>
            <Badge tone={supervisorTone(fleet)}>{supervisorLabel(fleet)}</Badge>
            <span className="muted small mono">VOWP {fleet.supervisor.version}</span>
          </div>
          <dl className="metric-grid">
            <div>
              <dt>Projects</dt>
              <dd>{fleet.projects.length}</dd>
            </div>
            <div>
              <dt>Running</dt>
              <dd>{fleet.running.length}</dd>
            </div>
            <div>
              <dt>Target</dt>
              <dd>{fleet.capacity.target.target_workers}</dd>
            </div>
            <div>
              <dt>Open inbox</dt>
              <dd>{openInbox}</dd>
            </div>
          </dl>
        </div>
        {connection.state === "disconnected" ? (
          <div className="notice" role="status">
            <Badge tone="unknown">disconnected</Badge> Showing data from{" "}
            {formatClock(fleet.generated_at)}. All fleet actions are disabled until you
            reconnect.
          </div>
        ) : null}
        {fleet.stale ? (
          <div className="notice notice--warn" role="status">
            <Badge tone="warn">stale</Badge> Showing data from{" "}
            {formatClock(fleet.generated_at)} ({formatAge(fleet.supervisor.last_tick_age_secs)}{" "}
            old). Refresh before changing capacity or jobs.
          </div>
        ) : null}
      </Panel>

      <div className="lane-preview" aria-label="Job lane counts">
        {LANE_LABELS.map(([key, label]) => {
          const lane = fleet.lanes[key as keyof typeof fleet.lanes];
          const cards = Array.isArray(lane) ? lane.length : 0;
          return (
            <Panel key={key} className="lane-preview__card">
              <span className="eyebrow">{label}</span>
              <strong className="lane-preview__count">{fleet.lanes.counts[key] ?? 0}</strong>
              <span className="muted small">
                {cards} card{cards === 1 ? "" : "s"} shown
              </span>
            </Panel>
          );
        })}
      </div>

      <Panel title="Projects">
        {projects.length === 0 ? (
          <p className="muted">No project matches the current filter.</p>
        ) : (
          <ul className="project-list">
            {projects.map((project) => (
              <li key={project.name} className="project-list__item">
                <span className="mono">{project.name}</span>
                <Badge tone={project.state === "active" ? "ok" : "warn"}>
                  {project.state}
                </Badge>
                <span className="muted small">
                  {project.jobs_by_state.running ?? 0} running ·{" "}
                  {project.jobs_by_state.ready ?? 0} waiting
                </span>
              </li>
            ))}
          </ul>
        )}
        <p className="muted small">
          The full canvas, worker ring and job drawers arrive with the Fleet Canvas job
          (D3). Preview and live data flow through the same SpiritBridge.
        </p>
      </Panel>
    </div>
  );
}
