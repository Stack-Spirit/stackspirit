import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { describe, expect, it } from "vitest";

import { selectMutationsAllowed, useFleetStore } from "../state/fleetStore";
import { renderApp, renderHostManager } from "../test/render";

const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"];

async function wcagViolations(container: HTMLElement) {
  const results = await axe.run(container, {
    runOnly: { type: "tag", values: WCAG_TAGS },
  });
  return results.violations;
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

  it("dims data, disables writes and starts a backoff after three failed polls", async () => {
    const user = userEvent.setup();
    const { container } = await renderApp("stale-after-3");
    await user.click(screen.getByTestId("topbar-connect"));
    await waitFor(() => expect(useFleetStore.getState().fleet).not.toBeNull());
    const refresh = screen.getByTestId("fleet-refresh");
    expect(refresh).toBeEnabled();

    for (let attempt = 0; attempt < 3; attempt += 1) {
      const revision = useFleetStore.getState().revision;
      await user.click(refresh);
      await waitFor(() =>
        expect(useFleetStore.getState().revision).toBeGreaterThan(revision),
      );
    }

    await waitFor(() => {
      expect(useFleetStore.getState().connection.state).toBe("stale");
      expect(container.querySelector(".data-dimmed")).not.toBeNull();
    });
    expect(selectMutationsAllowed(useFleetStore.getState())).toBe(false);
    expect(refresh).toBeEnabled();
    await waitFor(() =>
      expect(screen.getByTestId("backoff-countdown")).toHaveTextContent(
        /retrying in \d+s/,
      ),
    );
  });

  it("keeps the raw document scroll regions keyboard-focusable", async () => {
    const user = userEvent.setup();
    await renderApp("fresh");
    await user.click(screen.getByTestId("topbar-connect"));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Raw document" })).toBeEnabled(),
    );
    await user.click(screen.getByRole("button", { name: "Raw document" }));
    const drawer = await screen.findByRole("dialog", { name: "Raw VOWP document" });
    const regions = drawer.querySelectorAll("pre.raw-json");
    expect(regions).toHaveLength(2);
    for (const region of regions) expect(region).toHaveAttribute("tabindex", "0");
  });

  it("has no WCAG 2.2 AA axe violations on the shell", async () => {
    const { container } = await renderApp("fresh");
    expect(await wcagViolations(container)).toEqual([]);
  });

  it("has no WCAG 2.2 AA axe violations on the Host Manager", async () => {
    const { container } = await renderHostManager("fresh");
    expect(await wcagViolations(container)).toEqual([]);
  });
});
