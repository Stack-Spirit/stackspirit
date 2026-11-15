/**
 * Shared bridge surface. Host-specific adapters (Tauri, HTTP/SSE) live outside this
 * package and implement `SpiritBridge`; nothing here may import a host runtime.
 */
export type { SpiritBridge } from "./SpiritBridge";
export { MOCK_SCENARIOS, MockBridge, type MockScenario } from "./MockBridge";
export * from "./errors";
export * from "./wire";
