import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

import { E2E_LOCK, E2E_TIMEOUT } from "./support";

test.describe.configure({ lock: E2E_LOCK, timeout: E2E_TIMEOUT });

// The details panel slides and fades in; with reduced motion it appears at once, so
// axe measures the finished colours instead of a half-transparent animation frame.
test.use({ contextOptions: { reducedMotion: "reduce" } });

const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"];

async function expectNoWcagViolations(page: Page) {
  const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
  expect(
    results.violations.map((violation) => ({
      id: violation.id,
      targets: violation.nodes.map((node) => node.target.join(" ")),
    })),
  ).toEqual([]);
}

// Meadow is the only shell; the query only selects the mock scenario and theme.
const url = (scenario: string, theme: "light" | "dark" = "light") =>
  `/?scenario=${scenario}&theme=${theme}`;

async function connect(page: Page) {
  await page.getByTestId("sidebar-connect").click();
  await expect(page.getByTestId("health-card")).toBeVisible();
}

test("welcome with no saved server meets WCAG 2.2 AA in light and dark", async ({ page }) => {
  for (const theme of ["light", "dark"] as const) {
    await page.goto(url("empty", theme));
    await expect(page.getByRole("heading", { name: "No always-on server connected" })).toBeVisible();
    await expect(page.getByRole("button", { name: /Add server/ })).toBeVisible();
    await expectNoWcagViolations(page);
  }
});

test("setup assistant opens a calm hero, then sends without a password wall", async ({
  page,
}) => {
  await page.goto(url("empty"));
  await page.getByRole("button", { name: /Set up with AI/ }).click();
  await expect(page.getByTestId("session-location")).toContainText("Local");
  await expect(page.getByRole("heading", { name: "StackSpirit", level: 2 })).toBeVisible();
  await expect(page.getByTestId("composer-model")).toContainText("OpenCode free ·");
  // The secure card appears only when the owner asks to connect a new server.
  await expect(page.getByRole("form", { name: "Secure server details" })).toHaveCount(0);
  await expect(page.getByRole("textbox", { name: "Message" })).toBeEnabled();

  await page.getByRole("button", { name: "I have a new VPS" }).click();
  await expect(page.getByText(/Preview reply\./)).toBeVisible();
  await expect(page.getByTestId("preview-tag")).toBeVisible();

  await page.getByTestId("composer-connect-server").click();
  const form = page.getByRole("form", { name: "Secure server details" });
  await expect(form).toBeVisible();
  await form.getByLabel("Server address").fill("203.0.113.50");
  await form.getByLabel("One-time password from your provider").fill("not-a-real-secret");
  await expectNoWcagViolations(page);
  await form.getByRole("button", { name: "Connect securely" }).click();
  await expect(page.getByText(/Server details received for 203.0.113.50/)).toBeVisible();
  await expect(page.getByText("not-a-real-secret")).toHaveCount(0);
});

test("the Message field keeps focus on the hero dock and after a slow turn", async ({
  page,
}) => {
  await page.goto(url("empty"));
  await page.getByRole("button", { name: /Set up with AI/ }).click();
  const message = page.getByRole("textbox", { name: "Message" });
  await message.fill("keep my focus");
  await page.keyboard.press("Enter");
  await expect(page.getByText(/Preview reply\./)).toBeVisible();
  await expect(page.locator(".nx-hero")).toHaveClass(/nx-hero--docked/);
  await expect(message).toBeFocused();

  await page.goto(url("chat-slow"));
  await page.getByTestId("sidebar-connect").click();
  await expect(page.getByRole("heading", { name: "Global orchestrator", level: 1 })).toBeVisible();
  const slow = page.getByRole("textbox", { name: "Message" });
  await slow.fill("hold the turn open");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("session-state")).toContainText("Replying…");
  await expect(slow).toBeDisabled();
  await page.getByRole("button", { name: "Stop" }).click();
  await expect(page.getByTestId("session-state")).toContainText("Stopped");
  await page.getByRole("button", { name: "Start" }).click();
  await expect(page.getByTestId("session-state")).toContainText("Ready");
  await expect(slow).toBeFocused();
});

test("empty hero, inline model picker and context meter meet WCAG 2.2 AA", async ({
  page,
}) => {
  for (const theme of ["light", "dark"] as const) {
    await page.goto(url("empty", theme));
    await page.getByRole("button", { name: /Set up with AI/ }).click();
    await expect(page.getByRole("heading", { name: "StackSpirit", level: 2 })).toBeVisible();
    await expect(page.getByRole("form", { name: "Secure server details" })).toHaveCount(0);
    await expect(page.getByTestId("context-meter")).toContainText("18%");
    await expect(page.getByTestId("usage-pill-turns")).toContainText("3 turns");
    await expectNoWcagViolations(page);

    await page.getByTestId("composer-model").click();
    const models = page.getByRole("menu", { name: "Choose a model" });
    await expect(models).toBeVisible();
    await expect(models).toContainText("OpenCode free · DeepSeek V4.1 Flash Free");
    // Every row (and its raised checked background) stays inside the panel with the
    // same inset on both sides; the meta label keeps its normal right padding.
    const rows = await models.evaluate((menu) => {
      const panel = menu.getBoundingClientRect();
      return [...menu.querySelectorAll(".nx-model-menu__item")].map((item) => {
        const row = item.getBoundingClientRect();
        const meta = item.querySelector(".nx-menu__meta")?.getBoundingClientRect();
        return {
          leftInset: row.left - panel.left,
          rightInset: panel.right - row.right,
          metaRightInset: meta ? panel.right - meta.right : 0,
        };
      });
    });
    expect(rows).toHaveLength(3);
    for (const row of rows) {
      expect(row.leftInset).toBeGreaterThan(0);
      expect(row.rightInset).toBeGreaterThan(0);
      expect(Math.abs(row.rightInset - row.leftInset)).toBeLessThan(0.5);
      expect(row.metaRightInset).toBeGreaterThan(0);
    }
    await expectNoWcagViolations(page);
    await page.keyboard.press("Escape");

    await page.getByTestId("context-meter").click();
    const usage = page.getByRole("group", { name: "Context usage" });
    await expect(usage).toContainText("23,600 / 128,000 tokens");
    await expectNoWcagViolations(page);
    await page.keyboard.press("Escape");
  }
});

test("server chat shows the honest VOWP gate, not a fake reply", async ({ page }) => {
  await page.goto(url("chat-server-gated"));
  await connect(page);

  const gate = page.getByTestId("chat-error");
  await expect(gate).toBeVisible();
  await expect(gate).toHaveAttribute("data-gate", "true");
  await expect(gate).toContainText("Server chat needs VOWP 0.5's `vowp orchestrator endpoint`");
  await expect(gate).toContainText("the app-owned SSH forward is not approved");
  await expect(page.getByTestId("session-location")).toContainText("Not connected");
  await expect(page.getByRole("textbox", { name: "Message" })).toBeDisabled();
  await expect(page.getByText(/Preview reply\./)).toHaveCount(0);
  await expectNoWcagViolations(page);
});

test("connected shell, details panel and settings menu meet WCAG 2.2 AA", async ({ page }) => {
  for (const theme of ["light", "dark"] as const) {
    await page.goto(url("busy", theme));
    await connect(page);
    await expect(page.getByRole("heading", { name: "Global orchestrator", level: 1 })).toBeVisible();
    await expect(page.getByTestId("session-location")).toContainText("Local · this Mac");
    await expect(page.getByTestId("details-panel")).toHaveAttribute("aria-hidden", "true");
    await expectNoWcagViolations(page);

    await page.getByTestId("panel-toggle").click();
    await expect(page.getByTestId("details-panel")).toHaveAttribute("aria-hidden", "false");
    await expectNoWcagViolations(page);

    await page.getByTestId("settings-trigger").click();
    await expect(page.getByRole("menu", { name: "Settings" })).toBeVisible();
    await expectNoWcagViolations(page);
    await page.keyboard.press("Escape");
  }
});

test("server and workers pages meet WCAG 2.2 AA and open job details", async ({ page }) => {
  await page.goto(url("busy"));
  await connect(page);

  await page.getByTestId("health-card").click();
  await expect(page.getByRole("group", { name: "Server structure" })).toBeVisible();
  await expectNoWcagViolations(page);

  const sidebar = page.getByRole("navigation", { name: "Sessions" });
  await sidebar.getByRole("button", { name: /^vowp/ }).click();
  await sidebar.getByRole("button", { name: /^Workers/ }).click();
  await expect(page.getByRole("heading", { name: "vowp · Workers", level: 1 })).toBeVisible();
  await page.getByRole("region", { name: "Jobs" }).getByRole("button", { name: /^ss-2-desktop-shell/ }).click();
  await expect(page.getByTestId("details-panel").getByRole("heading", { name: "ss-2-desktop-shell" })).toBeVisible();
  await expectNoWcagViolations(page);
});

test("add project dialog meets WCAG 2.2 AA", async ({ page }) => {
  await page.goto(url("busy"));
  await connect(page);
  await page.getByRole("button", { name: "Add project" }).click();
  const dialog = page.getByRole("dialog", { name: "Add a project" });
  await dialog.getByLabel("Repository URL").fill("https://github.com/acme/billing-api.git");
  await expect(dialog.getByLabel("Project name")).toHaveValue("billing-api");
  await expectNoWcagViolations(page);
});
