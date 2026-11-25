import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import { resetConversations } from "../../next/conversations";
import { NextShell } from "../../next/NextShell";
import { resetShellUi } from "../../next/shellUi";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";

afterEach(() => {
  resetShellUi();
  resetConversations();
  resetFleetStore();
});

async function renderConnected() {
  resetFleetStore();
  resetConversations();
  const user = userEvent.setup();
  render(<NextShell bridge={new MockBridge("busy")} />);
  await waitFor(() => expect(useFleetStore.getState().hostsLoaded).toBe(true));
  await user.click(screen.getByTestId("connect-prompt-connect"));
  await waitFor(() => expect(useFleetStore.getState().connection.state).toBe("connected"));
  return user;
}

describe("mobile shell navigation", () => {
  it("opens and closes the navigation drawer from the mobile bar", async () => {
    const user = await renderConnected();
    const toggle = screen.getByTestId("mobile-nav-toggle");
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByTestId("next-shell")).not.toHaveClass("nx--nav-open");
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByTestId("next-shell")).toHaveClass("nx--nav-open");
    await user.click(screen.getByTestId("mobile-nav-scrim"));
    expect(screen.getByTestId("next-shell")).not.toHaveClass("nx--nav-open");
  });

  it("reaches the inbox from the mobile bar and closes the drawer", async () => {
    const user = await renderConnected();
    await user.click(screen.getByTestId("mobile-nav-toggle"));
    await user.click(screen.getByTestId("mobile-inbox"));
    expect(screen.getByRole("heading", { name: "Inbox", level: 1 })).toBeInTheDocument();
    expect(screen.getByTestId("next-shell")).not.toHaveClass("nx--nav-open");
  });
});

describe("mobile stylesheet contract", () => {
  const css = readFileSync(resolve(__dirname, "../../next/theme.css"), "utf8");
  const phone = css.slice(css.indexOf(".nx-mobilebar,\n.nx-scrim"));

  it("hides the mobile bar by default and turns the sidebar into a drawer on phones", () => {
    expect(css).toMatch(/\.nx-mobilebar,\s*\.nx-scrim\s*\{\s*display: none;/);
    expect(phone).toMatch(/@media \(max-width: 720px\)/);
    expect(phone).toMatch(/\.nx-side \{\s*position: fixed;/);
  });

  it("keeps 44px touch targets, a sticky composer and no horizontal overflow", () => {
    expect(phone).toMatch(/min-height: 44px;/);
    expect(phone).toMatch(/\.nx-composer-shell \{\s*position: sticky;\s*bottom: 0;/);
    expect(phone).toMatch(/overflow-x: hidden;/);
  });
});
