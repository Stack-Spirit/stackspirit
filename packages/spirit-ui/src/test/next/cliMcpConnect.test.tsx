import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import {
  BUILTIN_TOOLS,
  CliMcpConnectPanel,
  REMOTE_MCP_URL,
  clientConfig,
  filterTools,
  validateDraft,
  type McpClientId,
} from "../../next/CliMcpConnectPanel";

describe("catalog helpers", () => {
  it("ships the required presets", () => {
    expect(BUILTIN_TOOLS).toHaveLength(19);
    expect(BUILTIN_TOOLS.find((t) => t.id === "github")).toMatchObject({
      cli: "gh",
      mcp: "http",
    });
  });

  it("searches by words and filters by category", () => {
    expect(filterTools(BUILTIN_TOOLS, "gh", null).map((t) => t.id)).toContain(
      "github",
    );
    expect(
      filterTools(BUILTIN_TOOLS, "", "databases").map((t) => t.id),
    ).toEqual(["supabase", "postgres"]);
    expect(filterTools(BUILTIN_TOOLS, "zzzz", null)).toEqual([]);
    expect(filterTools(BUILTIN_TOOLS, "deploy", "code_issues")).toEqual([]);
  });

  it("builds local and remote configs for every client without a token", () => {
    const clients: McpClientId[] = [
      "claude-desktop",
      "claude-code",
      "antigravity",
      "cursor",
      "opencode",
    ];
    for (const client of clients) {
      expect(clientConfig(client, "local")).toContain("stackspirit");
      expect(clientConfig(client, "local")).not.toContain("https://");
      expect(clientConfig(client, "remote")).toContain(REMOTE_MCP_URL);
      expect(clientConfig(client, "remote").toLowerCase()).not.toMatch(
        /token|bearer/,
      );
    }
    expect(clientConfig("claude-code", "local")).toBe(
      "claude mcp add stackspirit -- stackspirit mcp",
    );
  });

  it("rejects secrets and bad input in a custom draft", () => {
    const base = {
      kind: "mcp" as const,
      name: "X",
      command: "",
      transport: "http" as const,
      url: "",
    };
    expect(
      validateDraft({ ...base, url: "https://u:p@x.example.com" }),
    ).toMatch(/secret|URL/i);
    expect(
      validateDraft({ ...base, url: "https://x.example.com/mcp?token=abc" }),
    ).toMatch(/secret/i);
    expect(
      validateDraft({ ...base, url: "https://x.example.com/mcp" }),
    ).toBeNull();
    expect(validateDraft({ ...base, kind: "cli", command: "/bin/x" })).toMatch(
      /bare/,
    );
    expect(validateDraft({ ...base, name: " " })).toMatch(/name/);
  });
});

describe("CliMcpConnectPanel", () => {
  it("copies the local and remote config per client", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    render(<CliMcpConnectPanel homeConnected />);
    await user.click(
      screen.getByRole("button", { name: "Copy config for Cursor" }),
    );
    expect(writeText).toHaveBeenLastCalledWith(clientConfig("cursor", "local"));
    await user.click(screen.getByTestId("mcp-mode-remote"));
    await user.click(
      screen.getByRole("button", { name: "Copy config for OpenCode" }),
    );
    expect(writeText).toHaveBeenLastCalledWith(
      clientConfig("opencode", "remote"),
    );
  });

  it("installs through the host callback and disables Install without one", async () => {
    const onInstallClient = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();
    const { unmount } = render(
      <CliMcpConnectPanel homeConnected onInstallClient={onInstallClient} />,
    );
    await user.click(
      screen.getByRole("button", { name: "Install into Claude Code" }),
    );
    expect(onInstallClient).toHaveBeenCalledWith("claude-code", "local");
    unmount();
    render(<CliMcpConnectPanel homeConnected />);
    expect(
      screen.getByRole("button", { name: "Install into Claude Code" }),
    ).toBeDisabled();
  });

  it("searches and filters the list", async () => {
    const user = userEvent.setup();
    render(<CliMcpConnectPanel homeConnected />);
    await user.type(
      screen.getByRole("searchbox", { name: "Search tools" }),
      "postgres",
    );
    expect(screen.getByTestId("tool-postgres")).toBeInTheDocument();
    expect(screen.queryByTestId("tool-github")).toBeNull();
    await user.clear(screen.getByRole("searchbox", { name: "Search tools" }));
    await user.click(screen.getByTestId("category-communication_calendar"));
    expect(screen.getByTestId("tool-slack")).toBeInTheDocument();
    expect(screen.queryByTestId("tool-docker")).toBeNull();
  });

  it("defaults to Both and tracks Mac and Home dots independently", async () => {
    const onConnectTool = vi
      .fn()
      .mockResolvedValue({ mac: "connected", home: "needs_attention" });
    const user = userEvent.setup();
    render(<CliMcpConnectPanel homeConnected onConnectTool={onConnectTool} />);
    const select = screen.getByRole("combobox", { name: "Target for GitHub" });
    expect(select).toHaveValue("both");
    await user.click(screen.getByRole("button", { name: "Connect GitHub" }));
    expect(onConnectTool).toHaveBeenCalledWith("github", "both");
    await waitFor(() =>
      expect(screen.getByTestId("dot-mac-github")).toHaveAccessibleName(
        "Mac connected",
      ),
    );
    expect(screen.getByTestId("dot-home-github")).toHaveAccessibleName(
      "Home needs attention",
    );
    expect(screen.getByTestId("dot-mac-vercel")).toHaveAccessibleName(
      "Mac not connected",
    );
  });

  it("falls back to Local when no Home is connected", async () => {
    const onConnectTool = vi.fn().mockResolvedValue({ mac: "connected" });
    const user = userEvent.setup();
    render(
      <CliMcpConnectPanel
        homeConnected={false}
        onConnectTool={onConnectTool}
      />,
    );
    expect(
      screen.getByRole("combobox", { name: "Target for GitHub" }),
    ).toHaveValue("local");
    await user.click(screen.getByRole("button", { name: "Connect GitHub" }));
    expect(onConnectTool).toHaveBeenCalledWith("github", "local");
  });

  it("marks failures as needing attention", async () => {
    const onConnectTool = vi.fn().mockRejectedValue(new Error("nope"));
    const user = userEvent.setup();
    render(
      <CliMcpConnectPanel
        homeConnected={false}
        onConnectTool={onConnectTool}
      />,
    );
    await user.click(screen.getByRole("button", { name: "Connect Docker" }));
    await waitFor(() =>
      expect(screen.getByTestId("dot-mac-docker")).toHaveAccessibleName(
        "Mac needs attention",
      ),
    );
  });

  it("adds a custom MCP and refuses a secret", async () => {
    const onSaveCustom = vi.fn().mockImplementation((d) => ({
      id: "mine",
      name: d.name,
      category: "custom",
      description: "",
      mcp: d.transport,
      custom: true,
    }));
    const user = userEvent.setup();
    render(<CliMcpConnectPanel homeConnected onSaveCustom={onSaveCustom} />);
    await user.click(
      screen.getByRole("button", { name: "+ Add custom CLI / MCP" }),
    );
    const form = screen.getByTestId("custom-tool-form");
    await user.selectOptions(
      within(form).getByLabelText("Custom tool kind"),
      "mcp",
    );
    await user.selectOptions(
      within(form).getByLabelText("MCP transport"),
      "http",
    );
    await user.type(within(form).getByLabelText("Custom tool name"), "Mine");
    await user.type(
      within(form).getByLabelText("URL"),
      "https://x.example.com/mcp?token=abc",
    );
    await user.click(within(form).getByRole("button", { name: "Save tool" }));
    expect(screen.getByRole("alert")).toHaveTextContent(/secret/i);
    expect(onSaveCustom).not.toHaveBeenCalled();
    await user.clear(within(form).getByLabelText("URL"));
    await user.type(
      within(form).getByLabelText("URL"),
      "https://x.example.com/mcp",
    );
    await user.click(within(form).getByRole("button", { name: "Save tool" }));
    await waitFor(() =>
      expect(screen.getByTestId("tool-mine")).toBeInTheDocument(),
    );
    expect(onSaveCustom).toHaveBeenCalledTimes(1);
  });
});
