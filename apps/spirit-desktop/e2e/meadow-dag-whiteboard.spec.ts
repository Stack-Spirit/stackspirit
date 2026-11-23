import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Locator, type Page } from "@playwright/test";

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

async function openStructure(page: Page, theme: "light" | "dark") {
  await page.goto(`/?scenario=dag&theme=${theme}`);
  await page.getByTestId("sidebar-connect").click();
  const sidebar = page.getByRole("navigation", { name: "Sessions" });
  await sidebar.getByRole("button", { name: /^vowp/ }).click();
  await sidebar.getByRole("button", { name: /^Workers/ }).click();
  await expect(page.getByTestId("dag-map")).toBeVisible();
}

const node = (scope: Page | Locator, id: string) => scope.locator(`[data-node-id="${id}"]`).first();

for (const theme of ["light", "dark"] as const) {
  test(`Structure is a compact dependency DAG with chained Build, Review and Fix nodes (${theme})`, async ({
    page,
  }) => {
    await openStructure(page, theme);
    const map = page.getByTestId("dag-map");
    // 8 jobs is above the threshold: compact nodes, no verbose metadata.
    await expect(map.getByTestId("dag-canvas")).toHaveAttribute("data-compact", "true");
    await expect(map.getByText(/Waiting:/)).toHaveCount(0);

    // Converging edges: ss-118 is fed by both ss-116b and ss-117; ss-119 sits behind ss-118.
    await expect(map.locator('[data-edge="ss-116b->ss-118"]')).toHaveCount(1);
    await expect(map.locator('[data-edge="ss-117->ss-118"]')).toHaveCount(1);
    await expect(map.locator('[data-edge="ss-118->ss-119"]')).toHaveCount(1);
    await expect(map.locator('[data-edge="orchestrator->ss-120"]')).toHaveCount(1);

    // Build -> Review -> Fix chain with distinct kind chips and active versus queued state.
    await expect(node(page, "ss-116b")).toHaveAttribute("data-kind", "build");
    await expect(node(page, "ss-116b")).toHaveAttribute("data-state", "active");
    await expect(node(page, "ss-116b-verify-0")).toHaveAttribute("data-kind", "review");
    await expect(node(page, "ss-116b-verify-0")).toHaveAttribute("data-state", "queued");
    await expect(node(page, "ss-116b-fix-1")).toHaveAttribute("data-kind", "fix");
    await expect(node(page, "ss-116b-fix-1")).toHaveAttribute("data-state", "queued");
    await expect(node(page, "ss-115-verify-0")).toHaveAttribute("data-state", "active");
    await expect(node(page, "ss-116b-verify-0").getByTestId("dag-kind-chip")).toHaveText("Review");
    await expect(node(page, "ss-116b-fix-1").getByTestId("dag-kind-chip")).toHaveText("Fix");
    await expect(map.locator('[data-edge="ss-116b->ss-116b-verify-0"]')).toHaveCount(1);
    await expect(map.locator('[data-edge="ss-116b-verify-0->ss-116b-fix-1"]')).toHaveCount(1);

    // The embedded map scrolls instead of clipping its right edge.
    const overflow = await map.evaluate((el) => getComputedStyle(el).overflowX);
    expect(overflow).toBe("auto");
    await expectNoWcagViolations(page);
  });

  test(`Expand map opens a pannable, zoomable whiteboard that closes cleanly (${theme})`, async ({ page }) => {
    await openStructure(page, theme);
    const opener = page.getByTestId("expand-map");
    await opener.click();
    const board = page.getByTestId("whiteboard");
    await expect(board).toBeVisible();
    // Full detail: provider/harness/model, state chips and the wait reason.
    await expect(board.getByTestId("dag-canvas")).toHaveAttribute("data-compact", "false");
    await expect(board.getByText(/Waiting: RAM 1.2 GB short/)).toBeVisible();
    await expect(board.getByText(/model-a/).first()).toBeVisible();
    await expect(node(board, "ss-116b-fix-1").getByTestId("dag-state-chip")).toHaveText("Queued");
    await expect(node(board, "ss-115-verify-0").getByTestId("dag-state-chip")).toHaveText("Active");

    // Zoom controls.
    const zoom = board.getByTestId("whiteboard-zoom");
    await board.getByRole("button", { name: "Zoom in" }).click();
    const zoomedIn = await zoom.innerText();
    await board.getByRole("button", { name: "Reset zoom to 100%" }).click();
    await expect(zoom).toHaveText("100%");
    expect(zoomedIn).not.toBe("100%");
    await board.getByRole("button", { name: "Fit view" }).click();
    await expect(zoom).not.toHaveText("");
    await board.getByRole("button", { name: "Zoom out" }).click();

    // Wheel zoom changes the scale.
    const viewport = board.getByTestId("whiteboard-viewport");
    const box = (await viewport.boundingBox())!;
    const before = await zoom.innerText();
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.wheel(0, -300);
    await expect(zoom).not.toHaveText(before);

    // Click-and-drag panning moves the stage.
    const stage = board.getByTestId("whiteboard-stage");
    const origin = (await stage.boundingBox())!;
    await page.mouse.move(box.x + 40, box.y + box.height - 40);
    await page.mouse.down();
    await page.mouse.move(box.x + 140, box.y + box.height - 90, { steps: 6 });
    await page.mouse.up();
    const moved = (await stage.boundingBox())!;
    expect(Math.abs(moved.x - origin.x) + Math.abs(moved.y - origin.y)).toBeGreaterThan(40);

    // A node opens its detail card without resizing the graph.
    await board.getByRole("button", { name: "Fit view" }).click();
    const stageBefore = (await stage.boundingBox())!;
    await node(board, "ss-118").click();
    const card = board.getByTestId("whiteboard-card");
    await expect(card).toBeVisible();
    await expect(card).toContainText("ss-116b, ss-117");
    const stageAfter = (await stage.boundingBox())!;
    expect(stageAfter.width).toBeCloseTo(stageBefore.width, 0);
    await expectNoWcagViolations(page);

    // Esc closes and returns to the page; Close does too.
    await page.keyboard.press("Escape");
    await expect(board).toHaveCount(0);
    await expect(page.getByTestId("dag-map")).toBeVisible();
    await opener.click();
    await expect(board).toBeVisible();
    await board.getByRole("button", { name: "Close", exact: true }).click();
    await expect(board).toHaveCount(0);
  });
}
