import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type {
  ChatHarnessSettingsView,
  ChatSettingsProblemView,
  HarnessConnectStarted,
  HarnessListView,
  HarnessProbeReport,
  HarnessTarget,
} from "../../bridge/wire";
import type {
  HarnessesBridge,
  HarnessesSurface,
  HarnessManifest,
} from "../../next/HarnessesPanel";
import { HarnessesPanel } from "../../next/HarnessesPanel";

function manifest(overrides: Partial<HarnessManifest> & { id: string }): HarnessManifest {
  return {
    name: "OpenCode",
    binary: "opencode",
    install_cmd: "",
    auth_cmd: "",
    protocol: "opencode-http",
    surface: "orchestrator_and_workers",
    supports_embedded_chat: true,
    auth_modes: ["api_key"],
    tos_note: "BYOK: provider billing and terms apply.",
    docs_url: "https://example.invalid/docs",
    oauth_callback_port: null,
    builtin: true,
    ...overrides,
  };
}

function probeReport(item: HarnessManifest, target: HarnessTarget): HarnessProbeReport {
  return {
    probe: {
      harness_id: item.id,
      target,
      protocol: item.protocol,
      supports_embedded_chat: item.supports_embedded_chat,
      binary: "found",
      version: "1.0.0",
      auth: "configured",
      auth_detail:
        "A stored sign-in was reported. No key or token value was read.",
      readiness: "ready",
      oauth_callback_port: null,
      requires_loopback_forward: false,
      warnings: [],
    },
    plan: {
      install_command: null,
      auth_command: null,
      requires_loopback_forward: false,
      forward_port: null,
    },
  };
}

function fakeSurface(initial: HarnessManifest[]) {
  const harnesses = [...initial];
  const list = vi.fn(async (): Promise<HarnessListView> => ({
    harnesses: [...harnesses],
    warnings: [],
  }));
  const saveCustom = vi.fn(async (next: HarnessManifest) => next);
  const deleteCustom = vi.fn(async () => true);
  const probe = vi.fn(
    async ({
      id,
      target,
    }: {
      id: string;
      target: HarnessTarget;
    }): Promise<HarnessProbeReport> => {
      const item = harnesses.find((candidate) => candidate.id === id);
      if (!item) throw new Error("missing fake harness");
      return probeReport(item, target);
    },
  );
  const connectStart = vi.fn(
    async ({
      id,
      target,
    }: {
      id: string;
      target: HarnessTarget;
    }): Promise<HarnessConnectStarted> => ({
      harness_id: id,
      target,
      auth_started: false,
      browser_opened: false,
      forward_port: null,
      message: "Preview only; nothing was started.",
    }),
  );
  const surface: HarnessesSurface = {
    list,
    saveCustom,
    deleteCustom,
    probe,
    connectStart,
  };
  return { surface, list, saveCustom, deleteCustom, probe, connectStart };
}

function fakeChat(initial: ChatHarnessSettingsView) {
  const harnessSettings = vi.fn(
    async (): Promise<ChatHarnessSettingsView> => initial,
  );
  const setDefaultHarness = vi.fn(
    async (harnessId: string): Promise<ChatHarnessSettingsView> => {
      void harnessId;
      return initial;
    },
  );
  const start = vi.fn(async () => undefined);
  return {
    chat: { harnessSettings, setDefaultHarness, start },
    harnessSettings,
    setDefaultHarness,
    start,
  };
}

function bridge(
  surface: HarnessesSurface | undefined,
  chat: HarnessesBridge["chat"],
  kind: HarnessesBridge["kind"] = "tauri",
  preview = false,
): HarnessesBridge {
  return { kind, preview, harnesses: surface, chat };
}

const FRESH_SETTINGS: ChatHarnessSettingsView = {
  default_harness_id: null,
  gate: { state: "ready", harness_id: "opencode", protocol: "opencode-http" },
};

const ACP_MANIFEST = manifest({
  id: "cursor-cli",
  name: "Cursor CLI",
  binary: "cursor-agent",
  protocol: "acp",
  auth_modes: ["subscription"],
});

const WORKERS_ONLY_MANIFEST = manifest({
  id: "claude-code",
  name: "Claude Code",
  binary: "claude",
  protocol: "stream-json",
  surface: "workers_only_terminal",
  supports_embedded_chat: false,
  auth_modes: ["subscription"],
});

// A malformed combination that still must never appear in the chat options.
const CLI_PTY_ORCHESTRATOR = manifest({
  id: "aider",
  name: "Aider",
  binary: "aider",
  protocol: "cli-pty",
});

describe("HarnessesPanel chat transport", () => {
  it("shows OpenCode selected and ready for a fresh settings file", async () => {
    const fake = fakeSurface([manifest({ id: "opencode" })]);
    const chat = fakeChat(FRESH_SETTINGS);
    render(<HarnessesPanel bridge={bridge(fake.surface, chat.chat)} />);

    const block = await screen.findByTestId("harness-chat-transport");
    const select = await within(block).findByLabelText("Default chat harness");
    expect(select).toHaveValue("opencode");
    expect(within(block).getByTestId("harness-chat-transport-selected")).toHaveTextContent(
      "OpenCode",
    );
    expect(within(block).getByTestId("harness-chat-transport-state")).toHaveTextContent(
      "ready",
    );
    expect(within(block).getByTestId("harness-chat-transport-gate")).toHaveTextContent(
      /OpenCode is ready for new chats/,
    );

    const scope = within(block).getByTestId("harness-chat-transport-scope");
    expect(scope).toHaveTextContent(/new app-owned local conversations only/i);
    expect(scope).toHaveTextContent(/global/);
    expect(scope).toHaveTextContent(/project/);
    expect(scope).toHaveTextContent(/global\/project chats stay OpenCode/);
    expect(scope).toHaveTextContent(/existing sessions do not move/i);

    expect(chat.harnessSettings).toHaveBeenCalledTimes(1);
    expect(chat.start).not.toHaveBeenCalled();
    expect(within(block).queryByRole("button", { name: "Use OpenCode" })).toBeNull();
  });

  it("lists an ACP orchestrator, saves once and renders harness_not_wired without starting a chat", async () => {
    const user = userEvent.setup();
    const fake = fakeSurface([
      manifest({ id: "opencode" }),
      ACP_MANIFEST,
      WORKERS_ONLY_MANIFEST,
      CLI_PTY_ORCHESTRATOR,
    ]);
    const chat = fakeChat(FRESH_SETTINGS);
    let resolveSave!: (view: ChatHarnessSettingsView) => void;
    chat.setDefaultHarness.mockImplementation(
      (harnessId: string) =>
        new Promise<ChatHarnessSettingsView>((resolve) => {
          expect(harnessId).toBe("cursor-cli");
          resolveSave = resolve;
        }),
    );

    render(<HarnessesPanel bridge={bridge(fake.surface, chat.chat)} />);

    const block = await screen.findByTestId("harness-chat-transport");
    const select = await within(block).findByLabelText("Default chat harness");
    await waitFor(() =>
      expect(
        within(select).getByRole("option", { name: /Cursor CLI/ }),
      ).toHaveTextContent("harness_not_wired"),
    );
    expect(
      within(select).queryByRole("option", { name: /Claude Code/ }),
    ).toBeNull();
    expect(within(select).queryByRole("option", { name: /Aider/ })).toBeNull();

    await user.selectOptions(select, "cursor-cli");
    expect(chat.setDefaultHarness).toHaveBeenCalledTimes(1);
    expect(chat.setDefaultHarness).toHaveBeenCalledWith("cursor-cli");
    expect(select).toBeDisabled();
    expect(
      within(block).getByTestId("harness-chat-transport-busy"),
    ).toHaveTextContent("Saving the chat default…");

    await act(async () => {
      resolveSave({
        default_harness_id: "cursor-cli",
        gate: {
          state: "harness_not_wired",
          harness_id: "cursor-cli",
          protocol: "acp",
        },
      });
    });

    const gate = await within(block).findByTestId("harness-chat-transport-gate");
    expect(gate).toHaveTextContent("harness_not_wired");
    expect(gate).toHaveTextContent(/chat adapter is not wired yet/i);
    expect(gate).toHaveTextContent(/new setup chat will return harness_not_wired/i);
    expect(gate).toHaveTextContent(/start no process/i);
    expect(within(block).getByTestId("harness-chat-transport-state")).toHaveTextContent(
      "harness_not_wired",
    );
    expect(select).toHaveValue("cursor-cli");
    expect(chat.start).not.toHaveBeenCalled();
  });

  it("writes opencode explicitly when the owner chooses Use OpenCode", async () => {
    const user = userEvent.setup();
    const fake = fakeSurface([manifest({ id: "opencode" }), ACP_MANIFEST]);
    const chat = fakeChat({
      default_harness_id: "cursor-cli",
      gate: {
        state: "harness_not_wired",
        harness_id: "cursor-cli",
        protocol: "acp",
      },
    });
    chat.setDefaultHarness.mockImplementation(async (harnessId: string) => {
      expect(harnessId).toBe("opencode");
      return FRESH_SETTINGS;
    });

    render(<HarnessesPanel bridge={bridge(fake.surface, chat.chat)} />);

    const block = await screen.findByTestId("harness-chat-transport");
    const button = await within(block).findByRole("button", {
      name: "Use OpenCode",
    });
    await user.click(button);

    expect(chat.setDefaultHarness).toHaveBeenCalledTimes(1);
    expect(chat.setDefaultHarness).toHaveBeenCalledWith("opencode");
    await waitFor(() =>
      expect(
        within(block).getByTestId("harness-chat-transport-state"),
      ).toHaveTextContent("ready"),
    );
    expect(within(block).getByTestId("harness-chat-transport-selected")).toHaveTextContent(
      "OpenCode",
    );
    expect(within(block).queryByRole("button", { name: "Use OpenCode" })).toBeNull();
    expect(chat.start).not.toHaveBeenCalled();
  });

  const unreadableCases: Array<{
    label: string;
    problem: ChatSettingsProblemView;
    copy: RegExp;
  }> = [
    {
      label: "corrupt",
      problem: { reason: "unreadable" },
      copy: /could not read it/i,
    },
    {
      label: "unsupported-version",
      problem: { reason: "unsupported_version", found: 2, supported: 1 },
      copy: /settings version 2; this app supports version 1/i,
    },
  ];

  it.each(unreadableCases)(
    "shows the $label settings gate with no write action and no silent reset",
    async ({ problem, copy }) => {
      const fake = fakeSurface([manifest({ id: "opencode" })]);
      const chat = fakeChat({
        default_harness_id: null,
        gate: { state: "chat_settings_unreadable", problem },
      });
      render(<HarnessesPanel bridge={bridge(fake.surface, chat.chat)} />);

      const block = await screen.findByTestId("harness-chat-transport");
      const gate = await within(block).findByTestId("harness-chat-transport-gate");
      expect(within(block).getByTestId("harness-chat-transport-state")).toHaveTextContent(
        "chat_settings_unreadable",
      );
      expect(gate).toHaveTextContent(copy);
      expect(gate).toHaveTextContent(/will not overwrite it/i);
      expect(gate).toHaveTextContent(/Fix or remove the file explicitly/i);
      expect(within(block).queryByLabelText("Default chat harness")).toBeNull();
      expect(within(block).queryByRole("button", { name: "Use OpenCode" })).toBeNull();
      expect(chat.setDefaultHarness).not.toHaveBeenCalled();
      expect(chat.harnessSettings).toHaveBeenCalledTimes(1);
    },
  );

  it("shows harness_missing without silently resetting the stored default", async () => {
    const fake = fakeSurface([manifest({ id: "opencode" })]);
    const chat = fakeChat({
      default_harness_id: "gone-harness",
      gate: { state: "harness_missing", harness_id: "gone-harness" },
    });
    render(<HarnessesPanel bridge={bridge(fake.surface, chat.chat)} />);

    const block = await screen.findByTestId("harness-chat-transport");
    const gate = await within(block).findByTestId("harness-chat-transport-gate");
    expect(within(block).getByTestId("harness-chat-transport-state")).toHaveTextContent(
      "harness_missing",
    );
    expect(gate).toHaveTextContent(/not in the harness registry/i);
    expect(gate).toHaveTextContent(/did not change the saved default/i);
    expect(within(block).getByLabelText("Default chat harness")).toHaveValue("");
    expect(within(block).queryByTestId("harness-chat-transport-selected")).toBeNull();
    expect(within(block).getByRole("button", { name: "Use OpenCode" })).toBeInTheDocument();
    expect(chat.setDefaultHarness).not.toHaveBeenCalled();
  });

  it("renders no control when the bridge has no harness settings method", async () => {
    const fake = fakeSurface([manifest({ id: "opencode" })]);
    const setDefaultHarness = vi.fn(
      async (harnessId: string): Promise<ChatHarnessSettingsView> => {
        void harnessId;
        return FRESH_SETTINGS;
      },
    );
    render(
      <HarnessesPanel
        bridge={bridge(fake.surface, { setDefaultHarness })}
      />,
    );

    await screen.findByTestId("harness-opencode");
    expect(screen.queryByTestId("harness-chat-transport")).toBeNull();
  });

  it("omits the control in preview and non-desktop bridges", async () => {
    const fake = fakeSurface([manifest({ id: "opencode" })]);
    const chat = fakeChat(FRESH_SETTINGS);
    const preview = render(
      <HarnessesPanel bridge={bridge(fake.surface, chat.chat, "mock", true)} />,
    );
    await screen.findByTestId("harness-opencode");
    expect(screen.queryByTestId("harness-chat-transport")).toBeNull();
    preview.unmount();

    render(<HarnessesPanel bridge={bridge(fake.surface, chat.chat, "http-sse")} />);
    await screen.findByTestId("harness-opencode");
    expect(screen.queryByTestId("harness-chat-transport")).toBeNull();
    expect(chat.harnessSettings).not.toHaveBeenCalled();
  });
});
