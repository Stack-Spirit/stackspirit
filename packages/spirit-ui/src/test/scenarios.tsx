import { MockBridge, MOCK_SCENARIOS, type MockScenario } from "../bridge/MockBridge";

/** Named preview scenarios shared by tests and the preview picker. */
export const SCENARIOS: Array<{ id: MockScenario; label: string }> = [
  { id: "fresh", label: "Fresh · 1 running" },
  { id: "empty", label: "Empty · no hosts" },
  { id: "idle", label: "Idle supervisor" },
  { id: "busy", label: "Busy · limits + waiting" },
  { id: "storage-low", label: "Storage low · brake" },
  { id: "supervisor-down", label: "Supervisor down" },
  { id: "stale-cache", label: "Stale cache" },
  { id: "warnings", label: "Parse warnings" },
  { id: "host-key-mismatch", label: "Host key mismatch" },
  { id: "probe-failure", label: "Probe failure" },
  { id: "connect-failure", label: "SSH auth failure" },
  { id: "stale-after-3", label: "Stale after three polls" },
  { id: "gates-ready", label: "Synthetic gates ready" },
  { id: "gate-d6-waiting", label: "D6 · gate waiting" },
  { id: "gate-d6-incompatible", label: "D6 · incompatible agent" },
  { id: "gate-d6-ready", label: "D6 · ready local orchestrator" },
  { id: "gate-d6-revoked", label: "D6 · revoked local agent" },
];

export function scenarioBridge(scenario: MockScenario, now?: string): MockBridge {
  return new MockBridge({ scenario, now });
}

export { MOCK_SCENARIOS };
