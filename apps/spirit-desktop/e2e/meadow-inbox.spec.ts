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

async function openInbox(page: Page, scenario: string, theme: "light" | "dark") {
  await page.goto(`/?scenario=${scenario}&theme=${theme}`);
  await page.getByTestId("sidebar-connect").click();
  await page.getByTestId("health-card").click();
  await page.getByTestId("server-inbox-link").click();
  await expect(page.getByTestId("inbox-view")).toBeVisible();
}

for (const theme of ["light", "dark"] as const) {
  test(`a Notice offers Dismiss and Answer with orchestrator, never a Send box (${theme})`, async ({
    page,
  }) => {
    await page.goto(`/?scenario=inbox-notice&theme=${theme}`);
    await page.getByTestId("sidebar-connect").click();
    const sidebar = page.getByRole("navigation", { name: "Sessions" });
    await sidebar.getByRole("button", { name: /^vowp/ }).click();
    await sidebar.getByRole("button", { name: /^Workers/ }).click();
    await page.getByTestId("project-inbox-tab").click();
    const card = page
      .getByTestId("project-inbox")
      .getByTestId("inbox-item")
      .filter({ hasText: "Merge finished" });
    await expect(card.getByRole("button", { name: "Dismiss" })).toBeVisible();
    await expect(card.getByRole("button", { name: "Answer with orchestrator" })).toBeVisible();
    await expect(card.getByRole("button", { name: "Send" })).toHaveCount(0);
    await expect(card.getByRole("textbox")).toHaveCount(0);
    await expectNoWcagViolations(page);
    await card.getByRole("button", { name: "Dismiss" }).click();
    await expect(page.getByTestId("project-inbox").getByText("Merge finished")).toHaveCount(0);
  });

  test(`Answer with orchestrator opens the matching chat with the composer prefilled (${theme})`, async ({
    page,
  }) => {
    await openInbox(page, "busy", theme);
    const global = page
      .getByTestId("inbox-item")
      .filter({ hasText: "Raise the worker target to 6?" });
    await global.getByRole("button", { name: "Answer with orchestrator" }).click();
    await expect(page.getByRole("textbox", { name: "Message" })).toHaveValue(
      /Regarding inbox 01MOCKINBOX000000000000000002 \(Raise the worker target to 6\?\)/,
    );

    await page.getByTestId("health-card").click();
    await page.getByTestId("server-inbox-link").click();
    const project = page
      .getByTestId("inbox-item")
      .filter({ hasText: "Choose a merge queue policy" });
    await project.getByRole("button", { name: "Answer with orchestrator" }).click();
    await expect(page.getByRole("textbox", { name: "Message" })).toHaveValue(
      /Regarding inbox 01MOCKINBOX000000000000000001 \(Choose a merge queue policy\)/,
    );
  });

  test(`narrow pane with the details panel open clips no header button and overlaps no job text (${theme})`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 900, height: 700 });
    await page.goto(`/?scenario=busy&theme=${theme}`);
    await page.getByTestId("sidebar-connect").click();
    const sidebar = page.getByRole("navigation", { name: "Sessions" });
    await sidebar.getByRole("button", { name: /^vowp/ }).click();
    await sidebar.getByRole("button", { name: /^Workers/ }).click();
    await page.getByTestId("panel-toggle").click();

    const main = page.getByRole("region", { name: "vowp workers" });
    const head = main.locator("header.nx-chat-head");
    const headBox = (await head.boundingBox())!;
    for (const name of ["Map", "Board", "Active", "History", "Inbox", "Open orchestrator"]) {
      const box = await head.getByRole("button", { name: new RegExp(`^${name}`) }).boundingBox();
      expect(box, name).not.toBeNull();
      expect(box!.y, `${name} top`).toBeGreaterThanOrEqual(0);
      expect(box!.x, `${name} left`).toBeGreaterThanOrEqual(headBox.x - 1);
      expect(box!.x + box!.width, `${name} right`).toBeLessThanOrEqual(
        headBox.x + headBox.width + 1,
      );
      expect(box!.y + box!.height, `${name} bottom`).toBeLessThanOrEqual(
        headBox.y + headBox.height + 1,
      );
    }

    const rows = main.locator("button.nx-row");
    const count = await rows.count();
    for (let index = 0; index < count; index += 1) {
      const title = await rows.nth(index).locator(".nx-row__title").boundingBox();
      const id = rows.nth(index).locator(".nx-row__id");
      if (title && (await id.count()) > 0) {
        const idBox = (await id.boundingBox())!;
        expect(title.x + title.width, `row ${index} title/id`).toBeLessThanOrEqual(idBox.x + 1);
      }
    }
    await expectNoWcagViolations(page);
  });
}
