import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { MockBridge, type MockScenario } from "../../bridge/MockBridge";
import type { SpiritBridge } from "../../bridge/SpiritBridge";
import { resetConversations } from "../../next/conversations";
import { NextShell } from "../../next/NextShell";
import { resetShellUi } from "../../next/shellUi";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";

const bridges: MockBridge[] = [];

async function openProject(scenario: MockScenario) {
  resetFleetStore();
  resetConversations();
  const bridge = new MockBridge(scenario);
  bridges.push(bridge);
  const user = userEvent.setup();
  render(<NextShell bridge={bridge as SpiritBridge} />);
  await waitFor(() => expect(useFleetStore.getState().hostsLoaded).toBe(true));
  await user.click(screen.getByTestId("connect-prompt-connect"));
  await waitFor(() =>
    expect(useFleetStore.getState().connection.state).toBe("connected"),
  );
  const row = await screen.findByRole("button", { name: /^stackspirit/ });
  await user.click(row);
  const group = row.closest(".nx-proj") as HTMLElement;
  await user.click(within(group).getByRole("button", { name: "Orchestrator" }));
  await screen.findByRole("heading", { name: "stackspirit", level: 1 });
  return { user, bridge };
}

afterEach(() => {
  for (const bridge of bridges.splice(0)) bridge.dispose?.();
  resetConversations();
  resetShellUi();
  resetFleetStore();
});

describe("session host control", () => {
  it("moves the project chat to the local node and shows the refreshed host", async () => {
    const { user, bridge } = await openProject("owner-project");
    const host = await screen.findByTestId("orchestrator-host");
    expect(host).toHaveTextContent("Home");

    await user.click(host);
    const panel = await screen.findByRole("group", { name: "Choose session host" });
    expect(within(panel).getByRole("button", { name: "Home (this server)" })).toBeDisabled();
    const local = await within(panel).findByRole("button", { name: "Local · local-mac" });
    await waitFor(() => expect(local).toBeEnabled());
    // Nothing moved just because the menu opened.
    expect(bridge.orchestratorMoves).toEqual([]);

    await user.click(local);
    await waitFor(() =>
      expect(bridge.orchestratorMoves).toEqual([
        { name: "local-chat", target: { kind: "node", node: "local-mac" } },
      ]),
    );
    await waitFor(() =>
      expect(screen.getByTestId("orchestrator-host")).toHaveTextContent("Local · local-mac"),
    );
    expect(screen.getByTestId("orchestrator-host-note")).toHaveTextContent(
      "Moved to local-mac. Reconnect the chat to use the new host.",
    );
    expect(screen.queryByRole("group", { name: "Choose session host" })).not.toBeInTheDocument();
  });

  it("moves back Home and reports a typed failure without changing the label", async () => {
    const { user, bridge } = await openProject("owner-project");
    await user.click(await screen.findByTestId("orchestrator-host"));
    const first = await screen.findByRole("group", { name: "Choose session host" });
    await user.click(await within(first).findByRole("button", { name: "Local · local-mac" }));
    await waitFor(() =>
      expect(screen.getByTestId("orchestrator-host")).toHaveTextContent("Local · local-mac"),
    );

    await user.click(screen.getByTestId("orchestrator-host"));
    const panel = await screen.findByRole("group", { name: "Choose session host" });
    await waitFor(() =>
      expect(within(panel).getByRole("button", { name: "Local · local-mac" })).toBeDisabled(),
    );
    await user.click(within(panel).getByRole("button", { name: "Home (this server)" }));
    await waitFor(() =>
      expect(screen.getByTestId("orchestrator-host")).toHaveTextContent("Home"),
    );
    expect(bridge.orchestratorMoves.at(-1)).toEqual({
      name: "local-chat",
      target: { kind: "home" },
    });

    bridge.localOrchestrators!.orchestratorMove = async () => {
      throw { code: "gate", message: "not_supported", retryable: false };
    };
    await user.click(screen.getByTestId("orchestrator-host"));
    await user.click(
      await screen.findByRole("button", { name: "Local · local-mac" }),
    );
    expect(await screen.findByTestId("orchestrator-host-error")).toHaveTextContent(
      "This VOWP cannot move orchestrators",
    );
    expect(screen.getByTestId("orchestrator-host")).toHaveTextContent("Home");
  });

  it("closes the panel with Escape", async () => {
    const { user } = await openProject("owner-project");
    await user.click(await screen.findByTestId("orchestrator-host"));
    await screen.findByRole("group", { name: "Choose session host" });
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("group", { name: "Choose session host" })).not.toBeInTheDocument();
  });

  it("renders no host control without a project orchestrator card", async () => {
    resetFleetStore();
    resetConversations();
    const bridge = new MockBridge("busy");
    bridges.push(bridge);
    const user = userEvent.setup();
    render(<NextShell bridge={bridge as SpiritBridge} />);
    await waitFor(() => expect(useFleetStore.getState().hostsLoaded).toBe(true));
    await user.click(screen.getByTestId("connect-prompt-connect"));
    await screen.findByRole("heading", { name: "Global orchestrator", level: 1 });
    expect(screen.queryByTestId("orchestrator-host")).not.toBeInTheDocument();
  });
});
