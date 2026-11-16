import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

import { E2E_LOCK, E2E_TIMEOUT } from "./support";

test.describe.configure({ lock: E2E_LOCK, timeout: E2E_TIMEOUT });

const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"];

async function expectNoWcagViolations(page: Page) {
  const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
  expect(results.violations).toEqual([]);
}

test("preview shell meets WCAG 2.2 AA", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Preview data").first()).toBeVisible();
  await expect(page.getByRole("heading", { name: "Fleet", level: 1 })).toBeVisible();
  await expectNoWcagViolations(page);

  await page.getByTestId("topbar-connect").click();
  await expect(page.getByTestId("topbar-connect")).toHaveText("Disconnect");
  await expectNoWcagViolations(page);

  await page.getByRole("button", { name: "Raw document" }).click();
  await expect(
    page.getByRole("heading", { name: "Raw VOWP document", level: 2 }),
  ).toBeVisible();
  await expectNoWcagViolations(page);
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("heading", { name: "Raw VOWP document" }),
  ).toHaveCount(0);
});

test("host manager is reachable from the left rail and meets WCAG 2.2 AA", async ({
  page,
}) => {
  await page.goto("/");
  await page
    .getByRole("navigation", { name: "Primary views" })
    .getByRole("button", { name: "Hosts" })
    .click();
  await expect(page.getByRole("heading", { name: "Hosts", level: 1 })).toBeVisible();
  await expect(page.getByRole("button", { name: "Add your VPS" }).first()).toBeVisible();
  await expectNoWcagViolations(page);

  const hostManager = page.getByTestId("host-manager");
  await hostManager
    .getByRole("button", { name: "Connect to staging-vps" })
    .click();
  await expect(hostManager.getByRole("button", { name: "Disconnect" })).toBeVisible();
  await expectNoWcagViolations(page);
});

test("both add-host wizard steps meet WCAG 2.2 AA", async ({ page }) => {
  await page.goto("/");
  await page
    .getByRole("navigation", { name: "Primary views" })
    .getByRole("button", { name: "Hosts" })
    .click();
  await page.getByRole("button", { name: "Add your VPS" }).first().click();

  const dialog = page.getByRole("dialog", { name: "Add your VPS" });
  await expect(dialog).toBeVisible();
  await expectNoWcagViolations(page);

  await dialog.getByLabel(/^Label/).fill("audit-vps");
  await dialog.getByLabel(/^Host/).fill("203.0.113.20");
  await dialog.getByRole("button", { name: "Next: verify host key" }).click();
  await expect(
    dialog.getByRole("checkbox", { name: /I verified this fingerprint/ }),
  ).toBeVisible();
  await expectNoWcagViolations(page);

  await dialog.getByRole("checkbox", { name: /I verified this fingerprint/ }).click();
  await dialog.getByRole("button", { name: "Trust and continue" }).click();
  await expect(dialog.getByTestId("probe-stage-ssh")).toContainText("Passed");
  await expectNoWcagViolations(page);
});
