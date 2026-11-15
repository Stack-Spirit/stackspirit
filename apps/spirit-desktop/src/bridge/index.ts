import type { SpiritBridge } from "@stackspirit/ui";
import { MOCK_SCENARIOS, MockBridge, type MockScenario } from "@stackspirit/ui";

/**
 * Desktop bridge factory. Preview mode is opt-in with `VITE_SPIRIT_MOCK=1`; anything
 * else uses the Tauri host, and the renderer never falls back silently to fake data.
 */
export async function createBridge(): Promise<SpiritBridge> {
  if (import.meta.env.VITE_SPIRIT_MOCK === "1") {
    const requested = import.meta.env.VITE_SPIRIT_MOCK_SCENARIO;
    const scenario: MockScenario =
      typeof requested === "string" &&
      (MOCK_SCENARIOS as readonly string[]).includes(requested)
        ? (requested as MockScenario)
        : "fresh";
    return new MockBridge(scenario);
  }
  return new (await import("./TauriBridge")).TauriBridge();
}
