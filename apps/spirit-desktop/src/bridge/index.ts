import type { SpiritBridge } from "./SpiritBridge";
import { MOCK_SCENARIOS, type MockScenario } from "./MockBridge";

export type { SpiritBridge } from "./SpiritBridge";
export { MOCK_SCENARIOS, MockBridge, type MockScenario } from "./MockBridge";
export { TauriBridge } from "./TauriBridge";
export * from "./errors";
export * from "./wire";

/**
 * Preview mode is opt-in with `VITE_SPIRIT_MOCK=1`. Anything else uses the Tauri
 * host; the renderer never falls back silently to fake data.
 */
export async function createBridge(): Promise<SpiritBridge> {
  if (import.meta.env.VITE_SPIRIT_MOCK === "1") {
    const requested = import.meta.env.VITE_SPIRIT_MOCK_SCENARIO;
    const scenario: MockScenario =
      typeof requested === "string" &&
      (MOCK_SCENARIOS as readonly string[]).includes(requested)
        ? (requested as MockScenario)
        : "fresh";
    return new (await import("./MockBridge")).MockBridge(scenario);
  }
  return new (await import("./TauriBridge")).TauriBridge();
}
