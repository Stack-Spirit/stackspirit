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

async function openGlobal(scenario: MockScenario) {
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
  await screen.findByRole("heading", { name: "Global orchestrator", level: 1 });
  return { user, bridge };
}

afterEach(() => {
  for (const bridge of bridges.splice(0)) bridge.dispose?.();
  resetConversations();
  resetShellUi();
  resetFleetStore();
});

describe("session ownership banner", () => {
  it("shows where an idle session lives and takes over in one click", async () => {
    const { user, bridge } = await openGlobal("owner-idle");
    const banner = await screen.findByTestId("ownership-banner");
    expect(within(banner).getByTestId("ownership-where")).toHaveTextContent(
      "Active on OpenCode · macbook",
    );
    expect(within(banner).getByTestId("ownership-activity")).toHaveTextContent(
      "Idle",
    );

    await user.click(within(banner).getByRole("button", { name: "Take over" }));
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    await waitFor(() =>
      expect(screen.queryByTestId("ownership-banner")).not.toBeInTheDocument(),
    );
    expect(bridge.takeovers).toEqual(["01JCF5WQ8ZK3M2V7N4P6R8T0AB"]);
  });

  it("asks for an extra confirmation before taking over a running turn", async () => {
    const { user, bridge } = await openGlobal("owner-running");
    const banner = await screen.findByTestId("ownership-banner");
    expect(within(banner).getByTestId("ownership-activity")).toHaveTextContent(
      "Running",
    );

    await user.click(within(banner).getByRole("button", { name: "Take over" }));
    const dialog = await screen.findByRole("alertdialog", {
      name: "Confirm take over",
    });
    expect(dialog).toHaveTextContent(
      "OpenCode · macbook is currently running a turn. Taking over will fence its writes and tell it to stop. Take over anyway?",
    );
    expect(bridge.takeovers).toEqual([]);

    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(bridge.takeovers).toEqual([]);

    await user.click(screen.getByRole("button", { name: "Take over" }));
    await user.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Take over anyway",
      }),
    );
    await waitFor(() => expect(bridge.takeovers).toHaveLength(1));
    await waitFor(() =>
      expect(screen.queryByTestId("ownership-banner")).not.toBeInTheDocument(),
    );
  });

  it("shows no banner when StackSpirit already owns the session", async () => {
    await openGlobal("owner-local");
    expect(screen.queryByTestId("ownership-banner")).not.toBeInTheDocument();
  });

  it("shows no banner when the snapshot has no owner", async () => {
    await openGlobal("busy");
    expect(screen.queryByTestId("ownership-banner")).not.toBeInTheDocument();
  });

  it("stays read-only and never fakes ownership when StackSpirit lacks takeover", async () => {
    const { user, bridge } = await openGlobal("owner-unsupported");
    const banner = await screen.findByTestId("ownership-banner");
    await user.click(within(banner).getByRole("button", { name: "Take over" }));
    expect(await screen.findByTestId("ownership-error")).toHaveTextContent(
      "Take over isn't available with this StackSpirit yet",
    );
    expect(screen.getByTestId("ownership-banner")).toBeInTheDocument();
    expect(
      within(screen.getByTestId("ownership-banner")).getByRole("button", {
        name: "Take over",
      }),
    ).toBeDisabled();
    expect(bridge.takeovers).toEqual([]);
  });
});
