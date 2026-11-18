import { beforeEach, describe, expect, it, vi } from "vitest";

import type {
  SecretListView,
  SecretRemoveOutcome,
  SecretSetInput,
  SecretSetOutcome,
} from "@stackspirit/ui";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import { TauriBridge } from "./TauriBridge";

const LIST: SecretListView = {
  secrets: [
    {
      name: "OPENAI_API_KEY",
      last4: "9f2a",
      created_at: "2026-11-18T10:00:00.000Z",
      updated_at: "2026-11-18T10:00:00.000Z",
    },
  ],
  storage: null,
};

const SET_OUTCOME: SecretSetOutcome = { name: "OPENAI_API_KEY", last4: "9f2a" };
const REMOVE_OUTCOME: SecretRemoveOutcome = { name: "OPENAI_API_KEY", removed: true };

function setInput(value: string): SecretSetInput {
  return { name: "OPENAI_API_KEY", value };
}

describe("TauriBridge server secrets", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it("runs the exact commands with the exact argument shapes", async () => {
    const bridge = new TauriBridge();
    invoke.mockResolvedValueOnce(LIST);
    await expect(bridge.secrets.list()).resolves.toEqual(LIST);
    expect(invoke).toHaveBeenLastCalledWith("secrets_list", undefined);

    invoke.mockResolvedValueOnce(SET_OUTCOME);
    const input = setInput(["sk", "test", "value"].join("-"));
    await expect(bridge.secrets.set(input)).resolves.toEqual(SET_OUTCOME);
    expect(invoke).toHaveBeenLastCalledWith("secrets_set", { input });

    invoke.mockResolvedValueOnce(REMOVE_OUTCOME);
    await expect(bridge.secrets.remove("OPENAI_API_KEY")).resolves.toEqual(REMOVE_OUTCOME);
    expect(invoke).toHaveBeenLastCalledWith("secrets_remove", { name: "OPENAI_API_KEY" });

    expect(invoke.mock.calls.map(([command]) => command)).toEqual([
      "secrets_list",
      "secrets_set",
      "secrets_remove",
    ]);
  });

  it("rejects a blank name or value before any invoke", async () => {
    const bridge = new TauriBridge();
    const marker = ["sk", "guard", "marker"].join("-");

    for (const name of ["", "   ", "\t\n"]) {
      const error = await bridge.secrets
        .set({ name, value: marker })
        .catch((caught: unknown) => caught);
      expect(error).toMatchObject({ code: "invalid_input" });
      expect(JSON.stringify(error)).not.toContain(marker);
    }

    for (const value of ["", "   ", "\t\n"]) {
      const error = await bridge.secrets
        .set({ name: "OPENAI_API_KEY", value })
        .catch((caught: unknown) => caught);
      expect(error).toMatchObject({ code: "invalid_input" });
    }

    for (const name of ["", "   ", "\t\n"]) {
      const error = await bridge.secrets.remove(name).catch((caught: unknown) => caught);
      expect(error).toMatchObject({ code: "invalid_input" });
    }

    expect(invoke).not.toHaveBeenCalled();
  });

  it("returns only the safe set outcome and never the value", async () => {
    const bridge = new TauriBridge();
    const value = ["sk", "outcome", "marker"].join("-");
    invoke.mockResolvedValueOnce(SET_OUTCOME);

    const outcome = await bridge.secrets.set(setInput(value));

    expect(outcome).toEqual({ name: "OPENAI_API_KEY", last4: "9f2a" });
    expect(Object.keys(outcome).sort()).toEqual(["last4", "name"]);
    expect(JSON.stringify(outcome)).not.toContain(value);
  });

  it("passes a typed invoke failure through without the value", async () => {
    const bridge = new TauriBridge();
    const value = ["sk", "failure", "marker"].join("-");
    const remoteError = {
      code: "remote",
      message: "SSH transport failed",
      hint: "Check the SSH connection and retry.",
      retryable: true,
    };
    invoke.mockRejectedValueOnce(remoteError);

    const error = await bridge.secrets.set(setInput(value)).catch((caught: unknown) => caught);

    expect(error).toEqual(remoteError);
    expect(JSON.stringify(error)).not.toContain(value);
    expect(String((error as { message?: unknown }).message)).not.toContain(value);
  });

  it("never logs the value", async () => {
    const bridge = new TauriBridge();
    const value = ["sk", "log", "marker"].join("-");
    const spies = [
      vi.spyOn(console, "log"),
      vi.spyOn(console, "info"),
      vi.spyOn(console, "warn"),
      vi.spyOn(console, "error"),
      vi.spyOn(console, "debug"),
    ];
    invoke.mockResolvedValueOnce(SET_OUTCOME);

    await bridge.secrets.set(setInput(value));

    for (const spy of spies) {
      expect(spy).not.toHaveBeenCalled();
      expect(spy.mock.calls.flat().map(String).join(" ")).not.toContain(value);
    }
  });
});
