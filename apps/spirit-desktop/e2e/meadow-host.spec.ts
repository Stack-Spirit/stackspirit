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
  test(`the project chat host moves Home to Local on an explicit choice (${theme})`, async ({
    page,
  }) => {
    await page.goto(`/?scenario=owner-project&theme=${theme}`);
    await page.getByTestId("sidebar-connect").click();
    await page.getByRole("button", { name: /^stackspirit/ }).click();
    await page
      .locator(".nx-proj", { hasText: "stackspirit" })
      .getByRole("button", { name: "Orchestrator" })
      .click();
    await expect(
      page.getByRole("heading", { name: "stackspirit", level: 1 }),
    ).toBeVisible();

    const host = page.getByTestId("orchestrator-host");
    await expect(host).toHaveText("Home");

    await host.click();
    const panel = page.getByRole("group", { name: "Choose session host" });
    await expect(panel).toBeVisible();
    await expect(
      panel.getByRole("button", { name: "Home (this server)" }),
    ).toBeDisabled();
    const local = panel.getByRole("button", { name: "Local · local-mac" });
    await expect(local).toBeEnabled();
    await expectNoWcagViolations(page);

    await page.keyboard.press("Escape");
    await expect(panel).toHaveCount(0);

    await host.click();
    await local.click();
    await expect(host).toHaveText("Local · local-mac");
    await expect(page.getByTestId("orchestrator-host-note")).toContainText(
      "Moved to local-mac. Reconnect the chat to use the new host.",
    );
    await expect(panel).toHaveCount(0);
    await expectNoWcagViolations(page);
  });
}
