import { act, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { describe, expect, it } from "vitest";

import { selectMutationsAllowed, useFleetStore } from "../state/fleetStore";
import { renderApp, renderHostManager } from "../test/render";

async function criticalViolations(container: HTMLElement) {
  const results = await axe.run(container, {
    rules: { "color-contrast": { enabled: false } },
  });
  return results.violations.filter((violation) => violation.impact === "critical");
}

describe("AppShell", () => {
  it("shows a persistent Preview data badge in mock mode", async () => {
    await renderApp("fresh");
    const badges = screen.getAllByText("Preview data");
    expect(badges.length).toBeGreaterThanOrEqual(1);
    expect(badges[0]).toHaveAttribute(
      "title",
      "Mock data. Nothing here comes from a real VPS.",
    );
  });

  it("never shows a green host without a successful connection", async () => {
    const user = userEvent.setup();
    await renderApp("fresh");
    const status = screen.getByTestId("host-status");
    expect(status).toHaveAttribute("data-tone", "unknown");
    await user.click(screen.getByTestId("topbar-connect"));
    await waitFor(() => expect(status).toHaveAttribute("data-tone", "ok"));
    expect(screen.getByTestId("topbar-connect")).toHaveTextContent("Disconnect");
  });

  it("keeps the host dot red when the SSH connection fails", async () => {
    const user = userEvent.setup();
    await renderApp("connect-failure");
    const status = screen.getByTestId("host-status");
    await user.click(screen.getByTestId("topbar-connect"));
    await waitFor(() => expect(status).toHaveAttribute("data-tone", "danger"));
    expect(screen.getByTestId("topbar-connect")).toHaveTextContent("Connect");
  });

  it("announces connection changes through a polite live region", async () => {
    const user = userEvent.setup();
    await renderApp("fresh");
    await user.click(screen.getByTestId("topbar-connect"));
    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent(/Connected to staging-vps/),
    );
  });

  it("dims data and disables writes after the connection goes stale", async () => {
    const user = userEvent.setup();
    const { container } = await renderApp("stale-after-3");
    await user.click(screen.getByTestId("topbar-connect"));
    await waitFor(() => expect(useFleetStore.getState().fleet).not.toBeNull());
    await act(async () => {
      for (let attempt = 0; attempt < 3; attempt += 1) {
        await useFleetStore.getState().refresh();
      }
    });
    await waitFor(() => {
      expect(useFleetStore.getState().connection.state).toBe("stale");
      expect(container.querySelector(".data-dimmed")).not.toBeNull();
    });
    expect(selectMutationsAllowed(useFleetStore.getState())).toBe(false);
  });

  it("has no critical axe violations on the shell", async () => {
    const { container } = await renderApp("fresh");
    expect(await criticalViolations(container)).toEqual([]);
  });

  it("has no critical axe violations on the Host Manager", async () => {
    const { container } = await renderHostManager("fresh");
    expect(await criticalViolations(container)).toEqual([]);
  });
});
