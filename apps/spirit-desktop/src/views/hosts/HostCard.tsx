import { Badge, Button, Sparkline, Tooltip } from "../../components";
import { formatClock, formatDateTime, latencyLabel } from "../../domain/format";
import {
  connectionLabel,
  connectionTone,
  supervisorLabel,
  supervisorTone,
} from "../../domain/connection";
import type { ConnectionSnapshot, FleetView, HostProfile } from "../../bridge/wire";
import { ConnectionControl } from "./ConnectionControl";

export interface HostCardProps {
  host: HostProfile;
  connection: ConnectionSnapshot;
  fleet: FleetView | null;
  latencyHistory: number[];
  selected: boolean;
  compact?: boolean;
  testIdPrefix?: string;
  onSelect: () => void;
  onConnect: () => void;
  onDisconnect: () => void;
  onEdit?: () => void;
  onRepin?: () => void;
  onRemove?: () => void;
}

export function HostCard({
  host,
  connection,
  fleet,
  latencyHistory,
  selected,
  compact = false,
  testIdPrefix = "host-card",
  onSelect,
  onConnect,
  onDisconnect,
  onEdit,
  onRepin,
  onRemove,
}: HostCardProps) {
  const active = connection.host_id === host.id;
  const tone = active ? connectionTone(connection, fleet) : "unknown";
  const label = active ? connectionLabel(connection, fleet) : "Not connected";
  const latency = active ? connection.latency : null;

  return (
    <article
      className={`host-card${selected ? " host-card--selected" : ""}${
        compact ? " host-card--compact" : ""
      }`}
      data-testid={`${testIdPrefix}-${host.id}`}
      aria-label={`${host.label}, ${label}`}
    >
      <div className="host-card__top">
        <button type="button" className="host-card__select" onClick={onSelect}>
          <span className={`status-dot status-dot--${tone}`} data-tone={tone} aria-hidden="true" />
          <span className="host-card__label">{host.label}</span>
        </button>
        <Badge tone={active ? tone : "unknown"}>{label}</Badge>
        {!compact ? (
          <details className="host-card__menu">
            <summary aria-label={`More actions for ${host.label}`}>⋯</summary>
            <div className="host-card__menu-items">
              {onEdit ? (
                <Button variant="ghost" size="sm" onClick={onEdit}>
                  Edit
                </Button>
              ) : null}
              {onRepin ? (
                <Button variant="ghost" size="sm" onClick={onRepin}>
                  Re-pin key
                </Button>
              ) : null}
              {onRemove ? (
                <Button variant="danger" size="sm" onClick={onRemove}>
                  Remove
                </Button>
              ) : null}
            </div>
          </details>
        ) : null}
      </div>

      <div className="host-card__destination mono">
        {host.ssh.user}@{host.ssh.host}:{host.ssh.port}
      </div>

      {!compact ? (
        <dl className="host-card__meta">
          <div>
            <dt>Last connected</dt>
            <dd>{formatDateTime(host.last_connected_at)}</dd>
          </div>
          <div>
            <dt>Host key</dt>
            <dd className="mono" title={host.host_key.sha256}>
              {host.host_key.algorithm}
            </dd>
          </div>
          <div>
            <dt>VOWP home</dt>
            <dd className="mono">{host.vowp_home ?? "remote default"}</dd>
          </div>
        </dl>
      ) : null}

      {active ? (
        <div className="host-card__live">
          <div className="host-card__latency">
            <Tooltip
              content={`last ${latencyLabel(latency?.last_ms)} · p50 ${latencyLabel(
                latency?.p50_ms,
              )} · p95 ${latencyLabel(latency?.p95_ms)} · ${
                latency?.samples ?? 0
              } samples · updated ${formatClock(fleet?.fetched_at)}`}
            >
              <span className="mono">{latencyLabel(latency?.last_ms)}</span>
              <Sparkline
                samples={latencyHistory}
                p50={latency?.p50_ms}
                p95={latency?.p95_ms}
                label={`Latency for ${host.label}`}
              />
            </Tooltip>
          </div>
          <Badge tone={supervisorTone(fleet)} title={supervisorLabel(fleet)}>
            {supervisorLabel(fleet)}
          </Badge>
        </div>
      ) : null}

      {active && connection.state === "connected" ? (
        <div className="host-card__since">
          Connected since {formatClock(connection.since)}
        </div>
      ) : null}

      <div className="host-card__actions">
        <ConnectionControl
          state={active ? connection.state : "disconnected"}
          hostLabel={host.label}
          onConnect={onConnect}
          onDisconnect={onDisconnect}
        />
        {!selected ? (
          <Button variant="ghost" size="sm" onClick={onSelect}>
            Select
          </Button>
        ) : null}
      </div>
    </article>
  );
}
