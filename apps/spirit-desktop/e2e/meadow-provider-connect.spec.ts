import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

import { E2E_LOCK, E2E_TIMEOUT } from "./support";

test.describe.configure({ lock: E2E_LOCK, timeout: E2E_TIMEOUT });

// Dialogs and menus fade in; reduced motion keeps axe on the finished colours.
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

const url = (theme: "light" | "dark") => `/?scenario=empty&theme=${theme}`;

for (const theme of THEMES) {
  test(`OpenCode-first provider connect and grouped model picker (${theme})`, async ({
    page,
  }) => {
    await page.goto(url(theme));
    await page.getByRole("button", { name: /Set up with AI/ }).click();

    const model = page.getByTestId("composer-model");
    await expect(model).toContainText("OpenCode free");
    await model.click();
    const menu = page.getByTestId("model-picker-popover");
    await expect(menu).toBeVisible();
    await expect(menu.getByTestId("composer-model-search")).toBeFocused();
    await expect(menu.getByRole("group", { name: "OpenCode Free" })).toContainText("Free");
    await expect(menu.getByRole("group", { name: "DeepSeek" })).toContainText("Connected");
    await expect(menu.getByTestId("composer-manage-models")).toContainText("Manage models");
    await expect(menu.getByTestId("composer-connect-provider")).toContainText(
      "Connect another provider…",
    );
    await expectNoWcagViolations(page);

    // Search narrows the list, an empty search says so, and axe stays clean on both.
    const modelSearch = menu.getByTestId("composer-model-search");
    await modelSearch.fill("deepseek");
    await expect(menu.getByRole("option")).toHaveCount(2);
    await expectNoWcagViolations(page);
    await modelSearch.fill("zzz-no-such-model");
    await expect(menu).toContainText("No models match “zzz-no-such-model”.");
    await expectNoWcagViolations(page);

    // Switch to the connected provider's model with the keyboard.
    await modelSearch.fill("deepseek flash");
    await menu.getByRole("option", { name: /^DeepSeek V4\.1 Flash(?! Free)/ }).click();
    await expect(model).toContainText("BYOK · DeepSeek V4.1 Flash");
    await expect(menu).toHaveCount(0);

    // Manage models opens Engine & Models in place and closes the picker.
    await model.click();
    await page.getByTestId("composer-manage-models").click();
    await expect(page.getByTestId("model-picker-popover")).toHaveCount(0);
    await expect(page.getByRole("dialog", { name: "Engine & Models" })).toBeVisible();
    await page.getByRole("dialog", { name: "Engine & Models" }).getByRole("button", { name: "Close dialog" }).click();
    await model.click();
    await menu.getByTestId("composer-connect-provider").click();
    const dialog = page.getByRole("dialog", { name: "Engine & Models" });
    await expect(dialog).toBeVisible();
    const card = dialog.getByTestId("engine-models-settings");
    await expect(card.getByTestId("provider-opencode")).toContainText(
      "Free models included",
    );
    await expectNoWcagViolations(page);

    const search = card.getByRole("searchbox", { name: "Search providers" });
    await search.fill("anthropic");
    const row = card.getByTestId("provider-anthropic");
    await expect(row).toContainText("Anthropic");
    await expect(card.getByTestId("provider-openai")).toHaveCount(0);
    await row.getByRole("button", { name: "Connect Anthropic" }).click();

    const key = row.getByLabel("API key for Anthropic");
    await key.fill("not-a-real-provider-key");
    await row.getByRole("button", { name: "Save key" }).click();
    await expect(card.getByRole("status")).toContainText("Anthropic connected");
    await expect(card.getByTestId("provider-anthropic")).toContainText("Connected");
    await expect(key).toHaveCount(0);
    await expectNoWcagViolations(page);

    // The composer picker shows the newly unlocked model without re-opening the chat.
    await dialog.getByRole("button", { name: "Close dialog" }).click();
    await model.click();
    await expect(
      page.getByTestId("model-picker-popover").getByRole("group", { name: "Anthropic" }),
    ).toContainText("Claude Sonnet 4");
    await expectNoWcagViolations(page);
    await page.keyboard.press("Escape");
    await model.click();
    await page.getByTestId("composer-connect-provider").click();

    // Removing the key refreshes the live list through the mock engine's restart path.
    await card.getByRole("button", { name: "Remove Anthropic key" }).click();
    const confirm = page.getByRole("dialog", { name: "Remove provider key?" });
    await confirm.getByRole("button", { name: "Remove key" }).click();
    await expect(card.getByRole("status")).toContainText("Anthropic key removed");
    await expect(card.getByTestId("provider-anthropic")).toContainText("Available");
    await expectNoWcagViolations(page);

    // Closing the panel returns to the picker, which dropped the removed key's model.
    await dialog.getByRole("button", { name: "Close dialog" }).click();
    await model.click();
    await expect(page.getByTestId("model-picker-popover")).not.toContainText(
      "Claude Sonnet 4",
    );
  });
}

for (const theme of THEMES) {
  test(`Settings shows the Engine & Models panel (${theme})`, async ({ page }) => {
    await page.goto(`/?theme=${theme}`);
    await expect(page.getByTestId("next-shell")).toBeVisible();
    await page.getByTestId("settings-trigger").click();
    await page.getByRole("menuitem", { name: "All settings" }).click();
    await expect(page.getByRole("heading", { name: "Settings", level: 1 })).toBeVisible();

    const card = page.getByTestId("engine-models-settings");
    await expect(card.getByTestId("provider-opencode")).toContainText("Free models included");
    // A provider the engine reports as connected outside the keychain is grouped as such.
    await expect(card.getByTestId("provider-google")).toContainText("Connected outside the keychain");
    await expectNoWcagViolations(page);
  });
}
