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

for (const theme of ["light", "dark"] as const) {
  test(`the Server page shows live Memory, Disk and CPU (${theme})`, async ({ page }) => {
    await page.goto(`/?scenario=owner-idle&theme=${theme}`);
    await page.getByTestId("sidebar-connect").click();
    await page.getByTestId("health-card").click();
    const server = page.getByRole("region", { name: "Server" });
    await expect(server).toBeVisible();

    const card = (label: string) =>
      server.locator(".nx-card", { has: page.locator(".nx-card__label", { hasText: label }) });
    await expect(card("Memory")).toContainText(/\d+(\.\d+)? GiB of \d+(\.\d+)? GiB/);
    await expect(card("Disk")).toContainText(/\d+(\.\d+)? GiB free/);
    await expect(card("CPU")).not.toContainText("—");
    await expectNoWcagViolations(page);
  });

  test(`Engine & Models shows the accurate Default model badge (${theme})`, async ({ page }) => {
    await page.goto(`/?scenario=empty&theme=${theme}`);
    await page.getByRole("button", { name: /Set up with AI/ }).click();
    await page.getByTestId("composer-model").click();
    await page.getByTestId("composer-model-row").click();
    await page.getByTestId("composer-connect-provider").click();
    const dialog = page.getByRole("dialog", { name: "Engine & Models" });
    await expect(dialog.getByTestId("engine-models-settings")).toContainText(/Default · \S/);
    await expectNoWcagViolations(page);
  });
}
