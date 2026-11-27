import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import type {
  HarnessListView,
  HarnessManifest,
  HarnessProbeReport,
  HarnessTarget,
  HarnessesBridge,
  HarnessesSurface,
} from "../../next/HarnessesPanel";
import { HarnessesPanel } from "../../next/HarnessesPanel";

function manifest(overrides: Partial<HarnessManifest> & { id: string }): HarnessManifest {
  return {
    name: "OpenCode",
    binary: "opencode",
    install_cmd: "curl -fsSL https://opencode.ai/install | bash",
    auth_cmd: "opencode auth login",
    protocol: "opencode-http",
    supports_embedded_chat: true,
    auth_modes: ["api_key"],
    tos_note: "BYOK: provider billing and terms apply.",
    docs_url: "https://opencode.ai/docs/",
    oauth_callback_port: null,
    builtin: true,
    ...overrides,
  };
}

function probeReport(
  item: HarnessManifest,
  target: HarnessTarget,
  overrides: Partial<HarnessProbeReport["probe"]> = {},
): HarnessProbeReport {
  const binary = overrides.binary ?? "found";
  return {
    probe: {
      harness_id: item.id,
      target,
      protocol: item.protocol,
      supports_embedded_chat: item.supports_embedded_chat,
      binary,
      version: binary === "found" ? `${item.binary} 1.2.3` : null,
      auth: binary === "found" ? "configured" : "unknown",
      auth_detail:
        binary === "found"
          ? "A stored sign-in was reported. No key or token value was read."
          : "Skipped because the harness binary was not found.",
      readiness: binary === "found" ? "ready" : "install_required",
      oauth_callback_port: item.oauth_callback_port,
      requires_loopback_forward:
        target === "home_vps" && item.oauth_callback_port !== null,
      warnings: [],
      ...overrides,
    },
    plan: {
      install_command: binary === "missing" ? item.install_cmd : null,
      auth_command: item.auth_cmd || null,
      requires_loopback_forward:
        target === "home_vps" && item.oauth_callback_port !== null,
      forward_port:
        target === "home_vps" && item.oauth_callback_port !== null
          ? item.oauth_callback_port
          : null,
    },
  };
}

interface FakeOptions {
  initial?: HarnessManifest[];
  warnings?: string[];
  listError?: Error;
  probeError?: Error;
  connectError?: Error;
}

function fakeHarnesses(options: FakeOptions = {}) {
  let harnesses = options.initial ?? [
    manifest({ id: "opencode" }),
    manifest({
      id: "claude-code",
      name: "Claude Code",
      binary: "claude",
      auth_cmd: "claude auth login",
      protocol: "stream-json",
      auth_modes: ["api_key", "subscription"],
      tos_note:
        "API-key use is billed under Anthropic's commercial API terms. Claude Pro/Max subscription sign-in is governed by Anthropic's Consumer Terms.",
      docs_url: "https://docs.anthropic.com/en/docs/claude-code",
      oauth_callback_port: 54545,
    }),
    manifest({
      id: "aider",
      name: "Aider",
      binary: "aider",
      protocol: "cli-pty",
      supports_embedded_chat: false,
      auth_modes: ["api_key"],
      auth_cmd: "",
      tos_note: "BYOK: Aider uses the provider API key you configure.",
      docs_url: "https://aider.chat/docs/",
      builtin: true,
    }),
  ];

  const list = vi.fn(async (): Promise<HarnessListView> => {
    if (options.listError) throw options.listError;
    return { harnesses: [...harnesses], warnings: options.warnings ?? [] };
  });
  const saveCustom = vi.fn(async (next: HarnessManifest) => {
    harnesses = [...harnesses.filter((item) => item.id !== next.id), next];
    return next;
  });
  const deleteCustom = vi.fn(async (id: string) => {
    const before = harnesses.length;
    harnesses = harnesses.filter((item) => item.id !== id);
    return harnesses.length !== before;
  });
  const probe = vi.fn(async ({
    id,
    target,
  }: {
    id: string;
    target: HarnessTarget;
  }): Promise<HarnessProbeReport> => {
    if (options.probeError) throw options.probeError;
    const item = harnesses.find((candidate) => candidate.id === id);
    if (!item) throw new Error(`missing harness ${id}`);
    return probeReport(item, target);
  });
  const connectStart = vi.fn(
    async ({ id, target }: { id: string; target: HarnessTarget }) => {
      if (options.connectError) throw options.connectError;
      const item = harnesses.find((candidate) => candidate.id === id);
      if (!item) throw new Error(`missing harness ${id}`);
      return {
        harness_id: id,
        target,
        auth_started: true,
        browser_opened: true,
        forward_port:
          target === "home_vps" ? item.oauth_callback_port : null,
        message: `Started ${item.auth_cmd || item.binary}. Opened the sign-in in your browser.`,
      };
    },
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

function bridge(
  surface: HarnessesSurface | undefined,
  kind: HarnessesBridge["kind"] = "tauri",
  preview = false,
): HarnessesBridge {
  return { kind, preview, harnesses: surface };
}

describe("HarnessesPanel", () => {
  it("lists built-ins with embedded/worker badges, ToS notes and both target statuses", async () => {
    const fake = fakeHarnesses();
    render(
      <HarnessesPanel
        bridge={bridge(fake.surface)}
        homeConnected
        homeLabel="Studio VPS"
      />,
    );

    const opencode = await screen.findByTestId("harness-opencode");
    expect(within(opencode).getByText("Embedded Chat")).toBeTruthy();
    expect(within(opencode).getByText("API key · BYOK")).toBeTruthy();
    expect(within(opencode).getByText(/provider billing and terms apply/)).toBeTruthy();

    const aider = screen.getByTestId("harness-aider");
    expect(within(aider).getByText("Worker-Only")).toBeTruthy();

    const claude = screen.getByTestId("harness-claude-code");
    expect(within(claude).getByText(/Consumer Terms/)).toBeTruthy();
    expect(within(claude).getByText("Subscription sign-in")).toBeTruthy();

    await waitFor(() => {
      expect(within(opencode).getByTestId("harness-opencode-local")).toHaveTextContent(
        "Ready",
      );
      expect(within(opencode).getByTestId("harness-opencode-home")).toHaveTextContent(
        "Ready",
      );
    });
    expect(fake.probe).toHaveBeenCalledWith({ id: "opencode", target: "local" });
    expect(fake.probe).toHaveBeenCalledWith({ id: "opencode", target: "home_vps" });
  });

  it("says Connect does not change the embedded chat transport and keeps Connect available", async () => {
    const fake = fakeHarnesses();
    render(<HarnessesPanel bridge={bridge(fake.surface)} />);

    expect(await screen.findByTestId("harness-chat-transport-note")).toHaveTextContent(
      "Connect installs and signs in a harness. The embedded chat still runs on OpenCode; choosing another chat transport is not available yet.",
    );

    const claude = await screen.findByTestId("harness-claude-code");
    expect(within(claude).getByText("Embedded Chat")).toBeTruthy();
    expect(
      within(claude).getByRole("button", { name: "Connect on this Mac" }),
    ).toBeEnabled();
  });

  it("keeps the chat transport note visible in preview", async () => {
    const fake = fakeHarnesses();
    render(<HarnessesPanel bridge={bridge(fake.surface, "mock", true)} />);

    expect(await screen.findByTestId("harness-chat-transport-note")).toHaveTextContent(
      /choosing another chat transport is not available yet/,
    );
  });

  it("starts a one-click Home VPS connect and shows the automatic loopback forward", async () => {
    const user = userEvent.setup();
    const fake = fakeHarnesses();
    render(
      <HarnessesPanel
        bridge={bridge(fake.surface)}
        homeConnected
        homeLabel="Studio VPS"
      />,
    );

    const claude = await screen.findByTestId("harness-claude-code");
    await waitFor(() =>
      expect(fake.probe).toHaveBeenCalledWith({
        id: "claude-code",
        target: "home_vps",
      }),
    );
    expect(
      within(claude).getByTestId("harness-claude-code-forward"),
    ).toHaveTextContent("127.0.0.1:54545");

    await user.click(within(claude).getByRole("button", { name: "Connect on Studio VPS" }));
    expect(fake.connectStart).toHaveBeenCalledWith({
      id: "claude-code",
      target: "home_vps",
    });
    const result = await screen.findByTestId("harness-connect-result");
    expect(result).toHaveTextContent("Claude Code");
    expect(result).toHaveTextContent("Temporary loopback forward on 127.0.0.1:54545");
  });

  it("disables Connect in a non-desktop bridge and labels it desktop-only", async () => {
    const user = userEvent.setup();
    const fake = fakeHarnesses();
    render(<HarnessesPanel bridge={bridge(fake.surface, "http-sse")} />);

    const opencode = await screen.findByTestId("harness-opencode");
    expect(within(opencode).getByText("Desktop only")).toBeTruthy();
    expect(
      within(opencode).getByRole("button", { name: "Connect on this Mac" }),
    ).toBeDisabled();
    await user.click(screen.getByTestId("harness-add-custom"));
    expect(screen.getByRole("button", { name: "Add harness" })).toBeDisabled();
  });

  it("shows the install hint and disables Connect when the binary is missing", async () => {
    const initial = [
      manifest({
        id: "claude-code",
        name: "Claude Code",
        binary: "claude",
        install_cmd: "npm install -g @anthropic-ai/claude-code",
      }),
    ];
    const fake = fakeHarnesses({ initial });
    fake.probe.mockImplementation(async ({ target }) =>
      probeReport(initial[0]!, target, {
        binary: "missing",
        readiness: "install_required",
        auth: "unknown",
      }),
    );
    render(<HarnessesPanel bridge={bridge(fake.surface)} />);

    const card = await screen.findByTestId("harness-claude-code");
    await waitFor(() =>
      expect(within(card).getByTestId("harness-claude-code-install")).toHaveTextContent(
        "npm install -g @anthropic-ai/claude-code",
      ),
    );
    expect(
      within(card).getByRole("button", { name: "Connect on this Mac" }),
    ).toBeDisabled();
  });

  it("saves an owner-defined manifest with builtin false and both auth modes", async () => {
    const user = userEvent.setup();
    const fake = fakeHarnesses();
    render(<HarnessesPanel bridge={bridge(fake.surface)} />);
    await screen.findByTestId("harness-opencode");

    await user.click(screen.getByTestId("harness-add-custom"));
    await user.type(screen.getByLabelText("ID"), "my-harness");
    await user.type(screen.getByLabelText("Binary"), "my-harness");
    await user.type(screen.getByLabelText("Name"), "My Harness");
    await user.selectOptions(screen.getByLabelText("Protocol"), "stream-json");
    await user.type(screen.getByLabelText("OAuth callback port"), "1455");
    await user.type(
      screen.getByLabelText("Install command (shown, never run automatically)"),
      "npm install -g my-harness",
    );
    await user.type(
      screen.getByLabelText("Sign-in command (shell-free)"),
      "my-harness login",
    );
    await user.click(screen.getByLabelText("Supports Embedded Chat"));
    await user.click(screen.getByLabelText("Subscription sign-in"));
    await user.type(
      screen.getByLabelText("API-key versus subscription terms note"),
      "API-key use follows API terms; subscription use follows consumer terms.",
    );
    await user.type(
      screen.getByLabelText("Documentation URL"),
      "https://example.com/docs",
    );
    await user.click(screen.getByRole("button", { name: "Add harness" }));

    await waitFor(() => expect(fake.saveCustom).toHaveBeenCalledTimes(1));
    expect(fake.saveCustom).toHaveBeenCalledWith(
      expect.objectContaining({
        id: "my-harness",
        name: "My Harness",
        binary: "my-harness",
        protocol: "stream-json",
        supports_embedded_chat: true,
        auth_modes: ["api_key", "subscription"],
        oauth_callback_port: 1455,
        builtin: false,
      }),
    );
    expect(await screen.findByTestId("harness-my-harness")).toBeTruthy();
  });

  it("removes a custom harness only after the inline confirmation", async () => {
    const user = userEvent.setup();
    const custom = manifest({
      id: "mine",
      name: "Mine",
      binary: "mine",
      builtin: false,
      auth_modes: ["api_key"],
    });
    const fake = fakeHarnesses({ initial: [custom] });
    render(<HarnessesPanel bridge={bridge(fake.surface)} />);

    const card = await screen.findByTestId("harness-mine");
    await user.click(within(card).getByRole("button", { name: "Remove" }));
    expect(fake.deleteCustom).not.toHaveBeenCalled();
    await user.click(within(card).getByRole("button", { name: "Confirm remove" }));

    await waitFor(() => expect(fake.deleteCustom).toHaveBeenCalledWith("mine"));
    await waitFor(() => expect(screen.queryByTestId("harness-mine")).toBeNull());
  });

  it("shows a safe probe failure and a registry warning", async () => {
    const fake = fakeHarnesses({
      warnings: ["broken.toml: unknown field 'api_key'"],
      probeError: {
        code: "ssh_auth",
        message: "SSH authentication failed",
        hint: null,
        retryable: false,
      } as unknown as Error,
    });
    render(<HarnessesPanel bridge={bridge(fake.surface)} />);

    expect(
      await screen.findByRole("list", { name: "Harness registry warnings" }),
    ).toHaveTextContent("unknown field 'api_key'");
    const opencode = await screen.findByTestId("harness-opencode");
    await waitFor(() =>
      expect(within(opencode).getByTestId("harness-opencode-local")).toHaveTextContent(
        "Check failed: SSH authentication failed",
      ),
    );
  });

  it("nests provider management inside the default OpenCode card", async () => {
    const user = userEvent.setup();
    const fake = fakeHarnesses({ initial: [manifest({ id: "opencode" })] });
    const mock = new MockBridge("empty");
    const desktop = {
      ...mock,
      kind: "tauri" as const,
      preview: false,
      harnesses: fake.surface,
    } as unknown as HarnessesBridge;
    render(<HarnessesPanel bridge={desktop} />);

    const opencode = await screen.findByTestId("harness-opencode");
    expect(within(opencode).getByTestId("harness-opencode-default")).toHaveTextContent(
      "Default harness",
    );
    expect(within(opencode).getByTestId("harness-opencode-providers")).toBeInTheDocument();
    const row = await within(opencode).findByTestId("opencode-provider-deepseek");
    expect(row).toHaveTextContent("DeepSeek");

    const secret = ["sk", "harnessOnly1234567890"].join("_");
    await user.click(within(row).getByRole("button", { name: "Connect DeepSeek" }));
    await user.type(within(row).getByLabelText("API key for DeepSeek"), secret);
    await user.click(within(row).getByRole("button", { name: "Save key" }));

    expect(
      await within(opencode).findByTestId("harness-opencode-provider-notice"),
    ).toHaveTextContent(/DeepSeek (connected|key saved)/);
    expect(document.body.innerHTML).not.toContain(secret);
    mock.dispose();
  });

  it("collapses the OpenCode providers inside a bounded scroll region", async () => {
    const user = userEvent.setup();
    const fake = fakeHarnesses({ initial: [manifest({ id: "opencode" })] });
    const mock = new MockBridge("empty");
    const desktop = {
      ...mock,
      kind: "tauri" as const,
      preview: false,
      harnesses: fake.surface,
    } as unknown as HarnessesBridge;
    render(<HarnessesPanel bridge={desktop} />);

    const opencode = await screen.findByTestId("harness-opencode");
    const toggle = within(opencode).getByTestId("harness-opencode-providers-toggle");
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    const scroll = within(opencode).getByTestId("harness-opencode-provider-scroll");
    expect(scroll).toHaveClass("nx-harness__provider-scroll");
    expect(scroll).toHaveAttribute("tabindex", "0");
    expect(await within(scroll).findByTestId("opencode-provider-deepseek")).toBeInTheDocument();

    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(
      within(opencode).queryByTestId("harness-opencode-provider-scroll"),
    ).not.toBeInTheDocument();

    await user.click(toggle);
    expect(
      within(opencode).getByTestId("harness-opencode-provider-scroll"),
    ).toBeInTheDocument();
    mock.dispose();
  });

  it("says honestly when the bridge has no harness surface", async () => {
    render(<HarnessesPanel bridge={bridge(undefined)} />);
    expect(
      screen.getByText(/Harness connect is available in the desktop app/),
    ).toBeTruthy();
  });
});
