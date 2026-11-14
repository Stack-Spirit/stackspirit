import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

test("preview shell renders without critical accessibility violations", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Preview data").first()).toBeVisible();
  await expect(page.getByRole("heading", { name: "Fleet", level: 1 })).toBeVisible();

  const results = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"])
    .analyze();
  expect(results.violations.filter((violation) => violation.impact === "critical")).toEqual([]);
});

test("host manager is reachable from the left rail", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("navigation", { name: "Primary views" }).getByRole("button", { name: "Hosts" }).click();
  await expect(page.getByRole("heading", { name: "Hosts", level: 1 })).toBeVisible();
  await expect(page.getByRole("button", { name: "Add your VPS" }).first()).toBeVisible();
});
