import type { FleetView } from "../../bridge/wire";
import { Badge } from "../../components";
import { supervisorLabel, supervisorTone } from "../../domain/connection";
import { formatAge } from "../../domain/format";
import { laneCount } from "../../domain/fleet";

export function VpsRootCard({ fleet }: { fleet: FleetView }) {
  const running = laneCount(fleet.lanes, "running");
  const queue = laneCount(fleet.lanes, "ready");
  const limits = fleet.capacity.limits.length;
  const tickAge = fleet.supervisor.last_tick_age_secs;

  return (
    <article
      className="vps-root-card"
      aria-label={`VPS root ${fleet.home}`}
      data-testid="vps-root-card"
      data-stale={fleet.stale ? "true" : "false"}
    >
      <span className="eyebrow">VPS root</span>
      <div className="vps-root-card__name">
        <strong className="mono">{fleet.home}</strong>
        <Badge tone={supervisorTone(fleet)}>{supervisorLabel(fleet)}</Badge>
      </div>
      <dl className="vps-root-card__metrics">
        <div>
          <dt>Projects</dt>
          <dd>{fleet.projects.length}</dd>
        </div>
        <div>
          <dt>Running</dt>
          <dd>{running}</dd>
        </div>
        <div>
          <dt>Queue</dt>
          <dd>{queue}</dd>
        </div>
        <div>
          <dt>Limits</dt>
          <dd>{limits}</dd>
        </div>
      </dl>
      <div className="vps-root-card__footer">
        <span className="mono small">VOWP {fleet.supervisor.version}</span>
        <span className="muted small">
          last tick {tickAge === null ? "unknown" : `${formatAge(tickAge)} ago`}
        </span>
      </div>
    </article>
  );
}
