import type { CapacityView } from "../../bridge/wire";
import { Badge, Gauge, Panel } from "../../components";
import { cpuStateCopy, resourceTone } from "../../domain/capacity";
import { formatPercent, formatPercentOne } from "../../domain/format";

export function CpuPanel({ capacity }: { capacity: CapacityView }) {
  const samplePresent = capacity.host !== null;
  const cpu = capacity.cpu;
  const state = samplePresent ? cpu.state : "unknown";
  const max = Math.max(cpu.cpu_count, 1);
  const percent = formatPercent(Math.min(1, Math.max(0, cpu.load_ratio)) * 100);

  return (
    <Panel
      title="CPU"
      actions={<Badge tone={resourceTone(state)}>{state}</Badge>}
      className="cpu-panel"
      data-testid="cpu-panel"
      data-state={state}
    >
      <Gauge
        label="CPU load"
        value={samplePresent ? cpu.load1 : 0}
        max={max}
        valueLabel={
          samplePresent
            ? `${cpu.load1.toFixed(2)} / ${cpu.cpu_count} cores (${percent})`
            : "Unknown"
        }
        state={state}
        ariaLabel={`CPU load, ${
          samplePresent ? cpu.load1.toFixed(2) : "unknown"
        } on ${cpu.cpu_count} cores`}
        detail={
          <span className="muted small">
            Pressure CPU avg10{" "}
            {cpu.pressure_avg10 === null
              ? "not reported"
              : formatPercentOne(cpu.pressure_avg10)}
          </span>
        }
      />
      {capacity.host?.cpu_quota_cores ? (
        <p className="muted small">
          quota {capacity.host.cpu_quota_cores.toFixed(1)} cores
        </p>
      ) : null}
      <p className="muted small">{cpuStateCopy(state)}</p>
    </Panel>
  );
}
