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
  test(`Global Inbox shows global and project items, answering resolves (${theme})`, async ({ page }) => {
    await page.goto(`/?scenario=busy&theme=${theme}`);
    await page.getByTestId("sidebar-connect").click();
    await expect(page.getByTestId("nav-inbox-count")).toHaveText("2");

    // The server page surfaces only the Global item, with a link to the full Inbox.
    await page.getByTestId("health-card").click();
    const serverInbox = page.getByTestId("server-inbox");
    await expect(serverInbox).toContainText("Raise the worker target to 6?");
    await expect(serverInbox).not.toContainText("Choose a merge queue policy");
    await page.getByTestId("server-inbox-link").click();

    const inbox = page.getByTestId("inbox-view");
    await expect(page.getByRole("heading", { name: "Inbox", level: 1 })).toBeVisible();
    await expect(inbox.getByTestId("inbox-item")).toHaveCount(2);
    await expect(inbox.getByTestId("inbox-project").filter({ hasText: "Global" })).toHaveCount(1);
    await expect(inbox.getByTestId("inbox-project").filter({ hasText: "vowp" })).toHaveCount(1);
    await expectNoWcagViolations(page);

    await inbox.getByRole("button", { name: /^Global/ }).click();
    await expect(inbox.getByTestId("inbox-item")).toHaveCount(1);

    // One click on an option resolves the item.
    await inbox.getByRole("button", { name: "Keep" }).click();
    await expect(inbox.getByTestId("inbox-confirmation")).toContainText("Keep");
    await expect(page.getByTestId("nav-inbox-count")).toHaveText("1");
    await expect(inbox.getByText("Raise the worker target to 6?", { exact: true })).toHaveCount(0);
    await expectNoWcagViolations(page);
  });

  test(`Project Inbox shows only that project's items (${theme})`, async ({ page }) => {
    await page.goto(`/?scenario=busy&theme=${theme}`);
    await page.getByTestId("sidebar-connect").click();
    const sidebar = page.getByRole("navigation", { name: "Sessions" });
    await sidebar.getByRole("button", { name: /^vowp/ }).click();
    await sidebar.getByRole("button", { name: /^Workers/ }).click();

    await expect(page.getByTestId("project-inbox-count")).toHaveText("1");
    await page.getByTestId("project-inbox-tab").click();
    const projectInbox = page.getByTestId("project-inbox");
    await expect(projectInbox.getByTestId("inbox-item")).toHaveCount(1);
    await expect(projectInbox).toContainText("Choose a merge queue policy");
    await expect(projectInbox).not.toContainText("Raise the worker target to 6?");
    await expectNoWcagViolations(page);

    await projectInbox.getByRole("button", { name: "One at a time" }).click();
    await expect(projectInbox.getByTestId("inbox-confirmation")).toContainText("One at a time");
    await expect(projectInbox.getByTestId("inbox-empty")).toHaveText("All clear — nothing waiting for you");
    await expect(page.getByTestId("project-inbox-count")).toHaveText("0");
    await expectNoWcagViolations(page);
  });
}
