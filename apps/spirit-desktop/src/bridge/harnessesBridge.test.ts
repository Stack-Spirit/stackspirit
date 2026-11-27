import { beforeEach, describe, expect, it, vi } from "vitest";

import type {
  HarnessConnectStarted,
  HarnessListView,
  HarnessManifest,
  HarnessProbeReport,
  HarnessTarget,
} from "@stackspirit/ui";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import { TauriBridge } from "./TauriBridge";

const MANIFEST: HarnessManifest = {
  id: "claude-code",
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
};

const LIST: HarnessListView = { harnesses: [MANIFEST], warnings: [] };

function probeReport(input: { id: string; target: HarnessTarget }): HarnessProbeReport {
  return {
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
  };
}

function connectStarted(input: {
  id: string;
  target: HarnessTarget;
}): HarnessConnectStarted {
  return {
    harness_id: input.id,
    target: input.target,
    auth_started: true,
    browser_opened: true,
    forward_port: input.target === "home_vps" ? 54545 : null,
    message: "Started the sign-in in your browser.",
  };
}

describe("TauriBridge Settings harnesses and CLI/MCP connect", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it("routes the registry and one-click connect to the exact host commands", async () => {
    const bridge = new TauriBridge();

    invoke.mockResolvedValueOnce(LIST);
    await expect(bridge.harnesses.list()).resolves.toEqual(LIST);
    expect(invoke).toHaveBeenLastCalledWith("harnesses_list", undefined);

    invoke.mockResolvedValueOnce(MANIFEST);
    await expect(bridge.harnesses.saveCustom(MANIFEST)).resolves.toEqual(MANIFEST);
    expect(invoke).toHaveBeenLastCalledWith("harnesses_save_custom", {
      manifest: MANIFEST,
    });

    invoke.mockResolvedValueOnce(true);
    await expect(bridge.harnesses.deleteCustom("claude-code")).resolves.toBe(true);
    expect(invoke).toHaveBeenLastCalledWith("harnesses_delete_custom", {
      id: "claude-code",
    });

    const localProbe = { id: "claude-code", target: "local" as const };
    const localReport = probeReport(localProbe);
    invoke.mockResolvedValueOnce(localReport);
    await expect(bridge.harnesses.probe(localProbe)).resolves.toEqual(localReport);
    expect(invoke).toHaveBeenLastCalledWith("harnesses_probe", { input: localProbe });

    const homeConnect = { id: "claude-code", target: "home_vps" as const };
    const started = connectStarted(homeConnect);
    invoke.mockResolvedValueOnce(started);
    await expect(bridge.harnesses.connectStart(homeConnect)).resolves.toEqual(started);
    expect(invoke).toHaveBeenLastCalledWith("harnesses_connect_start", {
      input: homeConnect,
    });

    expect(invoke.mock.calls.map(([command]) => command)).toEqual([
      "harnesses_list",
      "harnesses_save_custom",
      "harnesses_delete_custom",
      "harnesses_probe",
      "harnesses_connect_start",
    ]);
  });

  it("passes only the manifest or the id and target to the host", async () => {
    const bridge = new TauriBridge();
    invoke.mockResolvedValue({});

    await bridge.harnesses.saveCustom(MANIFEST);
    await bridge.harnesses.deleteCustom("claude-code");
    await bridge.harnesses.probe({ id: "claude-code", target: "local" });
    await bridge.harnesses.connectStart({ id: "claude-code", target: "home_vps" });

    expect(invoke.mock.calls).toEqual([
      ["harnesses_save_custom", { manifest: MANIFEST }],
      ["harnesses_delete_custom", { id: "claude-code" }],
      ["harnesses_probe", { input: { id: "claude-code", target: "local" } }],
      [
        "harnesses_connect_start",
        { input: { id: "claude-code", target: "home_vps" } },
      ],
    ]);
    // The renderer never sends a key, token, local path or SSH option here.
    const payloads = JSON.stringify(invoke.mock.calls);
    expect(payloads).not.toMatch(/identity_file|password|secret|ssh/i);
  });
});
