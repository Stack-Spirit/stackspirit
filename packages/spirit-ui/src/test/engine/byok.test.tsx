import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import { engineWith, renderEngine } from "./helpers";

const NOW = "2026-11-14T12:00:00.000Z";

describe("BYOK provider keys", () => {
  it("byok_save_writes_the_keychain_and_never_the_document", async () => {
    const user = userEvent.setup();
    const bridge = new MockBridge({ scenario: "engine-ready" });
    const { container } = renderEngine(engineWith(), { bridge });
    const secret = ["sk", "testOnly1234567890"].join("_");

    await user.type(screen.getByLabelText("API key"), secret);
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(
      await screen.findByText("Saved. The local engine could not verify this key yet."),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("API key")).toHaveValue("");
    expect(container.innerHTML).not.toContain(secret);
    const statuses = await bridge.engine.keyStatus();
    expect(statuses.some((status) => status.provider === "deepseek")).toBe(true);
  });

  it("byok_verify_failure_keeps_the_key_with_invalid_status", async () => {
    const user = userEvent.setup();
    const bridge = new MockBridge({ scenario: "engine-ready" });
    vi.spyOn(bridge.engine, "verifyKey").mockResolvedValue({
      provider: "deepseek",
      state: "invalid",
      last_checked_at: NOW,
    });
    renderEngine(engineWith(), { bridge });

    await user.type(screen.getByLabelText("API key"), "not-a-real-key");
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(
      await screen.findByText(/the engine rejected this key/),
    ).toBeInTheDocument();
    const statuses = await bridge.engine.keyStatus();
    expect(statuses.find((status) => status.provider === "deepseek")?.state).toBe(
      "saved",
    );
  });

  it("byok_delete_removes_only_the_keychain_entry", async () => {
    const user = userEvent.setup();
    const bridge = new MockBridge({ scenario: "engine-ready" });
    renderEngine(engineWith(), { bridge });

    await user.type(screen.getByLabelText("API key"), "not-a-real-key");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText("Saved. The local engine could not verify this key yet.");

    await user.click(
      await screen.findByRole("button", { name: "Delete DeepSeek key" }),
    );
    const dialog = screen.getByRole("dialog", { name: "Delete provider key?" });
    await user.click(within(dialog).getByRole("button", { name: "Delete key" }));

    await waitFor(() => {
      expect(
        screen.queryByRole("button", { name: "Delete DeepSeek key" }),
      ).not.toBeInTheDocument();
    });
    expect(screen.queryByText(/No key is stored/)).toBeInTheDocument();
  });

  it("byok_errors_never_echo_the_key", async () => {
    const user = userEvent.setup();
    const bridge = new MockBridge({ scenario: "engine-ready" });
    const secret = ["sk", "errorOnly1234567890"].join("_");
    vi.spyOn(bridge.engine, "storeKey").mockRejectedValue(
      new Error("keychain unavailable"),
    );
    renderEngine(engineWith(), { bridge });

    await user.type(screen.getByLabelText("API key"), secret);
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByText(/keychain unavailable/)).toBeInTheDocument();
    expect(screen.getByLabelText("API key")).toHaveValue("");
    expect(document.body.innerHTML).not.toContain(secret);
  });
});
