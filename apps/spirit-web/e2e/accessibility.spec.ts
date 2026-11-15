import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"];

async function expectNoWcagViolations(page: Page) {
  const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
  expect(results.violations).toEqual([]);
}

test("the read-only fleet view meets WCAG 2.2 AA", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("web-lanes")).toBeVisible();
  await expectNoWcagViolations(page);
});

test("the read-only inbox drawer meets WCAG 2.2 AA", async ({ page }) => {
  await page.goto("/");
  await page.getByTestId("web-inbox-button").click();
  await expect(page.getByTestId("web-inbox-drawer")).toBeVisible();
  await expectNoWcagViolations(page);
});

test("the job drawer meets WCAG 2.2 AA", async ({ page }) => {
  await page.goto("/");
  await page.getByTestId("job-card-ss-9-gate-check").click();
  await expect(page.getByTestId("job-detail-fields")).toBeVisible();
  await expectNoWcagViolations(page);
});
