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

async function openFiles(page: Page, theme: "light" | "dark") {
  await page.goto(`/?scenario=busy&theme=${theme}`);
  await page.getByTestId("sidebar-connect").click();
  await page.getByTestId("health-card").click();
  await expect(page.getByRole("region", { name: "Server" })).toBeVisible();
  await page.getByTestId("server-browse-files").click();
  const files = page.getByRole("region", { name: "Files" });
  await expect(files.getByRole("table", { name: /\/home\/owner/ })).toBeVisible();
  return files;
}

for (const theme of ["light", "dark"] as const) {
  test(`browses Home, VOWP Home and Projects, and previews a file (${theme})`, async ({ page }) => {
    const files = await openFiles(page, theme);
    const nav = files.getByRole("navigation", { name: "Locations" });
    await expect(nav).toContainText("live database");

    await nav.getByRole("button", { name: /VOWP Home/ }).click();
    await expect(files.getByRole("table", { name: /\.vowp/ })).toBeVisible();
    await expect(files.getByTestId("file-row-vowp.db")).toContainText("Live VOWP path");
    await expect(files.getByTestId("file-row-logs")).toContainText("supervisor logs");

    await files.getByRole("button", { name: "Home" }).first().click();
    await files.getByRole("button", { name: "notes.md", exact: true }).click();
    await expect(files.getByTestId("file-preview")).toContainText("Server notes");

    await nav.getByRole("button", { name: "Root" }).click();
    await expect(files.getByTestId("file-row-home")).toBeVisible();
    await expectNoWcagViolations(page);
  });

  test(`masks .env by default and reveals it only on request (${theme})`, async ({ page }) => {
    const files = await openFiles(page, theme);
    await files.getByRole("button", { name: "stackspirit", exact: true }).click();
    await files.getByRole("button", { name: ".env", exact: true }).click();
    await expect(files.getByTestId("masked-preview")).toHaveText("Secret file hidden by default — never sent to AI");
    await expect(files.getByText(/EXAMPLE_VALUE/)).toHaveCount(0);
    await expectNoWcagViolations(page);
    await files.getByRole("button", { name: "Reveal" }).click();
    await expect(files.getByText(/EXAMPLE_VALUE/)).toBeVisible();
  });

  test(`creates, renames and moves a folder (${theme})`, async ({ page }) => {
    const files = await openFiles(page, theme);
    await files.getByRole("button", { name: "New folder" }).click();
    await page.getByLabel("Folder name").fill("scratch");
    await expectNoWcagViolations(page);
    await page.getByRole("button", { name: "Create" }).click();
    await expect(files.getByTestId("file-row-scratch")).toBeVisible();

    await files.getByRole("button", { name: "Actions for scratch" }).click();
    await page.getByRole("menuitem", { name: "Rename" }).click();
    await page.getByLabel("New name").fill("archive");
    await page.getByRole("button", { name: "Rename" }).click();
    await expect(files.getByTestId("file-row-archive")).toBeVisible();

    await files.getByRole("button", { name: "Actions for archive" }).click();
    await page.getByRole("menuitem", { name: "Move…" }).click();
    await page.getByLabel("Destination folder").fill("~/vowp");
    await page.getByRole("button", { name: "Move", exact: true }).click();
    await expect(files.getByTestId("file-row-archive")).toHaveCount(0);

    await files.getByTestId("file-row-vowp").getByRole("button", { name: "vowp", exact: true }).click();
    await expect(files.getByTestId("file-row-archive")).toBeVisible();
  });

  test(`drags a file onto a folder (${theme})`, async ({ page }) => {
    const files = await openFiles(page, theme);
    await files.getByTestId("file-row-notes.md").dragTo(files.getByTestId("file-row-vowp"));
    await expect(files.getByTestId("file-row-notes.md")).toHaveCount(0);
  });

  test(`warns before deleting the live vowp.db (${theme})`, async ({ page }) => {
    const files = await openFiles(page, theme);
    await files.getByRole("navigation", { name: "Locations" }).getByRole("button", { name: /VOWP Home/ }).click();
    await files.getByRole("button", { name: "Actions for vowp.db" }).click();
    await page.getByRole("menuitem", { name: "Delete" }).click();
    await page.getByRole("dialog").getByRole("button", { name: "Delete" }).click();
    const seatbelt = page.getByRole("dialog", { name: "Live VOWP path" });
    await expect(seatbelt.getByTestId("seatbelt-text")).toHaveText(
      "This is a live VOWP system path — moving or deleting it may interrupt running work. Continue anyway?",
    );
    await expectNoWcagViolations(page);
    await seatbelt.getByRole("button", { name: "Cancel" }).click();
    await expect(files.getByTestId("file-row-vowp.db")).toBeVisible();
  });
}
