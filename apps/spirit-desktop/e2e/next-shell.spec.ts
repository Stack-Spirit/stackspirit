import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

import { E2E_LOCK, E2E_TIMEOUT } from "./support";

test.describe.configure({ lock: E2E_LOCK, timeout: E2E_TIMEOUT });

// The details panel slides and fades in; with reduced motion it appears at once, so
// axe measures the finished colours instead of a half-transparent animation frame.
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

// The Meadow shell is the app default; the web server for this suite builds the
// classic shell for the older specs, so these pages opt back in with ?ui=next.
const url = (scenario: string, theme: "light" | "dark" = "light") =>
  `/?ui=next&scenario=${scenario}&theme=${theme}`;

async function connect(page: Page) {
  await page.getByTestId("sidebar-connect").click();
  await expect(page.getByTestId("health-card")).toBeVisible();
}

test("welcome with no saved server meets WCAG 2.2 AA in light and dark", async ({ page }) => {
  for (const theme of ["light", "dark"] as const) {
    await page.goto(url("empty", theme));
    await expect(page.getByRole("heading", { name: "No always-on server connected" })).toBeVisible();
    await expect(page.getByRole("button", { name: /Add server/ })).toBeVisible();
    await expectNoWcagViolations(page);
  }
});

test("setup assistant keeps the password out of the chat", async ({ page }) => {
  await page.goto(url("empty"));
  await page.getByRole("button", { name: /Set up with AI/ }).click();
  await page.getByRole("button", { name: "I have a new VPS" }).click();
  const form = page.getByRole("form", { name: "Secure server details" });
  await form.getByLabel("Server address").fill("203.0.113.50");
  await form.getByLabel("One-time password from your provider").fill("not-a-real-secret");
  await expectNoWcagViolations(page);
  await form.getByRole("button", { name: "Connect securely" }).click();
  await expect(page.getByText(/Server details received for 203.0.113.50/)).toBeVisible();
  await expect(page.getByText("not-a-real-secret")).toHaveCount(0);
});

test("connected shell, details panel and settings menu meet WCAG 2.2 AA", async ({ page }) => {
  for (const theme of ["light", "dark"] as const) {
    await page.goto(url("busy", theme));
    await connect(page);
    await expect(page.getByRole("heading", { name: "Global orchestrator", level: 1 })).toBeVisible();
    await expect(page.getByTestId("details-panel")).toHaveAttribute("aria-hidden", "true");
    await expectNoWcagViolations(page);

    await page.getByTestId("panel-toggle").click();
    await expect(page.getByTestId("details-panel")).toHaveAttribute("aria-hidden", "false");
    await expectNoWcagViolations(page);

    await page.getByTestId("settings-trigger").click();
    await expect(page.getByRole("menu", { name: "Settings" })).toBeVisible();
    await expectNoWcagViolations(page);
    await page.keyboard.press("Escape");
  }
});

test("server and workers pages meet WCAG 2.2 AA and open job details", async ({ page }) => {
  await page.goto(url("busy"));
  await connect(page);

  await page.getByTestId("health-card").click();
  await expect(page.getByRole("group", { name: "Server structure" })).toBeVisible();
  await expectNoWcagViolations(page);

  const sidebar = page.getByRole("navigation", { name: "Sessions" });
  await sidebar.getByRole("button", { name: /^vowp/ }).click();
  await sidebar.getByRole("button", { name: /^Workers/ }).click();
  await expect(page.getByRole("heading", { name: "vowp · Workers", level: 1 })).toBeVisible();
  await page.getByRole("region", { name: "Jobs" }).getByRole("button", { name: /^ss-2-desktop-shell/ }).click();
  await expect(page.getByTestId("details-panel").getByRole("heading", { name: "ss-2-desktop-shell" })).toBeVisible();
  await expectNoWcagViolations(page);
});

test("add project dialog meets WCAG 2.2 AA", async ({ page }) => {
  await page.goto(url("busy"));
  await connect(page);
  await page.getByRole("button", { name: "Add project" }).click();
  const dialog = page.getByRole("dialog", { name: "Add a project" });
  await dialog.getByLabel("Repository URL").fill("https://github.com/acme/billing-api.git");
  await expect(dialog.getByLabel("Project name")).toHaveValue("billing-api");
  await expectNoWcagViolations(page);
});
