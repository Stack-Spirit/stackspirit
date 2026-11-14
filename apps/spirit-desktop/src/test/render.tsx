import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect } from "vitest";

import { AppShell } from "../app/AppShell";
import { MockBridge, type MockScenario } from "../bridge/MockBridge";
import { resetFleetStore, useFleetStore } from "../state/fleetStore";
import { HostManager } from "../views/hosts/HostManager";

export async function renderApp(scenario: MockScenario = "fresh") {
  resetFleetStore();
  const bridge = new MockBridge(scenario);
  const result = render(<AppShell bridge={bridge} />);
  await waitFor(() => {
    expect(useFleetStore.getState().hostsLoaded).toBe(true);
  });
  return { bridge, ...result };
}

export async function renderHostManager(scenario: MockScenario = "fresh") {
  resetFleetStore();
  const bridge = new MockBridge(scenario);
  await useFleetStore.getState().initialize(bridge);
  const result = render(<HostManager />);
  await waitFor(() => {
    expect(screen.getByTestId("host-manager")).toBeInTheDocument();
  });
  return { bridge, ...result };
}

export function nav(): HTMLElement {
  return screen.getByRole("navigation", { name: "Primary views" });
}

export async function openHostsView(): Promise<void> {
  const user = userEvent.setup();
  await user.click(within(nav()).getByRole("button", { name: /hosts/i }));
  await screen.findByTestId("host-manager");
}
