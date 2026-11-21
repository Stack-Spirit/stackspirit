import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

import { E2E_LOCK, E2E_TIMEOUT } from "./support";

test.describe.configure({ lock: E2E_LOCK, timeout: E2E_TIMEOUT });
test.use({ contextOptions: { reducedMotion: "reduce", permissions: ["clipboard-read", "clipboard-write"] } });

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

async function startTurn(page: Page, scenario: string, theme: "light" | "dark") {
  await page.goto(`/?scenario=${scenario}&theme=${theme}`);
  await page.getByTestId("sidebar-connect").click();
  await expect(
    page.getByRole("heading", { name: "Global orchestrator", level: 1 }),
  ).toBeVisible();
  await page.getByRole("textbox", { name: "Message" }).fill("tidy the shared folder");
  await page.keyboard.press("Enter");
}

for (const theme of ["light", "dark"] as const) {
  test(`assistant turn has no sender header and a working answer footer (${theme})`, async ({
    page,
  }) => {
    await startTurn(page, "chat-tools", theme);
    await expect(page.getByTestId("session-state")).toContainText("Ready");
    const message = page.getByTestId("assistant-message").last();
    await expect(message.getByText("StackSpirit", { exact: true })).toHaveCount(0);
    const footer = message.getByTestId("answer-footer");
    await expect(footer).toBeVisible();
    await footer.getByRole("button", { name: "Copy answer" }).click();
    await expect(footer.getByRole("button", { name: "Copied" })).toBeVisible();
    await expect
      .poll(() => page.evaluate(() => navigator.clipboard.readText()))
      .toContain("Preview reply");
    await expectNoWcagViolations(page);
  });

  test(`the SpiritLoader shows while a turn is replying (${theme})`, async ({ page }) => {
    await startTurn(page, "chat-permission", theme);
    await expect(page.getByTestId("session-state")).toContainText("Replying…");
    const loader = page.getByTestId("spirit-loader");
    await expect(loader).toBeVisible();
    await expect(loader.getByRole("status")).toHaveCount(0);
    await expect(loader).toContainText("StackSpirit is replying");
    await expect(loader.locator("svg .nx-loader__eye")).toHaveCount(2);
    await expectNoWcagViolations(page);
  });
}
