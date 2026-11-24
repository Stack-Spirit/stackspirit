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

async function openWorkerPanel(page: Page, theme: "light" | "dark") {
  await page.goto(`/?scenario=busy&theme=${theme}`);
  await page.getByTestId("sidebar-connect").click();
  const sidebar = page.getByRole("navigation", { name: "Sessions" });
  await sidebar.getByRole("button", { name: /^vowp/ }).click();
  await sidebar.getByRole("button", { name: /^Workers/ }).click();
  await page
    .getByRole("region", { name: "Jobs" })
    .getByRole("button", { name: /^ss-2-desktop-shell/ })
    .click();
}

for (const theme of ["light", "dark"] as const) {
  test(`Open session streams events and Stop & Take over confirms (${theme})`, async ({ page }) => {
    await openWorkerPanel(page, theme);
    const panel = page.getByTestId("details-panel");
    await panel.getByRole("button", { name: "Open session" }).click();

    const view = page.getByTestId("worker-session");
    await expect(view.getByTestId("session-status")).toHaveText("Live · polling");
    await expect(view.getByTestId("session-events").getByRole("listitem").first()).toBeVisible();
    await expectNoWcagViolations(page);

    await view.getByRole("button", { name: "Stop & Take over" }).click();
    const dialog = page.getByRole("alertdialog", { name: "Confirm take over" });
    await expect(dialog).toContainText("hands the session to you. Take over?");
    await expectNoWcagViolations(page);

    await dialog.getByRole("button", { name: "Take over" }).click();
    await expect(page.getByTestId("takeover-result")).toContainText("You now own this session");
    await expect(view.getByTestId("session-status")).toHaveText("Taken over");
    await expectNoWcagViolations(page);
  });
}
