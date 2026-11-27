import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Locator, type Page } from "@playwright/test";

import { E2E_LOCK, E2E_TIMEOUT } from "./support";

test.describe.configure({ lock: E2E_LOCK, timeout: E2E_TIMEOUT });
test.use({ contextOptions: { reducedMotion: "reduce" } });

const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"];
const THEMES = ["light", "dark"] as const;
const RAIL = ["General", "Account", "Server & Machines", "Harnesses", "CLI & MCP Connect"] as const;

async function expectNoWcagViolations(page: Page) {
  const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
  expect(
    results.violations.map((violation) => ({
      id: violation.id,
      targets: violation.nodes.map((node) => node.target.join(" ")),
    })),
  ).toEqual([]);
}

async function openSettings(page: Page, theme: (typeof THEMES)[number]) {
  await page.goto(`/?scenario=busy&theme=${theme}`);
  await expect(page.getByTestId("next-shell")).toBeVisible();
  const opener = page.getByTestId("settings-trigger");
  await opener.click();
  const dialog = page.getByRole("dialog", { name: "Settings" });
  await expect(dialog).toBeVisible();
  return { dialog, opener };
}

async function selectSection(dialog: Locator, label: string, id: string) {
  const tab = dialog.getByRole("tab", { name: label, exact: true });
  await tab.click();
  await expect(tab).toHaveAttribute("aria-selected", "true");
  const pane = dialog.getByTestId(`settings-pane-${id}`);
  await expect(pane).toBeVisible();
  await expect(pane).toHaveAttribute("aria-labelledby", `settings-tab-${id}`);
  return pane;
}

for (const theme of THEMES) {
  test(`Settings opens as a modal and switches every rail section (${theme})`, async ({ page }) => {
    const { dialog } = await openSettings(page, theme);

    await expect(
      dialog.getByRole("heading", { name: "Settings", level: 2, exact: true }),
    ).toBeVisible();
    await expect(page.getByTestId("settings-backdrop")).toBeVisible();
    await expect(dialog.getByRole("tab")).toHaveCount(RAIL.length);
    for (const label of RAIL) {
      await expect(dialog.getByRole("tab", { name: label, exact: true })).toBeVisible();
    }
    await expect(dialog.getByTestId("settings-close")).toBeFocused();

    const general = await selectSection(dialog, "General", "general");
    await expect(
      general.getByRole("heading", { name: "General", level: 3, exact: true }),
    ).toBeVisible();
    await expect(general.getByRole("group", { name: "Theme" })).toBeVisible();

    const account = await selectSection(dialog, "Account", "account");
    await expect(
      account.getByRole("heading", { name: "Account", level: 3, exact: true }),
    ).toBeVisible();
    await expect(account).toContainText("StackSpirit Account");
    await expect(account).toContainText("Managed Homes");

    const server = await selectSection(dialog, "Server & Machines", "server");
    await expect(
      server.getByRole("heading", { name: "Server & Machines", level: 3, exact: true }),
    ).toBeVisible();
    await expect(server).toContainText("Home server");
    await expect(server).toContainText("Always-On Machines");

    const harnesses = await selectSection(dialog, "Harnesses", "harnesses");
    await expect(
      harnesses.getByRole("heading", { name: "Harnesses", level: 3, exact: true }),
    ).toBeVisible();
    await expect(harnesses.getByTestId("harnesses-panel")).toBeVisible();

    const cli = await selectSection(dialog, "CLI & MCP Connect", "cli-mcp");
    await expect(
      cli.getByRole("heading", { name: "CLI & MCP Connect", level: 3, exact: true }),
    ).toBeVisible();
    await expect(cli.getByTestId("cli-mcp-connect")).toBeVisible();
  });

  test(`Escape and Close return focus to the sidebar opener (${theme})`, async ({ page }) => {
    const { dialog, opener } = await openSettings(page, theme);

    // Tab stays inside the modal, then Escape returns to the opener.
    for (let index = 0; index < 30; index += 1) await page.keyboard.press("Tab");
    expect(await dialog.evaluate((node) => node.contains(document.activeElement))).toBe(true);

    await page.keyboard.press("Escape");
    await expect(dialog).toHaveCount(0);
    await expect(page.getByTestId("settings-backdrop")).toHaveCount(0);
    await expect(opener).toBeFocused();

    await opener.click();
    await expect(dialog).toBeVisible();
    await dialog.getByTestId("settings-close").click();
    await expect(dialog).toHaveCount(0);
    await expect(page.getByTestId("settings-backdrop")).toHaveCount(0);
    await expect(opener).toBeFocused();
  });
}

for (const theme of THEMES) {
  test(`Harnesses lists the built-ins and keeps the preview gate honest (${theme})`, async ({
    page,
  }) => {
    const { dialog } = await openSettings(page, theme);
    const pane = await selectSection(dialog, "Harnesses", "harnesses");
    const panel = pane.getByTestId("harnesses-panel");
    await expect(panel).toBeVisible();

    const builtins = [
      ["opencode", "OpenCode", "Embedded Chat"],
      ["claude-code", "Claude Code", "Embedded Chat"],
      ["codex", "Codex", "Embedded Chat"],
      ["gemini-cli", "Gemini CLI", "Embedded Chat"],
      ["antigravity", "Antigravity", "Worker-Only"],
      ["cursor-cli", "Cursor CLI", "Worker-Only"],
      ["aider", "Aider", "Worker-Only"],
      ["goose", "Goose", "Embedded Chat"],
    ] as const;

    for (const [id, name, badge] of builtins) {
      const card = panel.getByTestId(`harness-${id}`);
      await expect(card).toBeVisible();
      await expect(card.getByRole("heading", { name, level: 3, exact: true })).toBeVisible();
      await expect(card.getByText(badge, { exact: true })).toBeVisible();
      await expect(card.getByText("Built-in", { exact: true })).toBeVisible();
      // The MockBridge is a preview, so the product must never claim it can sign in.
      const connect = card.getByRole("button", { name: "Connect on this Mac" });
      await expect(connect).toBeDisabled();
      await expect(connect).toHaveAttribute(
        "title",
        "Connect is available in the desktop app only.",
      );
    }

    await expect(panel.getByText("Desktop only", { exact: true }).first()).toBeVisible();
    await expect(panel.getByRole("list", { name: "Harness registry warnings" })).toContainText(
      "Preview: built-in presets only",
    );
    // Preview probes fail instead of inventing a "Ready" state.
    await expect(panel.getByTestId("harness-opencode-local")).toContainText("Check failed:");
  });

  test(`CLI & MCP Connect shows copy-only commands without secrets (${theme})`, async ({
    page,
  }) => {
    const { dialog } = await openSettings(page, theme);
    const pane = await selectSection(dialog, "CLI & MCP Connect", "cli-mcp");
    const panel = pane.getByTestId("cli-mcp-connect");
    await expect(panel).toBeVisible();
    await expect(panel.getByTestId("cli-mcp-target-local")).toHaveAttribute("aria-pressed", "true");

    const command = panel.getByTestId("cli-install-command");
    await expect(command).toHaveText("npm install -g stackspirit");

    const snippet = panel.getByTestId("mcp-snippet");
    await expect(snippet).toBeVisible();
    const text = (await snippet.textContent()) ?? "";
    expect(text).toContain("stackspirit");
    expect(text).toContain("mcp");
    expect(JSON.parse(text)).toEqual({
      mcpServers: {
        stackspirit: {
          command: "stackspirit",
          args: ["mcp"],
        },
      },
    });

    // The snippet is a local stdio entry, never a secret or an SSH transport.
    for (const forbidden of [
      "token",
      "key",
      "apikey",
      "api_key",
      "password",
      "passphrase",
      "secret",
      "ssh",
      "identity",
      "credential",
      "privatekey",
      "private_key",
    ]) {
      expect(text.toLowerCase(), forbidden).not.toContain(forbidden);
      expect(((await command.textContent()) ?? "").toLowerCase(), forbidden).not.toContain(
        forbidden,
      );
    }
  });
}

test("copy controls write the exact MCP config and install command when the clipboard is available", async ({
  page,
  context,
}) => {
  let granted = true;
  try {
    await context.grantPermissions(["clipboard-read", "clipboard-write"], {
      origin: "http://127.0.0.1:1420",
    });
  } catch {
    granted = false;
  }
  test.skip(
    !granted,
    "This browser does not grant clipboard permissions; skipping the copy assertion.",
  );

  const { dialog } = await openSettings(page, "light");
  const pane = await selectSection(dialog, "CLI & MCP Connect", "cli-mcp");
  const panel = pane.getByTestId("cli-mcp-connect");

  await panel.getByTestId("copy-mcp-config").click();
  await expect(panel.getByTestId("cli-mcp-copied")).toHaveText("MCP config copied.");
  let copied = await page.evaluate(async () => {
    try {
      return await navigator.clipboard.readText();
    } catch {
      return null;
    }
  });
  test.skip(copied === null, "Clipboard readback is not granted in this browser.");
  expect(JSON.parse(copied!)).toEqual({
    mcpServers: {
      stackspirit: {
        command: "stackspirit",
        args: ["mcp"],
      },
    },
  });

  await panel.getByTestId("copy-cli-install").click();
  await expect(panel.getByTestId("cli-mcp-copied")).toHaveText("Install command copied.");
  copied = await page.evaluate(async () => {
    try {
      return await navigator.clipboard.readText();
    } catch {
      return null;
    }
  });
  test.skip(copied === null, "Clipboard readback is not granted in this browser.");
  expect(copied).toBe("npm install -g stackspirit");
});

test("Settings, Harnesses and CLI & MCP meet WCAG 2.2 AA in light and dark", async ({ page }) => {
  for (const theme of THEMES) {
    const { dialog } = await openSettings(page, theme);

    // Settings root, with General selected.
    await expect(
      dialog.getByRole("heading", { name: "Settings", level: 2, exact: true }),
    ).toBeVisible();
    await expectNoWcagViolations(page);

    const harnesses = await selectSection(dialog, "Harnesses", "harnesses");
    const panel = harnesses.getByTestId("harnesses-panel");
    await expect(panel.getByTestId("harness-opencode")).toBeVisible();
    await expect(panel.getByTestId("harness-opencode-local")).toContainText("Check failed:");
    await expectNoWcagViolations(page);

    const cli = await selectSection(dialog, "CLI & MCP Connect", "cli-mcp");
    await expect(cli.getByTestId("mcp-snippet")).toBeVisible();
    await expectNoWcagViolations(page);
  }
});
