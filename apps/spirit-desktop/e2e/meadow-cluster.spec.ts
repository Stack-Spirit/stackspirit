import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

import { E2E_LOCK, E2E_TIMEOUT } from "./support";

test.describe.configure({ lock: E2E_LOCK, timeout: E2E_TIMEOUT });

// Dialogs slide and fade in; with reduced motion they appear at once, so axe
// measures the finished colours instead of a half-transparent animation frame.
test.use({ contextOptions: { reducedMotion: "reduce" } });

const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"];
const THEMES = ["light", "dark"] as const;

// Meadow is the only shell; the query only selects the mock scenario and theme.
const url = (theme: "light" | "dark" = "light") => `/?scenario=fresh&theme=${theme}`;

async function expectNoWcagViolations(page: Page) {
  const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
  expect(
    results.violations.map((violation) => ({
      id: violation.id,
      targets: violation.nodes.map((node) => node.target.join(" ")),
    })),
  ).toEqual([]);
}

async function connect(page: Page) {
  await page.getByTestId("sidebar-connect").click();
  await expect(page.getByTestId("health-card")).toBeVisible();
}

/**
 * Preview seeds one saved server and has no two-server scenario. The Add server
 * wizard is a real `MockBridge.hosts.add` action, so this creates a saved profile
 * through the product UI. It never stubs a cluster, a health read or a pooled total.
 */
async function addServer(page: Page, label: string, host: string) {
  await page.getByTestId("settings-trigger").click();
  await page.getByRole("menuitem", { name: "Add server" }).click();

  const dialog = page.getByRole("dialog", { name: "Add a server" });
  await expect(dialog).toBeVisible();
  await dialog.getByLabel(/^Label/).fill(label);
  await dialog.getByLabel(/^Host/).fill(host);
  await dialog.getByRole("button", { name: "Next: verify host key" }).click();
  await dialog.getByRole("checkbox", { name: /I verified this fingerprint/ }).check();
  await dialog.getByRole("button", { name: "Trust and continue" }).click();
  await expect(dialog.getByTestId("probe-stage-ssh")).toContainText("Passed");
  await dialog.getByRole("button", { name: "Save host" }).click();
  await expect(dialog).toHaveCount(0);
}

async function openServerPage(page: Page) {
  await page.getByTestId("health-card").click();
  await expect(page.getByRole("region", { name: "Server" })).toBeVisible();
}

test("one saved server never offers the Form a cluster entry point", async ({ page }) => {
  await page.goto(url());
  await expect(page.getByTestId("next-shell")).toBeVisible();
  await connect(page);

  // The Server card's options menu has no cluster action with one saved server.
  await page.getByRole("button", { name: "Server options" }).click();
  const menu = page.getByRole("menu", { name: /options$/ });
  await expect(menu).toBeVisible();
  await expect(menu.getByRole("menuitem", { name: /Form a cluster/ })).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);

  // The Server page renders no cluster block, so there is no invitation either.
  await openServerPage(page);
  await expect(page.getByRole("region", { name: "Cluster" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Form a cluster" })).toHaveCount(0);
});

test("two saved servers open the form and preview refuses without a fake cluster", async ({
  page,
}) => {
  await page.goto(url());
  await expect(page.getByTestId("next-shell")).toBeVisible();
  await connect(page);
  await addServer(page, "worker-a", "203.0.113.11");
  await addServer(page, "worker-b", "203.0.113.12");

  // Entry point 1: the Server card's ⋯ menu.
  await page.getByRole("button", { name: "Server options" }).click();
  const menu = page.getByRole("menu", { name: /options$/ });
  await expect(menu).toBeVisible();
  await menu.getByRole("menuitem", { name: /Form a cluster/ }).click();
  const fromMenu = page.getByRole("dialog", { name: "Form a cluster" });
  await expect(fromMenu).toBeVisible();
  await fromMenu.getByRole("button", { name: "Close dialog" }).click();
  await expect(fromMenu).toHaveCount(0);

  // Entry point 2: the Cluster invitation on the Server page.
  await openServerPage(page);
  const section = page.getByRole("region", { name: "Cluster" });
  await expect(section).toBeVisible();
  await expect(section).toContainText("3 servers are saved.");
  await section.getByRole("button", { name: "Form a cluster" }).click();

  const dialog = page.getByRole("dialog", { name: "Form a cluster" });
  await expect(dialog).toBeVisible();
  const submit = dialog.getByRole("button", { name: "Form cluster" });
  await expect(submit).toBeDisabled();

  // Exactly one Home: a single-select with one selected server.
  const home = dialog.getByLabel("Home server");
  await expect(home).not.toHaveAttribute("multiple");
  expect(
    await home.evaluate((element) => (element as HTMLSelectElement).selectedOptions.length),
  ).toBe(1);
  await home.selectOption({ label: "worker-b" });
  await expect(home.locator("option:checked")).toHaveText("worker-b");
  await home.selectOption({ label: "staging-vps" });
  await expect(home.locator("option:checked")).toHaveText("staging-vps");

  // At least one Worker/Backup server is required, and the message is visible.
  const workerError = dialog.getByText("Choose at least one Worker/Backup server.");
  await expect(workerError).toBeVisible();
  await dialog.getByLabel("Use worker-a as a worker").check();
  await expect(workerError).toHaveCount(0);
  await dialog.getByLabel("Use worker-b as a worker").check();

  // Node names must be unique; the invalid field is marked for assistive tech.
  const nodeB = dialog.getByLabel("Node name for worker-b");
  await nodeB.fill("worker-a");
  const duplicate = dialog.getByText("Node names must be unique inside the cluster.");
  await expect(duplicate).toBeVisible();
  await expect(nodeB).toHaveAttribute("aria-invalid", "true");
  await expect(submit).toBeDisabled();
  await nodeB.fill("worker-b");
  await expect(duplicate).toHaveCount(0);
  await expect(nodeB).toHaveAttribute("aria-invalid", "false");

  await expect(dialog.getByLabel("Project name on Home")).toHaveValue("vowp");
  await dialog.getByLabel("Project checkout on Home").fill("/srv/code/stackspirit");
  await dialog.getByLabel("Backup destination").fill("backup-host:/srv/vowp-backups");
  await dialog.getByLabel("Confirm cluster changes").check();
  await expect(submit).toBeEnabled();

  // Preview has no desktop host, so the host refuses and the form stays open.
  await submit.click();
  const gate = page.getByTestId("cluster-error");
  await expect(gate).toBeVisible();
  await expect(gate).toContainText("preview_unavailable");
  await expect(gate).toContainText("Clusters need the desktop host");
  await expect(gate).toContainText(
    "preview mode has no SSH connection and does not form, read or promote one.",
  );
  await expect(gate).toContainText(
    "Run the desktop app without VITE_SPIRIT_MOCK to work with clusters.",
  );
  await expect(dialog.getByLabel("Project checkout on Home")).toHaveValue("/srv/code/stackspirit");
  await expect(submit).toBeEnabled();

  // No success state, pooled totals, fake worker rows or toast was invented.
  await expect(page.getByTestId("cluster-formed")).toHaveCount(0);
  await expect(page.getByText("Cluster formed", { exact: true })).toHaveCount(0);
  await expect(page.getByText(/Pooled compute is/)).toHaveCount(0);
  await expect(page.getByText(/\d+ Worker\/Backup servers? are enrolled/)).toHaveCount(0);
  await expect(page.getByTestId("cluster-pooled")).toHaveCount(0);
  await expect(page.getByTestId("cluster-section")).toHaveCount(0);
  await expect(page.getByRole("list", { name: "Cluster servers" })).toHaveCount(0);
  await expect(page.getByText(/pooled workers busy/)).toHaveCount(0);
  await expect(page.getByRole("region", { name: "Notifications" })).toHaveCount(0);
  await expect(page.getByTestId("server-card")).not.toHaveAttribute("data-cluster");

  await dialog.getByRole("button", { name: "Close dialog" }).click();
  await expect(dialog).toHaveCount(0);

  // Promote backup is not reachable in preview: it needs a saved cluster config,
  // and preview deliberately skips the cluster health read. The only honest
  // assertion is that the action is absent, not that it was faked and refused.
  await expect(section.getByRole("button", { name: "Promote backup to Home" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Promote", exact: true })).toHaveCount(0);
  await expect(page.getByTestId("promote-outcome")).toHaveCount(0);
  await expect(page.getByText(/Backup promoted/)).toHaveCount(0);
});

test("cluster section and form meet WCAG 2.2 AA in light and dark", async ({ page }) => {
  for (const theme of THEMES) {
    await page.goto(url(theme));
    await expect(page.getByTestId("next-shell")).toBeVisible();
    await connect(page);
    await addServer(page, "worker-a", "203.0.113.11");
    await openServerPage(page);

    const section = page.getByRole("region", { name: "Cluster" });
    await expect(section).toBeVisible();
    await expect(section).toContainText("2 servers are saved.");
    await expectNoWcagViolations(page);

    await section.getByRole("button", { name: "Form a cluster" }).click();
    const dialog = page.getByRole("dialog", { name: "Form a cluster" });
    await expect(dialog).toBeVisible();
    await expectNoWcagViolations(page);

    await dialog.getByLabel("Use worker-a as a worker").check();
    await expect(dialog.getByLabel("Node name for worker-a")).toHaveValue("worker-a");
    await expectNoWcagViolations(page);

    await page.keyboard.press("Escape");
    await expect(dialog).toHaveCount(0);
  }
});
