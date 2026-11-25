import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import type { ModelChoice } from "../../bridge/wire";
import { defaultModelLabel } from "../../next/SettingsPage";
import { NextShell } from "../../next/NextShell";
import { resetConversations, useConversations } from "../../next/conversations";
import { resetShellUi } from "../../next/shellUi";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";

const bridges: MockBridge[] = [];

async function openProviderPanel(scenario: "empty" | "stopped" = "empty") {
  resetFleetStore();
  resetConversations();
  const bridge = new MockBridge("empty");
  // "empty" has a ready preview engine; a stopped one has no live catalog to show.
  if (scenario === "stopped")
    await bridge.engine.selectExternal("opencode", "/usr/bin/opencode");
  bridges.push(bridge);
  const rendered = render(<NextShell bridge={bridge} />);
  await waitFor(() => expect(useFleetStore.getState().hostsLoaded).toBe(true));
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: /Set up with AI/ }));
  await user.click(await screen.findByTestId("composer-model"));
  await user.click(screen.getByTestId("composer-model-row"));
  await user.click(screen.getByTestId("composer-connect-provider"));
  const dialog = await screen.findByRole("dialog", { name: "Engine & Models" });
  return { bridge, user, dialog, ...rendered };
}

afterEach(() => {
  for (const bridge of bridges.splice(0)) bridge.dispose?.();
  resetConversations();
  resetShellUi();
  resetFleetStore();
  delete document.documentElement.dataset.ui;
  delete document.documentElement.dataset.theme;
});

describe("Composer → Engine & Models", () => {
  it("shows OpenCode first and searches the live provider catalog", async () => {
    const { user, dialog } = await openProviderPanel();
    const card = within(dialog).getByTestId("engine-models-settings");
    const opencode = within(card).getByTestId("provider-opencode");
    expect(
      within(opencode).getByRole("heading", { name: "OpenCode" }),
    ).toBeInTheDocument();
    expect(
      within(opencode).getByText("Free models included"),
    ).toBeInTheDocument();

    const search = within(card).getByRole("searchbox", {
      name: "Search providers",
    });
    await user.type(search, "anthropic");
    const row = within(card).getByTestId("provider-anthropic");
    expect(row).toHaveTextContent("Anthropic");
    expect(row).toHaveTextContent("Available");
    expect(
      within(card).queryByTestId("provider-openai"),
    ).not.toBeInTheDocument();

    await user.clear(search);
    await user.type(search, "not a provider");
    expect(within(card).getByText(/No provider matches/)).toBeInTheDocument();
  });

  it("connects a provider through the keychain form and never echoes the key", async () => {
    const seen: string[] = [];
    const { bridge, user, dialog, container } = await openProviderPanel();
    const card = within(dialog).getByTestId("engine-models-settings");
    const storeKey = bridge.engine.storeKey.bind(bridge.engine);
    vi.spyOn(bridge.engine, "storeKey").mockImplementation(
      async (provider, secret) => {
        seen.push(provider, secret);
        return storeKey(provider, secret);
      },
    );
    const secret = ["sk", "providerOnly1234567890"].join("_");

    await user.type(
      within(card).getByRole("searchbox", { name: "Search providers" }),
      "anthropic",
    );
    const row = within(card).getByTestId("provider-anthropic");
    await user.click(
      within(row).getByRole("button", { name: "Connect Anthropic" }),
    );
    await user.type(
      within(row).getByLabelText("API key for Anthropic"),
      secret,
    );
    await user.click(within(row).getByRole("button", { name: "Save key" }));

    expect(await within(card).findByRole("status")).toHaveTextContent(
      "Anthropic connected",
    );
    expect(within(card).getByTestId("provider-anthropic")).toHaveTextContent(
      "Connected",
    );
    expect(seen).toEqual(["anthropic", secret]);
    expect(container.innerHTML).not.toContain(secret);
    expect(
      within(card).queryByLabelText("API key for Anthropic"),
    ).not.toBeInTheDocument();
    expect(
      (await bridge.engine.keyStatus()).find(
        (status) => status.provider === "anthropic",
      )?.state,
    ).not.toBe("missing");
  });

  it("removes a connected provider key after the confirmation dialog", async () => {
    const { user, dialog } = await openProviderPanel();
    const card = within(dialog).getByTestId("engine-models-settings");
    await user.type(
      within(card).getByRole("searchbox", { name: "Search providers" }),
      "anthropic",
    );
    const row = within(card).getByTestId("provider-anthropic");
    await user.click(
      within(row).getByRole("button", { name: "Connect Anthropic" }),
    );
    await user.type(
      within(row).getByLabelText("API key for Anthropic"),
      "not-a-real-key",
    );
    await user.click(within(row).getByRole("button", { name: "Save key" }));
    await within(card).findByRole("status");

    await user.click(
      await within(card).findByRole("button", { name: "Remove Anthropic key" }),
    );
    const confirm = screen.getByRole("dialog", {
      name: "Remove provider key?",
    });
    await user.click(
      within(confirm).getByRole("button", { name: "Remove key" }),
    );

    await waitFor(() =>
      expect(within(card).getByTestId("provider-anthropic")).toHaveTextContent(
        "Available",
      ),
    );
    expect(within(card).getByRole("status")).toHaveTextContent(
      "Anthropic key removed",
    );
  });

  it("keeps a failed save error free of the typed secret", async () => {
    const { bridge, user, dialog, container } = await openProviderPanel();
    const secret = ["sk", "errorOnly1234567890"].join("_");
    vi.spyOn(bridge.engine, "storeKey").mockRejectedValue(
      new Error("keychain unavailable"),
    );
    const card = within(dialog).getByTestId("engine-models-settings");
    await user.type(
      within(card).getByRole("searchbox", { name: "Search providers" }),
      "openai",
    );
    const row = within(card).getByTestId("provider-openai");
    await user.click(
      within(row).getByRole("button", { name: "Connect OpenAI" }),
    );
    await user.type(within(row).getByLabelText("API key for OpenAI"), secret);
    await user.click(within(row).getByRole("button", { name: "Save key" }));

    expect(await within(card).findByRole("alert")).toHaveTextContent(
      "keychain unavailable",
    );
    expect(container.innerHTML).not.toContain(secret);
  });

  it("keeps a key that fails verification invalid after Save key", async () => {
    const { user, dialog } = await openProviderPanel();
    const card = within(dialog).getByTestId("engine-models-settings");
    await user.type(
      within(card).getByRole("searchbox", { name: "Search providers" }),
      "openai",
    );
    const row = within(card).getByTestId("provider-openai");
    await user.click(
      within(row).getByRole("button", { name: "Connect OpenAI" }),
    );
    await user.type(
      within(row).getByLabelText("API key for OpenAI"),
      "invalid-key-value",
    );
    await user.click(within(row).getByRole("button", { name: "Save key" }));

    await within(card).findByRole("status");
    await waitFor(() =>
      expect(within(card).getByTestId("provider-openai")).toHaveTextContent(
        "Invalid key",
      ),
    );
  });

  it("updates the composer model picker right after Save key and Remove key", async () => {
    const { user, dialog } = await openProviderPanel();
    const card = within(dialog).getByTestId("engine-models-settings");
    await user.type(
      within(card).getByRole("searchbox", { name: "Search providers" }),
      "anthropic",
    );
    const row = within(card).getByTestId("provider-anthropic");
    await user.click(
      within(row).getByRole("button", { name: "Connect Anthropic" }),
    );
    await user.type(
      within(row).getByLabelText("API key for Anthropic"),
      "not-a-real-key",
    );
    await user.click(within(row).getByRole("button", { name: "Save key" }));
    await within(card).findByRole("status");

    await waitFor(() =>
      expect(
        useConversations
          .getState()
          .views.setup?.available_models?.map((m) => m.id),
      ).toContain("anthropic/claude-sonnet-4"),
    );

    await user.click(
      await within(card).findByRole("button", { name: "Remove Anthropic key" }),
    );
    const confirm = screen.getByRole("dialog", {
      name: "Remove provider key?",
    });
    await user.click(
      within(confirm).getByRole("button", { name: "Remove key" }),
    );
    await waitFor(() =>
      expect(
        useConversations
          .getState()
          .views.setup?.available_models?.map((m) => m.id),
      ).not.toContain("anthropic/claude-sonnet-4"),
    );
  });

  it("lists a connected OpenRouter in its own searchable picker section", async () => {
    const { user, dialog } = await openProviderPanel();
    const card = within(dialog).getByTestId("engine-models-settings");
    await user.type(
      within(card).getByRole("searchbox", { name: "Search providers" }),
      "openrouter",
    );
    const row = within(card).getByTestId("provider-openrouter");
    await user.click(
      within(row).getByRole("button", { name: "Connect OpenRouter" }),
    );
    await user.type(
      within(row).getByLabelText("API key for OpenRouter"),
      "not-a-real-key",
    );
    await user.click(within(row).getByRole("button", { name: "Save key" }));
    await within(card).findByRole("status");
    await waitFor(() =>
      expect(
        useConversations
          .getState()
          .views.setup?.available_models?.map((m) => m.id),
      ).toContain("openrouter/anthropic/claude-sonnet-4"),
    );

    await user.keyboard("{Escape}");
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "Engine & Models" }),
      ).not.toBeInTheDocument(),
    );
    await user.click(screen.getByTestId("composer-model"));
    await user.click(screen.getByTestId("composer-model-row"));
    const section = screen.getByRole("group", { name: "OpenRouter" });
    expect(section).toHaveTextContent("Connected");
    await user.type(
      screen.getByTestId("composer-model-search"),
      "openrouter claude",
    );
    const options = screen.getAllByRole("option");
    expect(options.length).toBe(2);
    expect(options[0]).toHaveTextContent("anthropic/claude-sonnet-4");
  });

  async function connectAnthropic(
    user: ReturnType<typeof userEvent.setup>,
    card: HTMLElement,
  ) {
    await user.type(
      within(card).getByRole("searchbox", { name: "Search providers" }),
      "anthropic",
    );
    const row = within(card).getByTestId("provider-anthropic");
    await user.click(
      within(row).getByRole("button", { name: "Connect Anthropic" }),
    );
    await user.type(
      within(row).getByLabelText("API key for Anthropic"),
      "not-a-real-key",
    );
    await user.click(within(row).getByRole("button", { name: "Save key" }));
    await within(card).findByRole("status");
    return within(card).getByTestId("provider-anthropic");
  }

  it.each(["invalid", "unverified"] as const)(
    "keeps the key visible with Verify, Replace and Remove after a %s verify",
    async (state) => {
      const { bridge, user, dialog } = await openProviderPanel();
      const card = within(dialog).getByTestId("engine-models-settings");
      await connectAnthropic(user, card);
      const verdict = { provider: "anthropic", state, last_checked_at: null };
      vi.spyOn(bridge.engine, "verifyKey").mockResolvedValue(verdict);
      // The desktop core records the verdict, so the next key listing agrees with it.
      vi.spyOn(bridge.engine, "keyStatus").mockResolvedValue([verdict]);
      const row = within(card).getByTestId("provider-anthropic");
      await user.click(within(row).getByRole("button", { name: "Verify key" }));
      await waitFor(() =>
        expect(
          within(card).getByTestId("provider-anthropic"),
        ).toHaveTextContent(state === "invalid" ? "Invalid key" : "Unverified"),
      );
      const kept = within(card).getByTestId("provider-anthropic");
      expect(
        within(kept).getByRole("button", { name: "Verify key" }),
      ).toBeInTheDocument();
      expect(
        within(kept).getByRole("button", { name: "Replace Anthropic key" }),
      ).toBeInTheDocument();
      expect(
        within(kept).getByRole("button", { name: "Remove Anthropic key" }),
      ).toBeInTheDocument();
    },
  );

  it("shows Verifying… while checking, then Verified, and keeps the key", async () => {
    const { bridge, user, dialog } = await openProviderPanel();
    const card = within(dialog).getByTestId("engine-models-settings");
    const row = await connectAnthropic(user, card);
    let finish!: (value: {
      provider: string;
      state: "saved";
      last_checked_at: string;
    }) => void;
    const pending = new Promise<{
      provider: string;
      state: "saved";
      last_checked_at: string;
    }>((resolve) => {
      finish = resolve;
    });
    vi.spyOn(bridge.engine, "verifyKey").mockReturnValue(pending);
    await user.click(within(row).getByRole("button", { name: "Verify key" }));
    const busy = await within(card).findByRole("button", {
      name: "Verifying…",
    });
    expect(busy).toBeDisabled();
    const verdict = {
      provider: "anthropic",
      state: "saved" as const,
      last_checked_at: "2026-11-21T00:00:00Z",
    };
    vi.spyOn(bridge.engine, "keyStatus").mockResolvedValue([verdict]);
    finish(verdict);
    await waitFor(() =>
      expect(within(card).getByTestId("provider-anthropic")).toHaveTextContent(
        "Verified",
      ),
    );
    expect(
      within(within(card).getByTestId("provider-anthropic")).getByRole(
        "button",
        {
          name: "Remove Anthropic key",
        },
      ),
    ).toBeInTheDocument();
  });

  it("opens an inline key form from Replace key", async () => {
    const { user, dialog } = await openProviderPanel();
    const card = within(dialog).getByTestId("engine-models-settings");
    const row = await connectAnthropic(user, card);
    await user.click(
      within(row).getByRole("button", { name: "Replace Anthropic key" }),
    );
    expect(
      within(within(card).getByTestId("provider-anthropic")).getByLabelText(
        "API key for Anthropic",
      ),
    ).toBeInTheDocument();
  });

  it("shows an explicit alert when verify reports the key missing", async () => {
    const { bridge, user, dialog } = await openProviderPanel();
    const card = within(dialog).getByTestId("engine-models-settings");
    const row = await connectAnthropic(user, card);
    vi.spyOn(bridge.engine, "verifyKey").mockResolvedValue({
      provider: "anthropic",
      state: "missing",
      last_checked_at: null,
    });
    await user.click(within(row).getByRole("button", { name: "Verify key" }));
    const alert = await within(card).findByRole("alert");
    expect(alert).toHaveTextContent(/not found in this Mac's private provider-key file/);
    expect(card).not.toHaveTextContent("still unverified");
  });

  it("refreshes engine, keys and setup models after Verify key", async () => {
    const { bridge, user, dialog } = await openProviderPanel();
    const card = within(dialog).getByTestId("engine-models-settings");
    const row = await connectAnthropic(user, card);
    const attach = vi.spyOn(bridge.chat!, "attach");
    const keyStatus = vi.spyOn(bridge.engine, "keyStatus");
    const status = vi.spyOn(bridge.engine, "status");
    await user.click(within(row).getByRole("button", { name: "Verify key" }));
    await waitFor(() => expect(attach).toHaveBeenCalledWith("setup"));
    expect(keyStatus).toHaveBeenCalled();
    expect(status).toHaveBeenCalled();
  });

  it("labels a label-less openrouter entry as OpenRouter in rows and notices", async () => {
    const { user, dialog } = await openProviderPanel();
    const engine = useFleetStore.getState().engine;
    expect(engine?.providers).toBeDefined();
    act(() => {
      useFleetStore.setState({
        engine: {
          ...engine!,
          providers: engine!.providers!.map((p) =>
            p.id === "openrouter"
              ? ({ id: p.id, connected: p.connected } as typeof p)
              : p,
          ),
        },
      });
    });
    const card = within(dialog).getByTestId("engine-models-settings");
    await user.type(
      within(card).getByRole("searchbox", { name: "Search providers" }),
      "openrouter",
    );
    const row = within(card).getByTestId("provider-openrouter");
    expect(row).toHaveTextContent("OpenRouter");
    expect(row).not.toHaveTextContent("Openrouter");
    await user.click(
      within(row).getByRole("button", { name: "Connect OpenRouter" }),
    );
    await user.type(
      within(row).getByLabelText("API key for OpenRouter"),
      "invalid-key-value",
    );
    await user.click(within(row).getByRole("button", { name: "Save key" }));
    const notice = await within(card).findByRole("status");
    expect(notice).toHaveTextContent("OpenRouter rejected this key");
    expect(notice).not.toHaveTextContent("Openrouter");
  });

  it("shows the opencode-go provider as OpenCode Go", async () => {
    const { dialog } = await openProviderPanel();
    const card = within(dialog).getByTestId("engine-models-settings");
    expect(
      within(within(card).getByTestId("provider-opencode")).getByRole(
        "button",
        {
          name: "Connect OpenCode Go",
        },
      ),
    ).toBeInTheDocument();
  });

  it("groups a provider the engine reports as connected without a keychain key", async () => {
    const { dialog } = await openProviderPanel();
    const card = within(dialog).getByTestId("engine-models-settings");
    const section = within(card).getByRole("list", {
      name: "Connected providers",
    });
    const google = within(section).getByTestId("provider-google");
    expect(google).toHaveTextContent("Connected");
    expect(google).toHaveTextContent("Connected outside the keychain");
    expect(
      within(card).getByRole("list", { name: "Available providers" }),
    ).not.toHaveTextContent("Google");
  });

  it("says the engine is stopped instead of inventing providers", async () => {
    const { dialog } = await openProviderPanel("stopped");
    const card = within(dialog).getByTestId("engine-models-settings");
    expect(within(card).getByText(/StackSpirit is not running/i)).toBeInTheDocument();
    expect(
      within(card).getByRole("searchbox", { name: "Search providers" }),
    ).toBeDisabled();
    expect(
      within(card).queryByTestId("provider-anthropic"),
    ).not.toBeInTheDocument();
  });
});

describe("Settings default-model badge", () => {
  const free: ModelChoice = {
    id: "big-pickle",
    label: "Big Pickle",
    provider: "opencode",
    access: "included",
    context_tokens: null,
    notes: "",
    verified_at: null,
  };
  const paid: ModelChoice = {
    ...free,
    id: "deepseek-chat",
    label: "DeepSeek Chat",
    provider: "deepseek",
    access: "byok",
  };

  it("shows an explicit selection, else the first free model, else No model", () => {
    expect(defaultModelLabel(paid, [free, paid])).toBe(
      "Default · DeepSeek Chat",
    );
    expect(defaultModelLabel(null, [paid, free])).toBe("Default · Big Pickle");
    expect(defaultModelLabel(null, [paid])).toBe("Default · DeepSeek Chat");
    expect(defaultModelLabel(null, [])).toBe("No model");
  });

  it("never says No model beside a verified free model count", async () => {
    const { dialog } = await openProviderPanel();
    const card = within(dialog).getByTestId("engine-models-settings");
    await waitFor(() =>
      expect(card).toHaveTextContent(/free models? verified/),
    );
    expect(card).not.toHaveTextContent("No model");
    expect(card).toHaveTextContent("Default ·");
  });
});
