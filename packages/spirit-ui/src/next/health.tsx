import { formatGiB } from "../domain/format";
import { useCapacity, useFleet } from "../state/fleetStore";
import { activeRunningWorkers, meterTone, usedRatio } from "./summary";

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
  /** True when load1 is above cpu_count, so the clamped ratio hides the real figure. */
  cpuOver: boolean;
  /** The most pressing of RAM and CPU, so one number can stand for "load". */
  load: { ratio: number; source: "RAM" | "CPU" } | null;
  /** Short disk number for compact places, e.g. "42 GiB free". */
  diskShort: string;
  brake: boolean;
  brakeReasons: string[];
  stale: boolean;
}

/** The slice of a host sample the CPU line needs. */
export interface CpuSample {
  load1: number;
  cpu_count: number;
  pressure_cpu_avg10?: number | null;
}

/**
 * CPU numbers for display. `ratio` is Linux PSI CPU pressure (avg10 / 100) when present,
 * else the 1-minute load average divided by cores, clamped to 1 for the bar only; `text` always keeps the real figures, so
 * `10.3 / 8 cores` is never shown as a flat 100%. No sample (or no core count) gives
 * `null` and `—`, never a made-up `0%`.
 */
export function cpuDisplay(sample: CpuSample | null | undefined): {
  ratio: number | null;
  text: string;
  over: boolean;
} {
  if (!sample || !(sample.cpu_count > 0) || !Number.isFinite(sample.load1)) {
    return { ratio: null, text: "—", over: false };
  }
  const raw = sample.load1 / sample.cpu_count;
  const pressure = sample.pressure_cpu_avg10;
  const load = `load ${sample.load1.toFixed(1)} / ${sample.cpu_count} cores`;
  if (pressure != null && Number.isFinite(pressure)) {
    // Real stall time (PSI) drives the bar: load1 counts runnable threads, so parallel
    // builds can read 100% load while the CPU is barely stalled.
    return {
      ratio: Math.min(1, Math.max(0, pressure / 100)),
      text: `${Math.round(pressure)}% pressure · ${load}`,
      over: raw > 1,
    };
  }
  return { ratio: Math.min(1, Math.max(0, raw)), text: load, over: raw > 1 };
}

export function useServerHealth(): ServerHealth {
  const fleet = useFleet();
  const capacity = useCapacity() ?? fleet?.capacity ?? null;
  const sample = capacity?.host ?? null;
  const running = activeRunningWorkers(fleet).length;
  const target = capacity?.target.target_workers ?? null;
  // `capacity.disk` is zero-filled when there is no host sample, so the sample is the
  // only honest source here; a missing sample gives "unknown", never "0 B free".
  const diskFree = sample?.disk_free_home_bytes ?? null;
  const ramRatio = usedRatio(sample?.mem_available_bytes, sample?.mem_total_bytes);
  const cpuPressure = capacity?.cpu?.pressure_avg10 ?? null;
  const cpu = cpuDisplay(sample && sample.pressure_cpu_avg10 == null && cpuPressure != null ? { ...sample, pressure_cpu_avg10: cpuPressure } : sample);
  const cpuRatio = cpu.ratio;
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
      : "—",
    diskText: diskFree != null ? `${formatGiB(diskFree)} free` : "unknown",
    diskFloorText: capacity ? `keeps ${formatGiB(capacity.disk.floor_bytes)} free` : null,
    cpuRatio,
    cpuText: cpu.text,
    cpuOver: cpu.over,
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
