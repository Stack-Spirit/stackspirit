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

    // A non-reasoning model shows no Reasoning effort selector.
    const model = page.getByTestId("composer-model");
    await model.click();
    const menu = page.getByTestId("model-picker-popover");
    await expect(menu).toBeVisible();
    await expect(page.getByTestId("reasoning-effort")).toHaveCount(0);

    // A reasoning-capable model shows it; picking High updates the trigger.
    await menu.getByTestId("composer-model-search").fill("deepseek flash");
    await menu.getByRole("option", { name: /^DeepSeek V4\.1 Flash(?! Free)/ }).click();
    await model.click();
    const effort = page.getByTestId("reasoning-effort");
    await expect(effort).toBeVisible();
    await page.getByTestId("reasoning-high").click();
    await expect(page.getByTestId("composer-model")).toContainText("DeepSeek V4.1 Flash · High");
    await expectNoWcagViolations(page);
  });
}
