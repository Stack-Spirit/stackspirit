/// <reference types="vitest/config" />
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// The desktop shell is served from a fixed loopback port so the Tauri window can
// point at it during `tauri dev`. Preview mode is selected with VITE_SPIRIT_MOCK=1.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
  },
  envPrefix: ["VITE_"],
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
});
