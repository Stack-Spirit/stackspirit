import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

import { E2E_LOCK, E2E_TIMEOUT } from "./support";

test.describe.configure({ lock: E2E_LOCK, timeout: E2E_TIMEOUT });
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

async function openServer(page: Page, theme: "light" | "dark") {
  await page.goto(`/?scenario=busy&theme=${theme}`);
  await page.getByTestId("sidebar-connect").click();
  await page.getByTestId("health-card").click();
  const server = page.getByRole("region", { name: "Server" });
  await expect(server).toBeVisible();
  return server;
}

for (const theme of ["light", "dark"] as const) {
  test(`22% CPU pressure with load 9.7 / 8 shows a calm 22% bar (${theme})`, async ({ page }) => {
    const server = await openServer(page, theme);
    const card = server.locator(".nx-card", { has: page.locator(".nx-card__label", { hasText: "CPU" }) });
    await expect(card).toContainText("22% pressure · load 9.7 / 8 cores");
    const meter = card.getByRole("meter", { name: "Load" });
    await expect(meter).toHaveAttribute("aria-valuenow", "22");
    await expect(card.locator(".nx-meter__fill--danger, .nx-meter__fill--bad")).toHaveCount(0);
    await expectNoWcagViolations(page);
  });

  test(`details panel names Provider and Harness (${theme})`, async ({ page }) => {
    const server = await openServer(page, theme);
    await server.getByRole("button", { name: /Anthropic · Claude Code/ }).first().click();
    const panel = page.getByTestId("details-panel");
    await expect(panel).toContainText("Provider");
    await expect(panel).toContainText("Anthropic");
    await expect(panel).toContainText("Harness");
    await expect(panel).toContainText("Claude Code");
    await expectNoWcagViolations(page);
  });

  test(`Structure shows queued leaves (${theme})`, async ({ page }) => {
    const server = await openServer(page, theme);
    await expect(server.getByRole("group", { name: "Server structure" })).toContainText(/queued · /);
    await expectNoWcagViolations(page);
  });
}
