import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

import {
  E2E_LOCK,
  E2E_TIMEOUT,
  startViteServer,
  stopViteServer,
  type ViteServer,
} from "./support";

// Both gated-off cases share the two dev servers started in `beforeAll`; keep them in
// one worker so a parallel worker never races for the same ports.
test.describe.configure({ mode: "serial", lock: E2E_LOCK, timeout: E2E_TIMEOUT });

const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"];
const WAITING_PORT = 1426;
const INCOMPATIBLE_PORT = 1427;
const WAITING_URL = `http://127.0.0.1:${WAITING_PORT}`;
const INCOMPATIBLE_URL = `http://127.0.0.1:${INCOMPATIBLE_PORT}`;

const servers: ViteServer[] = [];

async function expectNoWcagViolations(page: Page) {
  const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
  expect(results.violations).toEqual([]);
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
  try {
    servers.push(await startViteServer("gate-d6-waiting", WAITING_PORT));
    servers.push(await startViteServer("gate-d6-incompatible", INCOMPATIBLE_PORT));
  } catch (error) {
    await Promise.all(servers.map((server) => stopViteServer(server.process)));
    throw error;
  }
});

test.afterAll(async () => {
  await Promise.all(servers.map((server) => stopViteServer(server.process)));
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
