import { beforeEach, describe, expect, it, vi } from "vitest";

import type {
  EngineStatus,
  ExternalHarness,
  KeyStatus,
  ModelChoice,
} from "@stackspirit/ui";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import { TauriBridge } from "./TauriBridge";

const AT = "2026-11-27T10:00:00.000Z";

const MODEL: ModelChoice = {
  id: "opencode/deepseek-v4",
  label: "DeepSeek V4",
  provider: "opencode",
  access: "included",
  context_tokens: 128_000,
  notes: "",
  verified_at: AT,
};

const STATUS: EngineStatus = {
  choice: { type: "bundled" },
  state: { state: "ready" },
  version: "1.19.5",
  binary_path: "/app/bin/opencode",
  port: 4096,
  selected_model: MODEL,
  included_models_available: true,
  started_at: AT,
  last_error: null,
  providers: [{ id: "opencode", label: "OpenCode", connected: true, source: "config" }],
};

const KEY: KeyStatus = {
  provider: "deepseek",
  state: "saved",
  last_checked_at: AT,
};

// Built at runtime like the other bridge tests so no API-key-shaped literal lives in
// the repository while the write-only store path stays covered.
const PROVIDER_VALUE = ["sk", "test", "value"].join("-");

const EXTERNAL: ExternalHarness = {
  kind: "opencode",
  path: "/opt/opencode/bin/opencode",
  version: "1.19.5",
  usable: true,
};

describe("TauriBridge Settings OpenCode provider dropdown", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it("routes status, models, keys and external scans to the exact host commands", async () => {
    const bridge = new TauriBridge();

    invoke.mockResolvedValueOnce(STATUS);
    await expect(bridge.engine.status()).resolves.toEqual(STATUS);
    expect(invoke).toHaveBeenLastCalledWith("engine_status", undefined);

    invoke.mockResolvedValueOnce([MODEL]);
    await expect(bridge.engine.models()).resolves.toEqual([MODEL]);
    expect(invoke).toHaveBeenLastCalledWith("engine_models", undefined);

    invoke.mockResolvedValueOnce([KEY]);
    await expect(bridge.engine.keyStatus()).resolves.toEqual([KEY]);
    expect(invoke).toHaveBeenLastCalledWith("key_status", undefined);

    invoke.mockResolvedValueOnce(KEY);
    await expect(bridge.engine.storeKey("deepseek", PROVIDER_VALUE)).resolves.toEqual(KEY);
    expect(invoke).toHaveBeenLastCalledWith("store_key", {
      provider: "deepseek",
      secret: PROVIDER_VALUE,
    });

    invoke.mockResolvedValueOnce(KEY);
    await expect(bridge.engine.verifyKey("deepseek")).resolves.toEqual(KEY);
    expect(invoke).toHaveBeenLastCalledWith("verify_key", { provider: "deepseek" });

    invoke.mockResolvedValueOnce(KEY);
    await expect(bridge.engine.deleteKey("deepseek")).resolves.toEqual(KEY);
    expect(invoke).toHaveBeenLastCalledWith("delete_key", { provider: "deepseek" });

    invoke.mockResolvedValueOnce(STATUS);
    await expect(bridge.engine.selectModel(MODEL.id)).resolves.toEqual(STATUS);
    expect(invoke).toHaveBeenLastCalledWith("select_model", { modelId: MODEL.id });

    expect(invoke.mock.calls.map(([command]) => command)).toEqual([
      "engine_status",
      "engine_models",
      "key_status",
      "store_key",
      "verify_key",
      "delete_key",
      "select_model",
    ]);
  });

  it("routes the external harness scan and selection used by the engine card", async () => {
    const bridge = new TauriBridge();

    invoke.mockResolvedValueOnce([EXTERNAL]);
    await expect(bridge.engine.scanExternal()).resolves.toEqual([EXTERNAL]);
    expect(invoke).toHaveBeenLastCalledWith("scan_external", undefined);

    invoke.mockResolvedValueOnce(STATUS);
    await expect(
      bridge.engine.selectExternal("opencode", EXTERNAL.path),
    ).resolves.toEqual(STATUS);
    expect(invoke).toHaveBeenLastCalledWith("select_external", {
      kind: "opencode",
      path: EXTERNAL.path,
    });

    invoke.mockResolvedValueOnce(STATUS);
    await expect(bridge.engine.useBundled()).resolves.toEqual(STATUS);
    expect(invoke).toHaveBeenLastCalledWith("use_bundled", undefined);

    invoke.mockResolvedValueOnce(STATUS);
    await expect(bridge.engine.restart()).resolves.toEqual(STATUS);
    expect(invoke).toHaveBeenLastCalledWith("restart_engine", undefined);
  });
});
