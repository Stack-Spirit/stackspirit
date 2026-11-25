import { expect, test } from "@playwright/test";

test("read-only preview renders the board, lanes, capacity notice, and preview badge", async ({
  page,
}) => {
  const pageErrors: string[] = [];
  const consoleErrors: string[] = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push(message.text());
  });
  await page.goto("/");
  await expect(page.getByText("Preview data").first()).toBeVisible();
  await expect(page.getByRole("heading", { name: "Fleet", level: 1 })).toBeVisible();
  await expect(page.getByTestId("web-lanes")).toBeVisible();
  await expect(page.getByTestId("running-workers")).toBeVisible();
  await expect(page.getByTestId("capacity-unavailable")).toContainText(
    "Capacity detail unavailable",
  );
  const reviewLane = page.getByTestId("lane-review_fix");
  await expect(reviewLane.getByTestId("job-card-ss-9-gate-check")).toBeVisible();
  await expect(reviewLane.getByTestId("gate-failures-unavailable")).toHaveCount(0);

  const nav = page.getByRole("navigation", { name: "Primary views" });
  await expect(nav.getByRole("button")).toHaveCount(1);
  await expect(nav.getByRole("button", { name: "Fleet" })).toBeVisible();
  expect(pageErrors).toEqual([]);
  expect(consoleErrors).toEqual([]);
});

test("the job drawer shows the real job, runs, and gate failures", async ({ page }) => {
  await page.goto("/");
  await page.getByTestId("job-card-ss-9-gate-check").click();
  const drawer = page.getByTestId("web-job-detail");
  await expect(drawer).toBeVisible();
  await expect(drawer).toContainText("Gate check");
  await expect(page.getByTestId("job-detail-fields")).toContainText("reviewing");
  await expect(page.getByTestId("job-runs")).toContainText("Runs (2)");
  await expect(page.getByTestId("job-gate-0")).toContainText("report");
  await expect(page.getByTestId("job-gate-0")).toContainText("missing: tests");
  await expect(page.getByTestId("job-gate-2")).toContainText("secret_scan");
  await expect(page.getByTestId("job-detail-error")).toHaveCount(0);
  await expect(page.getByTestId("gate-failures-unavailable")).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("web-job-detail")).toHaveCount(0);
});

test("the job drawer shows a true empty state for a job with no runs", async ({ page }) => {
  await page.goto("/");
  await page.getByTestId("job-card-ss-5-mock").click();
  await expect(page.getByTestId("job-runs")).toContainText("No runs recorded");
  await expect(page.getByTestId("job-gates")).toContainText("No gate results");
});

test("read-only preview sends no API request, no POST, and no Authorization header", async ({
  page,
}) => {
  const requests: Array<{ url: string; method: string; authorization: string | null }> = [];
  page.on("request", (request) => {
    requests.push({
      url: request.url(),
      method: request.method(),
      authorization: request.headers()["authorization"] ?? null,
    });
  });
  await page.goto("/");
  await expect(page.getByTestId("web-lanes")).toBeVisible();

  const apiRequests = requests.filter((request) => request.url.includes("/api/"));
  expect(apiRequests).toEqual([]);
  expect(requests.every((request) => request.authorization === null)).toBe(true);
  expect(requests.some((request) => request.method === "POST")).toBe(false);
});

test("desktop-only routes show the exact copy without a bridge read", async ({ page }) => {
  const apiRequests: string[] = [];
  page.on("request", (request) => {
    if (request.url().includes("/api/")) apiRequests.push(request.url());
  });
  await page.goto("/hosts");
  await expect(page.getByTestId("desktop-only-hosts")).toContainText(
    "Host profiles and SSH keys live in the desktop app.",
  );
  expect(apiRequests).toEqual([]);
  await page.goto("/engine");
  await expect(page.getByTestId("desktop-only-engine")).toContainText(
    "StackSpirit runs beside the desktop app.",
  );
  await page.goto("/gates");
  await expect(page.getByTestId("desktop-only-gates")).toContainText(
    "Capability gates are probed over SSH from the desktop app.",
  );
});

test("the inbox drawer is read-only and shows the full body and options", async ({ page }) => {
  await page.goto("/");
  await page.getByTestId("web-inbox-button").click();
  const drawer = page.getByTestId("web-inbox-drawer");
  await expect(drawer.getByText("Choose a merge queue policy")).toBeVisible();
  await expect(drawer.getByTestId("inbox-body")).toContainText("one job at a time or batch");
  await expect(drawer.getByRole("textbox")).toHaveCount(0);
  await expect(drawer.getByText("Send answer")).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(drawer).toHaveCount(0);
});
