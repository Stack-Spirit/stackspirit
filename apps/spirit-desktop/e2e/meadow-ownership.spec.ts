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

async function openGlobal(page: Page, scenario: string, theme: "light" | "dark") {
  await page.goto(`/?scenario=${scenario}&theme=${theme}`);
  await page.getByTestId("sidebar-connect").click();
  await expect(
    page.getByRole("heading", { name: "Global orchestrator", level: 1 }),
  ).toBeVisible();
}

for (const theme of ["light", "dark"] as const) {
  test(`an idle owner is taken over in one click (${theme})`, async ({ page }) => {
    await openGlobal(page, "owner-idle", theme);
    const banner = page.getByTestId("ownership-banner");
    await expect(banner).toContainText("Active on OpenCode · macbook");
    await expect(page.getByTestId("ownership-activity")).toHaveText("Idle");
    await expectNoWcagViolations(page);

    await banner.getByRole("button", { name: "Take over" }).click();
    await expect(page.getByRole("alertdialog")).toHaveCount(0);
    await expect(banner).toHaveCount(0);
    await expectNoWcagViolations(page);
  });

  test(`a running owner needs an extra confirmation (${theme})`, async ({ page }) => {
    await openGlobal(page, "owner-running", theme);
    const banner = page.getByTestId("ownership-banner");
    await expect(page.getByTestId("ownership-activity")).toHaveText("Running");

    await banner.getByRole("button", { name: "Take over" }).click();
    const dialog = page.getByRole("alertdialog", { name: "Confirm take over" });
    await expect(dialog).toContainText(
      "OpenCode · macbook is currently running a turn. Taking over will fence its writes and tell it to stop. Take over anyway?",
    );
    await expect(banner).toBeVisible();
    await expectNoWcagViolations(page);

    await dialog.getByRole("button", { name: "Cancel" }).click();
    await expect(dialog).toHaveCount(0);
    await expect(banner).toBeVisible();

    await banner.getByRole("button", { name: "Take over" }).click();
    await dialog.getByRole("button", { name: "Take over anyway" }).click();
    await expect(banner).toHaveCount(0);
  });

  test(`no banner when StackSpirit owns the session or no owner is known (${theme})`, async ({
    page,
  }) => {
    await openGlobal(page, "owner-local", theme);
    await expect(page.getByTestId("ownership-banner")).toHaveCount(0);
    await expectNoWcagViolations(page);

    await openGlobal(page, "busy", theme);
    await expect(page.getByTestId("ownership-banner")).toHaveCount(0);
  });
}
