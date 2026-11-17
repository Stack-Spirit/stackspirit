import { formatGiB } from "../domain/format";
import { useCapacity, useFleet } from "../state/fleetStore";
import { meterTone, usedRatio } from "./summary";

/** The server numbers the sidebar card, details panel and server page share. */
export interface ServerHealth {
  running: number;
  target: number | null;
  advised: number | null;
  slots: number;
  ramRatio: number | null;
  ramText: string;
  diskText: string;
  diskFloorText: string | null;
  cpuRatio: number | null;
  cpuText: string;
  /** The most pressing of RAM and CPU, so one number can stand for "load". */
  load: { ratio: number; source: "RAM" | "CPU" } | null;
  /** Short disk number for compact places, e.g. "42 GiB free". */
  diskShort: string;
  brake: boolean;
  brakeReasons: string[];
  stale: boolean;
}

export function useServerHealth(): ServerHealth {
  const fleet = useFleet();
  const capacity = useCapacity() ?? fleet?.capacity ?? null;
  const sample = capacity?.host ?? null;
  const running = fleet?.running.length ?? 0;
  const target = capacity?.target.target_workers ?? null;
  const diskFree = capacity?.disk.free_home_bytes ?? sample?.disk_free_home_bytes ?? null;
  const ramRatio = usedRatio(sample?.mem_available_bytes, sample?.mem_total_bytes);
  const cpuRatio = capacity ? Math.min(1, capacity.cpu.load_ratio) : null;
  const load =
    ramRatio == null && cpuRatio == null
      ? null
      : (cpuRatio ?? -1) > (ramRatio ?? -1)
        ? { ratio: cpuRatio ?? 0, source: "CPU" as const }
        : { ratio: ramRatio ?? 0, source: "RAM" as const };
  return {
    running,
    target,
    advised: capacity?.advised_workers ?? null,
    slots: Math.max(target ?? 0, running),
    load,
    diskShort: diskFree != null ? `${Math.round(diskFree / 1024 ** 3)} GiB free` : "disk unknown",
    ramRatio,
    ramText: sample
      ? `${formatGiB(sample.mem_total_bytes - sample.mem_available_bytes)} of ${formatGiB(sample.mem_total_bytes)}`
      : "unknown",
    diskText: diskFree != null ? `${formatGiB(diskFree)} free` : "unknown",
    diskFloorText: capacity ? `keeps ${formatGiB(capacity.disk.floor_bytes)} free` : null,
    cpuRatio,
    cpuText: capacity ? `${capacity.cpu.load1.toFixed(1)} on ${capacity.cpu.cpu_count} cores` : "unknown",
    brake: capacity?.brake.active ?? false,
    brakeReasons: capacity?.brake.reasons.map((reason) => reason.detail || reason.code) ?? [],
    stale: fleet?.stale ?? false,
  };
}

export function Meter({
  label,
  ratio,
  value,
  compact = false,
}: {
  label: string;
  ratio: number | null;
  value: string;
  compact?: boolean;
}) {
  const tone = meterTone(ratio);
  return (
    <div className={`nx-meter${compact ? " nx-meter--compact" : ""}`}>
      <div className="nx-meter__label">
        <span>{label}</span>
        <span>{value}</span>
      </div>
      {ratio != null ? (
        <div
          className="nx-meter__track"
          role="meter"
          aria-label={label}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(ratio * 100)}
        >
          <div
            className={`nx-meter__fill${tone ? ` nx-meter__fill--${tone}` : ""}`}
            style={{ width: `${Math.round(ratio * 100)}%` }}
          />
        </div>
      ) : null}
    </div>
  );
}

export function Slots({ running, slots }: { running: number; slots: number }) {
  if (slots <= 0) return null;
  return (
    <div className="nx-slots" aria-hidden="true">
      {Array.from({ length: slots }, (_, index) => (
        <span key={index} className={`nx-slot${index < running ? " nx-slot--on" : ""}`} />
      ))}
    </div>
  );
}
