import { renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { CapacityView, HostSampleView } from "../../bridge/wire";
import { cpuDisplay, useServerHealth } from "../../next/health";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";

const GIB = 1024 ** 3;

function sample(overrides: Partial<HostSampleView> = {}): HostSampleView {
  return {
    at: "2026-11-21T10:00:00.000Z",
    interval_secs: 10,
    mem_total_bytes: 16 * GIB,
    mem_available_bytes: 12 * GIB,
    swap_used_bytes: 0,
    load1: 2.1,
    load5: 1.8,
    load15: 1.5,
    cpu_count: 8,
    cpu_quota_cores: null,
    disk_free_home_bytes: 80 * GIB,
    disk_free_worktrees_bytes: null,
    pressure_cpu_avg10: null,
    pressure_mem_avg10: null,
    pressure_io_avg10: null,
    source: "capacity_report",
    ...overrides,
  };
}

function capacityWith(host: HostSampleView | null): CapacityView {
  // The Rust projection reports zeros for an absent sample; reproduce that.
  return {
    host,
    target: { target_workers: 2, advised_workers: 2 },
    disk: { free_home_bytes: host?.disk_free_home_bytes ?? 0, floor_bytes: 10 * GIB },
    cpu: {
      load1: host?.load1 ?? 0,
      cpu_count: host?.cpu_count ?? 0,
      load_ratio: host && host.cpu_count > 0 ? host.load1 / host.cpu_count : 0,
      pressure_avg10: host?.pressure_cpu_avg10 ?? null,
      state: host ? "ok" : "unknown",
    },
    brake: { active: false, reasons: [], since: null },
    advised_workers: 2,
  } as unknown as CapacityView;
}

function health(host: HostSampleView | null) {
  useFleetStore.setState({ capacity: capacityWith(host), fleet: null });
  return renderHook(() => useServerHealth()).result.current;
}

afterEach(() => resetFleetStore());

describe("CPU load display", () => {
  it("shows a dash, not 0%, before any host sample arrives", () => {
    const result = health(null);
    expect(result.cpuRatio).toBeNull();
    expect(result.cpuText).toBe("—");
    expect(result.ramText).toBe("—");
    expect(result.diskText).toBe("unknown");
    expect(result.diskShort).toBe("disk unknown");
    expect(result.cpuOver).toBe(false);
    expect(result.load).toBeNull();
  });

  it("shows load / cores plus RAM and disk for a normal sample", () => {
    const result = health(sample());
    expect(result.cpuText).toBe("load 2.1 / 8 cores");
    expect(result.cpuRatio).toBeCloseTo(2.1 / 8);
    expect(result.cpuOver).toBe(false);
    expect(result.ramText).toContain("of");
    expect(result.diskText).toContain("free");
  });

  it("appends CPU pressure when reported", () => {
    expect(cpuDisplay(sample({ pressure_cpu_avg10: 12.4 })).text).toBe(
      "load 2.1 / 8 cores · 12% CPU pressure",
    );
  });

  it("keeps the real load when load1 exceeds cpu_count; only the bar clamps", () => {
    const result = health(sample({ load1: 10.3 }));
    expect(result.cpuText).toBe("load 10.3 / 8 cores");
    expect(result.cpuRatio).toBe(1);
    expect(result.cpuOver).toBe(true);
    expect(cpuDisplay(sample({ load1: 10.3 })).over).toBe(true);
  });

  it("treats a zero core count as unknown", () => {
    expect(cpuDisplay(sample({ cpu_count: 0 })).ratio).toBeNull();
  });
});
