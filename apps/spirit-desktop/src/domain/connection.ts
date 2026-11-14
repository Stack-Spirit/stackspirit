import type { ConnectionSnapshot, FleetView } from "../bridge/wire";

export type Tone = "ok" | "info" | "warn" | "danger" | "unknown";

export function connectionTone(
  connection: ConnectionSnapshot,
  fleet: FleetView | null,
): Tone {
  if (connection.state === "connected") {
    if (!fleet) return "info";
    return fleet.stale ? "warn" : "ok";
  }
  switch (connection.state) {
    case "connecting":
    case "disconnecting":
      return "info";
    case "stale":
    case "failed":
      return connection.state === "stale" ? "warn" : "danger";
    default:
      return "unknown";
  }
}

export function connectionLabel(
  connection: ConnectionSnapshot,
  fleet: FleetView | null,
): string {
  switch (connection.state) {
    case "connected":
      return fleet?.stale ? "Connected · stale" : "Connected";
    case "connecting":
      return "Connecting…";
    case "disconnecting":
      return "Disconnecting…";
    case "stale":
      return "Stale";
    case "failed":
      return "Failed";
    default:
      return "Not connected";
  }
}

export function supervisorTone(fleet: FleetView | null): Tone {
  if (!fleet) return "unknown";
  const supervisor = fleet.supervisor;
  if (supervisor.paused) return "warn";
  if (!supervisor.running) return "danger";
  const quietAfter = Math.max(supervisor.tick_secs * 3, 180);
  if (
    supervisor.last_tick_age_secs !== null &&
    supervisor.last_tick_age_secs > quietAfter
  ) {
    return "warn";
  }
  return "ok";
}

export function supervisorLabel(fleet: FleetView | null): string {
  if (!fleet) return "supervisor unknown";
  const supervisor = fleet.supervisor;
  if (supervisor.paused) return "supervisor paused";
  if (!supervisor.running) return "supervisor stopped";
  const quietAfter = Math.max(supervisor.tick_secs * 3, 180);
  if (
    supervisor.last_tick_age_secs !== null &&
    supervisor.last_tick_age_secs > quietAfter
  ) {
    return `supervisor quiet · ${supervisor.last_tick_age_secs}s`;
  }
  return `supervisor up · tick ${supervisor.tick_secs}s`;
}

export function gateTone(state: string): Tone {
  switch (state) {
    case "ready":
      return "ok";
    case "waiting_for_vowp_phase3":
    case "waiting_for_vowp_asif":
      return "warn";
    case "incompatible":
    case "unreachable":
      return "danger";
    case "probing":
      return "info";
    default:
      return "unknown";
  }
}

export function gateStateLabel(state: string): string {
  switch (state) {
    case "ready":
      return "Ready";
    case "probing":
      return "Probing";
    case "waiting_for_vowp_phase3":
    case "waiting_for_vowp_asif":
      return "Waiting";
    case "incompatible":
      return "Incompatible";
    case "unreachable":
      return "Unreachable";
    default:
      return "Unknown";
  }
}

export function engineTone(engine: {
  state: { state: string };
  choice: { type: string };
} | null): Tone {
  if (!engine) return "unknown";
  switch (engine.state.state) {
    case "ready":
      return "ok";
    case "starting":
      return "info";
    case "degraded":
    case "failed":
      return "danger";
    default:
      return engine.choice.type === "external" ? "info" : "unknown";
  }
}

export function engineLabel(engine: {
  state: { state: string };
  choice: { type: string; kind?: string };
} | null): string {
  if (!engine) return "Engine unknown";
  if (engine.choice.type === "external") {
    return `External: ${engine.choice.kind === "claude-code" ? "Claude Code" : "opencode"}`;
  }
  switch (engine.state.state) {
    case "ready":
      return "Engine ready";
    case "starting":
      return "Engine starting";
    case "degraded":
    case "failed":
      return "Engine error";
    case "stopped":
      return "Engine stopped";
    default:
      return "Engine needs setup";
  }
}
