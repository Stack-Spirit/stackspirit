import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import { NextShell } from "../../next/NextShell";
import { resetConversations } from "../../next/conversations";
import { resetShellUi } from "../../next/shellUi";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";

const bridges: MockBridge[] = [];

async function openSettings() {
  resetFleetStore();
  resetConversations();
  const bridge = new MockBridge("fresh");
  bridges.push(bridge);
  const rendered = render(<NextShell bridge={bridge} />);
  await waitFor(() => expect(useFleetStore.getState().hostsLoaded).toBe(true));
  const user = userEvent.setup();
  await user.click(screen.getByTestId("settings-trigger"));
  await user.click(screen.getByRole("menuitem", { name: "All settings" }));
  return { bridge, user, ...rendered };
}

afterEach(() => {
  for (const bridge of bridges.splice(0)) bridge.dispose?.();
  resetConversations();
  resetShellUi();
  resetFleetStore();
  delete document.documentElement.dataset.ui;
  delete document.documentElement.dataset.theme;
});

describe("Settings → Engine & Models", () => {
  it("shows OpenCode first and searches the live provider catalog", async () => {
    const { user } = await openSettings();
    const card = await screen.findByTestId("engine-models-settings");
    expect(
      within(card).getByRole("heading", { name: "Engine & Models" }),
    ).toBeInTheDocument();
    const opencode = within(card).getByTestId("provider-opencode");
    expect(within(opencode).getByRole("heading", { name: "OpenCode" })).toBeInTheDocument();
    expect(within(opencode).getByText("Free models included")).toBeInTheDocument();

    const search = within(card).getByRole("searchbox", { name: "Search providers" });
    await user.type(search, "anthropic");
    const row = within(card).getByTestId("provider-anthropic");
    expect(row).toHaveTextContent("Anthropic");
    expect(row).toHaveTextContent("Available");
    expect(within(card).queryByTestId("provider-openai")).not.toBeInTheDocument();

    await user.clear(search);
    await user.type(search, "not a provider");
    expect(within(card).getByText(/No provider matches/)).toBeInTheDocument();
  });

  it("connects a provider through the keychain form and never echoes the key", async () => {
    const seen: string[] = [];
    const { bridge, user, container } = await openSettings();
    const card = await screen.findByTestId("engine-models-settings");
    const storeKey = bridge.engine.storeKey.bind(bridge.engine);
    vi.spyOn(bridge.engine, "storeKey").mockImplementation(async (provider, secret) => {
      seen.push(provider, secret);
      return storeKey(provider, secret);
    });
    const secret = ["sk", "providerOnly1234567890"].join("_");

    await user.type(
      within(card).getByRole("searchbox", { name: "Search providers" }),
      "anthropic",
    );
    const row = within(card).getByTestId("provider-anthropic");
    await user.click(within(row).getByRole("button", { name: "Connect Anthropic" }));
    await user.type(
      within(row).getByLabelText("API key for Anthropic"),
      secret,
    );
    await user.click(within(row).getByRole("button", { name: "Save key" }));

    expect(await within(card).findByRole("status")).toHaveTextContent("Anthropic connected");
    expect(within(card).getByTestId("provider-anthropic")).toHaveTextContent("Connected");
    expect(seen).toEqual(["anthropic", secret]);
    expect(container.innerHTML).not.toContain(secret);
    expect(within(card).queryByLabelText("API key for Anthropic")).not.toBeInTheDocument();
    expect(bridge.connectedProviderIds()).toContain("anthropic");
  });

  it("removes a connected provider key after the confirmation dialog", async () => {
    const { user } = await openSettings();
    const card = await screen.findByTestId("engine-models-settings");
    await user.type(
      within(card).getByRole("searchbox", { name: "Search providers" }),
      "anthropic",
    );
    const row = within(card).getByTestId("provider-anthropic");
    await user.click(within(row).getByRole("button", { name: "Connect Anthropic" }));
    await user.type(within(row).getByLabelText("API key for Anthropic"), "not-a-real-key");
    await user.click(within(row).getByRole("button", { name: "Save key" }));
    await within(card).findByRole("status");

    await user.click(
      within(card).getByRole("button", { name: "Remove Anthropic key" }),
    );
    const dialog = screen.getByRole("dialog", { name: "Remove provider key?" });
    await user.click(within(dialog).getByRole("button", { name: "Remove key" }));

    await waitFor(() =>
      expect(within(card).getByTestId("provider-anthropic")).not.toHaveTextContent("Connected"),
    );
    expect(within(card).getByRole("status")).toHaveTextContent("Anthropic key removed");
  });

  it("keeps a failed save error free of the typed secret", async () => {
    const { bridge, user, container } = await openSettings();
    const secret = ["sk", "errorOnly1234567890"].join("_");
    vi.spyOn(bridge.engine, "storeKey").mockRejectedValue(new Error("keychain unavailable"));
    const card = await screen.findByTestId("engine-models-settings");
    await user.type(
      within(card).getByRole("searchbox", { name: "Search providers" }),
      "openai",
    );
    const row = within(card).getByTestId("provider-openai");
    await user.click(within(row).getByRole("button", { name: "Connect OpenAI" }));
    await user.type(within(row).getByLabelText("API key for OpenAI"), secret);
    await user.click(within(row).getByRole("button", { name: "Save key" }));

    expect(await within(card).findByRole("alert")).toHaveTextContent("keychain unavailable");
    expect(container.innerHTML).not.toContain(secret);
  });
});
