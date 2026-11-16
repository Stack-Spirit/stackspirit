import { spawn, type ChildProcess } from "node:child_process";
import { dirname, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";

import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

// Both gated-off cases share the two dev servers started in `beforeAll`; keep them in
// one worker so a parallel worker never races for the same ports.
test.describe.configure({ mode: "serial" });

const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"];
const WAITING_PORT = 1426;
const INCOMPATIBLE_PORT = 1427;
const WAITING_URL = `http://127.0.0.1:${WAITING_PORT}`;
const INCOMPATIBLE_URL = `http://127.0.0.1:${INCOMPATIBLE_PORT}`;
const APP_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const servers: ChildProcess[] = [];
let serverLog = "";

async function expectNoWcagViolations(page: Page) {
  const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
  expect(results.violations).toEqual([]);
}

async function startServer(scenario: string, port: number): Promise<void> {
  const server = spawn(
    process.platform === "win32" ? "npm.cmd" : "npm",
    ["run", "dev", "--", "--port", String(port), "--strictPort"],
    {
      cwd: APP_ROOT,
      env: {
        ...process.env,
        VITE_SPIRIT_MOCK: "1",
        VITE_SPIRIT_MOCK_SCENARIO: scenario,
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
  servers.push(server);

  const url = `http://127.0.0.1:${port}`;
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    if (server.exitCode !== null) {
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

async function openEngine(page: Page, url: string) {
  await page.goto(url);
  await expect(page.getByText("Preview data").first()).toBeVisible();
  await page.getByTestId("topbar-connect").click();
  await expect(page.getByTestId("topbar-connect")).toHaveText("Disconnect");
  await page
    .getByRole("navigation", { name: "Primary views" })
    .getByRole("button", { name: "Engine" })
    .click();
  await expect(page.getByRole("heading", { name: "Engine", level: 1 })).toBeVisible();
}

test.beforeAll(async () => {
  await Promise.all([
    startServer("gate-d6-waiting", WAITING_PORT),
    startServer("gate-d6-incompatible", INCOMPATIBLE_PORT),
  ]);
});

test.afterAll(async () => {
  await Promise.all(
    servers.map(async (server) => {
      if (server.exitCode !== null) return;
      server.kill("SIGTERM");
      await new Promise<void>((resolveExit) => {
        server.once("exit", () => resolveExit());
        setTimeout(resolveExit, 5_000);
      });
    }),
  );
});

test("a Waiting local-orchestrator gate keeps every start action disabled", async ({
  page,
}) => {
  await openEngine(page, WAITING_URL);

  const card = page.getByTestId("gate-card-local");
  await expect(card.getByText("Waiting", { exact: true })).toBeVisible();
  await expect(card.getByRole("button", { name: "Run locally" })).toBeDisabled();
  await expect(card.getByText(/local agent connection/).first()).toBeVisible();

  const panel = page.getByTestId("local-orchestrator-panel");
  await expect(panel.getByTestId("local-orchestrator-reason")).toContainText(
    "local agent connection",
  );
  await expect(panel.getByTestId("local-orchestrator-create")).toBeDisabled();
  await expect(panel.getByTestId("local-orchestrator-pair")).toBeDisabled();
  await expect(panel.getByTestId("local-orchestrator-row-vowp-local")).toHaveCount(0);
  await expectNoWcagViolations(page);
});

test("an Incompatible local-orchestrator gate names the protocol and stays disabled", async ({
  page,
}) => {
  await openEngine(page, INCOMPATIBLE_URL);

  const card = page.getByTestId("gate-card-local");
  await expect(card.getByText("Incompatible", { exact: true })).toBeVisible();
  await expect(card.getByRole("button", { name: "Run locally" })).toBeDisabled();
  await expect(card.getByText(/vowp-agent\/0/).first()).toBeVisible();

  const panel = page.getByTestId("local-orchestrator-panel");
  await expect(panel.getByTestId("local-orchestrator-reason")).toContainText(
    "vowp-agent/0",
  );
  await expect(panel.getByTestId("local-orchestrator-create")).toBeDisabled();
  await expect(panel.getByTestId("local-orchestrator-pair")).toBeDisabled();
  await expectNoWcagViolations(page);
});
