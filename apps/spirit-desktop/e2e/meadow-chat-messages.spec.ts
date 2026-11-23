import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

import { E2E_LOCK, E2E_TIMEOUT } from "./support";

test.describe.configure({ lock: E2E_LOCK, timeout: E2E_TIMEOUT });
test.use({ contextOptions: { reducedMotion: "reduce", permissions: ["clipboard-read", "clipboard-write"] } });

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

async function startTurn(page: Page, scenario: string, theme: "light" | "dark") {
  await page.goto(`/?scenario=${scenario}&theme=${theme}`);
  await page.getByTestId("sidebar-connect").click();
  await expect(
    page.getByRole("heading", { name: "Global orchestrator", level: 1 }),
  ).toBeVisible();
  await page.getByRole("textbox", { name: "Message" }).fill("tidy the shared folder");
  await page.keyboard.press("Enter");
}

for (const theme of ["light", "dark"] as const) {
  test(`user meta, assistant footer and telemetry strip pass WCAG 2.2 AA (${theme})`, async ({
    page,
  }) => {
    await startTurn(page, "chat-tools", theme);
    await expect(page.getByTestId("session-state")).toContainText("Ready");

    const meta = page.getByTestId("user-meta").last();
    await expect(meta.getByTestId("user-time")).toHaveText(/^\d{2}\/\d{2} \d{2}:\d{2}$/);
    await meta.getByRole("button", { name: "Copy message" }).click();
    await expect(meta.getByRole("button", { name: "Copied" })).toBeVisible();
    await expect
      .poll(() => page.evaluate(() => navigator.clipboard.readText()))
      .toContain("tidy the shared folder");

    const footer = page.getByTestId("assistant-message").last().getByTestId("answer-footer");
    await expect(footer).toBeVisible();
    await expect(footer.getByTestId("answer-meta")).toContainText(/\d{2}\/\d{2} \d{2}:\d{2}/);
    await expect(page.getByTestId("completion-header").last()).toHaveText(
      /^Completed in \d+s · 3 steps$/,
    );
    await expectNoWcagViolations(page);
  });

  test(`telemetry strip under the composer passes WCAG 2.2 AA (${theme})`, async ({ page }) => {
    await page.goto(`/?scenario=empty&theme=${theme}`);
    await page.getByRole("button", { name: /Set up with AI/ }).click();
    await expect(page.getByTestId("usage-pill-turns")).toContainText("3 turns");
    await expect(page.getByTestId("usage-pill-tokens")).toContainText(/tok · Cache hit \d+%/);
    await expect(page.getByTestId("context-meter")).toContainText("18%");
    await expectNoWcagViolations(page);
  });
}
