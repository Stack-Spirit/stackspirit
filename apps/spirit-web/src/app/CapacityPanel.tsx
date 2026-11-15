import { Badge, Panel, formatCount } from "@stackspirit/ui";

import {
  CAPACITY_DETAIL_EXPLAIN,
  CAPACITY_DETAIL_UNAVAILABLE,
  PROJECT_STATE_UNAVAILABLE,
  SUPERVISOR_STATE_UNAVAILABLE,
} from "../copy";
import type { WebFleetView } from "@stackspirit/ui";

function usd(value: number | null): string {
  if (value === null) return "Unknown — the daemon reported no USD amount today.";
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
  }).format(value);
}

export function SupervisorCard({ snapshot }: { snapshot: WebFleetView }) {
  const heartbeat = snapshot.daemon.heartbeat_at;
  const heartbeatAge =
    heartbeat === null
      ? null
      : Math.max(0, Math.floor((Date.parse(snapshot.daemon.received_at) - Date.parse(heartbeat)) / 1000));
  return (
    <Panel title="Supervisor" data-testid="supervisor-panel">
      <p data-testid="supervisor-unavailable">{SUPERVISOR_STATE_UNAVAILABLE}</p>
      <dl className="web-facts">
        <div>
          <dt>Last heartbeat</dt>
          <dd data-testid="supervisor-heartbeat">
            {heartbeat === null
              ? "No supervisor heartbeat is present in /api/v1/health."
              : `${heartbeatAge}s before this health read`}
          </dd>
        </div>
        <div>
          <dt>Daemon version</dt>
          <dd className="mono">{snapshot.daemon.version}</dd>
        </div>
        <div>
          <dt>Store schema</dt>
          <dd className="mono">{snapshot.daemon.schema_version}</dd>
        </div>
      </dl>
      <p className="muted small">
        The heartbeat is a separate fact, not proof that the supervisor is running or stopped.
      </p>
    </Panel>
  );
}

export function CapacityPanel({ snapshot }: { snapshot: WebFleetView }) {
  const facts = snapshot.board;
  return (
    <div className="view-stack" data-testid="capacity-panel">
      <Panel title="Capacity & VPS Storage" tone="warn">
        <p data-testid="capacity-unavailable">{CAPACITY_DETAIL_UNAVAILABLE}</p>
        <p className="muted small">{CAPACITY_DETAIL_EXPLAIN}</p>
        <p className="notice notice--warn" data-testid="capacity-not-a-view">
          There is no CapacityView in this build: no gauges, target, brake, estimate, or
          waiting list is shown.
        </p>
      </Panel>

      <Panel title="Board facts" data-testid="board-facts">
        <dl className="web-facts">
          <div>
            <dt>Configured slots</dt>
            <dd data-testid="configured-slots">
              {facts.slots_busy} / {facts.slots_total} configured slots
            </dd>
          </div>
          <div>
            <dt>Cost today</dt>
            <dd data-testid="cost-today">{usd(facts.cost_today_usd)}</dd>
          </div>
          <div>
            <dt>Open inbox</dt>
            <dd data-testid="inbox-open">{formatCount(facts.inbox_open)}</dd>
          </div>
          <div>
            <dt>Last checkpoint</dt>
            <dd>{facts.last_checkpoint ?? "None reported by the board."}</dd>
          </div>
          <div>
            <dt>Planner</dt>
            <dd className="mono">{facts.planner ?? "Not configured"}</dd>
          </div>
          <div>
            <dt>Keeper</dt>
            <dd className="mono">{facts.keeper ?? "Not configured"}</dd>
          </div>
        </dl>
        <p className="muted small">
          These are exact board values. Configured slots are not a worker target.
        </p>

        <h3 className="web-subtitle">Active limits</h3>
        {facts.limits.length === 0 ? (
          <p className="muted small" data-testid="limits-empty">
            The board reports no active limits.
          </p>
        ) : (
          <ul className="web-list" data-testid="active-limits">
            {facts.limits.map((limit) => (
              <li key={`${limit.provider}:${limit.kind}`}>
                <Badge tone="warn">{limit.kind}</Badge>{" "}
                <span className="mono">{limit.provider}</span>{" "}
                <span className="muted small">
                  {limit.resets_at ? `resets ${limit.resets_at}` : "needs a human"}
                </span>
              </li>
            ))}
          </ul>
        )}
      </Panel>

      <Panel title="Project state" data-testid="project-state-panel">
        <p data-testid="project-state-unavailable">{PROJECT_STATE_UNAVAILABLE}</p>
        <dl className="web-facts">
          <div>
            <dt>Project</dt>
            <dd className="mono">{snapshot.project.name}</dd>
          </div>
          <div>
            <dt>Jobs by state</dt>
            <dd className="mono">
              {Object.entries(snapshot.project.jobs_by_state)
                .map(([state, count]) => `${state} ${count}`)
                .join(" · ") || "No job counts reported"}
            </dd>
          </div>
        </dl>
      </Panel>
    </div>
  );
}
