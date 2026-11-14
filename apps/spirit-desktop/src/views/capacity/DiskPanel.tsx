import type { CapacityView } from "../../bridge/wire";
import { Badge, Gauge, Panel } from "../../components";
import { diskStateCopy, resourceTone } from "../../domain/capacity";
import { formatExactBytes, formatGiB } from "../../domain/format";

export function DiskPanel({ capacity }: { capacity: CapacityView }) {
  const disk = capacity.disk;
  const samplePresent = capacity.host !== null;
  const state = samplePresent ? disk.state : "unknown";
  const max = Math.max(disk.floor_bytes * 3, disk.free_home_bytes, 1);
  const sourceLabel = !samplePresent
    ? "no sample"
    : disk.source === "capacity_report"
      ? "capacity report"
      : disk.source === "snapshot"
        ? "snapshot"
        : "no sample";

  return (
    <Panel
      title="Home disk"
      actions={<Badge tone={resourceTone(state)}>{sourceLabel}</Badge>}
      className="disk-panel"
      data-testid="disk-panel"
      data-state={state}
      data-source={disk.source}
    >
      <Gauge
        label="Home disk free"
        value={samplePresent ? disk.free_home_bytes : 0}
        max={max}
        marker={disk.floor_bytes}
        valueLabel={samplePresent ? `${formatGiB(disk.free_home_bytes)} free` : "Unknown"}
        state={state}
        ariaLabel={`Home disk free, ${
          samplePresent ? formatGiB(disk.free_home_bytes) : "unknown"
        }, floor ${formatGiB(disk.floor_bytes)}`}
        detail={
          <span
            className="muted small"
            title={
              samplePresent
                ? `${formatExactBytes(disk.free_home_bytes)} free; floor ${formatExactBytes(
                    disk.floor_bytes,
                  )}`
                : "No host sample in this snapshot."
            }
          >
            floor {formatGiB(disk.floor_bytes)} · source {sourceLabel}
          </span>
        }
      />
      {samplePresent &&
      disk.free_worktrees_bytes !== null &&
      disk.free_worktrees_bytes !== disk.free_home_bytes ? (
        <Gauge
          label="Worktrees free"
          value={disk.free_worktrees_bytes}
          max={Math.max(disk.free_worktrees_bytes, disk.floor_bytes * 3, 1)}
          marker={disk.floor_bytes}
          valueLabel={`${formatGiB(disk.free_worktrees_bytes)} free`}
          state={state}
          ariaLabel={`Worktrees free, ${formatGiB(disk.free_worktrees_bytes)}`}
          detail={
            <span className="muted small" title={formatExactBytes(disk.free_worktrees_bytes)}>
              separate worktree filesystem
            </span>
          }
        />
      ) : null}
      <p className="muted small">{diskStateCopy(state)}</p>
    </Panel>
  );
}
