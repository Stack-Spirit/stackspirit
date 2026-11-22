import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import { harnessLabel, parseExecutor, executorProviderLabel } from "../../domain/executor";
import { ContextPanel } from "../../next/ContextPanel";
import { cpuDisplay, useServerHealth } from "../../next/health";
import { ServerPage } from "../../next/ServerPage";
import { useShellUi } from "../../next/shellUi";
import { providerModel } from "../../next/summary";
import { resetFleetStore } from "../../state/fleetStore";
import { seedStore } from "../render";
import { renderHook } from "@testing-library/react";

async function seededBusy() {
  const bridge = new MockBridge("busy");
  const host = (await bridge.hosts.list())[0];
  if (host) await bridge.connection.connect(host.id);
  const fleet = await bridge.fleet.refresh();
  seedStore(fleet, { bridge });
  return fleet;
}

afterEach(() => {
  resetFleetStore();
  useShellUi.getState().closePanel();
});

describe("CPU pressure drives the load bar", () => {
  const base = { load1: 9.7, cpu_count: 8 };

  it("shows 22% pressure as a calm bar with load as context, not a red 100%", () => {
    const cpu = cpuDisplay({ ...base, pressure_cpu_avg10: 22 });
    expect(cpu.ratio).toBeCloseTo(0.22);
    expect(cpu.text).toBe("22% pressure · load 9.7 / 8 cores");
    expect(cpu.over).toBe(true);
  });

  it("falls back to load1 / cpu_count without pressure", () => {
    expect(cpuDisplay(base).ratio).toBe(1);
    expect(cpuDisplay(base).text).toBe("load 9.7 / 8 cores");
  });

  it("clamps pressure to 0..1", () => {
    expect(cpuDisplay({ ...base, pressure_cpu_avg10: 140 }).ratio).toBe(1);
    expect(cpuDisplay({ ...base, pressure_cpu_avg10: -3 }).ratio).toBe(0);
  });

  it("feeds the sidebar load from pressure", async () => {
    await seededBusy();
    const { result } = renderHook(() => useServerHealth());
    expect(result.current.cpuRatio).toBeCloseTo(0.22);
    expect(result.current.cpuText).toBe("22% pressure · load 9.7 / 8 cores");
    expect(result.current.load?.ratio).toBeCloseTo(0.22);
  });
});

describe("executor labels", () => {
  it("names provider, harness and model", () => {
    expect(executorProviderLabel("claude")).toBe("Anthropic");
    expect(executorProviderLabel("deepseek")).toBe("DeepSeek");
    expect(executorProviderLabel("mistral")).toBe("Mistral");
    expect(harnessLabel("claude-code")).toBe("Claude Code");
    expect(harnessLabel("acp")).toBe("ACP");
    expect(harnessLabel("other")).toBe("other");
    const executor = parseExecutor("claude-code:claude-sonnet-5-5@medium");
    expect(executor.display).toBe("Claude Sonnet 5.5 · medium");
    expect(providerModel({ executor } as never)).toBe("Anthropic · Claude Code · Claude Sonnet 5.5 · medium");
  });
});

describe("details panel and structure", () => {
  it("ContextPanel shows Provider and Harness", async () => {
    const fleet = await seededBusy();
    const claude = fleet.running.find((worker) => worker.executor?.harness === "claude-code");
    expect(claude).toBeDefined();
    useShellUi.getState().openPanel({ kind: "job", project: claude!.project, jobId: claude!.job_id });
    render(<ContextPanel project={null} onOpenServer={() => {}} onOpenWorkers={() => {}} onOpenSession={() => {}} />);
    const panel = screen.getByTestId("details-panel");
    expect(panel).toHaveTextContent("ProviderAnthropic");
    expect(panel).toHaveTextContent("HarnessClaude Code");
  });

  it("Structure lists queued leaves", async () => {
    await seededBusy();
    render(
      <ServerPage
        serverLabel="vps"
        onOpenProject={() => {}}
        onFormCluster={() => {}}
        onPromote={() => {}}
      />,
    );
    const map = screen.getByRole("group", { name: "Server structure" });
    expect(map.textContent).toMatch(/queued · /);
  });
});
