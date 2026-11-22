import { defineConfig, devices } from "@playwright/test";

// Parallel job worktrees share one host, so the port is overridable
// (SPIRIT_E2E_PORT) instead of assuming 1420 is free.
const PORT = Number(process.env.SPIRIT_E2E_PORT ?? 1420);
const ORIGIN = `http://127.0.0.1:${PORT}`;

export default defineConfig({
  // Meadow is the only shell. Playwright owns the flows under e2e/; the desktop
  // Vitest files under src/ are a separate runner.
  testDir: ".",
  testMatch: ["e2e/**/*.spec.ts"],
  timeout: 30_000,
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 2 : 0,
  reporter: [["list"], ["html", { open: "never" }]],
  use: {
    baseURL: ORIGIN,
    trace: "on-first-retry",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    // Build once with the MockBridge inlined, then serve the production preview.
    command: `npm run build && npm run preview -- --host 127.0.0.1 --port ${PORT} --strictPort`,
    url: ORIGIN,
    reuseExistingServer: false,
    timeout: 120_000,
    env: { VITE_SPIRIT_MOCK: "1" },
  },
});
