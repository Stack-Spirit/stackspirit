import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import type { EngineStatus, ModelChoice } from "../../bridge/wire";
import { engineWithoutModel, renderEngine } from "./helpers";

const NOW = "2026-11-14T12:00:00.000Z";

interface BundledCatalogEntry {
  id: string;
  label: string;
  provider: string;
  context_tokens: number | null;
  notes: string;
}

function bundledModels(): ModelChoice[] {
  const source = readFileSync(
    resolve(process.cwd(), "../../engine/models.json"),
    "utf8",
  );
  const catalog = JSON.parse(source) as { entries: BundledCatalogEntry[] };
  return catalog.entries.map((entry) => ({
    ...entry,
    access: "included",
    verified_at: NOW,
  }));
}

describe("StackSpirit Engine first-run flow", () => {
  it("first_run_does_not_require_opencode_branding", async () => {
    const user = userEvent.setup();
    const bridge = new MockBridge({ scenario: "engine-ready" });
    const models = bundledModels();
    vi.spyOn(bridge.engine, "models").mockResolvedValue(models);
    renderEngine(engineWithoutModel(), { bridge });

    expect(screen.getAllByText("StackSpirit Engine").length).toBeGreaterThan(0);
    expect(screen.queryByText(/opencode/i)).not.toBeInTheDocument();
    expect(screen.getByText("StackSpirit Engine check")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(
      await screen.findByRole("heading", { name: "Choose models" }),
    ).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getAllByText("Included · no key").length).toBe(models.length);
    });
    expect(screen.queryByText(/opencode/i)).not.toBeInTheDocument();
    expect(screen.getAllByText("StackSpirit Engine").length).toBeGreaterThan(0);

    await user.click(screen.getByRole("button", { name: "Advanced details" }));
    expect(
      screen.getByText(/Engine implementation: opencode/),
    ).toBeInTheDocument();
  });

  it("a_saved_byok_setup_skips_the_first_run_flow_after_relaunch", () => {
    const status = {
      ...engineWithoutModel(),
      onboarding_complete: true,
    } as EngineStatus;
    renderEngine(status);

    expect(screen.queryByTestId("engine-onboarding")).not.toBeInTheDocument();
    expect(screen.getByTestId("engine-status-card")).toBeInTheDocument();
  });

  it("first_run_selects_an_included_model_and_finishes", async () => {
    const user = userEvent.setup();
    const { bridge } = renderEngine(engineWithoutModel());
    const selectModel = vi.spyOn(bridge.engine, "selectModel");

    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(
      await screen.findByRole("heading", { name: "Choose models" }),
    ).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getAllByText("Included · no key").length).toBe(2);
    });

    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByRole("heading", { name: "Finish" })).toBeInTheDocument();
    expect(screen.getByText(/Model selected · DeepSeek V4.1 Flash/)).toBeInTheDocument();
    expect(selectModel).toHaveBeenCalledWith("deepseek/deepseek-flash");

    await user.click(screen.getByRole("button", { name: "Finish setup" }));
    await waitFor(() => {
      expect(screen.queryByTestId("engine-onboarding")).not.toBeInTheDocument();
    });
    expect(await screen.findByTestId("engine-status-card")).toBeInTheDocument();
  });

  it("no_included_models_shows_byok_empty_state", async () => {
    const user = userEvent.setup();
    const bridge = new MockBridge({ scenario: "engine-ready" });
    vi.spyOn(bridge.engine, "models").mockResolvedValue([]);
    renderEngine(engineWithoutModel(), { bridge });

    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(
      await screen.findByText(/no included models available/),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "Add your own provider key" }),
    );
    expect(screen.getByLabelText("API key")).toBeInTheDocument();
  });

  it("external_selection_never_switches_back_without_an_explicit_action", async () => {
    const user = userEvent.setup();
    const bridge = new MockBridge({ scenario: "engine-ready" });
    vi.spyOn(bridge.engine, "scanExternal").mockResolvedValue([
      {
        kind: "opencode",
        path: "/opt/example/bin/opencode",
        version: "1.19.5",
        usable: true,
      },
    ]);
    const useBundled = vi.spyOn(bridge.engine, "useBundled");
    renderEngine(engineWithoutModel(), { bridge });

    await user.click(screen.getByRole("button", { name: "Continue" }));
    await user.click(screen.getByRole("radio", { name: /Use an existing harness/ }));
    await user.click(screen.getByRole("button", { name: "Scan PATH" }));
    expect(await screen.findByText("/opt/example/bin/opencode")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Use this installation" }));

    expect(useBundled).not.toHaveBeenCalled();
    expect(
      await screen.findByRole("button", { name: "Use bundled engine" }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByText(/External harness selected/)).toBeInTheDocument();
    expect(useBundled).not.toHaveBeenCalled();
  });
});
