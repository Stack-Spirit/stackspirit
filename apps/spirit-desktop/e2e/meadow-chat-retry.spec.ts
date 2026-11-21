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

async function openFreeLimit(page: Page, theme: "light" | "dark") {
  await page.goto(`/?scenario=chat-free-limit&theme=${theme}`);
  await page.getByRole("button", { name: /Setup assistant/ }).click();
  const message = page.getByRole("textbox", { name: "Message" });
  await expect(message).toBeEnabled();
  await message.fill("explain the server");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("retry-notice")).toBeVisible();
  await expect(page.getByTestId("session-state")).toContainText("Retrying…");
  return message;
}

for (const theme of ["light", "dark"] as const) {
  test(`the free-limit notice is calm and recoverable (${theme})`, async ({ page }) => {
    await openFreeLimit(page, theme);
    const notice = page.getByTestId("retry-notice");
    await expect(notice.getByRole("heading", { name: "Free limit reached" })).toBeVisible();
    await expect(notice).toContainText("usage limit");
    await expect(notice).toContainText(/Retrying in \d+s/);
    await expect(notice.getByRole("button", { name: "Stop retrying" })).toBeEnabled();
    await expectNoWcagViolations(page);

    // Switch model opens the searchable composer picker with the free list and the
    // connected BYOK provider, and puts focus in the search field.
    await notice.getByRole("button", { name: "Switch model" }).click();
    const menu = page.getByTestId("model-picker-popover");
    await expect(menu).toBeVisible();
    await expect(menu.getByTestId("composer-model-search")).toBeFocused();
    await expect(menu.getByRole("group", { name: "OpenCode Free" })).toContainText("Big Pickle");
    await expect(menu.getByRole("group", { name: "DeepSeek" })).toContainText(
      /DeepSeek V4\.1 Flash(?! Free)/,
    );
    await expect(menu.getByRole("option", { name: /Big Pickle/ })).toBeVisible();

    await menu.getByRole("option", { name: /Big Pickle/ }).click();
    await expect(page.getByTestId("composer-model")).toContainText("OpenCode free · Big Pickle");
    await expect(notice).toBeVisible();
    await expectNoWcagViolations(page);
  });

  test(`Stop retrying returns to Ready and the next turn works (${theme})`, async ({ page }) => {
    const message = await openFreeLimit(page, theme);
    await page.getByTestId("retry-notice").getByRole("button", { name: "Stop retrying" }).click();
    await expect(page.getByTestId("retry-notice")).toHaveCount(0);
    await expect(page.getByTestId("session-state")).toContainText("Ready");
    await expect(message).toBeEnabled();
    await expect(page.getByRole("button", { name: "Start" })).toHaveCount(0);
    await expectNoWcagViolations(page);

    await message.fill("continue with another model");
    await page.keyboard.press("Enter");
    await expect(page.getByText(/Preview reply\./)).toBeVisible();
    await expect(page.getByTestId("session-state")).toContainText("Ready");
    await expectNoWcagViolations(page);
  });
}

test("Connect a provider opens Settings with Engine advanced", async ({ page }) => {
  await openFreeLimit(page, "light");
  await page.getByTestId("retry-notice").getByRole("button", { name: "Connect a provider" }).click();
  await expect(page.getByRole("heading", { name: "Settings", level: 1 })).toBeVisible();
  await expect(page.getByRole("button", { name: "Hide advanced settings" })).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "StackSpirit Engine", exact: true }),
  ).toBeVisible();
  await expectNoWcagViolations(page);
});
