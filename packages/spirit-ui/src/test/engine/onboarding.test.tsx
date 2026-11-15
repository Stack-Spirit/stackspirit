import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import type { EngineStatus, ModelChoice } from "../../bridge/wire";
import {
  engineSetupComplete,
  needsEngineOnboarding,
} from "../../domain/engine";
import { useFleetStore } from "../../state/fleetStore";
import { engineWith, engineWithoutModel, renderEngine } from "./helpers";

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

  it("a_saved_byok_setup_survives_engine_events", async () => {
    const saved = {
      ...engineWithoutModel(),
      onboarding_complete: true,
    } as EngineStatus;
    renderEngine(saved);

    await act(async () => {
      useFleetStore.getState().applyEvent({
        kind: "engine",
        revision: useFleetStore.getState().revision + 1,
        engine: { ...saved, onboarding_complete: false } as EngineStatus,
      });
    });

    const current = useFleetStore.getState().engine;
    expect(engineSetupComplete(current)).toBe(true);
    expect(needsEngineOnboarding(current, false)).toBe(false);
    expect(screen.queryByTestId("engine-onboarding")).not.toBeInTheDocument();
    expect(screen.getByTestId("engine-status-card")).toBeInTheDocument();
  });

  it("first_run_never_offers_a_byok_only_model_as_included", async () => {
    const user = userEvent.setup();
    const bridge = new MockBridge({ scenario: "engine-ready" });
    const [included, paid] = bundledModels();
    if (!included || !paid) throw new Error("the bundled catalog needs two models");
    vi.spyOn(bridge.engine, "models").mockResolvedValue([
      included,
      { ...paid, access: "byok" },
    ]);
    renderEngine(engineWithoutModel(), { bridge });

    await user.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(() => {
      expect(screen.getAllByText("Included · no key")).toHaveLength(1);
    });
    expect(screen.queryByText("Uses your provider key")).not.toBeInTheDocument();
    expect(
      screen.getByRole("radio", { name: new RegExp(included.label) }),
    ).toHaveAttribute("aria-label", `${included.label}, StackSpirit Engine`);
    expect(
      screen.queryByRole("radio", { name: new RegExp(paid.label) }),
    ).not.toBeInTheDocument();
  });

  it("a_paid_only_catalog_shows_the_byok_empty_state", async () => {
    const user = userEvent.setup();
    const bridge = new MockBridge({ scenario: "engine-ready" });
    const paid = bundledModels()[0];
    if (!paid) throw new Error("the bundled catalog needs a model");
    vi.spyOn(bridge.engine, "models").mockResolvedValue([
      { ...paid, access: "byok" },
    ]);
    renderEngine(engineWithoutModel(), { bridge });

    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(
      await screen.findByText(/no included models available/),
    ).toBeInTheDocument();
    expect(screen.queryByText("Included · no key")).not.toBeInTheDocument();
  });

  it("failure_screen_exposes_the_bounded_redacted_log_tail", async () => {
    const user = userEvent.setup();
    const status = {
      ...engineWithoutModel(),
      state: { state: "failed", detail: "StackSpirit Engine could not start." },
      logs: ["engine line 1", "engine line 2"],
    } as EngineStatus;
    renderEngine(status);

    await user.click(screen.getByRole("button", { name: "View logs" }));
    const logs = await screen.findByTestId("onboarding-engine-logs");
    expect(within(logs).getByText("engine line 1")).toBeInTheDocument();
    expect(within(logs).getByText("engine line 2")).toBeInTheDocument();
  });

  it("byok_provider_picker_uses_the_engine_reported_providers", async () => {
    const user = userEvent.setup();
    const bridge = new MockBridge({ scenario: "engine-ready" });
    const status = {
      ...engineWithoutModel(),
      providers: [
        { id: "groq", label: "Groq" },
        { id: "mistral", label: "Mistral" },
        { id: "opencode", label: "OpenCode" },
      ],
    } as EngineStatus;
    renderEngine(status, { bridge });

    await user.click(screen.getByRole("button", { name: "Continue" }));
    await user.click(screen.getByRole("radio", { name: /Use my own provider key/ }));
    expect(await screen.findByRole("option", { name: "Groq" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Mistral" })).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "OpenCode" })).not.toBeInTheDocument();
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

  it("engine_status_card_shows_the_bounded_redacted_log_tail", async () => {
    const user = userEvent.setup();
    const logs = Array.from({ length: 205 }, (_, index) => `engine line ${index + 1}`);
    const status = { ...engineWith(), logs } as EngineStatus;
    renderEngine(status);

    await user.click(screen.getByRole("button", { name: "View logs" }));
    const dialog = screen.getByRole("dialog", { name: "StackSpirit Engine logs" });
    expect(within(dialog).getByText("Last 200 redacted lines (maximum 200). Provider keys and full prompts are never written here.")).toBeInTheDocument();
    expect(within(dialog).getByText("engine line 205")).toBeInTheDocument();
    expect(within(dialog).queryByText("engine line 1")).not.toBeInTheDocument();
    expect(within(dialog).queryByText("engine line 5")).not.toBeInTheDocument();
  });

  it("an_external_harness_never_shows_a_stale_bundled_model", () => {
    const status = engineWith({
      choice: {
        type: "external",
        kind: "opencode",
        path: "/opt/homebrew/bin/opencode",
      },
      state: { state: "stopped" },
    });
    expect(status.selected_model?.label).toBe("DeepSeek V4.1 Flash");
    renderEngine(status);

    expect(screen.getByText("Managed by the external harness")).toBeInTheDocument();
    expect(screen.queryByText("DeepSeek V4.1 Flash")).not.toBeInTheDocument();
  });
});
