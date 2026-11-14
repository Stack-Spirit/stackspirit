import type { CapacityView } from "../../bridge/wire";
import { Badge, Panel } from "../../components";
import {
  CAPACITY_DETAIL_UNAVAILABLE,
  groupWaiting,
  waitingAgeSeconds,
} from "../../domain/capacity";
import { formatAgeParts } from "../../domain/format";

export function WaitingPanel({ capacity }: { capacity: CapacityView }) {
  if (!capacity.report_loaded) {
    return (
      <Panel title="Waiting reasons" data-testid="waiting-panel">
        <p className="muted small" data-testid="waiting-unavailable">
          {CAPACITY_DETAIL_UNAVAILABLE}
        </p>
      </Panel>
    );
  }

  const groups = groupWaiting(capacity.waiting);
  if (capacity.waiting.length === 0) {
    return (
      <Panel title="Waiting reasons" data-testid="waiting-panel">
        <p className="muted small">No jobs are waiting for capacity.</p>
      </Panel>
    );
  }

  return (
    <Panel
      title="Waiting reasons"
      actions={<Badge tone="warn">{capacity.waiting.length} waiting</Badge>}
      data-testid="waiting-panel"
    >
      <div className="waiting-panel__groups">
        {groups.map((group) => (
          <Badge key={group.code} tone="warn" title={`${group.code}: ${group.count}`}>
            {group.label} · {group.count}
          </Badge>
        ))}
      </div>
      <ul className="reason-list waiting-panel__rows">
        {capacity.waiting.slice(0, 5).map((waiting) => (
          <li key={`${waiting.project}-${waiting.job}`} data-testid="waiting-row">
            <span className="mono">
              {waiting.project} · {waiting.job}
            </span>
            <span>{waiting.detail}</span>
            <span className="muted small">
              waiting{" "}
              {formatAgeParts(
                waitingAgeSeconds(waiting.since, capacity.generated_at),
              )}
            </span>
          </li>
        ))}
      </ul>
      {capacity.waiting.length > 5 ? (
        <p className="muted small">
          Showing the first 5 of {capacity.waiting.length} waiting jobs.
        </p>
      ) : null}
    </Panel>
  );
}
