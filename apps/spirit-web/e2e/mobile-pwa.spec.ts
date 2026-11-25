import { expect, test, type Page } from "@playwright/test";

test.use({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });

async function noHorizontalOverflow(page: Page) {
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(overflow).toBeLessThanOrEqual(0);
}

test("the production build ships a valid manifest, icons and iOS meta tags", async ({
  page,
  request,
}) => {
  await page.goto("/");
  const href = await page.locator('link[rel="manifest"]').getAttribute("href");
  expect(href).toBe("/manifest.webmanifest");
  const response = await request.get(href ?? "");
  expect(response.ok()).toBe(true);
  const manifest = await response.json();
  expect(manifest.name).toBe("StackSpirit");
  expect(manifest.short_name).toBe("StackSpirit");
  expect(manifest.display).toBe("standalone");
  expect(manifest.theme_color).toMatch(/^#[0-9a-f]{6}$/i);
  const sizes = manifest.icons.map((icon: { sizes: string }) => icon.sizes);
  expect(sizes).toEqual(expect.arrayContaining(["192x192", "512x512"]));
  for (const icon of manifest.icons as Array<{ src: string }>) {
    expect((await request.get(icon.src)).ok()).toBe(true);
  }
  await expect(page.locator('meta[name="apple-mobile-web-app-capable"]')).toHaveAttribute(
    "content",
    "yes",
  );
  await expect(page.locator('meta[name="viewport"]')).toHaveAttribute(
    "content",
    /viewport-fit=cover/,
  );
  await expect(page.locator('link[rel="apple-touch-icon"]')).toHaveCount(1);
  const sw = await request.get("/sw.js");
  expect(sw.ok()).toBe(true);
  expect(await sw.text()).toContain('"/api/"');
});

test("390x844 has no horizontal overflow and navigates Fleet / Workers / Inbox / Capacity", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByTestId("web-lanes")).toBeVisible();
  await noHorizontalOverflow(page);

  const tabs = page.getByRole("navigation", { name: "Mobile tabs" });
  await expect(tabs).toBeVisible();
  for (const id of ["tab-fleet", "tab-workers", "tab-inbox", "tab-capacity"]) {
    const box = await tabs.getByTestId(id).boundingBox();
    expect(box?.height ?? 0).toBeGreaterThanOrEqual(44);
    expect(box?.width ?? 0).toBeGreaterThanOrEqual(44);
  }

  await tabs.getByTestId("tab-workers").click();
  await expect(page.getByTestId("running-workers")).toBeInViewport();

  await tabs.getByTestId("tab-inbox").click();
  const inbox = page.getByTestId("web-inbox-drawer");
  await expect(inbox.getByText("Choose a merge queue policy")).toBeVisible();
  await noHorizontalOverflow(page);
  await page.keyboard.press("Escape");
  await expect(inbox).toHaveCount(0);

  await tabs.getByTestId("tab-capacity").click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await noHorizontalOverflow(page);
  await page.keyboard.press("Escape");

  await tabs.getByTestId("tab-fleet").click();
  await expect(page.getByRole("heading", { name: "Fleet", level: 1 })).toBeVisible();
});

test("desktop keeps the side rail and hides the mobile tab bar", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto("/");
  await expect(page.getByTestId("web-tabbar")).toBeHidden();
  await expect(page.getByRole("navigation", { name: "Primary views" })).toBeVisible();
});
