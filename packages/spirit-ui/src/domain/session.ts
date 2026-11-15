import type { SpiritError } from "../bridge/errors";
import { toSpiritError } from "../bridge/errors";
import type { AsifLoss, GateSnapshot, SessionCardView } from "../bridge/wire";
import type { Tone } from "./connection";
import { gateStateLabel, gateTone } from "./connection";
import { gateActionEnabled, gateActionReason, gateMissingMilestones } from "./engine";
import { formatAge, formatDateTime } from "./format";

/** The adapter's hard archive cap from `VOWP-CONTRACT.md` §11.2.3. */
export const ASIF_PACKAGE_LIMIT_BYTES = 272 * 1024 * 1024;

/** Human label for the archive cap. */
export const ASIF_PACKAGE_LIMIT_LABEL = "272 MiB";

export type AsifAction = "asif_export" | "asif_import";

export type AsifDirection = "export" | "import";

/** The two advanced loss policies. They are mutually exclusive by contract. */
export type AsifLossPolicyChoice = "allow-loss" | "strict";

export const DEFAULT_ASIF_LOSS_POLICY: AsifLossPolicyChoice = "allow-loss";

/** Which section the Sessions drawer opens on. */
export type SessionsIntent = "list" | "import";

export interface AsifActionAvailability {
  enabled: boolean;
  reason: string;
}

export interface AsifGateBadge {
  label: string;
  tone: Tone;
}

export interface AsifErrorCopy {
  /** Fixed title, including the VOWP exit code when the error maps to one. */
  title: string;
  /** VOWP's safe message. */
  detail: string;
  /** One next step. It never suggests a mutated retry or a force switch. */
  next: string;
}

export interface SessionUpdated {
  absolute: string;
  relative: string;
}

const SESSION_STATE_LABELS: Record<string, string> = {
  active: "Active",
  idle: "Idle",
  paused: "Paused",
  stopped: "Stopped",
  failed: "Failed",
  error: "Error",
  ended: "Ended",
};

/**
 * The sessions list is read-only. A state this build does not know stays visible instead
 * of being mapped to an invented happy state.
 */
export function sessionStateLabel(state: string): string {
  const known = SESSION_STATE_LABELS[state.trim().toLowerCase()];
  if (known) return known;
  return state.trim() ? `Unknown (${state})` : "Unknown";
}

export function sessionStateTone(state: string): Tone {
  switch (state.trim().toLowerCase()) {
    case "active":
    case "running":
    case "ready":
      return "ok";
    case "idle":
      return "info";
    case "paused":
    case "stopped":
      return "warn";
    case "failed":
    case "error":
      return "danger";
    default:
      return "unknown";
  }
}

/** A relative "updated" label using the snapshot clock, plus the exact timestamp. */
export function sessionUpdated(
  session: SessionCardView,
  snapshotAt: string | null | undefined,
): SessionUpdated {
  const updated = Date.parse(session.last_event_at);
  const reference = snapshotAt ? Date.parse(snapshotAt) : Number.NaN;
  if (!Number.isFinite(updated)) {
    return { absolute: "unknown", relative: "updated unknown" };
  }
  if (!Number.isFinite(reference)) {
    return { absolute: formatDateTime(session.last_event_at), relative: "updated unknown" };
  }
  const seconds = Math.max(0, Math.round((reference - updated) / 1000));
  return {
    absolute: formatDateTime(session.last_event_at),
    relative: `updated ${formatAge(seconds)} ago`,
  };
}

/** The ASIF badge shows a partial Ready state honestly instead of a full green gate. */
export function asifGateBadge(gates: GateSnapshot | null): AsifGateBadge {
  const gate = gates?.asif;
  if (!gate) return { label: "Unknown", tone: "unknown" };
  if (gate.state !== "ready") {
    return { label: gateStateLabel(gate.state), tone: gateTone(gate.state) };
  }
  if (gate.export && gate.import) return { label: "Ready", tone: "ok" };
  if (gate.export) return { label: "Ready · export only", tone: "warn" };
  if (gate.import) return { label: "Ready · import only", tone: "warn" };
  return { label: "Ready · no action proven", tone: "warn" };
}

/** One sentence of gate detail. Missing milestones stay named, never implied. */
export function asifGateDetail(gates: GateSnapshot | null): string {
  const gate = gates?.asif;
  if (!gate) return "Probe the VOWP home to check ASIF support.";
  switch (gate.state) {
    case "ready": {
      const exportLabel = gate.export ? "proven" : "not proven";
      const importLabel = gate.import ? "proven" : "not proven";
      return `Export ${exportLabel} · import ${importLabel}.`;
    }
    case "waiting_for_vowp_asif":
      return gate.missing.length > 0
        ? `Waiting for VOWP ASIF. Missing: ${gate.missing.join(" · ")}`
        : "Waiting for VOWP ASIF.";
    case "incompatible":
      return `Incompatible ASIF surface: ${gate.detected} (required ${gate.required}).`;
    case "unreachable":
      return `The VOWP home could not be reached: ${gate.detail}`;
    case "probing":
      return "Probing the VOWP home.";
    default:
      return "Probe the VOWP home to check ASIF support.";
  }
}

export function asifGateMissing(gates: GateSnapshot | null): readonly string[] {
  return gateMissingMilestones(gates).asif;
}

/**
 * Direction availability combines the connection, the ASIF gate and the build feature
 * flag. A stale or disconnected session snapshot can never leave an action enabled.
 */
export function asifActionAvailability(input: {
  gates: GateSnapshot | null;
  action: AsifAction;
  connectionState: string;
  bridgeAvailable: boolean;
}): AsifActionAvailability {
  if (!input.bridgeAvailable) {
    return {
      enabled: false,
      reason: "ASIF sessions are not available in this build.",
    };
  }
  if (input.connectionState === "stale") {
    return {
      enabled: false,
      reason:
        "The VOWP connection is stale. Reconnect before exporting or importing an ASIF package.",
    };
  }
  if (input.connectionState !== "connected") {
    return {
      enabled: false,
      reason:
        "Connect to a VOWP home before exporting or importing an ASIF package.",
    };
  }
  if (gateActionEnabled(input.gates, input.action)) {
    return { enabled: true, reason: "" };
  }
  return {
    enabled: false,
    reason: gateActionReason(input.gates, input.action),
  };
}

export function asifDirectionLabel(direction: AsifDirection): string {
  return direction === "export" ? "Export" : "Import";
}

export function asifDirectionAction(direction: AsifDirection): AsifAction {
  return direction === "export" ? "asif_export" : "asif_import";
}

export function asifLossPolicyTitle(policy: AsifLossPolicyChoice): string {
  return policy === "strict"
    ? "Strict — refuse blocking or integrity losses"
    : "Allow declared losses (default)";
}

export function asifLossPolicyHint(
  policy: AsifLossPolicyChoice,
  direction: AsifDirection,
): string {
  if (policy === "strict") {
    return direction === "export"
      ? "Strict refuses blocking or integrity losses before the package is written. Inherent omissions and normalizations stay non-fatal."
      : "Strict refuses blocking or integrity losses before the new session becomes visible. Inherent omissions and normalizations stay non-fatal.";
  }
  return direction === "export"
    ? "VOWP writes the package, then reports every omission or reconstruction. StackSpirit shows the full declared-loss list."
    : "VOWP creates a new idle session, then reports every omission or reconstruction. StackSpirit shows the full declared-loss list.";
}

/** Visible warning beside the loss-policy choice. Never a claim of an exact package. */
export function asifLossPolicyWarning(
  policy: AsifLossPolicyChoice,
  direction: AsifDirection,
): string {
  if (policy === "strict") {
    return "Strict can refuse a package that would otherwise succeed. StackSpirit never adds a force switch or an allow-unsupported switch.";
  }
  return direction === "export"
    ? "Allow-loss is the default. The package can omit or reconstruct VOWP state, and VOWP will list every declared loss. StackSpirit never calls the result an exact copy."
    : "Allow-loss is the default. The imported session can omit or reconstruct VOWP state, and VOWP will list every declared loss. StackSpirit never calls the result an exact copy.";
}

export function asifLossPolicyOptions(
  policy: AsifLossPolicyChoice,
): { strict: true } | { allowLoss: true } {
  return policy === "strict" ? { strict: true } : { allowLoss: true };
}

/** A count mismatch is shown rather than silently trusting one of the two values. */
export function asifLossCountMismatch(count: number, losses: AsifLoss[]): boolean {
  return count !== losses.length;
}

const TRANSPORT_CODES = new Set([
  "remote",
  "remote_timeout",
  "ssh_auth",
  "ssh_unavailable",
]);

function isOversizedPackage(message: string): boolean {
  return /272\s*MiB|272\s*\*\s*1024|archive (?:cap|limit)|exceeds the 272/i.test(message);
}

function isSessionIdError(message: string): boolean {
  return /session id/i.test(message);
}

/**
 * Maps a bridge failure to fixed, honest ASIF copy. Exit numbers follow
 * `VOWP-CONTRACT.md` §11.2.4; local validation failures say they were local.
 */
export function asifErrorCopy(
  error: SpiritError | unknown,
  direction: AsifDirection,
): AsifErrorCopy & { code: string } {
  const normalized = toSpiritError(error);
  const detail = normalized.message;
  const directionLabel = asifDirectionLabel(direction);
  const base = { detail, code: normalized.code };

  switch (normalized.code) {
    case "refused":
      return {
        ...base,
        title: `${directionLabel} refused (exit 5)`,
        next:
          direction === "export"
            ? "Review the declared losses. Choose allow-loss explicitly if that policy is acceptable; StackSpirit has no force or allow-unsupported switch."
            : "Review the declared losses and choose allow-loss explicitly only if that policy is acceptable. StackSpirit has no force or allow-unsupported switch.",
      };
    case "conflict":
      return {
        ...base,
        title: `${directionLabel} conflict (exit 4)`,
        next:
          "StackSpirit did not retry with a changed package. Inspect the source session and choose a clean package.",
      };
    case "not_found":
      if (/no active project/i.test(detail)) {
        return {
          ...base,
          title: "No active project",
          next:
            "Activate a project in VOWP or refresh the snapshot, then choose the package again.",
        };
      }
      if (/no host is connected/i.test(detail)) {
        return {
          ...base,
          title: "No host is connected",
          next: "Connect to a VOWP home before retrying the ASIF action.",
        };
      }
      return {
        ...base,
        title: "Unknown source session (exit 3)",
        next:
          "Check the source session and the connected VOWP home, then choose the package again.",
      };
    case "invalid_input":
      if (isOversizedPackage(detail)) {
        return {
          ...base,
          title: `Package exceeds the ${ASIF_PACKAGE_LIMIT_LABEL} limit`,
          next: "No upload was attempted. Choose a package within the adapter limit.",
        };
      }
      if (isSessionIdError(detail)) {
        return {
          ...base,
          title: "Invalid session id",
          next:
            "Refresh the snapshot. This row did not carry a VOWP session id that the host accepts.",
        };
      }
      return {
        ...base,
        title: "Package rejected (exit 2)",
        next:
          "Nothing was imported. Choose a package exported by a compatible VOWP ASIF adapter.",
      };
    case "parse":
      return {
        ...base,
        title: "Package rejected (exit 2)",
        next:
          "VOWP or StackSpirit could not read the package. Nothing was imported; choose a compatible package.",
      };
    case "gate":
      return {
        ...base,
        title: "ASIF action unavailable",
        next:
          normalized.hint ??
          "Probe the VOWP home gates, then retry the action from the Sessions drawer.",
      };
    case "limit":
      return {
        ...base,
        title: "A limit blocked the ASIF action",
        next: normalized.hint ?? "Resolve the limit, then retry manually.",
      };
    case "auth":
      return {
        ...base,
        title: "Authentication failed",
        next: normalized.hint ?? "Check the VOWP authentication, then retry manually.",
      };
    case "vowp_missing":
      return {
        ...base,
        title: "VOWP is not installed on this host",
        next: normalized.hint ?? "Install the VOWP version that provides ASIF.",
      };
    case "vowp_incompatible":
      return {
        ...base,
        title: "Incompatible VOWP ASIF surface",
        next:
          normalized.hint ??
          "Update VOWP or StackSpirit. StackSpirit never guesses at a changed schema.",
      };
    case "vowp_command":
      return {
        ...base,
        title: "VOWP rejected the ASIF command",
        next: normalized.hint ?? "Read VOWP's message and retry manually.",
      };
    case "io":
      return {
        ...base,
        title: `Local file failure during ${direction}`,
        next:
          normalized.hint ??
          "The selected local file could not be read or written. Choose it again.",
      };
    default:
      if (TRANSPORT_CODES.has(normalized.code)) {
        return {
          ...base,
          title: "Transport failure (exit 7)",
          next:
            "Check the SSH connection, then retry manually. Nothing was imported or exported.",
        };
      }
      return {
        ...base,
        title: `ASIF ${direction} failed`,
        next: normalized.hint ?? "Nothing changed on the VOWP home. Retry manually.",
      };
  }
}

export function asifPackageLimitCopy(): string {
  return `The host checks the selected .asif.zip size before upload. The adapter limit is ${ASIF_PACKAGE_LIMIT_LABEL}.`;
}
