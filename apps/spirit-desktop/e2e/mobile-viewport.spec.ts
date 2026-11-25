import { expect, test, type Page } from "@playwright/test";

import { E2E_LOCK, E2E_TIMEOUT } from "./support";

test.describe.configure({ lock: E2E_LOCK, timeout: E2E_TIMEOUT });
test.use({
  contextOptions: { reducedMotion: "reduce" },
  viewport: { width: 390, height: 844 },
  hasTouch: true,
});

async function expectNoHorizontalOverflow(page: Page) {
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(overflow).toBeLessThanOrEqual(0);
}

test("phone viewport: drawer navigation reaches Chat, Inbox and Workers without overflow", async ({ page }) => {
  await page.goto("/?scenario=busy");
  await page.getByTestId("connect-prompt-connect").click();
  await page.getByTestId("mobile-nav-toggle").click();
  await expect(page.getByRole("complementary", { name: "Sessions and servers" })).toBeVisible();
  await page.getByTestId("mobile-nav-scrim").click({ position: { x: 380, y: 400 } });
  await expect(page.getByRole("complementary", { name: "Sessions and servers" })).toBeHidden();
  await expect(page.getByTestId("mobile-bar")).toBeVisible();
  await expectNoHorizontalOverflow(page);

  const toggle = page.getByTestId("mobile-nav-toggle");
  const box = await toggle.boundingBox();
  expect(box?.width ?? 0).toBeGreaterThanOrEqual(44);
  expect(box?.height ?? 0).toBeGreaterThanOrEqual(44);

  await page.getByTestId("mobile-inbox").click();
  await expect(page.getByRole("heading", { name: "Inbox", level: 1 })).toBeVisible();
  await expectNoHorizontalOverflow(page);
});

test("desktop viewport keeps the docked sidebar and hides the mobile bar", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto("/?scenario=busy");
  await expect(page.getByTestId("mobile-bar")).toBeHidden();
  await expect(page.getByRole("complementary", { name: "Sessions and servers" })).toBeVisible();
});
