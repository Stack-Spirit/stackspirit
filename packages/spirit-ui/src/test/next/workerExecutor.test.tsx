import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import {
  contextPercent,
  ContextPanel,
  costLabel,
  formatTokens,
  tokenSummary,
} from "../../next/ContextPanel";
import { useShellUi } from "../../next/shellUi";
import { resetFleetStore } from "../../state/fleetStore";
import type { JobCardView } from "../../bridge/wire";
import { seedStore } from "../render";

async function seededBusy() {
  const bridge = new MockBridge("busy");
  const host = (await bridge.hosts.list())[0];
  if (host) await bridge.connection.connect(host.id);
  const fleet = await bridge.fleet.refresh();
  seedStore(fleet, { bridge });
  return { bridge, fleet };
}

function openJob(jobId: string) {
  useShellUi.getState().openPanel({ kind: "job", project: "vowp", jobId });
  render(
    <ContextPanel
      project="vowp"
      onOpenServer={() => {}}
      onOpenWorkers={() => {}}
      onOpenSession={() => {}}
    />,
  );
}

afterEach(() => {
  resetFleetStore();
  useShellUi.getState().closePanel();
});

const card = (extra: Partial<JobCardView>): JobCardView => ({
  job_id: "x",
  project: "vowp",
  title: null,
  kind: "build",
  role: "worker",
  state: "active",
  priority: 0,
  age_secs: 1,
  reason: null,
  detail: null,
  gate: null,
  gate_at: null,
  gate_evidence: null,
  executor: null,
  session_id: null,
  ...extra,
});

describe("usage formatting", () => {
  it("formats tokens compactly", () => {
    expect(formatTokens(842)).toBe("842");
    expect(formatTokens(142_000)).toBe("142K");
    expect(formatTokens(1_250_000)).toBe("1.3M");
  });

  it("shows API dollars only for api billing, never for a subscription", () => {
    expect(costLabel(card({ billing_mode: "api", cost_usd: 0.14 }))).toBe("$0.14");
    expect(costLabel(card({ billing_mode: "api", cost_usd: null }))).toBeNull();
    expect(costLabel(card({ billing_mode: "subscription", cost_usd: 3.2 }))).toBe("Subscription");
    expect(costLabel(card({ billing_mode: "free" }))).toBe("Free");
    expect(costLabel(card({}))).toBeNull();
  });

  it("computes the context ring and token summary", () => {
    const job = card({
      tokens_input: 100_000,
      tokens_output: 42_000,
      tokens_cache_read: 9_000,
      context_window: 200_000,
    });
    expect(contextPercent(job)).toBe(71);
    expect(tokenSummary(job)).toBe("142K tok · 9K cached");
    expect(contextPercent(card({ tokens_input: 5 }))).toBeNull();
  });
});

describe("worker executor details", () => {
  it("shows the planned provider, harness and model on a queued job", async () => {
    await seededBusy();
    openJob("ss-5-mock");
    const panel = screen.getByTestId("details-panel");
    expect(within(panel).getByText("Planned provider")).toBeTruthy();
    expect(within(panel).getByText("Planned harness")).toBeTruthy();
    expect(within(panel).getByText("Planned model")).toBeTruthy();
    expect(within(panel).getByText(/Claude Sonnet 5.5 · medium/)).toBeTruthy();
  });

  it("shows API cost, tokens and the context ring on a running job", async () => {
    await seededBusy();
    openJob("ss-2-desktop-shell");
    expect(screen.getByTestId("job-cost").textContent).toBe("$0.14");
    expect(screen.getByTestId("job-tokens").textContent).toBe("142K tok · 96K cached");
    expect(screen.getByRole("img", { name: "Context window 71% used" })).toBeTruthy();
  });

  it("applies a queued model change immediately through the cascade", async () => {
    const { bridge } = await seededBusy();
    const user = userEvent.setup();
    openJob("ss-5-mock");
    await user.click(screen.getByRole("button", { name: "Change model" }));
    const form = screen.getByRole("form", { name: "Change model" });
    await user.selectOptions(within(form).getByLabelText("Harness"), "opencode");
    await user.selectOptions(within(form).getByLabelText("Provider"), "deepseek");
    await user.selectOptions(within(form).getByLabelText("Model"), "deepseek/deepseek-flash");
    await user.selectOptions(within(form).getByLabelText("Reasoning"), "max");
    await user.click(within(form).getByRole("button", { name: "Apply" }));
    await waitFor(() =>
      expect(bridge.executorChanges).toEqual([
        { project: "vowp", jobId: "ss-5-mock", executor: "opencode:deepseek/deepseek-flash#max" },
      ]),
    );
    expect((await screen.findByTestId("change-model-result")).textContent).toContain("Planned model changed");
  });

  it("hides Reasoning for a model without it", async () => {
    await seededBusy();
    const user = userEvent.setup();
    openJob("ss-5-mock");
    await user.click(screen.getByRole("button", { name: "Change model" }));
    const form = screen.getByRole("form", { name: "Change model" });
    await user.selectOptions(within(form).getByLabelText("Harness"), "claude-code");
    await user.selectOptions(within(form).getByLabelText("Model"), "claude-haiku-5-5");
    expect(within(form).queryByLabelText("Reasoning")).toBeNull();
  });

  it("asks for confirmation before switching a running worker", async () => {
    const { bridge } = await seededBusy();
    const user = userEvent.setup();
    openJob("ss-2-desktop-shell");
    await user.click(screen.getByRole("button", { name: "Change model" }));
    const form = screen.getByRole("form", { name: "Change model" });
    await user.click(within(form).getByRole("button", { name: "Review switch" }));
    expect(bridge.executorChanges).toEqual([]);
    expect(screen.getByRole("alert").textContent).toContain(
      "Switch this running worker via universal session handoff? It will drain the current step, save a capsule, and resume on",
    );
    fireEvent.click(screen.getByRole("button", { name: "Switch worker" }));
    await waitFor(() => expect(bridge.executorChanges).toHaveLength(1));
    expect(bridge.executorChanges[0]?.jobId).toBe("ss-2-desktop-shell");
  });
});
