import { spawn, type ChildProcess } from "node:child_process";
import { dirname, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";

import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

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
const APP_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

let server: ChildProcess | null = null;
let serverLog = "";

async function expectNoWcagViolations(page: Page) {
  const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
  expect(results.violations).toEqual([]);
}

async function waitForServer(url: string): Promise<void> {
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    if (server && server.exitCode !== null) {
      throw new Error(`Vite exited before serving ${url}:\n${serverLog.slice(-4000)}`);
    }
    try {
      const response = await fetch(url);
      if (response.ok) return;
    } catch {
      // The dev server socket is not open yet.
    }
    await delay(200);
  }
  throw new Error(`Vite did not serve ${url}:\n${serverLog.slice(-4000)}`);
}

test.beforeAll(async () => {
  server = spawn(
    process.platform === "win32" ? "npm.cmd" : "npm",
    ["run", "dev", "--", "--port", String(PORT), "--strictPort"],
    {
      cwd: APP_ROOT,
      env: {
        ...process.env,
        VITE_SPIRIT_MOCK: "1",
        VITE_SPIRIT_MOCK_SCENARIO: "asif-import-warnings",
      },
      stdio: ["ignore", "pipe", "pipe"],
    },
  );
  server.stdout?.on("data", (chunk: Buffer) => {
    serverLog += chunk.toString();
  });
  server.stderr?.on("data", (chunk: Buffer) => {
    serverLog += chunk.toString();
  });
  await waitForServer(BASE_URL);
});

test.afterAll(async () => {
  if (!server || server.exitCode !== null) return;
  server.kill("SIGTERM");
  await new Promise<void>((resolveExit) => {
    server?.once("exit", () => resolveExit());
    setTimeout(resolveExit, 5_000);
  });
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
  await expect(page.getByText("Preview data").first()).toBeVisible();
  await page.getByTestId("topbar-connect").click();
  await expect(page.getByTestId("topbar-connect")).toHaveText("Disconnect");

  await page.getByTestId("open-sessions").click();
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
  await expect(
    page.getByRole("button", { name: /^(Run|Start|Continue|Resume)\b/i }),
  ).toHaveCount(0);
  await expectNoWcagViolations(page);

  await page.keyboard.press("Escape");
  await expect(drawer).toHaveCount(0);
  await expect(page.getByTestId("open-sessions")).toBeFocused();
});
