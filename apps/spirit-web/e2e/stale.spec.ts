import { expect, test } from "@playwright/test";

test("the stale preview shows the no-current-data banner and the retry countdown", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByTestId("stale-banner")).toBeVisible();
  await expect(page.getByTestId("stale-banner")).toContainText("no data from the daemon");
  await expect(page.getByText("no data from the daemon").first()).toBeVisible();
  await expect(page.getByTestId("web-status-strip")).toContainText("Connected · no current data");
});
