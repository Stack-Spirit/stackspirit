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

async function openJobsPage(page: Page, theme: "light" | "dark") {
  await page.goto(`/?scenario=busy&theme=${theme}`);
  await page.getByTestId("sidebar-connect").click();
  const sidebar = page.getByRole("navigation", { name: "Sessions" });
  await sidebar.getByRole("button", { name: /^vowp/ }).click();
  await sidebar.getByRole("button", { name: /^Workers/ }).click();
}

for (const theme of ["light", "dark"] as const) {
  test(`queued job shows its planned executor and changes model immediately (${theme})`, async ({
    page,
  }) => {
    await openJobsPage(page, theme);
    const jobs = page.getByRole("region", { name: "Jobs" });
    await expect(jobs.getByTestId("job-planned").first()).toContainText("Planned:");
    await jobs.getByRole("button", { name: /Build the visual fleet canvas/ }).click();

    const panel = page.getByTestId("details-panel");
    await expect(panel.getByText("Planned provider")).toBeVisible();
    await expect(panel.getByText("Planned harness")).toBeVisible();
    await expect(panel.getByText("Planned model")).toBeVisible();
    await expect(panel).toContainText("Claude Sonnet 5.5 · medium");

    await panel.getByRole("button", { name: "Change model" }).click();
    const form = panel.getByRole("form", { name: "Change model" });
    await form.getByLabel("Harness").selectOption("opencode");
    await form.getByLabel("Provider").selectOption("deepseek");
    await form.getByLabel("Model").selectOption("deepseek/deepseek-flash");
    await form.getByLabel("Reasoning").selectOption("max");
    await expectNoWcagViolations(page);
    await form.getByRole("button", { name: "Apply" }).click();

    await expect(panel.getByTestId("change-model-result")).toContainText("Planned model changed");
    await expect(panel).toContainText("DeepSeek V4.1 Flash · max");
    await expectNoWcagViolations(page);
  });

  test(`running job confirms the handoff and shows API cost (${theme})`, async ({ page }) => {
    await openJobsPage(page, theme);
    await page
      .getByRole("region", { name: "Jobs" })
      .getByRole("button", { name: /^ss-2-desktop-shell/ })
      .click();

    const panel = page.getByTestId("details-panel");
    await expect(panel.getByTestId("job-cost")).toHaveText("$0.14");
    await expect(panel.getByTestId("job-tokens")).toContainText("142K tok");
    await expect(panel.getByRole("img", { name: /Context window \d+% used/ })).toBeVisible();

    await panel.getByRole("button", { name: "Change model" }).click();
    const form = panel.getByRole("form", { name: "Change model" });
    await form.getByLabel("Harness").selectOption("claude-code");
    await form.getByLabel("Model").selectOption("claude-opus-5-5");
    await form.getByRole("button", { name: "Review switch" }).click();
    await expect(panel.getByRole("alert")).toContainText(
      "Switch this running worker via universal session handoff?",
    );
    await expectNoWcagViolations(page);
    await panel.getByRole("button", { name: "Switch worker" }).click();
    await expect(panel.getByTestId("change-model-result")).toContainText("Switching to");
    await expectNoWcagViolations(page);
  });

  test(`a subscription shows Subscription, never a dollar amount (${theme})`, async ({ page }) => {
    await openJobsPage(page, theme);
    const usage = page.getByRole("region", { name: "Jobs" }).getByTestId("job-usage");
    await expect(usage.filter({ hasText: "Subscription" }).first()).toBeVisible();
    await expect(usage.filter({ hasText: "Subscription" }).first()).not.toContainText("$");
    await expectNoWcagViolations(page);
  });
}
