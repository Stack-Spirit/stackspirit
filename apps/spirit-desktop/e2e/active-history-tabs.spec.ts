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
  test(`Workers shows active work first and history behind a tab (${theme})`, async ({ page }) => {
    await page.goto(`/?scenario=busy&theme=${theme}`);
    await page.getByTestId("sidebar-connect").click();
    const sidebar = page.getByRole("navigation", { name: "Sessions" });
    await sidebar.getByRole("button", { name: /^vowp/ }).click();
    await sidebar.getByRole("button", { name: /^Workers/ }).click();
    await expect(page.getByRole("heading", { name: "vowp · Workers", level: 1 })).toBeVisible();

    const jobs = page.getByRole("region", { name: "Jobs" });
    // Active tab: live work with model/provider badges and wait reasons.
    await expect(jobs.getByTestId("job-model").first()).not.toBeEmpty();
    await expect(jobs.getByTestId("job-phase").first()).toBeVisible();
    await expect(jobs.getByTestId("job-wait-reason").first()).toContainText("Waiting:");
    await expect(jobs.getByRole("button", { name: /^Done/ })).toHaveCount(0);
    await expect(page.getByText("unknown · unknown")).toHaveCount(0);
    await expectNoWcagViolations(page);

    // One extra click reveals history, with the real completed count.
    const historyTab = page.getByRole("button", { name: /^History/ });
    await expect(page.getByTestId("history-count")).toHaveText("37");
    await historyTab.click();
    await expect(page.getByTestId("history-view")).toBeVisible();
    await expect(page.getByTestId("history-summary")).toContainText("37 completed");
    await expect(jobs).toHaveCount(0);
    await expectNoWcagViolations(page);

    await page.getByRole("button", { name: /^Active/ }).click();
    await expect(page.getByRole("region", { name: "Jobs" })).toBeVisible();
  });
}
