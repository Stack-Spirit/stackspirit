import type { CapacityView, WorkerCardView } from "../../bridge/wire";
import { Badge, Gauge, Panel } from "../../components";
import {
  CAPACITY_DETAIL_UNAVAILABLE,
  ramStateCopy,
  railEstimateQuery,
  ramWithEstimate,
  resourceTone,
} from "../../domain/capacity";
import { formatExactBytes, formatGiB } from "../../domain/format";

export function RamPanel({
  capacity,
  running,
}: {
  capacity: CapacityView;
  running: readonly WorkerCardView[];
}) {
  const samplePresent = capacity.host !== null;
  const state = samplePresent ? capacity.ram.state : "unknown";
  const ram = ramWithEstimate(
    capacity.ram,
    capacity.estimates,
    railEstimateQuery(running),
  );
  const max = Math.max(
    capacity.host?.mem_total_bytes ?? 0,
    ram.available_bytes + ram.reserve_bytes,
    ram.reserve_bytes * 2,
    1,
  );
  const sourceLabel = ram.per_worker_source ?? null;

  return (
    <Panel
      title="Memory"
      actions={<Badge tone={resourceTone(state)}>{state}</Badge>}
      className="ram-panel"
      data-testid="ram-panel"
      data-state={state}
      data-estimate-source={sourceLabel ?? "none"}
    >
      <Gauge
        label="Memory available"
        value={samplePresent ? ram.available_bytes : 0}
        max={max}
        marker={ram.reserve_bytes}
        valueLabel={samplePresent ? `${formatGiB(ram.available_bytes)} available` : "Unknown"}
        state={state}
        ariaLabel={`Memory available, ${
          samplePresent ? formatGiB(ram.available_bytes) : "unknown"
        }, reserve ${formatGiB(ram.reserve_bytes)}`}
        detail={
          <span
            className="muted small"
            title={
              samplePresent
                ? `${formatExactBytes(ram.available_bytes)} available; reserve ${formatExactBytes(
                    ram.reserve_bytes,
                  )}`
                : "No host sample in this snapshot."
            }
          >
            Reserve {formatGiB(ram.reserve_bytes)} · running p90{" "}
            {formatGiB(ram.running_p90_bytes)}
          </span>
        }
      />

      {!capacity.report_loaded ? (
        <p className="muted small" data-testid="ram-detail-unavailable">
          {CAPACITY_DETAIL_UNAVAILABLE}
        </p>
      ) : ram.per_worker_p90_bytes === null ? (
        <p className="muted small" data-testid="ram-no-estimate">
          Not enough samples for a per-worker estimate.
        </p>
      ) : (
        <div className="ram-panel__estimate" data-testid="ram-estimate">
          <p className="small">
            Per-worker p90 {formatGiB(ram.per_worker_p90_bytes)} (
            {ram.per_worker_source ?? "unknown"}, {ram.per_worker_samples ?? 0} samples)
            {ram.per_worker_source === "default" ? (
              <Badge tone="unknown">default estimate</Badge>
            ) : null}
          </p>
          {ram.estimated_extra_slots === null ? null : ram.estimated_extra_slots > 0 ? (
            <p className="small ram-panel__slots" data-testid="ram-extra-slots">
              ~{ram.estimated_extra_slots} more by RAM · target-limited
            </p>
          ) : (
            <p className="small muted" data-testid="ram-extra-slots">
              No extra slots by RAM.
            </p>
          )}
        </div>
      )}

      <p className="muted small">{ramStateCopy(state)}</p>
    </Panel>
  );
}
