import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  // D3 browser-mode flows live under src/test/e2e so they stay inside the job's
  // ownership; D2's shell flows remain in e2e/.
  testDir: ".",
  testMatch: ["e2e/**/*.spec.ts", "src/test/e2e/**/*.spec.ts"],
  timeout: 30_000,
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 2 : 0,
  reporter: [["list"], ["html", { open: "never" }]],
  use: {
    baseURL: "http://127.0.0.1:1420",
    trace: "on-first-retry",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    // Build once with the MockBridge inlined, then serve the production preview.
    command:
      "npm run build && npm run preview -- --host 127.0.0.1 --port 1420 --strictPort",
    url: "http://127.0.0.1:1420",
    reuseExistingServer: false,
    timeout: 120_000,
    env: { VITE_SPIRIT_MOCK: "1" },
  },
});
