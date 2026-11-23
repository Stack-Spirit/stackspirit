import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { SpiritBridge } from "../../bridge/SpiritBridge";
import type { ChatModelView, ChatUsageView } from "../../bridge/wire";
import { MockBridge } from "../../bridge/MockBridge";
import { ComposerBar, groupChatModels } from "../../next/ComposerBar";
import { ContextMeter } from "../../next/ContextMeter";
import { resetConversations, useConversations } from "../../next/conversations";
import { NextShell } from "../../next/NextShell";
import { resetShellUi } from "../../next/shellUi";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";

const bridges: SpiritBridge[] = [];

async function renderWithBridge(bridge: SpiritBridge) {
  resetFleetStore();
  resetConversations();
  bridges.push(bridge);
  const result = render(<NextShell bridge={bridge} />);
  await waitFor(() => expect(useFleetStore.getState().hostsLoaded).toBe(true));
  return { bridge, ...result };
}

async function openSetup(bridge: SpiritBridge = new MockBridge("empty")) {
  const user = userEvent.setup();
  const rendered = await renderWithBridge(bridge);
  await user.click(screen.getByRole("button", { name: /Set up with AI/ }));
  return { user, ...rendered };
}

afterEach(() => {
  for (const bridge of bridges.splice(0)) bridge.dispose?.();
  resetConversations();
  resetShellUi();
  resetFleetStore();
  delete document.documentElement.dataset.ui;
  delete document.documentElement.dataset.theme;
});

function usage(overrides: Partial<ChatUsageView> = {}): ChatUsageView {
  return {
    context_tokens_used: 23_600,
    context_limit: 128_000,
    input_tokens: 12_800,
    output_tokens: 2_400,
    cache_read_tokens: 8_400,
    turns: 3,
    cost_usd: 0,
    ...overrides,
  };
}

describe("Meadow composer", () => {
  it("shows the empty hero without a password wall and opens the secure card on demand", async () => {
    const { user } = await openSetup();

    expect(screen.getByRole("heading", { name: "StackSpirit", level: 2 })).toBeInTheDocument();
    expect(screen.queryByRole("form", { name: "Secure server details" })).not.toBeInTheDocument();
    expect(screen.queryByLabelText("One-time password from your provider")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Message" })).toBeEnabled();

    const model = await screen.findByTestId("composer-model");
    expect(model).toHaveTextContent("OpenCode free · DeepSeek V4.1 Flash Free");

    await user.click(screen.getByTestId("composer-connect-server"));
    expect(
      await screen.findByRole("form", { name: "Secure server details" }),
    ).toBeInTheDocument();
  });

  it("keeps the same composer node and focus when the hero moves to the docked layout", async () => {
    const { user } = await openSetup();
    const composer = screen.getByRole("textbox", { name: "Message" });
    await user.type(composer, "hold this composer{Enter}");

    expect(await screen.findByText(/Preview reply\./)).toBeInTheDocument();
    const docked = screen.getByRole("textbox", { name: "Message" });
    expect(docked).toBe(composer);
    await waitFor(() => expect(document.activeElement).toBe(docked));
    expect(screen.queryByRole("heading", { name: "StackSpirit", level: 2 })).not.toBeInTheDocument();
  });

  it("restores focus to the Message field when a slow turn returns to ready", async () => {
    const { user } = await openSetup(new MockBridge({ scenario: "chat-slow", hosts: [] }));
    const composer = screen.getByRole("textbox", { name: "Message" });
    await user.type(composer, "hold the turn open{Enter}");

    await waitFor(() =>
      expect(screen.getByTestId("session-state")).toHaveTextContent("Replying…"),
    );
    // A disabled control cannot keep focus in a browser (Chromium is covered by
    // the desktop e2e). The field must not remount or forget that it owned focus,
    // so it is returnable once the session is ready again.
    expect(composer).toBeDisabled();

    await user.click(screen.getByRole("button", { name: "Stop" }));
    await waitFor(() =>
      expect(screen.getByTestId("session-state")).toHaveTextContent("Ready"),
    );
    expect(composer).toBeEnabled();
    expect(screen.queryByRole("button", { name: "Start" })).not.toBeInTheDocument();
    await waitFor(() => expect(document.activeElement).toBe(composer));

    // Stop is a turn control: the same composer accepts the next message right away.
    await user.type(composer, "continue with another model{Enter}");
    expect(await screen.findByText(/Preview reply\./)).toBeInTheDocument();
  });

  it("keeps the included free OpenCode model active and switches it inline", async () => {
    const { user, bridge } = await openSetup();
    const model = await screen.findByTestId("composer-model");
    expect(model).toHaveTextContent("OpenCode free · DeepSeek V4.1 Flash Free");
    expect(useConversationsView()?.model?.tier).toBe("included");

    const setModel = vi.spyOn(bridge.chat!, "setModel");
    await user.click(model);
    await user.click(screen.getByTestId("composer-model-row"));
    const popover = screen.getByTestId("model-picker-popover");
    expect(popover).toHaveAttribute("role", "dialog");
    expect(screen.getByTestId("composer-model-search")).toHaveFocus();
    const list = within(popover).getByRole("listbox");
    expect(within(list).getByRole("option", { name: /Big Pickle/ })).toBeInTheDocument();
    expect(within(list).getByRole("option", { name: /DeepSeek V4.1 Flash(?! Free)/ })).toBeInTheDocument();

    await user.click(within(list).getByRole("option", { name: /Big Pickle/ }));
    await waitFor(() =>
      expect(setModel).toHaveBeenCalledWith("setup", "opencode/big-pickle"),
    );
    expect(screen.queryByTestId("model-picker-popover")).not.toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByTestId("composer-model")).toHaveTextContent(
        "OpenCode free · Big Pickle",
      ),
    );
  });

  it("groups the picker by OpenCode Free and named provider sections", async () => {
    const { user } = await openSetup();
    await user.click(await screen.findByTestId("composer-model"));
    await user.click(screen.getByTestId("composer-model-row"));
    const popover = screen.getByTestId("model-picker-popover");
    const groups = within(popover).getAllByRole("group");
    expect(groups.map((group) => group.getAttribute("aria-label"))).toEqual([
      "OpenCode Free",
      "DeepSeek",
    ]);
    expect(groups[0]).toHaveTextContent("Free");
    expect(groups[0]).toHaveTextContent("DeepSeek V4.1 Flash Free");
    expect(groups[1]).toHaveTextContent("Connected");
    expect(groups[1]).not.toHaveTextContent("BYOK ·");
    expect(within(groups[0]!).getByRole("option", { selected: true })).toHaveTextContent("DeepSeek V4.1 Flash Free");
  });

  it("Manage models opens Engine & Models in place and closes the picker", async () => {
    const { user } = await openSetup();
    await user.click(await screen.findByTestId("composer-model"));
    await user.click(screen.getByTestId("composer-model-row"));
    await user.click(screen.getByTestId("composer-manage-models"));
    expect(screen.queryByTestId("model-picker-popover")).not.toBeInTheDocument();
    const dialog = await screen.findByRole("dialog", { name: "Engine & Models" });
    expect(within(dialog).getByTestId("engine-models-settings")).toBeInTheDocument();
  });

  it("Connect another provider opens Engine & Models", async () => {
    const { user } = await openSetup();
    await user.click(await screen.findByTestId("composer-model"));
    await user.click(screen.getByTestId("composer-model-row"));
    await user.click(screen.getByTestId("composer-connect-provider"));
    const dialog = await screen.findByRole("dialog", { name: "Engine & Models" });
    expect(within(dialog).getByTestId("engine-models-settings")).toBeInTheDocument();
  });

  it("renders the context ring, exact popover and compact stats pills", async () => {
    const { user } = await openSetup();

    const meter = await screen.findByTestId("context-meter");
    expect(meter).toHaveTextContent("18%");
    expect(meter).toHaveAttribute("aria-label", "Context 18% used");
    expect(screen.getByTestId("usage-pill-turns")).toHaveTextContent("3 turns");
    expect(screen.getByTestId("usage-pill-tokens")).toHaveTextContent("23.6K tok · Cache hit 40%");

    await user.click(meter);
    const popover = screen.getByTestId("context-popover");
    expect(popover).toHaveTextContent("23,600 / 128,000 tokens");
    expect(popover).toHaveTextContent("12,800 tokens");
    expect(popover).toHaveTextContent("2,400 tokens");
    expect(popover).toHaveTextContent("8,400 tokens");
    expect(screen.getByRole("group", { name: "Context usage" })).toBeInTheDocument();

    await user.keyboard("{Escape}");
    expect(screen.queryByTestId("context-popover")).not.toBeInTheDocument();
  });

  it("hides the context meter until usage reports used tokens and a limit", () => {
    const { container, rerender } = render(<ContextMeter usage={null} />);
    expect(container).toBeEmptyDOMElement();

    rerender(<ContextMeter usage={usage({ context_limit: null })} />);
    expect(container).toBeEmptyDOMElement();

    rerender(<ContextMeter usage={usage({ context_tokens_used: null })} />);
    expect(container).toBeEmptyDOMElement();

    rerender(<ContextMeter usage={usage()} />);
    expect(screen.getByTestId("context-meter")).toHaveTextContent("18%");
  });

  it("shows the honest no-model state with a Settings → Engine link", async () => {
    const base = new MockBridge("empty");
    if (base.chat) {
      const start = base.chat.start;
      base.chat.start = async (input) => ({
        ...(await start(input)),
        model: null,
        available_models: [],
      });
    }

    resetFleetStore();
    resetConversations();
    bridges.push(base);
    render(<NextShell bridge={base} />);
    await waitFor(() => expect(useFleetStore.getState().hostsLoaded).toBe(true));

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: /Set up with AI/ }));

    const setup = await screen.findByTestId("composer-setup-model");
    expect(setup).toHaveTextContent("Set up a model");
    expect(screen.queryByTestId("composer-model")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Message" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();

    await user.click(setup);
    expect(screen.getByRole("heading", { name: "Settings", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Hide advanced settings" })).toBeInTheDocument();
  });
});

function useConversationsView() {
  // Read the same store the shell reads without adding a hook to the test body.
  return useConversations.getState().views.setup ?? null;
}

const PICKER_MODELS: ChatModelView[] = [
  { id: "opencode/big-pickle", label: "Big Pickle", provider: "opencode", tier: "included" },
  { id: "openrouter/anthropic/claude-sonnet-4", label: "Claude Sonnet 4", provider: "openrouter", tier: "byok" },
  { id: "openrouter/google/gemini-2.5-pro", label: "Gemini 2.5 Pro", provider: "openrouter", tier: "byok" },
  { id: "anthropic/claude-sonnet-4", label: "Claude Sonnet 4", provider: "anthropic", tier: "byok" },
  { id: "deepseek/deepseek-flash", label: "DeepSeek Flash", provider: "deepseek", tier: "byok" },
  { id: "acme/zeta", label: "Zeta", provider: "acme", tier: "byok" },
];

function renderPicker(onSelect = vi.fn(), onOpenEngineSettings = vi.fn()) {
  const user = userEvent.setup();
  render(
    <ComposerBar
      session="setup"
      placeholder="Message"
      model={PICKER_MODELS[0]!}
      models={PICKER_MODELS}
      modelMissing={false}
      disabled={false}
      streaming={false}
      stopped={false}
      onSend={() => {}}
      onStop={() => {}}
      onStart={() => {}}
      onSelectModel={onSelect}
      onOpenEngineSettings={onOpenEngineSettings}
    />,
  );
  return { user, onSelect, onOpenEngineSettings };
}

describe("Searchable model picker", () => {
  it("orders sections: OpenCode Free, popular providers, then alphabetical", () => {
    expect(groupChatModels(PICKER_MODELS).map((group) => group.title)).toEqual([
      "OpenCode Free",
      "Anthropic",
      "OpenRouter",
      "Acme",
      "DeepSeek",
    ]);
  });

  it("shows provider sections with badges, counts, clean names and secondary ids", async () => {
    const { user } = renderPicker();
    await user.click(screen.getByTestId("composer-model"));
    await user.click(screen.getByTestId("composer-model-row"));
    const router = screen.getByRole("group", { name: "OpenRouter" });
    expect(router).toHaveTextContent("Connected");
    expect(router).toHaveTextContent("2");
    expect(router).toHaveTextContent("anthropic/claude-sonnet-4");
    expect(router).not.toHaveTextContent("BYOK");
    expect(screen.getByRole("group", { name: "OpenCode Free" })).toHaveTextContent("Free");
    const selected = screen.getByRole("option", { selected: true });
    expect(selected).toHaveTextContent("Big Pickle");
    expect(selected).toHaveTextContent("✓");
  });

  it("filters by model name, model id and provider name", async () => {
    const { user } = renderPicker();
    await user.click(screen.getByTestId("composer-model"));
    await user.click(screen.getByTestId("composer-model-row"));
    const search = screen.getByTestId("composer-model-search");
    expect(search).toHaveAttribute("type", "search");
    expect(search).toHaveAttribute("placeholder", "Search models…");

    await user.type(search, "gemini");
    expect(screen.getAllByRole("option")).toHaveLength(1);

    await user.clear(search);
    await user.type(search, "openrouter/anthropic");
    expect(screen.getAllByRole("option")).toHaveLength(1);

    await user.clear(search);
    await user.type(search, "OPENROUTER claude");
    const options = screen.getAllByRole("option");
    expect(options).toHaveLength(1);
    expect(options[0]).toHaveTextContent("Claude Sonnet 4");

    await user.clear(search);
    await user.type(search, "anthropic");
    expect(screen.getAllByRole("option").length).toBe(2);

    await user.clear(search);
    await user.type(search, "nothing-here");
    expect(screen.queryByRole("option")).not.toBeInTheDocument();
    expect(screen.getByText("No models match “nothing-here”.")).toBeInTheDocument();
  });

  it("navigates with ArrowDown/ArrowUp, selects with Enter and closes with Escape", async () => {
    const { user, onSelect } = renderPicker();
    await user.click(screen.getByTestId("composer-model"));
    await user.click(screen.getByTestId("composer-model-row"));
    const search = screen.getByTestId("composer-model-search");
    expect(search).toHaveFocus();
    // Highlight starts on the active model (Big Pickle), then moves across groups.
    await user.keyboard("{ArrowDown}{ArrowDown}");
    const highlighted = () =>
      document.getElementById(search.getAttribute("aria-activedescendant") ?? "");
    expect(highlighted()).toHaveTextContent("Claude Sonnet 4");
    expect(highlighted()).toHaveTextContent("anthropic/claude-sonnet-4".split("/")[1]!);
    await user.keyboard("{ArrowUp}");
    expect(highlighted()).toHaveTextContent("Claude Sonnet 4");
    await user.keyboard("{ArrowUp}");
    expect(highlighted()).toHaveTextContent("Big Pickle");

    await user.keyboard("{ArrowDown}{Enter}");
    expect(onSelect).toHaveBeenCalledWith("anthropic/claude-sonnet-4");
    expect(screen.queryByTestId("model-picker-popover")).not.toBeInTheDocument();

    await user.click(screen.getByTestId("composer-model"));

    await user.click(screen.getByTestId("composer-model-row"));
    // First Escape steps back to the two-row menu, the second closes it.
    await user.keyboard("{Escape}");
    expect(screen.getByTestId("composer-model-row")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByTestId("model-picker-popover")).not.toBeInTheDocument();
    expect(screen.getByTestId("composer-model")).toHaveFocus();
  });

  it("exposes a focusable, labelled scroll region that owns the listbox", async () => {
    const { user } = renderPicker();
    await user.click(screen.getByTestId("composer-model"));
    await user.click(screen.getByTestId("composer-model-row"));
    const scroller = screen.getByRole("region", { name: "Models" });
    expect(scroller).toHaveAttribute("tabindex", "0");
    expect(within(scroller).getByRole("listbox")).toBeInTheDocument();
    expect(screen.getByTestId("composer-model-search")).toHaveFocus();
    await user.tab();
    expect(scroller).toHaveFocus();
  });

  it("keeps the search header outside the scroll region so it cannot cover the first heading", async () => {
    const { user } = renderPicker();
    await user.click(screen.getByTestId("composer-model"));
    await user.click(screen.getByTestId("composer-model-row"));
    const scroller = screen.getByRole("region", { name: "Models" });
    const header = screen
      .getByTestId("composer-model-search")
      .closest(".nx-model-menu__search")!;
    expect(header).not.toBeNull();
    expect(scroller.contains(header)).toBe(false);
    const first = scroller.querySelector(".nx-model-menu__heading")!;
    expect(first).toHaveTextContent("OpenCode Free");
    expect(scroller.firstElementChild).toContainElement(first as HTMLElement);
    // Header precedes the scroller as a sibling (flex column), not a sticky overlay.
    expect(header.nextElementSibling).toBe(scroller);
  });

  it("still navigates with the keyboard and selects with the mouse", async () => {
    const { user, onSelect } = renderPicker();
    await user.click(screen.getByTestId("composer-model"));
    await user.click(screen.getByTestId("composer-model-row"));
    const search = screen.getByTestId("composer-model-search");
    await user.keyboard("{ArrowDown}");
    expect(
      document.getElementById(search.getAttribute("aria-activedescendant") ?? ""),
    ).toHaveTextContent("Claude Sonnet 4");
    await user.click(screen.getByRole("option", { name: /Zeta/ }));
    expect(onSelect).toHaveBeenCalledWith("acme/zeta");
  });

  it("Manage models opens Engine & Models in place (not onOpenEngineSettings)", async () => {
    const { user, onOpenEngineSettings } = renderPicker();
    await user.click(screen.getByTestId("composer-model"));
    await user.click(screen.getByTestId("composer-model-row"));
    await user.click(screen.getByTestId("composer-manage-models"));
    expect(screen.queryByTestId("model-picker-popover")).not.toBeInTheDocument();
    expect(await screen.findByRole("dialog", { name: "Engine & Models" })).toBeInTheDocument();
    expect(onOpenEngineSettings).not.toHaveBeenCalled();
  });
});
