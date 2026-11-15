/// <reference types="vitest/config" />
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

/**
 * The Web app never talks to a loopback daemon across origins: the daemon has no CORS.
 * In development this server proxies `/api` to `VOWP_DAEMON_URL` (the real loopback
 * daemon), while the browser uses the dev-server origin as `VITE_VOWP_DAEMON_URL`.
 * A production deployment uses the same shape behind one reverse proxy.
 */
export default defineConfig(() => {
  // Server-side proxy target only. Do not load or print .env contents here.
  const daemonUrl = process.env.VOWP_DAEMON_URL;
  return {
    plugins: [react()],
    clearScreen: false,
    server: {
      port: 5174,
      strictPort: true,
      host: "127.0.0.1",
      proxy: daemonUrl
        ? {
            "/api": {
              target: daemonUrl,
              changeOrigin: false,
              secure: false,
              ws: false,
            },
          }
        : undefined,
    },
    preview: {
      port: 4173,
      strictPort: true,
      host: "127.0.0.1",
    },
    envPrefix: ["VITE_"],
    optimizeDeps: {
      // @stackspirit/ui is linked from TypeScript source, like the desktop app.
      exclude: ["@stackspirit/ui"],
    },
    build: {
      outDir: "dist",
      sourcemap: true,
      target: "es2022",
    },
    test: {
      environment: "jsdom",
      setupFiles: ["./src/test/setup.ts"],
      css: false,
      globals: false,
      include: ["src/**/*.test.{ts,tsx}"],
      restoreMocks: true,
      testTimeout: 15000,
    },
  };
});
