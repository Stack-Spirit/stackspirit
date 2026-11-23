import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

import { E2E_LOCK, E2E_TIMEOUT } from "./support";

test.describe.configure({ lock: E2E_LOCK, timeout: E2E_TIMEOUT });
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

for (const theme of THEMES) {
  test(`composer + attaches/removes files and reasoning effort follows the model (${theme})`, async ({
    page,
  }) => {
    await page.goto(`/?scenario=empty&theme=${theme}`);
    await page.getByRole("button", { name: /Set up with AI/ }).click();

    // No Workspace Write dropdown anywhere in the composer.
    await expect(page.getByText(/Workspace Write/i)).toHaveCount(0);

    // `+` attach: chip appears, is removable, axe stays clean.
    await expect(page.getByTestId("composer-attach")).toBeVisible();
    await page.getByTestId("composer-file-input").setInputFiles({
      name: "notes.txt",
      mimeType: "text/plain",
      buffer: Buffer.from("hello"),
    });
    const chips = page.getByTestId("composer-attachments");
    await expect(chips).toContainText("notes.txt");
    await expectNoWcagViolations(page);
    await page.getByRole("button", { name: "Remove notes.txt" }).click();
    await expect(chips).toHaveCount(0);

    // Two-row menu: a non-reasoning model shows Model only, with no Effort row.
    const model = page.getByTestId("composer-model");
    await model.click();
    const menu = page.getByTestId("model-picker-popover");
    await expect(menu).toBeVisible();
    await expect(page.getByTestId("composer-model-row")).toContainText("Model");
    await expect(page.getByTestId("composer-effort-row")).toHaveCount(0);
    await expect(page.getByTestId("reasoning-effort")).toHaveCount(0);
    await expectNoWcagViolations(page);

    // Drill into Model, pick a reasoning-capable model: Effort defaults to an active
    // effort (never None) and Escape steps back.
    await page.getByTestId("composer-model-row").click();
    await menu.getByTestId("composer-model-search").fill("deepseek flash");
    await menu.getByRole("option", { name: /^DeepSeek V4\.1 Flash(?! Free)/ }).click();
    await expect(model).toContainText(/DeepSeek V4\.1 Flash · (Medium|High|\w+)/);
    await expect(model).not.toContainText(/None/i);
    await model.click();
    const effortRow = page.getByTestId("composer-effort-row");
    await expect(effortRow).toContainText("Effort");
    await expect(effortRow).not.toContainText(/None/i);
    await expectNoWcagViolations(page);
    await effortRow.click();
    await expect(page.getByTestId("reasoning-effort")).toBeVisible();
    await expect(page.getByRole("radio", { checked: true })).toHaveCount(1);
    await page.getByTestId("reasoning-high").click();
    await expect(page.getByTestId("composer-model")).toContainText("DeepSeek V4.1 Flash · High");
    await expectNoWcagViolations(page);
  });
}
