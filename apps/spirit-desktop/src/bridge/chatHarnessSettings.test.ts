import { beforeEach, describe, expect, it, vi } from "vitest";

import type { ChatHarnessSettingsView } from "@stackspirit/ui";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import { TauriBridge } from "./TauriBridge";

const FRESH: ChatHarnessSettingsView = {
  default_harness_id: null,
  gate: { state: "ready", harness_id: "opencode", protocol: "opencode-http" },
};

const PLANNING: ChatHarnessSettingsView = {
  default_harness_id: "goose",
  gate: {
    state: "harness_not_wired",
    harness_id: "goose",
    protocol: "acp",
  },
};

describe("TauriBridge harness chat settings", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it("reads the safe settings view and reports OpenCode ready on a fresh profile", async () => {
    const bridge = new TauriBridge();

    invoke.mockResolvedValueOnce(FRESH);
    await expect(bridge.chat.harnessSettings()).resolves.toEqual(FRESH);
    expect(invoke).toHaveBeenLastCalledWith("chat_harness_settings", undefined);

    const serialized = JSON.stringify(FRESH);
    for (const forbidden of [
      "path",
      "token",
      "endpoint",
      "port",
      "workspace",
      "native_session_id",
      "/Users/",
      "/home/",
    ]) {
      expect(serialized).not.toContain(forbidden);
    }
  });

  it("forwards only the harness id when saving the default", async () => {
    const bridge = new TauriBridge();

    invoke.mockResolvedValueOnce(PLANNING);
    await expect(bridge.chat.setDefaultHarness("goose")).resolves.toEqual(
      PLANNING,
    );

    // The renderer sends the id and nothing else: no path, token, endpoint, port or
    // native session id.
    expect(invoke.mock.calls).toEqual([
      ["chat_set_default_harness", { input: { harness_id: "goose" } }],
    ]);
  });
});
