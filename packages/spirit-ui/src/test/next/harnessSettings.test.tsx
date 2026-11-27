import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { afterEach, describe, expect, expectTypeOf, it, vi } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import type { HarnessesSurface, SpiritBridge } from "../../bridge/SpiritBridge";
import type {
  ConnectionSnapshot,
  HarnessConnectStarted,
  HarnessListView,
  HarnessManifest,
  HarnessProbeReport,
  HarnessTarget,
  HostProfile,
} from "../../bridge/wire";
import { SettingsView } from "../../next/SettingsView";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";

const here = dirname(fileURLToPath(import.meta.url));

function read(relative: string): string {
  return readFileSync(resolve(here, relative), "utf8");
}

function tauriBridgeSource(): string {
  return read("../../../../../apps/spirit-desktop/src/bridge/TauriBridge.ts");
}

function spiritBridgeSource(): string {
  return read("../../bridge/SpiritBridge.ts");
}

function wireSource(): string {
  return read("../../bridge/wire.ts");
}

function panelSource(): string {
  return read("../../next/HarnessesPanel.tsx");
}

async function captureError(action: () => Promise<unknown>): Promise<unknown> {
  try {
    await action();
  } catch (error) {
    return error;
  }
  throw new Error("expected the call to fail");
}

const AT = "2026-11-27T10:00:00.000Z";
const HOST_ID = "host-harness-settings";

function host(): HostProfile {
  return {
    id: HOST_ID,
    label: "Studio VPS",
    ssh: { user: "owner", host: "203.0.113.90", port: 22 },
    identity_file: null,
    vowp_home: null,
    default_project: null,
    host_key: {
      algorithm: "ssh-ed25519",
      sha256: "SHA256:harnessSettingsHost00000000000000000000",
      pinned_at: AT,
    },
    created_at: AT,
    updated_at: AT,
    last_connected_at: AT,
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

function manifest(): HarnessManifest {
  return {
    id: "opencode",
    name: "OpenCode",
    binary: "opencode",
    install_cmd: "curl -fsSL https://opencode.ai/install | bash",
    auth_cmd: "opencode auth login",
    protocol: "opencode-http",
    supports_embedded_chat: true,
    auth_modes: ["api_key"],
    tos_note:
      "BYOK: OpenCode uses the provider API key you connect. Provider billing and terms apply.",
    docs_url: "https://opencode.ai/docs/",
    oauth_callback_port: 54545,
    builtin: true,
  };
}

function report(target: HarnessTarget): HarnessProbeReport {
  const item = manifest();
  const remote = target === "home_vps";
  return {
    probe: {
      harness_id: item.id,
      target,
      protocol: item.protocol,
      supports_embedded_chat: item.supports_embedded_chat,
      binary: "found",
      version: "opencode 1.2.3",
      auth: "configured",
      auth_detail: "A stored sign-in was reported. No key or token value was read.",
      readiness: "ready",
      oauth_callback_port: item.oauth_callback_port,
      requires_loopback_forward: remote,
      warnings: [],
    },
    plan: {
      install_command: null,
      auth_command: item.auth_cmd,
      requires_loopback_forward: remote,
      forward_port: remote ? item.oauth_callback_port : null,
    },
  };
}

function started(target: HarnessTarget): HarnessConnectStarted {
  return {
    harness_id: "opencode",
    target,
    auth_started: true,
    browser_opened: true,
    forward_port: target === "home_vps" ? 54545 : null,
    message: "Started opencode auth login. Opened the sign-in in your browser.",
  };
}

/**
 * A desktop-shaped bridge for the Settings mount test. The mock bridge supplies every
 * unrelated namespace (hosts, engine, connection, ...), while this harness surface is a
 * transparent spy that reports a real desktop result.
 */
function desktopBridge() {
  const mock = new MockBridge("fresh");
  const list = vi.fn(async (): Promise<HarnessListView> => ({
    harnesses: [manifest()],
    warnings: [],
  }));
  const probe = vi.fn(
    async (input: { id: string; target: HarnessTarget }): Promise<HarnessProbeReport> =>
      report(input.target),
  );
  const connectStart = vi.fn(
    async (input: { id: string; target: HarnessTarget }): Promise<HarnessConnectStarted> =>
      started(input.target),
  );
  const surface: HarnessesSurface = {
    list,
    saveCustom: async (next: HarnessManifest) => next,
    deleteCustom: async () => true,
    probe,
    connectStart,
  };
  const bridge = {
    ...mock,
    kind: "tauri" as const,
    preview: false,
    harnesses: surface,
  } as unknown as SpiritBridge;
  return { bridge, mock, list, probe, connectStart };
}

function install(bridge: SpiritBridge, connection: ConnectionSnapshot, hosts: HostProfile[]) {
  resetFleetStore();
  useFleetStore.setState({
    bridge,
    preview: bridge.preview,
    status: "ready",
    connection,
    hosts,
    hostsLoaded: true,
    selectedHostId: hosts[0]?.id ?? null,
  });
}

afterEach(() => {
  resetFleetStore();
});

const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"];

describe("harness bridge contract", () => {
  it("exposes the namespace on the desktop adapter and the shared contract", () => {
    const source = tauriBridgeSource();
    expect(source).toMatch(/harnesses\s*=\s*\{/);
    expect(source).toContain('call("harnesses_list")');
    expect(source).toContain('call("harnesses_save_custom", { manifest })');
    expect(source).toContain('call("harnesses_delete_custom", { id })');
    expect(source).toContain('call("harnesses_probe", { input })');
    expect(source).toContain('call("harnesses_connect_start", { input })');
    expect(source).not.toContain("console.");

    const contract = spiritBridgeSource();
    expect(contract).toContain("export interface HarnessesSurface");
    expect(contract).toMatch(/harnesses\?: HarnessesSurface/);
    // The new namespace is additive: a bridge without it is still a `SpiritBridge`.
    expectTypeOf<undefined>().toMatchTypeOf<SpiritBridge["harnesses"]>();

    const wire = wireSource();
    for (const name of [
      "HarnessManifest",
      "HarnessListView",
      "HarnessProbe",
      "HarnessConnectPlanView",
      "HarnessProbeReport",
      "HarnessConnectStarted",
    ]) {
      expect(wire).toContain(`export interface ${name}`);
    }
    for (const name of [
      "HarnessProtocol",
      "HarnessAuthMode",
      "HarnessTarget",
      "HarnessBinaryState",
      "HarnessAuthState",
      "HarnessReadiness",
    ]) {
      expect(wire).toContain(`export type ${name}`);
    }
    expect(wire).not.toContain("HarnessesPanel");
  });

  it("imports the harness DTOs from the bridge instead of defining them in the panel", () => {
    const panel = panelSource();
    expect(panel).toContain('} from "../bridge/wire";');
    expect(panel).not.toMatch(/export interface HarnessManifest/);
    expect(panel).not.toMatch(/export type HarnessProtocol/);
    expect(panel).not.toMatch(/export interface HarnessesSurface/);
    expect(panel).toContain('export type { HarnessesSurface } from "../bridge/SpiritBridge"');
  });

  it("lists the built-in presets in preview and refuses every probe or change", async () => {
    const bridge = new MockBridge("fresh");
    const harnesses = bridge.harnesses;
    if (!harnesses) throw new Error("the mock bridge has no harnesses namespace");

    expect(Object.keys(harnesses).sort()).toEqual([
      "connectStart",
      "deleteCustom",
      "list",
      "probe",
      "saveCustom",
    ]);

    const view = await harnesses.list();
    expect(view.harnesses).toHaveLength(8);
    expect(view.harnesses.every((item) => item.builtin)).toBe(true);
    expect(view.harnesses.map((item) => item.id)).toEqual([
      "opencode",
      "claude-code",
      "codex",
      "gemini-cli",
      "antigravity",
      "cursor-cli",
      "aider",
      "goose",
    ]);
    const warnings = view.warnings.join(" ");
    expect(warnings).toMatch(/preview/i);
    expect(warnings).toMatch(/built-in presets only/i);

    // The manifest type has no credential field; values stay in the harness's store.
    expectTypeOf<HarnessManifest>().not.toHaveProperty("api_key");
    expectTypeOf<HarnessManifest>().not.toHaveProperty("token");
    expectTypeOf<HarnessManifest>().not.toHaveProperty("password");

    const custom = { ...view.harnesses[0]!, id: "mine", name: "Mine", builtin: false };
    for (const action of [
      () => harnesses.saveCustom(custom),
      () => harnesses.deleteCustom("mine"),
      () => harnesses.probe({ id: "opencode", target: "local" }),
      () => harnesses.connectStart({ id: "claude-code", target: "home_vps" }),
    ]) {
      const error = await captureError(action);
      expect(error).toMatchObject({ code: "preview_unavailable" });
      expect(String((error as { message?: unknown }).message)).toMatch(/desktop host/i);
    }

    // Preview never claims a live connect, even after a refused call.
    expect(bridge.preview).toBe(true);
    await expect(
      harnesses.connectStart({ id: "opencode", target: "local" }),
    ).rejects.toMatchObject({ code: "preview_unavailable" });
  });
});

describe("Settings → Advanced → Harnesses", () => {
  it("mounts the panel and passes the connected Home VPS through to the bridge", async () => {
    const user = userEvent.setup();
    const { bridge, probe, connectStart } = desktopBridge();
    install(bridge, snapshot("connected", HOST_ID), [host()]);
    render(<SettingsView theme="system" onTheme={() => undefined} />);

    await user.click(screen.getByRole("tab", { name: "Harnesses" }));
    const panel = await screen.findByTestId("harnesses-panel");
    const opencode = await within(panel).findByTestId("harness-opencode");

    await waitFor(() => {
      expect(within(opencode).getByTestId("harness-opencode-local")).toHaveTextContent(
        "Ready",
      );
      expect(within(opencode).getByTestId("harness-opencode-home")).toHaveTextContent(
        "Ready",
      );
    });
    expect(probe).toHaveBeenCalledWith({ id: "opencode", target: "local" });
    expect(probe).toHaveBeenCalledWith({ id: "opencode", target: "home_vps" });

    await user.click(within(opencode).getByRole("button", { name: "Connect on Studio VPS" }));
    expect(connectStart).toHaveBeenCalledWith({ id: "opencode", target: "home_vps" });
    expect(await screen.findByTestId("harness-connect-result")).toHaveTextContent(
      "Temporary loopback forward on 127.0.0.1:54545",
    );
  });

  it("keeps the preview honest: built-ins are readable and every probe or connect is refused", async () => {
    const user = userEvent.setup();
    const preview = new MockBridge("fresh");
    const hosts = await preview.hosts.list();
    install(preview, snapshot("disconnected", null), hosts);
    render(<SettingsView theme="system" onTheme={() => undefined} />);

    await user.click(screen.getByRole("tab", { name: "Harnesses" }));
    const panel = await screen.findByTestId("harnesses-panel");
    const opencode = await within(panel).findByTestId("harness-opencode");

    expect(within(panel).getByText(/Preview: built-in presets only/)).toBeInTheDocument();
    expect(
      within(panel).getByRole("button", { name: "Connect a Home VPS" }),
    ).toBeInTheDocument();
    expect(within(opencode).getByText("Desktop only")).toBeInTheDocument();
    expect(
      within(opencode).getByRole("button", { name: "Connect on this Mac" }),
    ).toBeDisabled();

    await waitFor(() =>
      expect(within(opencode).getByTestId("harness-opencode-local")).toHaveTextContent(
        /preview mode lists the built-in presets only/,
      ),
    );
    expect(screen.queryByTestId("harness-connect-result")).not.toBeInTheDocument();
    expect(within(opencode).getByTestId("harness-opencode-home")).toHaveTextContent(
      "Not connected.",
    );
  });

  it("has no WCAG 2.2 AA axe violations in the mounted panel", async () => {
    const user = userEvent.setup();
    const { bridge } = desktopBridge();
    install(bridge, snapshot("connected", HOST_ID), [host()]);
    render(<SettingsView theme="system" onTheme={() => undefined} />);

    await user.click(screen.getByRole("tab", { name: "Harnesses" }));
    const panel = await screen.findByTestId("harnesses-panel");
    await within(panel).findByTestId("harness-opencode");
    const results = await axe.run(panel, {
      runOnly: { type: "tag", values: WCAG_TAGS },
    });
    expect(results.violations).toEqual([]);
  });
});
