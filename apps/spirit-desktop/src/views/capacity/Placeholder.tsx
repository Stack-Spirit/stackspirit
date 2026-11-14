import { Badge, EmptyState, Gauge, Panel } from "../../components";
import { formatBytes, formatClock } from "../../domain/format";
import {
  useCapacity,
  useDataDimmed,
  useFleet,
  useProjectFilter,
} from "../../state/fleetStore";

export function CapacityPlaceholder({ onOpenHosts }: { onOpenHosts?: () => void }) {
  const capacity = useCapacity();
  const fleet = useFleet();
  const dimmed = useDataDimmed();
  const projectFilter = useProjectFilter();

  if (!capacity || !fleet) {
    return (
      <div className="view-stack" data-testid="capacity-rail">
        <Panel title="Capacity & VPS Storage">
          <EmptyState
            icon="▥"
            title="Capacity unavailable"
            body="Connect a verified VOWP host to read disk, RAM and CPU headroom."
            actionLabel={onOpenHosts ? "Open Hosts" : undefined}
            onAction={onOpenHosts}
          />
          <p className="muted small">
            This rail is a D2 placeholder; the full Capacity & Storage rail arrives with
            the Fleet Canvas job (D3).
          </p>
        </Panel>
      </div>
    );
  }

  const brakeTone = capacity.brake.active
    ? capacity.brake.reasons.some((reason) => reason.code === "disk_low")
      ? "danger"
      : "warn"
    : "ok";
  const diskMax = Math.max(capacity.disk.floor_bytes * 3, capacity.disk.free_home_bytes, 1);
  const memTotal = capacity.host?.mem_total_bytes ?? Math.max(capacity.ram.available_bytes, 1);
  const cpuMax = Math.max(capacity.cpu.cpu_count, 1);

  return (
    <div className={dimmed ? "view-stack data-dimmed" : "view-stack"} data-testid="capacity-rail">
      <Panel title="Capacity & VPS Storage" actions={<Badge tone="unknown">D2</Badge>}>
        <div className="capacity-source">
          <span className="muted small">
            {capacity.report_loaded
              ? `Capacity data from VOWP · ${formatClock(capacity.generated_at)}`
              : "Capacity detail unavailable — retrying."}
          </span>
          {projectFilter ? <Badge tone="info">node-wide</Badge> : null}
        </div>
      </Panel>

      <Panel
        title="Brake"
        tone={brakeTone === "danger" ? "danger" : brakeTone === "warn" ? "warn" : "default"}
      >
        {capacity.brake.active ? (
          <>
            <p className="brake-title">
              <Badge tone={brakeTone === "danger" ? "danger" : "warn"}>brake active</Badge>{" "}
              since {formatClock(capacity.brake.since)}
            </p>
            <ul className="reason-list">
              {capacity.brake.reasons.map((reason) => (
                <li key={`${reason.code}-${reason.detail}`}>
                  <span className="mono">{reason.code}</span>
                  <span className="muted">{reason.detail}</span>
                </li>
              ))}
            </ul>
          </>
        ) : (
          <p className="muted small">No brake active.</p>
        )}
      </Panel>

      <Panel title="Target advisor">
        <div className="target-row">
          <div>
            <span className="eyebrow">Worker target</span>
            <strong>{capacity.target.target_workers} workers</strong>
          </div>
          <Badge tone={capacity.target.is_override ? "warn" : "ok"}>
            {capacity.target.is_override ? "Owner override" : capacity.target.mode}
          </Badge>
        </div>
        <p className="muted small">
          VOWP advises {capacity.advised_workers ?? "no advice yet"}. VOWP still applies
          brakes and provider limits.
        </p>
      </Panel>

      <Panel title="Disk">
        <Gauge
          label="Home disk free"
          value={capacity.disk.free_home_bytes}
          max={diskMax}
          marker={capacity.disk.floor_bytes}
          valueLabel={formatBytes(capacity.disk.free_home_bytes)}
          state={capacity.disk.state}
          ariaLabel={`Home disk free, ${formatBytes(capacity.disk.free_home_bytes)}, floor ${formatBytes(
            capacity.disk.floor_bytes,
          )}`}
          detail={
            <span className="muted small">
              floor {formatBytes(capacity.disk.floor_bytes)} · source {capacity.disk.source}
            </span>
          }
        />
        {capacity.disk.free_worktrees_bytes !== null ? (
          <p className="muted small">
            Worktrees: {formatBytes(capacity.disk.free_worktrees_bytes)} free
          </p>
        ) : null}
      </Panel>

      <Panel title="Memory">
        <Gauge
          label="Memory available"
          value={capacity.ram.available_bytes}
          max={memTotal}
          marker={capacity.ram.reserve_bytes}
          valueLabel={formatBytes(capacity.ram.available_bytes)}
          state={capacity.ram.state}
          ariaLabel={`Memory available, ${formatBytes(
            capacity.ram.available_bytes,
          )}, reserve ${formatBytes(capacity.ram.reserve_bytes)}`}
          detail={
            <span className="muted small">
              reserve {formatBytes(capacity.ram.reserve_bytes)} · running p90{" "}
              {formatBytes(capacity.ram.running_p90_bytes)}
            </span>
          }
        />
        <p className="muted small">
          {capacity.ram.per_worker_p90_bytes
            ? `Per-worker p90 ${formatBytes(
                capacity.ram.per_worker_p90_bytes,
              )} (${capacity.ram.per_worker_source ?? "unknown"}, ${
                capacity.ram.per_worker_samples ?? 0
              } samples)`
            : "Not enough samples for a per-worker estimate."}
        </p>
      </Panel>

      <Panel title="CPU">
        <Gauge
          label="CPU load"
          value={capacity.cpu.load1}
          max={cpuMax}
          valueLabel={`${capacity.cpu.load1.toFixed(2)} / ${capacity.cpu.cpu_count} cores`}
          state={capacity.cpu.state}
          ariaLabel={`CPU load, ${capacity.cpu.load1.toFixed(2)} on ${
            capacity.cpu.cpu_count
          } cores`}
          detail={
            <span className="muted small">
              Pressure CPU avg10{" "}
              {capacity.cpu.pressure_avg10 === null
                ? "not reported"
                : `${capacity.cpu.pressure_avg10.toFixed(1)}%`}
            </span>
          }
        />
      </Panel>

      {capacity.waiting.length > 0 ? (
        <Panel title="Waiting reasons">
          <ul className="reason-list">
            {capacity.waiting.slice(0, 5).map((waiting) => (
              <li key={`${waiting.project}-${waiting.job}`}>
                <span className="mono">
                  {waiting.project} · {waiting.job}
                </span>
                <span className="muted">{waiting.detail}</span>
              </li>
            ))}
          </ul>
        </Panel>
      ) : null}

      {capacity.limits.length > 0 ? (
        <Panel title="Provider limits">
          <ul className="reason-list">
            {capacity.limits.map((limit) => (
              <li key={`${limit.provider}-${limit.kind}`}>
                <Badge tone="warn">{limit.kind}</Badge>
                <span>
                  {limit.provider}
                  {limit.harness ? ` · ${limit.harness}` : ""}
                </span>
                <span className="muted small">
                  {limit.resets_at ? `resets ${formatClock(limit.resets_at)}` : "needs a human"}
                </span>
              </li>
            ))}
          </ul>
        </Panel>
      ) : null}
    </div>
  );
}
