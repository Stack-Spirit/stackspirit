import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"];

async function expectNoWcagViolations(page: Page) {
  const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
  expect(results.violations).toEqual([]);
}

test("playwright_first_run_to_fleet_scenario", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Preview data").first()).toBeVisible();
  await page.getByTestId("topbar-connect").click();
  await expect(page.getByTestId("topbar-connect")).toHaveText("Disconnect");
  await expect(page.getByTestId("fleet-canvas")).toBeVisible();
  await expect(page.getByTestId("vps-root-card")).toContainText("vps");
  await expect(page.getByTestId("ring-summary")).toContainText(/worker slots, 1 busy/);
  await expect(page.getByTestId("lane-running")).toBeVisible();
  await expect(page.getByTestId("capacity-rail")).toBeVisible();
  await expectNoWcagViolations(page);

  await page.getByRole("button", { name: /Inbox/ }).click();
  const inbox = page.getByRole("dialog", { name: "Inbox" });
  await expect(inbox).toBeVisible();
  await expect(
    inbox.getByRole("heading", { name: "Choose a merge queue policy" }),
  ).toBeVisible();
  await expect(
    inbox.getByText("Should the supervisor merge one job at a time or batch ready jobs?"),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(inbox).toHaveCount(0);

  await page.getByTestId("capacity-freshness").click();
  const raw = page.getByRole("dialog", { name: "Raw VOWP document" });
  await expect(raw).toBeVisible();
  await expect(raw.getByText("generated_at", { exact: true })).toBeVisible();
  await expect(raw.getByText("fetched_at", { exact: true })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(raw).toHaveCount(0);
});
