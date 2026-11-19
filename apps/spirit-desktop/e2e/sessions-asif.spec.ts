import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

import {
  E2E_LOCK,
  E2E_TIMEOUT,
  startViteServer,
  stopViteServer,
  type ViteServer,
} from "./support";

test.describe.configure({ lock: E2E_LOCK, timeout: E2E_TIMEOUT });

const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"];

// The D5 preview fixture currently uses 27-character pseudo-ULIDs in
// `MockBridge.makeSessions`. The bridge correctly validates the 26-character VOWP ULID
// on export, so this browser suite rewrites the two quoted preview literals below to
// valid ULIDs. This keeps the UI flow under test without changing the shipped fixture.
const PREVIEW_SESSION_ID = "01JCF5WQ8ZK3M2V7N4P6R8T0AB";
const PREVIEW_IDLE_SESSION_ID = "01JCF5WQ8ZK3M2V7N4P6R8T0AC";
const IMPORTED_SESSION_ID = "01JCF5WQ8ZK3M2V7N4P6R8T0AF";
const PORT = 1425;
const BASE_URL = `http://127.0.0.1:${PORT}`;

let server: ViteServer | null = null;

async function expectNoWcagViolations(page: Page) {
  const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
  expect(results.violations).toEqual([]);
}

test.beforeAll(async () => {
  server = await startViteServer("asif-import-warnings", PORT);
});

test.afterAll(async () => {
  await stopViteServer(server?.process ?? null);
});

test("MockBridge export and import flows stay honest and idle", async ({ page }) => {
  await page.route(/\/MockBridge\.ts(?:\?.*)?$/, async (route) => {
    const response = await route.fetch();
    const body = (await response.text())
      .replaceAll(`"01MOCKSESSION00000000000000"`, `"${PREVIEW_SESSION_ID}"`)
      .replaceAll(`"01MOCKSESSION00000000000001"`, `"${PREVIEW_IDLE_SESSION_ID}"`);
    await route.fulfill({ response, body });
  });

  await page.goto(BASE_URL);
  await expect(page.getByTestId("next-shell")).toBeVisible();
  await page.getByTestId("sidebar-connect").click();
  await expect(page.getByTestId("health-card")).toBeVisible();

  // The server card's ⋯ menu is the Meadow path into the sessions drawer.
  await page.getByTestId("server-card").getByRole("button", { name: "Server options" }).click();
  await page.getByRole("menuitem", { name: "Sessions and .asif export" }).click();
  const drawer = page.getByRole("dialog", { name: "Sessions" });
  await expect(drawer).toBeVisible();
  await expect(drawer.getByTestId(`session-row-${PREVIEW_SESSION_ID}`)).toBeVisible();
  await expect(drawer.getByTestId(`session-row-${PREVIEW_IDLE_SESSION_ID}`)).toBeVisible();
  await expectNoWcagViolations(page);

  // Export defaults to allow-loss, shows the full declared-loss list, and only names
  // the local file.
  await drawer.getByTestId(`session-export-${PREVIEW_SESSION_ID}`).click();
  await expect(page.getByTestId("export-panel")).toBeVisible();
  await expect(page.getByTestId("export-policy-allow-loss")).toBeChecked();
  await expect(page.getByTestId("export-policy-warning")).toContainText(
    "Allow-loss is the default",
  );
  await page.getByRole("button", { name: "Export .asif.zip" }).click();
  await expect(page.getByTestId("export-result")).toBeVisible();
  await expect(page.getByTestId("export-file-name")).toHaveText(
    `${PREVIEW_SESSION_ID}.asif.zip`,
  );
  await expect(page.getByTestId("export-losses")).toContainText("cwd-omitted");
  await expect(page.getByTestId("export-losses")).toContainText("contexts-reconstructed");
  await expect(page.getByTestId("export-unsupported")).toContainText("vowp.jobs");
  await expect(page.getByText(/VOWP declared the losses above/)).toBeVisible();
  await expect(page.getByText(/lossless/i)).toHaveCount(0);
  await expectNoWcagViolations(page);

  await page.getByRole("button", { name: "← Back to sessions" }).click();
  await page.getByTestId("sessions-import").click();
  await expect(page.getByTestId("import-panel")).toBeVisible();
  await expect(page.getByTestId("import-size-limit")).toContainText("272 MiB");
  await page.getByRole("button", { name: "Import .asif.zip" }).click();
  await expect(page.getByTestId("import-result")).toBeVisible();
  await expect(page.getByTestId("import-session-id")).toHaveText(IMPORTED_SESSION_ID);
  await expect(page.getByTestId("import-state")).toContainText("Idle");
  await expect(page.getByTestId("import-losses")).toContainText("session-id-changed");
  await expect(page.getByTestId("import-warnings")).toContainText(
    "project example-source left null",
  );
  await expect(page.getByTestId("import-idle-note")).toContainText("did not run a turn");
  // A connected Meadow page has its own orchestrator Start control behind the
  // drawer; the imported session itself must still offer no run action.
  await expect(
    page
      .getByTestId("import-result")
      .getByRole("button", { name: /^(Run|Start|Continue|Resume)\b/i }),
  ).toHaveCount(0);
  await expectNoWcagViolations(page);

  await page.keyboard.press("Escape");
  await expect(drawer).toHaveCount(0);
});
