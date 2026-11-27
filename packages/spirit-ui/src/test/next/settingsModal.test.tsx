import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import type { SpiritBridge } from "../../bridge/SpiritBridge";
import type {
  ConnectionSnapshot,
  HarnessConnectStarted,
  HarnessListView,
  HarnessManifest,
  HarnessProbeReport,
  HarnessTarget,
  HostProfile,
} from "../../bridge/wire";
import { resetCloudAccount } from "../../cloud/store";
import { mcpConfigSnippet } from "../../next/CliMcpConnect";
import { resetConversations } from "../../next/conversations";
import { NextShell } from "../../next/NextShell";
import { resetShellUi } from "../../next/shellUi";
import { SettingsView } from "../../next/SettingsView";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";

const here = dirname(fileURLToPath(import.meta.url));
const bridges: SpiritBridge[] = [];
const AT = "2026-11-27T10:00:00.000Z";

afterEach(() => {
  for (const bridge of bridges.splice(0)) bridge.dispose?.();
  resetCloudAccount();
  resetConversations();
  resetShellUi();
  resetFleetStore();
  delete (navigator as unknown as { clipboard?: Clipboard }).clipboard;
  delete document.documentElement.dataset.ui;
  delete document.documentElement.dataset.theme;
});

async function renderShell() {
  resetFleetStore();
  resetConversations();
  resetShellUi();
  resetCloudAccount();
  const bridge = new MockBridge("busy");
  bridges.push(bridge);
  render(<NextShell bridge={bridge} />);
  await waitFor(() => expect(useFleetStore.getState().hostsLoaded).toBe(true));
  return bridge;
}

function manifest(overrides: Partial<HarnessManifest> & { id: string }): HarnessManifest {
  return {
    name: "Claude Code",
    binary: "claude",
    install_cmd: "npm install -g @anthropic-ai/claude-code",
    auth_cmd: "claude auth login",
    protocol: "stream-json",
    supports_embedded_chat: true,
    auth_modes: ["api_key", "subscription"],
    tos_note: "API-key use follows API terms; subscription use follows consumer terms.",
    docs_url: "https://docs.anthropic.com/en/docs/claude-code",
    oauth_callback_port: 54545,
    builtin: true,
    ...overrides,
  };
}

function fakeHarnesses() {
  const harnesses = [
    manifest({ id: "opencode", name: "OpenCode", binary: "opencode" }),
    manifest({ id: "claude-code", name: "Claude Code" }),
    manifest({ id: "codex", name: "Codex", binary: "codex", protocol: "codex-json" }),
    manifest({ id: "gemini-cli", name: "Gemini CLI", binary: "gemini", protocol: "acp" }),
    manifest({
      id: "antigravity",
      name: "Antigravity",
      binary: "antigravity",
      protocol: "cli-pty",
      supports_embedded_chat: false,
    }),
    manifest({
      id: "cursor-cli",
      name: "Cursor CLI",
      binary: "cursor-agent",
      protocol: "cli-pty",
      supports_embedded_chat: false,
    }),
    manifest({ id: "aider", name: "Aider", binary: "aider", protocol: "cli-pty" }),
    manifest({ id: "goose", name: "Goose", binary: "goose", protocol: "acp" }),
  ];
  const list = vi.fn(
    async (): Promise<HarnessListView> => ({ harnesses, warnings: [] }),
  );
  const connectStart = vi.fn(
    async (input: { id: string; target: HarnessTarget }): Promise<HarnessConnectStarted> => ({
      harness_id: input.id,
      target: input.target,
      auth_started: true,
      browser_opened: true,
      forward_port: input.target === "home_vps" ? 54545 : null,
      message: "Started the sign-in in your browser.",
    }),
  );
  const probe = vi.fn(
    async (input: { id: string; target: HarnessTarget }): Promise<HarnessProbeReport> => ({
      probe: {
        harness_id: input.id,
        target: input.target,
        protocol: "stream-json",
        supports_embedded_chat: true,
        binary: "found",
        version: "1.2.3",
        auth: "configured",
        auth_detail: "A stored sign-in was reported.",
        readiness: "ready",
        oauth_callback_port: 54545,
        requires_loopback_forward: input.target === "home_vps",
        warnings: [],
      },
      plan: {
        install_command: null,
        auth_command: "claude auth login",
        requires_loopback_forward: input.target === "home_vps",
        forward_port: input.target === "home_vps" ? 54545 : null,
      },
    }),
  );
  return {
    harnesses,
    surface: {
      list,
      saveCustom: async (next: HarnessManifest) => next,
      deleteCustom: async () => true,
      probe,
      connectStart,
    },
    connectStart,
  };
}

function snapshot(state: ConnectionSnapshot["state"], hostId: string | null): ConnectionSnapshot {
  return {
    state,
    host_id: hostId,
    since: state === "connected" ? AT : null,
    latency: null,
    last_ok_at: state === "connected" ? AT : null,
    error: null,
  };
}

function host(): HostProfile {
  return {
    id: "host-settings",
    label: "Studio VPS",
    ssh: { user: "owner", host: "203.0.113.90", port: 22 },
    identity_file: null,
    vowp_home: null,
    default_project: null,
    host_key: {
      algorithm: "ssh-ed25519",
      sha256: "SHA256:settingsModalHost0000000000000000000000",
      pinned_at: AT,
    },
    created_at: AT,
    updated_at: AT,
    last_connected_at: AT,
  };
}

async function installDesktopBridge() {
  const fake = fakeHarnesses();
  const mock = new MockBridge("busy");
  bridges.push(mock);
  const bridge = {
    ...mock,
    kind: "tauri" as const,
    preview: false,
    harnesses: fake.surface,
  } as unknown as SpiritBridge;
  resetFleetStore();
  useFleetStore.setState({
    bridge,
    preview: false,
    status: "ready",
    connection: snapshot("connected", "host-settings"),
    hosts: [host()],
    hostsLoaded: true,
    selectedHostId: "host-settings",
  });
  return { bridge, fake };
}

describe("Settings modal shell", () => {
  it("opens from the sidebar with a blurred backdrop, a focus trap and Esc/close", async () => {
    const user = userEvent.setup();
    await renderShell();
    expect(screen.queryByRole("dialog", { name: "Settings" })).not.toBeInTheDocument();

    await user.click(screen.getByTestId("settings-trigger"));
    const dialog = await screen.findByRole("dialog", { name: "Settings" });
    expect(dialog).toHaveClass("nx-settings-modal");
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(screen.getByTestId("settings-backdrop")).toBeInTheDocument();
    expect(screen.getByTestId("settings-view")).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "General" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByTestId("settings-pane-general")).toHaveTextContent("Theme");
    expect(dialog.contains(document.activeElement)).toBe(true);

    // Tab is trapped: many tabs later focus is still inside the dialog, never the page.
    for (let index = 0; index < 30; index += 1) await user.tab();
    expect(dialog.contains(document.activeElement)).toBe(true);

    await user.keyboard("{Escape}");
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: "Settings" })).not.toBeInTheDocument(),
    );

    await user.click(screen.getByTestId("settings-trigger"));
    await user.click(await screen.findByTestId("settings-close"));
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: "Settings" })).not.toBeInTheDocument(),
    );
  });

  it("switches every left-rail category, including Account and the account shortcut", async () => {
    const user = userEvent.setup();
    await renderShell();
    await user.click(screen.getByTestId("settings-trigger"));
    const dialog = await screen.findByRole("dialog", { name: "Settings" });

    await user.click(within(dialog).getByRole("tab", { name: "Account" }));
    expect(within(dialog).getByTestId("settings-pane-account")).toHaveTextContent(
      "StackSpirit Account",
    );
    expect(within(dialog).getByTestId("settings-pane-account")).toHaveTextContent(
      "Managed Homes (EUR & USD)",
    );

    await user.click(within(dialog).getByRole("tab", { name: "Server & Machines" }));
    const server = within(dialog).getByTestId("settings-pane-server");
    expect(server).toHaveTextContent("Home server");
    expect(server).toHaveTextContent("Always-On Machines");
    expect(server).toHaveTextContent("Off-site encrypted backups");

    await user.click(within(dialog).getByRole("tab", { name: "Harnesses" }));
    expect(await within(dialog).findByTestId("harnesses-panel")).toBeInTheDocument();

    await user.click(within(dialog).getByRole("tab", { name: "CLI & MCP Connect" }));
    expect(within(dialog).getByTestId("cli-mcp-connect")).toBeInTheDocument();

    await user.click(within(dialog).getByRole("tab", { name: "General" }));
    expect(within(dialog).getByRole("group", { name: "Theme" })).toBeInTheDocument();

    // Arrow keys move the tab selection and focus, like a vertical tablist.
    const general = within(dialog).getByRole("tab", { name: "General" });
    general.focus();
    await user.keyboard("{ArrowDown}");
    expect(within(dialog).getByRole("tab", { name: "Account" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(document.activeElement).toBe(
      within(dialog).getByRole("tab", { name: "Account" }),
    );

    await user.keyboard("{Escape}");
    await user.click(screen.getByTestId("account-trigger"));
    expect(await screen.findByRole("dialog", { name: "Settings" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Account" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });
});

describe("CLI & MCP Connect", () => {
  it("switches between This Mac and Home, copies the MCP config and starts a real sign-in", async () => {
    const user = userEvent.setup();
    const { fake } = await installDesktopBridge();
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    render(<SettingsView theme="system" onTheme={() => undefined} initialSection="cli-mcp" />);

    expect(await screen.findByTestId("cli-mcp-connect")).toBeInTheDocument();
    const local = screen.getByTestId("cli-mcp-target-local");
    expect(local).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByTestId("cli-mcp-target-home")).toHaveAttribute(
      "aria-pressed",
      "false",
    );

    const localSnippet = JSON.parse(screen.getByTestId("mcp-snippet").textContent ?? "");
    expect(localSnippet).toEqual({
      mcpServers: { stackspirit: { command: "stackspirit", args: ["mcp"] } },
    });
    await user.click(screen.getByTestId("copy-mcp-config"));
    expect(writeText).toHaveBeenCalledWith(mcpConfigSnippet("local", "Studio VPS"));
    expect(await screen.findByTestId("cli-mcp-copied")).toHaveTextContent(
      "MCP config copied.",
    );

    await user.click(screen.getByTestId("cli-mcp-target-home"));
    expect(screen.getByTestId("cli-mcp-target-home")).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    const homeSnippet = JSON.parse(screen.getByTestId("mcp-snippet").textContent ?? "");
    expect(homeSnippet).toEqual({
      mcpServers: {
        "stackspirit-studio-vps": { command: "stackspirit", args: ["mcp"] },
      },
    });
    expect(screen.getByTestId("cli-mcp-connect")).toHaveTextContent(
      "Home sign-ins use the host-owned loopback forward",
    );

    await user.click(screen.getByTestId("copy-cli-install"));
    expect(writeText).toHaveBeenCalledWith("npm install -g stackspirit");
    expect(await screen.findByTestId("cli-mcp-copied")).toHaveTextContent(
      "Install command copied.",
    );

    const claude = await screen.findByTestId("cli-tool-claude-code");
    await user.click(
      within(claude).getByRole("button", {
        name: "Connect Claude Code on Studio VPS",
      }),
    );
    expect(fake.connectStart).toHaveBeenCalledWith({
      id: "claude-code",
      target: "home_vps",
    });
    expect(await screen.findByTestId("cli-mcp-notice")).toHaveTextContent(
      "Claude Code · Studio VPS",
    );
    expect(await screen.findByTestId("cli-mcp-notice")).toHaveTextContent(
      /temporary loopback forward/i,
    );
    expect(within(claude).getByText("Sign-in started")).toBeInTheDocument();
  });

  it("disables the Home target without a connected Home and never fakes a connect", async () => {
    const fake = fakeHarnesses();
    const mock = new MockBridge("busy");
    bridges.push(mock);
    const bridge = {
      ...mock,
      kind: "tauri" as const,
      preview: false,
      harnesses: fake.surface,
    } as unknown as SpiritBridge;
    resetFleetStore();
    useFleetStore.setState({
      bridge,
      preview: false,
      status: "ready",
      connection: snapshot("disconnected", null),
      hosts: [host()],
      hostsLoaded: true,
      selectedHostId: "host-settings",
    });
    render(<SettingsView theme="system" onTheme={() => undefined} initialSection="cli-mcp" />);

    expect(await screen.findByTestId("cli-mcp-target-home")).toBeDisabled();
    expect(screen.getByTestId("cli-mcp-target-home")).toHaveTextContent("No Home");
    expect(screen.getByTestId("cli-mcp-connect")).toHaveTextContent(
      "Connect a Home VPS",
    );
    expect(fake.connectStart).not.toHaveBeenCalled();
  });

  it("states the CLI and MCP limits without a harness surface", async () => {
    const mock = new MockBridge("busy");
    bridges.push(mock);
    const bridge = {
      ...mock,
      kind: "http-sse" as const,
      preview: false,
    } as unknown as SpiritBridge;
    delete (bridge as unknown as { harnesses?: unknown }).harnesses;
    resetFleetStore();
    useFleetStore.setState({
      bridge,
      preview: false,
      status: "ready",
      connection: snapshot("disconnected", null),
      hosts: [host()],
      hostsLoaded: true,
      selectedHostId: "host-settings",
    });
    render(<SettingsView theme="system" onTheme={() => undefined} initialSection="cli-mcp" />);

    expect(await screen.findByTestId("cli-tools-unavailable")).toHaveTextContent(
      /does not expose it/,
    );
    expect(screen.getByTestId("cli-mcp-connect")).toHaveTextContent(
      "no ssh -L to paste",
    );
  });
});

describe("Settings appearance contracts", () => {
  it("keeps the OpenCode provider list inside a 240px internal scroll region", () => {
    const css = readFileSync(resolve(here, "../../next/theme.css"), "utf8");
    expect(css).toMatch(
      /\.nx-harness__provider-scroll\s*\{[^}]*max-height:\s*240px;/s,
    );
    expect(css).toMatch(
      /\.nx-harness__provider-scroll\s*\{[^}]*overflow-y:\s*auto;/s,
    );
    expect(css).toMatch(
      /\.nx-settings-backdrop\s*\{[^}]*backdrop-filter:\s*blur\(9px\)/s,
    );
  });
});
