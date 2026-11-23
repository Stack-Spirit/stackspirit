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

async function startTurn(page: Page, scenario: string, theme: "light" | "dark") {
  await page.goto(`/?scenario=${scenario}&theme=${theme}`);
  await page.getByTestId("sidebar-connect").click();
  await expect(
    page.getByRole("heading", { name: "Global orchestrator", level: 1 }),
  ).toBeVisible();
  const message = page.getByRole("textbox", { name: "Message" });
  await message.fill("tidy the shared folder");
  await page.keyboard.press("Enter");
}

for (const theme of ["light", "dark"] as const) {
  test(`tool activity rows are calm and pass WCAG 2.2 AA (${theme})`, async ({ page }) => {
    await startTurn(page, "chat-tools", theme);
    const rows = page.getByTestId("tool-row");
    await expect(rows.first()).toBeVisible();
    await expect(rows.first()).toHaveAttribute("data-status", "completed");
    await expectNoWcagViolations(page);
    await expect(page.getByTestId("session-state")).toContainText("Ready");
    await expect(rows).toHaveCount(3);
    await expect(rows.nth(2)).toContainText("failed");
    await expectNoWcagViolations(page);
  });

  test(`a pending permission card resolves the turn (${theme})`, async ({ page }) => {
    await startTurn(page, "chat-permission", theme);
    const card = page.getByTestId("permission-card");
    await expect(card).toBeVisible();
    await expect(card.getByRole("heading", { name: "Permission needed" })).toBeVisible();
    await expect(card).toContainText("/srv/shared/*");
    await expect(card).toContainText("Waiting for approval");
    // Calm neutral surface: no amber fill and no thick left stripe.
    const look = await card.evaluate((el) => {
      const style = getComputedStyle(el);
      return { bg: style.backgroundColor, left: style.borderLeftWidth };
    });
    expect(look.left).toBe("1px");
    const [r, g, b] = look.bg.match(/\d+/g)!.map(Number) as [number, number, number];
    expect(Math.abs(r - g)).toBeLessThan(40);
    expect(Math.abs(g - b)).toBeLessThan(40);
    await expect(page.getByTestId("session-state")).toContainText("Replying…");
    await expectNoWcagViolations(page);

    // Keyboard reachable, with a visible ring.
    const allow = card.getByRole("button", { name: "Allow once" });
    await allow.focus();
    await expect(allow).toBeFocused();

    await allow.click();
    await expect(card).toHaveCount(0);
    await expect(page.getByTestId("session-state")).toContainText("Ready");
    await expect(page.getByText(/You allowed the request/)).toBeVisible();
    await expectNoWcagViolations(page);
  });
}

test("Reject also resolves the turn", async ({ page }) => {
  await startTurn(page, "chat-permission", "light");
  await page.getByTestId("permission-card").getByRole("button", { name: "Reject" }).click();
  await expect(page.getByTestId("permission-card")).toHaveCount(0);
  await expect(page.getByText(/You rejected the request/)).toBeVisible();
  await expect(page.getByTestId("session-state")).toContainText("Ready");
});
