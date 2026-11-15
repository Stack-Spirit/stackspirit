import { describe, expect, it } from "vitest";

const TAURI_API = ["@tauri-apps", "api"].join("/");

/**
 * The desktop renderer boundary. `@stackspirit/ui` is host-agnostic, so the desktop app's
 * single Tauri adapter must be the only file in its renderer tree that imports the Tauri API.
 */
describe("renderer import boundary", () => {
  it("keeps the Tauri API inside SpiritBridge's Tauri adapter", () => {
    const sources = import.meta.glob("../**/*.{ts,tsx}", {
      query: "?raw",
      import: "default",
      eager: true,
    }) as Record<string, string>;
    const importers = Object.entries(sources)
      .filter(([path]) => !path.includes(".test.") && !path.includes("/test/"))
      .filter(([, source]) => source.includes(TAURI_API))
      .map(([path]) => path);
    expect(importers).toHaveLength(1);
    expect(importers[0]?.endsWith("TauriBridge.ts")).toBe(true);
  });
});
