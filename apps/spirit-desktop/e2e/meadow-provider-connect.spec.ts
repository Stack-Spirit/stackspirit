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

async function openSettings(page: Page, theme: "light" | "dark") {
  await page.goto(`/?theme=${theme}`);
  await expect(page.getByTestId("next-shell")).toBeVisible();
  await page.getByTestId("settings-trigger").click();
  await page.getByRole("menuitem", { name: "All settings" }).click();
  await expect(page.getByRole("heading", { name: "Settings", level: 1 })).toBeVisible();
}

for (const theme of THEMES) {
  test(`OpenCode-first provider connect and grouped model picker (${theme})`, async ({
    page,
  }) => {
    await openSettings(page, theme);
    const card = page.getByTestId("engine-models-settings");
    await expect(card.getByRole("heading", { name: "Engine & Models" })).toBeVisible();
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

    // Return to the setup composer: the mock bridge refreshed its model list on
    // connect, so the newly unlocked provider appears without an app restart.
    await page.getByRole("button", { name: /Setup assistant/ }).click();
    const model = page.getByTestId("composer-model");
    await expect(model).toContainText("OpenCode free");
    await model.click();
    const menu = page.getByRole("menu", { name: "Choose a model" });
    await expect(menu.getByRole("group", { name: "OpenCode Free" })).toContainText(
      "OpenCode free",
    );
    await expect(menu.getByRole("group", { name: "Connected providers" })).toContainText(
      "BYOK · Claude Sonnet 4.5",
    );
    await expect(menu.getByTestId("composer-connect-provider")).toContainText(
      "Connect another provider…",
    );
    await expectNoWcagViolations(page);

    // The footer link goes straight back to Settings → Engine & Models.
    await menu.getByTestId("composer-connect-provider").click();
    await expect(
      page.getByRole("heading", { name: "Engine & Models" }),
    ).toBeVisible();
    await expectNoWcagViolations(page);
  });
}
