import { defineConfig, devices } from "@playwright/test";

/**
 * The Web e2e suite is MockBridge-only. Each project builds its own preview bundle with a
 * fixed scenario at build time; no test switches the scenario at runtime and no test needs
 * a live daemon.
 */
export default defineConfig({
  testDir: "e2e",
  timeout: 30_000,
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 2 : 0,
  reporter: [["list"], ["html", { open: "never" }]],
  use: {
    trace: "on-first-retry",
  },
  projects: [
    {
      name: "web-read-only",
      use: { ...devices["Desktop Chrome"], baseURL: "http://127.0.0.1:4173" },
      testMatch: ["read-only.spec.ts", "accessibility.spec.ts", "mobile-pwa.spec.ts"],
    },
    {
      name: "web-stale",
      use: { ...devices["Desktop Chrome"], baseURL: "http://127.0.0.1:4174" },
      testMatch: ["stale.spec.ts"],
    },
  ],
  webServer: [
    {
      command:
        "npm run build:e2e:read-only && npm run preview -- --outDir dist-read-only --port 4173 --strictPort",
      url: "http://127.0.0.1:4173",
      reuseExistingServer: false,
      timeout: 120_000,
    },
    {
      command:
        "npm run build:e2e:stale && npm run preview -- --outDir dist-stale --port 4174 --strictPort",
      url: "http://127.0.0.1:4174",
      reuseExistingServer: false,
      timeout: 120_000,
    },
  ],
});
