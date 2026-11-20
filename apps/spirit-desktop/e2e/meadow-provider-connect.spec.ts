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
    const menu = page.getByRole("menu", { name: "Choose a model" });
    await expect(menu.getByRole("group", { name: "OpenCode Free" })).toContainText(
      "OpenCode free",
    );
    await expect(menu.getByRole("group", { name: "Connected providers" })).toContainText(
      "BYOK · DeepSeek V4.1 Flash",
    );
    await expect(menu.getByTestId("composer-connect-provider")).toContainText(
      "Connect another provider…",
    );
    await expectNoWcagViolations(page);

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

    // Removing the key refreshes the live list through the mock engine's restart path.
    await card.getByRole("button", { name: "Remove Anthropic key" }).click();
    const confirm = page.getByRole("dialog", { name: "Remove provider key?" });
    await confirm.getByRole("button", { name: "Remove key" }).click();
    await expect(card.getByRole("status")).toContainText("Anthropic key removed");
    await expect(card.getByTestId("provider-anthropic")).toContainText("Available");
    await expectNoWcagViolations(page);

    // Closing the panel returns to the grouped composer picker.
    await dialog.getByRole("button", { name: "Close dialog" }).click();
    await expect(model).toBeVisible();
  });
}
