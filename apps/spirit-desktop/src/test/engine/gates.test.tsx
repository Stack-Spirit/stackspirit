import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { engineWith, readyAsifGates, readyLocalGates, renderEngine, waitingGates } from "./helpers";

describe("VOWP capability gate cards", () => {
  it("gate_actions_disabled_when_waiting", () => {
    renderEngine(engineWith(), { gates: waitingGates() });
    const local = screen.getByTestId("gate-card-local");
    const asif = screen.getByTestId("gate-card-asif");

    expect(within(local).getByText("Waiting")).toBeInTheDocument();
    expect(
      within(local).getAllByText(/local opencode app host/).length,
    ).toBeGreaterThan(0);
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
    expect(within(drawer).getByText("backfill")).toBeInTheDocument();
    expect(within(drawer).getByText("local cache")).toBeInTheDocument();
    expect(
      within(drawer).getByText("vowp agent surface start --help: exit 2"),
    ).toBeInTheDocument();
    expect(
      within(drawer).getByText(/never contains a provider key/),
    ).toBeInTheDocument();
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
