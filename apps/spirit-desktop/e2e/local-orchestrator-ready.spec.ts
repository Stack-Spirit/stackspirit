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
const READY_PORT = 1428;
const READY_URL = `http://127.0.0.1:${READY_PORT}`;

let server: ViteServer | null = null;

async function expectNoWcagViolations(page: Page) {
  const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
  expect(results.violations).toEqual([]);
}

test.beforeAll(async () => {
  server = await startViteServer("gate-d6-ready", READY_PORT);
});

test.afterAll(async () => {
  await stopViteServer(server?.process ?? null);
});

test("Ready local orchestrator flow stays honest and gated by the connected agent", async ({
  page,
}) => {
  await page.goto(READY_URL);
  await expect(page.getByTestId("next-shell")).toBeVisible();
  await page.getByTestId("sidebar-connect").click();
  await expect(page.getByTestId("health-card")).toBeVisible();

  // Meadow keeps the engine and capability gates under Settings → Advanced.
  await page.getByTestId("settings-trigger").click();
  await page.getByRole("menuitem", { name: "All settings" }).click();
  await page.getByRole("button", { name: "Show advanced settings" }).click();
  await expect(page.getByTestId("engine-view")).toBeVisible();
  await expect(page.getByTestId("gate-card-local")).toBeVisible();

  const gateCard = page.getByTestId("gate-card-local");
  await expect(gateCard.getByText("Ready", { exact: true })).toBeVisible();
  await expect(gateCard.getByRole("button", { name: "Run locally" })).toBeEnabled();

  const panel = page.getByTestId("local-orchestrator-panel");
  await expect(panel.getByText("mac-studio").first()).toBeVisible();
  await expect(panel.getByText("vowp-agent/1").first()).toBeVisible();
  await expect(page.getByTestId("local-orchestrator-row-vowp-local")).toContainText(
    "StackSpirit Desktop",
  );
  await expect(page.getByTestId("local-orchestrator-row-chat-local")).toContainText(
    "Stopped",
  );
  await expect(page.getByTestId("local-orchestrator-row-docs-local")).toContainText(
    "Degraded",
  );
  await expect(page.getByTestId("local-orchestrator-row-phone-local")).toContainText(
    "Reconnecting",
  );
  await expect(
    page.getByTestId("local-surface-row-01J9Z00000000000000000000B"),
  ).toContainText("Revoked");
  await expect(
    page.getByTestId("local-surface-row-01J9Z00000000000000000000D"),
  ).toContainText("Superseded");
  await expectNoWcagViolations(page);

  // Create an orchestrator through the enabled Engine action.
  await gateCard.getByRole("button", { name: "Run locally" }).click();
  const create = page.getByRole("dialog", { name: "Create local orchestrator" });
  await expect(create).toBeVisible();
  await create.getByRole("textbox", { name: "Project" }).fill("vowp");
  await create.getByRole("textbox", { name: "Name" }).fill("browser-local");
  await create.getByTestId("local-orchestrator-create-submit").click();
  await expect(page.getByTestId("local-orchestrator-row-browser-local")).toContainText(
    "Running",
  );
  await expectNoWcagViolations(page);

  // Pair a surface, show the one-time code, then redeem it locally.
  await panel.getByTestId("local-orchestrator-pair").click();
  const pair = page.getByRole("dialog", { name: "Pair a local surface" });
  await expect(pair).toBeVisible();
  await pair.getByLabel("Session id").fill("01JCF5WQ8ZK3M2V7N4P6R8T0AE");
  await pair.getByTestId("local-surface-pair-submit").click();
  const pairing = pair.getByTestId("local-pairing-result");
  await expect(pairing.getByTestId("local-pairing-code")).toHaveText(
    "pairing-code-placeholder",
  );
  await expect(pairing.getByTestId("local-pairing-command")).toContainText(
    "vowp agent pair --code-file -",
  );
  const pendingSurface = page
    .locator('[data-testid^="local-surface-row-"]')
    .filter({ hasText: "Pending pairing" });
  await expect(pendingSurface).toBeVisible();
  const pendingSurfaceTestId = await pendingSurface.getAttribute("data-testid");
  if (!pendingSurfaceTestId) throw new Error("pending surface row has no test id");
  await pairing.getByTestId("local-pairing-agent").click();
  await expect(pair).toHaveCount(0);
  await expect(page.getByTestId(pendingSurfaceTestId)).toContainText("Active");

  // Stop/resume an orchestrator and rotate/revoke a surface.
  const running = page.getByTestId("local-orchestrator-row-vowp-local");
  await running.getByTestId("local-orchestrator-stop-vowp-local").click();
  await expect(running).toContainText("Stopped");
  await running.getByTestId("local-orchestrator-resume-vowp-local").click();
  await expect(running).toContainText("Running");

  const surface = page.getByTestId("local-surface-row-01J9Z00000000000000000000A");
  await surface.getByTestId("local-surface-rotate-01J9Z00000000000000000000A").click();
  await expect(surface).toContainText("Rotation queued");
  await surface.getByTestId("local-surface-revoke-01J9Z00000000000000000000A").click();
  await expect(surface).toContainText("Revoked");
  await expect(surface).toContainText("Read-only");
  await expectNoWcagViolations(page);
});
