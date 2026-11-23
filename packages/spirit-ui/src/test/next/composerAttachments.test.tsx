import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { ChatModelView } from "../../bridge/wire";
import { ComposerBar, MAX_ATTACHMENT_BYTES, defaultVariant } from "../../next/ComposerBar";

const FLASH: ChatModelView = {
  id: "deepseek/flash",
  label: "DeepSeek-V4.1-Flash",
  provider: "deepseek",
  tier: "byok",
  variants: ["low", "high"],
};
const PLAIN: ChatModelView = {
  id: "opencode/big-pickle",
  label: "Big Pickle",
  provider: "opencode",
  tier: "included",
};

function setup(model: ChatModelView, onSend = vi.fn()) {
  const user = userEvent.setup({ applyAccept: false });
  render(
    <ComposerBar
      session="setup"
      placeholder="Message"
      model={model}
      models={[FLASH, PLAIN]}
      modelMissing={false}
      disabled={false}
      streaming={false}
      stopped={false}
      onSend={onSend}
      onStop={() => {}}
      onStart={() => {}}
      onSelectModel={() => {}}
    />,
  );
  return { user, onSend };
}

describe("composer attachments", () => {
  it("attaches, shows a chip, removes it, and has no Workspace Write dropdown", async () => {
    const { user } = setup(PLAIN);
    expect(screen.queryByText(/Workspace Write/i)).not.toBeInTheDocument();
    const input = screen.getByTestId("composer-file-input");
    await user.upload(input, new File(["hi"], "notes.txt", { type: "text/plain" }));
    const chips = await screen.findByTestId("composer-attachments");
    expect(chips).toHaveTextContent("notes.txt");
    await user.click(screen.getByRole("button", { name: "Remove notes.txt" }));
    expect(screen.queryByTestId("composer-attachments")).not.toBeInTheDocument();
  });

  it("rejects oversized files with a clear error", async () => {
    const { user } = setup(PLAIN);
    const big = new File(["x"], "huge.png", { type: "image/png" });
    Object.defineProperty(big, "size", { value: MAX_ATTACHMENT_BYTES + 1 });
    await user.upload(screen.getByTestId("composer-file-input"), big);
    expect(await screen.findByTestId("composer-attach-error")).toHaveTextContent(/too large/);
    expect(screen.queryByTestId("composer-attachments")).not.toBeInTheDocument();
  });

  it("sends attachments as data URIs and clears the chips", async () => {
    const { user, onSend } = setup(PLAIN);
    await user.upload(
      screen.getByTestId("composer-file-input"),
      new File(["hi"], "notes.txt", { type: "text/plain" }),
    );
    await screen.findByTestId("composer-attachments");
    await user.type(screen.getByLabelText("Message"), "look{Enter}");
    await waitFor(() => expect(onSend).toHaveBeenCalled());
    const [text, options] = onSend.mock.calls[0]!;
    expect(text).toBe("look");
    expect(options.attachments).toEqual([
      expect.objectContaining({
        filename: "notes.txt",
        mime: "text/plain",
        url: expect.stringMatching(/^data:text\/plain;base64,/),
      }),
    ]);
    expect(screen.queryByTestId("composer-attachments")).not.toBeInTheDocument();
  });
});

describe("reasoning effort", () => {
  it("opens a two-row menu, defaults to Medium-or-better effort and sends the variant", async () => {
    const { user, onSend } = setup(FLASH);
    // Default effort is never None: FLASH declares low/high, so it starts on High.
    expect(screen.getByTestId("composer-model")).toHaveTextContent("DeepSeek-V4.1-Flash · High");
    await user.click(screen.getByTestId("composer-model"));
    expect(screen.getByTestId("composer-model-row")).toHaveTextContent("Model");
    const effortRow = screen.getByTestId("composer-effort-row");
    expect(effortRow).toHaveTextContent("Effort");
    expect(effortRow).toHaveTextContent("High");
    await user.click(effortRow);
    expect(screen.getByTestId("reasoning-effort")).toBeInTheDocument();
    expect(screen.getByTestId("reasoning-high")).toHaveAttribute("aria-checked", "true");
    await user.click(screen.getByTestId("reasoning-low"));
    expect(screen.getByTestId("composer-model")).toHaveTextContent("DeepSeek-V4.1-Flash · Low");
    await user.keyboard("{Escape}");
    await user.type(screen.getByLabelText("Message"), "go{Enter}");
    await waitFor(() => expect(onSend).toHaveBeenCalled());
    expect(onSend.mock.calls[0]![1].variant).toBe("low");
  });

  it("prefers medium, then high, then the first declared variant", () => {
    expect(defaultVariant(["low", "medium", "high"])).toBe("medium");
    expect(defaultVariant(["low", "high"])).toBe("high");
    expect(defaultVariant(["minimal", "max"])).toBe("minimal");
    expect(defaultVariant([])).toBeNull();
  });

  it("hides the Effort row for models without reasoning variants", async () => {
    const { user } = setup(PLAIN);
    await user.click(screen.getByTestId("composer-model"));
    expect(screen.getByTestId("model-picker-popover")).toBeInTheDocument();
    expect(screen.getByTestId("composer-model-row")).toBeInTheDocument();
    expect(screen.queryByTestId("composer-effort-row")).not.toBeInTheDocument();
    expect(screen.queryByTestId("reasoning-effort")).not.toBeInTheDocument();
  });
});
