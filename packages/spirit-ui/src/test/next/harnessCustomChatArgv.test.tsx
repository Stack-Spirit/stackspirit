import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type {
  HarnessManifest,
  HarnessesBridge,
  HarnessesSurface,
} from "../../next/HarnessesPanel";
import { HarnessesPanel } from "../../next/HarnessesPanel";

const FIELD = "Chat launch arguments (one per line)";

function builtinAcp(): HarnessManifest {
  return {
    id: "goose",
    name: "Goose",
    binary: "goose",
    install_cmd: "",
    auth_cmd: "",
    protocol: "acp",
    surface: "orchestrator_and_workers",
    supports_embedded_chat: true,
    auth_modes: ["api_key"],
    tos_note: "BYOK: provider billing and terms apply.",
    docs_url: "https://example.com/goose",
    oauth_callback_port: null,
    builtin: true,
    chat_argv: ["acp"],
  };
}

function setup(saveError?: Error) {
  const saveCustom = vi.fn(async (next: HarnessManifest) => {
    if (saveError) throw saveError;
    return next;
  });
  const surface: HarnessesSurface = {
    list: vi.fn(async () => ({ harnesses: [builtinAcp()], warnings: [] })),
    saveCustom,
    deleteCustom: vi.fn(async () => true),
    probe: vi.fn(async () => {
      throw new Error("no probe in this test");
    }),
    connectStart: vi.fn(async () => {
      throw new Error("no connect in this test");
    }),
  };
  const bridge: HarnessesBridge = { kind: "tauri", preview: false, harnesses: surface };
  return { saveCustom, bridge };
}

async function openForm(user: ReturnType<typeof userEvent.setup>) {
  await screen.findByTestId("harness-goose");
  await user.click(screen.getByTestId("harness-add-custom"));
}

async function fillRequired(user: ReturnType<typeof userEvent.setup>) {
  await user.type(screen.getByLabelText("ID"), "my-acp");
  await user.type(screen.getByLabelText("Binary"), "my-acp");
  await user.type(screen.getByLabelText("Name"), "My ACP");
  await user.type(
    screen.getByLabelText("API-key versus subscription terms note"),
    "API-key use follows API terms.",
  );
  await user.type(screen.getByLabelText("Documentation URL"), "https://example.com/docs");
}

async function typeArgs(user: ReturnType<typeof userEvent.setup>, text: string) {
  await user.click(screen.getByLabelText(FIELD));
  await user.paste(text);
}

describe("custom harness chat launch arguments", () => {
  it("shows the field for ACP orchestrators and hides it otherwise", async () => {
    const user = userEvent.setup();
    const { bridge } = setup();
    render(<HarnessesPanel bridge={bridge} />);
    await openForm(user);

    expect(screen.getByLabelText(FIELD)).toBeTruthy();
    expect(screen.getByTestId("harness-chat-argv-hint")).toHaveTextContent(
      "never runs it through a shell",
    );

    await user.selectOptions(screen.getByLabelText("Protocol"), "opencode-http");
    expect(screen.queryByLabelText(FIELD)).toBeNull();

    await user.selectOptions(screen.getByLabelText("Protocol"), "acp");
    expect(screen.getByLabelText(FIELD)).toBeTruthy();

    await user.selectOptions(screen.getByLabelText("Surface"), "workers_only_terminal");
    expect(screen.queryByLabelText(FIELD)).toBeNull();
  });

  it("submits exact argument boundaries, dropping blank lines", async () => {
    const user = userEvent.setup();
    const { bridge, saveCustom } = setup();
    render(<HarnessesPanel bridge={bridge} />);
    await openForm(user);
    await fillRequired(user);
    await typeArgs(user, "--acp\n\nacp\n");
    await user.click(screen.getByRole("button", { name: "Add harness" }));

    await vi.waitFor(() => expect(saveCustom).toHaveBeenCalledTimes(1));
    expect(saveCustom.mock.calls[0]![0].chat_argv).toEqual(["--acp", "acp"]);
  });

  it("allows an empty field and sends an empty array", async () => {
    const user = userEvent.setup();
    const { bridge, saveCustom } = setup();
    render(<HarnessesPanel bridge={bridge} />);
    await openForm(user);
    await fillRequired(user);
    await user.click(screen.getByRole("button", { name: "Add harness" }));

    await vi.waitFor(() => expect(saveCustom).toHaveBeenCalledTimes(1));
    expect(saveCustom.mock.calls[0]![0].chat_argv).toEqual([]);
  });

  it("omits chat_argv for OpenCode HTTP entries", async () => {
    const user = userEvent.setup();
    const { bridge, saveCustom } = setup();
    render(<HarnessesPanel bridge={bridge} />);
    await openForm(user);
    await fillRequired(user);
    await typeArgs(user, "acp");
    await user.selectOptions(screen.getByLabelText("Protocol"), "opencode-http");
    await user.click(screen.getByRole("button", { name: "Add harness" }));

    await vi.waitFor(() => expect(saveCustom).toHaveBeenCalledTimes(1));
    expect("chat_argv" in saveCustom.mock.calls[0]![0]).toBe(false);
  });

  it.each([
    ["quotes", '"acp"'],
    ["a space inside an argument", "serve acp"],
    ["a shell operator", "acp;rm"],
    ["a pipe", "acp|cat"],
    ["a substitution", "$HOME"],
    ["an API key lookalike", "sk-abcdef123456"],
  ])("rejects %s without saving", async (_label, value) => {
    const user = userEvent.setup();
    const { bridge, saveCustom } = setup();
    render(<HarnessesPanel bridge={bridge} />);
    await openForm(user);
    await fillRequired(user);
    await typeArgs(user, value);
    await user.click(screen.getByRole("button", { name: "Add harness" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(/Chat launch arguments/);
    expect(alert.textContent).not.toContain(value);
    expect(saveCustom).not.toHaveBeenCalled();
  });

  it("rejects too many or too long arguments", async () => {
    const user = userEvent.setup();
    const { bridge, saveCustom } = setup();
    render(<HarnessesPanel bridge={bridge} />);
    await openForm(user);
    await fillRequired(user);
    await typeArgs(user, Array.from({ length: 17 }, () => "a").join("\n"));
    await user.click(screen.getByRole("button", { name: "Add harness" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("at most 16");
    expect(saveCustom).not.toHaveBeenCalled();
  });

  it("shows a host save error in the normal error card", async () => {
    const user = userEvent.setup();
    const { bridge } = setup(new Error("host refused the manifest"));
    render(<HarnessesPanel bridge={bridge} />);
    await openForm(user);
    await fillRequired(user);
    await typeArgs(user, "acp");
    await user.click(screen.getByRole("button", { name: "Add harness" }));
    expect(await screen.findByRole("alert")).toBeTruthy();
  });

  it("does not expose an editable launch field on built-in ACP cards", async () => {
    const { bridge } = setup();
    render(<HarnessesPanel bridge={bridge} />);
    const card = await screen.findByTestId("harness-goose");
    expect(card.querySelector("textarea")).toBeNull();
    expect(screen.queryByLabelText(FIELD)).toBeNull();
  });
});
