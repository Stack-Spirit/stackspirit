import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { describe, expect, it } from "vitest";

import { MockBridge, type MockScenario } from "../../bridge/MockBridge";
import type { GateSnapshot } from "../../bridge/wire";
import { engineWith, renderEngine } from "./helpers";

const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"];
const SESSION = "01JCF5WQ8ZK3M2V7N4P6R8T0AE";

async function localBridge(scenario: MockScenario): Promise<{
  bridge: MockBridge;
  gates: GateSnapshot;
}> {
  const bridge = new MockBridge(scenario);
  const host = (await bridge.hosts.list())[0];
  if (!host) throw new Error(`${scenario} has no host`);
  await bridge.connection.connect(host.id);
  const gates = await bridge.gates.probe();
  return { bridge, gates };
}

async function wcagViolations(container: HTMLElement) {
  const results = await axe.run(container, {
    runOnly: { type: "tag", values: WCAG_TAGS },
  });
  return results.violations;
}

describe("D6 local orchestrator Engine panel", () => {
  it("keeps the waiting gate copy and disables every start action", async () => {
    const { bridge, gates } = await localBridge("gate-d6-waiting");
    renderEngine(engineWith(), { bridge, gates });

    const local = screen.getByTestId("gate-card-local");
    expect(within(local).getByText("Waiting")).toBeInTheDocument();
    expect(within(local).getByRole("button", { name: "Run locally" })).toBeDisabled();
    expect(within(local).getAllByText(/local agent connection/).length).toBeGreaterThan(0);

    const panel = screen.getByTestId("local-orchestrator-panel");
    expect(within(panel).getByTestId("local-orchestrator-reason")).toHaveTextContent(
      /local agent connection/,
    );
    expect(within(panel).getByTestId("local-orchestrator-create")).toBeDisabled();
    expect(within(panel).getByTestId("local-orchestrator-pair")).toBeDisabled();
    expect(screen.queryByTestId("local-orchestrator-row-vowp-local")).not.toBeInTheDocument();
    expect(bridge.recordedLocalCalls()).toEqual([]);
  });

  it("keeps an incompatible protocol disabled with the exact reason", async () => {
    const { bridge, gates } = await localBridge("gate-d6-incompatible");
    renderEngine(engineWith(), { bridge, gates });

    const local = screen.getByTestId("gate-card-local");
    expect(within(local).getByText("Incompatible")).toBeInTheDocument();
    expect(within(local).getByRole("button", { name: "Run locally" })).toBeDisabled();
    expect(within(local).getAllByText(/vowp-agent\/0/).length).toBeGreaterThan(0);

    const panel = screen.getByTestId("local-orchestrator-panel");
    expect(within(panel).getByTestId("local-orchestrator-reason")).toHaveTextContent(
      /vowp-agent\/0/,
    );
    expect(within(panel).getByTestId("local-orchestrator-create")).toBeDisabled();
    expect(within(panel).getByTestId("local-orchestrator-pair")).toBeDisabled();
  });

  it("requires a connected local agent even when the gate is Ready", async () => {
    const { bridge, gates } = await localBridge("gate-d6-ready");
    const disconnected: GateSnapshot = {
      ...gates,
      local_agent: {
        ...gates.local_agent,
        state: "not_enrolled",
        node: null,
        protocol: null,
      },
    };
    renderEngine(engineWith(), { bridge, gates: disconnected });

    const local = screen.getByTestId("gate-card-local");
    expect(within(local).getByText("Ready")).toBeInTheDocument();
    expect(within(local).getByRole("button", { name: "Run locally" })).toBeDisabled();
    const panel = screen.getByTestId("local-orchestrator-panel");
    expect(within(panel).getByTestId("local-orchestrator-reason")).toHaveTextContent(
      /local StackSpirit agent is not connected/,
    );
  });

  it("shows the node, protocol, agent, lease, honest states and all actions when Ready", async () => {
    const user = userEvent.setup();
    const { bridge, gates } = await localBridge("gate-d6-ready");
    renderEngine(engineWith(), { bridge, gates });

    const local = screen.getByTestId("gate-card-local");
    expect(within(local).getByText("Ready")).toBeInTheDocument();
    expect(within(local).getByRole("button", { name: "Run locally" })).toBeEnabled();

    const panel = screen.getByTestId("local-orchestrator-panel");
    expect(within(panel).getAllByText("mac-studio").length).toBeGreaterThan(0);
    expect(within(panel).getAllByText("vowp-agent/1").length).toBeGreaterThan(0);
    expect(within(panel).getByText(/re-checks the gate/)).toBeInTheDocument();

    const running = await screen.findByTestId("local-orchestrator-row-vowp-local");
    expect(within(running).getByText("Running")).toBeInTheDocument();
    expect(within(running).getByText("StackSpirit Desktop")).toBeInTheDocument();
    expect(within(running).getByText("mac-studio")).toBeInTheDocument();
    expect(within(running).getByText("vowp-agent/1")).toBeInTheDocument();

    expect(
      within(await screen.findByTestId("local-orchestrator-row-chat-local")).getByText(
        "Stopped",
      ),
    ).toBeInTheDocument();
    expect(
      within(await screen.findByTestId("local-orchestrator-row-docs-local")).getByText(
        "Degraded",
      ),
    ).toBeInTheDocument();
    expect(
      within(await screen.findByTestId("local-orchestrator-row-phone-local")).getByText(
        "Reconnecting",
      ),
    ).toBeInTheDocument();

    const revokedSurface = await screen.findByTestId(
      "local-surface-row-01J9Z00000000000000000000B",
    );
    expect(within(revokedSurface).getByText("Revoked")).toBeInTheDocument();
    expect(within(revokedSurface).getByText("Read-only")).toBeInTheDocument();
    const supersededSurface = await screen.findByTestId(
      "local-surface-row-01J9Z00000000000000000000D",
    );
    expect(within(supersededSurface).getByText("Superseded")).toBeInTheDocument();
    const degradedSurface = await screen.findByTestId(
      "local-surface-row-01J9Z00000000000000000000E",
    );
    expect(within(degradedSurface).getByText("Degraded")).toBeInTheDocument();

    // The gate-card action opens the create form, and a create is recorded.
    await user.click(within(local).getByRole("button", { name: "Run locally" }));
    const create = await screen.findByRole("dialog", {
      name: "Create local orchestrator",
    });
    await user.clear(within(create).getByLabelText("Project"));
    await user.type(within(create).getByLabelText("Project"), "vowp");
    await user.type(within(create).getByLabelText("Name"), "docs-local-2");
    await user.click(within(create).getByTestId("local-orchestrator-create-submit"));
    await waitFor(() => {
      expect(screen.getByTestId("local-orchestrator-row-docs-local-2")).toBeInTheDocument();
    });
    expect(bridge.recordedLocalCalls()[0]).toEqual({
      action: "orchestrator_new",
      name: "docs-local-2",
    });

    // Pairing shows the one-time code once and can redeem it locally.
    const refreshedPanel = screen.getByTestId("local-orchestrator-panel");
    await user.click(within(refreshedPanel).getByTestId("local-orchestrator-pair"));
    const pair = await screen.findByRole("dialog", { name: "Pair a local surface" });
    await user.type(within(pair).getByLabelText("Session id"), SESSION);
    await user.click(within(pair).getByTestId("local-surface-pair-submit"));
    const pairingResult = await within(pair).findByTestId("local-pairing-result");
    expect(within(pairingResult).getByTestId("local-pairing-code")).toHaveTextContent(
      "pairing-code-placeholder",
    );
    expect(within(pairingResult).getByTestId("local-pairing-command")).toHaveTextContent(
      /vowp agent pair --code-file -/,
    );
    await user.click(within(pairingResult).getByTestId("local-pairing-agent"));
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Pair a local surface" })).not.toBeInTheDocument();
    });

    // Stop and resume; rotate and revoke; each one is re-checked by the bridge.
    const runningRow = screen.getByTestId("local-orchestrator-row-vowp-local");
    await user.click(within(runningRow).getByTestId("local-orchestrator-stop-vowp-local"));
    await waitFor(() => {
      expect(within(runningRow).getByText("Stopped")).toBeInTheDocument();
    });
    await user.click(within(runningRow).getByTestId("local-orchestrator-resume-vowp-local"));
    await waitFor(() => {
      expect(within(runningRow).getByText("Running")).toBeInTheDocument();
    });

    const writableSurface = screen.getByTestId(
      "local-surface-row-01J9Z00000000000000000000A",
    );
    await user.click(
      within(writableSurface).getByTestId("local-surface-rotate-01J9Z00000000000000000000A"),
    );
    await waitFor(() => {
      expect(
        within(
          screen.getByTestId("local-surface-row-01J9Z00000000000000000000A"),
        ).getByText("Rotation queued"),
      ).toBeInTheDocument();
    });
    await user.click(
      within(screen.getByTestId("local-surface-row-01J9Z00000000000000000000A")).getByTestId(
        "local-surface-revoke-01J9Z00000000000000000000A",
      ),
    );
    await waitFor(() => {
      expect(
        within(
          screen.getByTestId("local-surface-row-01J9Z00000000000000000000A"),
        ).getByText("Revoked"),
      ).toBeInTheDocument();
    });

    const actions = bridge.recordedLocalCalls().map((call) => call.action);
    expect(actions).toContain("surface_pair");
    expect(actions).toContain("agent_pair");
    expect(actions).toContain("surface_rotate");
    expect(actions).toContain("surface_revoke");
    expect(actions).toContain("orchestrator_stop");
    expect(actions).toContain("orchestrator_resume");
  });

  it("has no critical or serious axe violations with the local panel", async () => {
    const { bridge, gates } = await localBridge("gate-d6-ready");
    const { container } = renderEngine(engineWith(), { bridge, gates });
    const violations = (await wcagViolations(container)).filter(
      (violation) => violation.impact === "critical" || violation.impact === "serious",
    );
    expect(violations).toEqual([]);
  });
});
