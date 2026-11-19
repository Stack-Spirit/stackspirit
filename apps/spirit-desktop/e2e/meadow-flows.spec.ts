import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

import { E2E_LOCK, E2E_TIMEOUT } from "./support";

test.describe.configure({ lock: E2E_LOCK, timeout: E2E_TIMEOUT });

// Dialogs and drawers slide and fade in; with reduced motion they appear at once, so
// axe measures the finished colours instead of a half-transparent animation frame.
test.use({ contextOptions: { reducedMotion: "reduce" } });

const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"];
const THEMES = ["light", "dark"] as const;

async function expectNoWcagViolations(page: Page) {
  const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
  expect(
    results.violations.map((violation) => ({
      id: violation.id,
      targets: violation.nodes.map((node) => node.target.join(" ")),
    })),
  ).toEqual([]);
}

/** Opens Settings through the Meadow sidebar and reveals the advanced cards. */
async function openAdvancedSettings(page: Page, theme: "light" | "dark") {
  await page.goto(`/?theme=${theme}`);
  await expect(page.getByTestId("next-shell")).toBeVisible();
  await page.getByTestId("settings-trigger").click();
  await page.getByRole("menuitem", { name: "All settings" }).click();
  await expect(page.getByRole("heading", { name: "Settings", level: 1 })).toBeVisible();
  await page.getByRole("button", { name: "Show advanced settings" }).click();
}

test("host manager is reachable from the left rail and meets WCAG 2.2 AA", async ({
  page,
}) => {
  for (const theme of THEMES) {
    await openAdvancedSettings(page, theme);
    const hostManager = page.getByTestId("host-manager");
    await expect(
      hostManager.getByRole("heading", { name: "Manage hosts" }),
    ).toBeVisible();
    await expect(
      hostManager.getByRole("button", { name: "Connect to staging-vps" }),
    ).toBeVisible();
    await expectNoWcagViolations(page);

    await hostManager.getByRole("button", { name: "Connect to staging-vps" }).click();
    await expect(hostManager.getByRole("button", { name: "Disconnect" })).toBeVisible();
    await expectNoWcagViolations(page);
  }
});

test("the add-server wizard steps meet WCAG 2.2 AA", async ({ page }) => {
  for (const theme of THEMES) {
    await page.goto(`/?theme=${theme}`);
    await expect(page.getByTestId("next-shell")).toBeVisible();
    await page.getByTestId("settings-trigger").click();
    await page.getByRole("menuitem", { name: "Add server" }).click();

    const dialog = page.getByRole("dialog", { name: "Add a server" });
    await expect(dialog).toBeVisible();
    await expectNoWcagViolations(page);

    await dialog.getByLabel(/^Label/).fill("audit-vps");
    await dialog.getByLabel(/^Host/).fill("203.0.113.20");
    await dialog.getByRole("button", { name: "Next: verify host key" }).click();
    await expect(
      dialog.getByRole("checkbox", { name: /I verified this fingerprint/ }),
    ).toBeVisible();
    await expectNoWcagViolations(page);

    await dialog.getByRole("checkbox", { name: /I verified this fingerprint/ }).check();
    await dialog.getByRole("button", { name: "Trust and continue" }).click();
    await expect(dialog.getByTestId("probe-stage-ssh")).toContainText("Passed");
    await expectNoWcagViolations(page);
  }
});
