import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import {
  engineWith,
  readyAsifGates,
  readyLocalGates,
  renderEngine,
  unprobedGates,
  waitingGates,
} from "./helpers";

describe("VOWP capability gate cards", () => {
  it("engine_view_probes_automatically_on_a_live_connection", async () => {
    const bridge = new MockBridge({ scenario: "engine-ready" });
    const probe = vi.spyOn(bridge.gates, "probe");
    renderEngine(engineWith(), { bridge, gates: unprobedGates() });

    await waitFor(() => {
      expect(probe).toHaveBeenCalledTimes(1);
    });
  });

  it("gate_actions_disabled_when_waiting", () => {
    renderEngine(engineWith(), { gates: waitingGates() });
    const local = screen.getByTestId("gate-card-local");
    const asif = screen.getByTestId("gate-card-asif");

    expect(within(local).getByText("Waiting")).toBeInTheDocument();
    expect(
      within(local).getAllByText(/local opencode app host/).length,
    ).toBeGreaterThan(0);
    expect(within(local).getAllByText(/surface bridge/).length).toBeGreaterThan(0);
    expect(within(local).getAllByText(/MCP proxy/).length).toBeGreaterThan(0);
    expect(within(local).queryByText(/backfill/)).not.toBeInTheDocument();
    expect(within(local).queryByText(/local cache/)).not.toBeInTheDocument();
    expect(within(local).getByRole("button", { name: "Run locally" })).toBeDisabled();
    expect(
      within(local).getAllByText(/Waiting for VOWP Phase 2\/3/).length,
    ).toBeGreaterThan(0);

    expect(within(asif).getByText("Waiting")).toBeInTheDocument();
    expect(within(asif).getByRole("button", { name: "Export .asif.zip" })).toBeDisabled();
    expect(within(asif).getByRole("button", { name: "Import .asif.zip" })).toBeDisabled();
    expect(within(asif).getAllByText(/v4-asif-core/).length).toBeGreaterThan(0);
  });

  it("gate_details_list_exact_missing_milestones", async () => {
    const user = userEvent.setup();
    renderEngine(engineWith(), { gates: waitingGates() });

    const local = screen.getByTestId("gate-card-local");
    await user.click(within(local).getByRole("button", { name: "Details" }));
    const drawer = screen.getByTestId("gate-drawer");
    expect(within(drawer).getByText("local opencode app host")).toBeInTheDocument();
    expect(within(drawer).getByText("surface bridge")).toBeInTheDocument();
    expect(within(drawer).getByText("MCP proxy")).toBeInTheDocument();
    expect(within(drawer).queryByText("backfill")).not.toBeInTheDocument();
    expect(within(drawer).queryByText("local cache")).not.toBeInTheDocument();
    expect(
      within(drawer).getByText("vowp agent surface start --help: exit 2"),
    ).toBeInTheDocument();
    expect(
      within(drawer).getByText(/never contains a provider key/),
    ).toBeInTheDocument();
  });

  it("gate_cards_and_details_never_add_unprobed_milestones", async () => {
    const user = userEvent.setup();
    const gates = waitingGates();
    gates.local_orchestrator = {
      state: "waiting_for_vowp_phase3",
      missing: ["local cache"],
      detected_version: "0.4.0",
      evidence: ["vowp agent surface start --help: exit 2"],
    };
    gates.asif = {
      state: "waiting_for_vowp_asif",
      missing: ["v4-asif-cli"],
      detected_version: "0.4.0",
      evidence: ["session export --format asif: exit 2"],
    };
    renderEngine(engineWith(), { gates });

    const local = screen.getByTestId("gate-card-local");
    const asif = screen.getByTestId("gate-card-asif");
    expect(within(local).getByText("local cache")).toBeInTheDocument();
    expect(within(local).queryByText("backfill")).not.toBeInTheDocument();
    expect(within(asif).getByText("v4-asif-cli")).toBeInTheDocument();
    expect(within(asif).queryByText("v4-asif-core")).not.toBeInTheDocument();

    await user.click(within(local).getByRole("button", { name: "Details" }));
    const drawer = screen.getByTestId("gate-drawer");
    expect(within(drawer).getByText("local cache")).toBeInTheDocument();
    expect(within(drawer).queryByText("backfill")).not.toBeInTheDocument();
    expect(within(drawer).queryByText("surface bridge")).not.toBeInTheDocument();
  });

  it("gate_ready_fixture_still_disables_unimplemented_action", () => {
    renderEngine(engineWith(), { gates: readyLocalGates() });
    const local = screen.getByTestId("gate-card-local");
    expect(within(local).getByText("Ready")).toBeInTheDocument();
    expect(within(local).getByRole("button", { name: "Run locally" })).toBeDisabled();
    expect(
      within(local).getByText(
        "VOWP is ready; local orchestrator execution is not implemented in this build.",
      ),
    ).toBeInTheDocument();
  });

  it("asif_export_and_import_stay_disabled_by_the_d4_flag", () => {
    renderEngine(engineWith(), { gates: readyAsifGates() });
    const asif = screen.getByTestId("gate-card-asif");
    expect(within(asif).getByText("Ready")).toBeInTheDocument();
    expect(within(asif).getByRole("button", { name: "Export .asif.zip" })).toBeDisabled();
    expect(within(asif).getByRole("button", { name: "Import .asif.zip" })).toBeDisabled();
    expect(
      within(asif).getAllByText(
        "VOWP is ready; ASIF export and import are not implemented in this build.",
      ).length,
    ).toBeGreaterThan(0);
  });

  it("asif_export_and_import_enable_independently_after_the_d5_flag", () => {
    const gates = readyAsifGates();
    gates.features.asif_actions = true;
    gates.asif = {
      state: "ready",
      export: true,
      import: false,
      detected_version: "0.4.0",
      evidence: ["synthetic export evidence"],
    };
    renderEngine(engineWith(), { gates });
    const asif = screen.getByTestId("gate-card-asif");
    expect(within(asif).getByRole("button", { name: "Export .asif.zip" })).toBeEnabled();
    expect(within(asif).getByRole("button", { name: "Import .asif.zip" })).toBeDisabled();
    expect(
      within(asif).getByText("ASIF import capability is not proven."),
    ).toBeInTheDocument();
  });

  it("local_orchestrator_requires_a_connected_local_agent", () => {
    const gates = readyLocalGates();
    gates.features.local_orchestrator_actions = true;
    gates.local_agent.state = "not_enrolled";
    renderEngine(engineWith(), { gates });
    const local = screen.getByTestId("gate-card-local");
    expect(within(local).getByRole("button", { name: "Run locally" })).toBeDisabled();
    expect(
      within(local).getByText(/local VOWP agent is not connected/),
    ).toBeInTheDocument();
  });
});
