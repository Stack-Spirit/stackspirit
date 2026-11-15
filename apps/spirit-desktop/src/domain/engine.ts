import type {
  EngineStatus,
  ExternalHarness,
  GateSnapshot,
  KeyStatus,
  LocalAgentStatus,
  ModelChoice,
} from "../bridge/wire";

/** The Phase 3 capability set named by VOWP-CONTRACT.md §11.3. */
export const PHASE3_MILESTONES = [
  "local opencode app host",
  "surface bridge",
  "MCP proxy",
  "orchestrator materialization",
  "backfill",
  "local cache",
] as const;

/** The VOWP milestones the ASIF gate needs. */
export const ASIF_MILESTONES = ["v4-asif-core", "v4-asif-cli"] as const;

export const NOT_IMPLEMENTED_COPY = "Not implemented in this build.";
export const GATE_NOT_IMPLEMENTED_LOCAL =
  "VOWP is ready; local orchestrator execution is not implemented in this build.";
export const GATE_NOT_IMPLEMENTED_ASIF =
  "VOWP is ready; ASIF export and import are not implemented in this build.";

const PROVIDER_LABELS: Record<string, string> = {
  anthropic: "Anthropic",
  deepseek: "DeepSeek",
  google: "Google",
  groq: "Groq",
  mistral: "Mistral",
  ollama: "Ollama",
  openai: "OpenAI",
  opencode: "OpenCode",
  "opencode-go": "OpenCode Go",
  openrouter: "OpenRouter",
  xai: "xAI",
};

/** Providers that belong to the bundled engine's implementation details. */
const ENGINE_INTERNAL_PROVIDERS = new Set(["opencode", "opencode-go"]);

/** Display name with a safe fallback to the provider id. */
export function providerLabel(provider: string): string {
  const trimmed = provider.trim();
  if (!trimmed) return "Unknown provider";
  const known = PROVIDER_LABELS[trimmed.toLowerCase()];
  if (known) return known;
  const words = trimmed.replaceAll(/[_-]+/g, " ").split(/\s+/).filter(Boolean);
  return words
    .map((word) => `${word.charAt(0).toUpperCase()}${word.slice(1)}`)
    .join(" ") || trimmed;
}

/**
 * The provider shown on an included-model row. The bundled engine's own provider id
 * is an implementation detail (PLAN §8.5.1), so it is presented as StackSpirit Engine.
 */
export function modelProviderLabel(provider: string): string {
  return ENGINE_INTERNAL_PROVIDERS.has(provider.trim().toLowerCase())
    ? "StackSpirit Engine"
    : providerLabel(provider);
}

/** Keeps bundled engine implementation branding out of included-model notes. */
export function modelNotes(provider: string, notes: string): string {
  if (!ENGINE_INTERNAL_PROVIDERS.has(provider.trim().toLowerCase())) return notes;
  return notes.replace(/\bopencode\b/gi, "StackSpirit Engine");
}

/** A small provider catalog for BYOK. Unknown providers fall back to their id. */
export function providerOptions(
  models: ModelChoice[],
  keyStatuses: KeyStatus[] = [],
): Array<{ id: string; label: string }> {
  const ids = new Set<string>([
    "deepseek",
    "openai",
    "anthropic",
    "google",
    "openrouter",
    "xai",
    "ollama",
  ]);
  for (const model of models) {
    if (model.provider !== "opencode" && model.provider !== "opencode-go") {
      ids.add(model.provider);
    }
  }
  for (const status of keyStatuses) {
    if (status.state !== "missing") ids.add(status.provider);
  }
  return [...ids].map((id) => ({ id, label: providerLabel(id) }));
}

export function selectedModelLabel(status: EngineStatus | null): string {
  if (!status?.selected_model) return "No model selected";
  return status.selected_model.label || status.selected_model.id;
}

/**
 * The bounded, redacted log tail returned with `engine_status`. It is not part of the
 * D2 bridge interface, so it is read structurally and capped again at the UI boundary.
 */
export function engineLogs(status: EngineStatus | null): string[] {
  if (!status) return [];
  const raw = (status as { logs?: unknown }).logs;
  if (!Array.isArray(raw)) return [];
  return raw
    .filter((line): line is string => typeof line === "string")
    .slice(-200);
}

export function modelContextLabel(contextTokens: number | null): string {
  if (contextTokens === null) return "Context window unknown";
  if (contextTokens >= 1_000_000) {
    return `${(contextTokens / 1_000_000).toFixed(1)}M context`;
  }
  return `${Math.round(contextTokens / 1000)}K context`;
}

export function includedModels(models: ModelChoice[]): ModelChoice[] {
  return models.filter((model) => model.access === "included");
}

export function modelAvailable(
  models: ModelChoice[],
  modelId: string | null | undefined,
): boolean {
  return Boolean(modelId) && models.some((model) => model.id === modelId);
}

/**
 * The saved local first-run flag returned with `engine_status`. It is not part of the
 * D2 bridge interface, so it is read structurally from the status object.
 */
export function engineSetupComplete(status: EngineStatus | null): boolean {
  if (!status) return false;
  return (status as { onboarding_complete?: unknown }).onboarding_complete === true;
}

/** True when the first-run flow should own the Engine view. */
export function needsEngineOnboarding(
  status: EngineStatus | null,
  onboardingComplete: boolean,
): boolean {
  if (!status || onboardingComplete || engineSetupComplete(status)) return false;
  if (status.choice.type === "external") return false;
  if (status.state.state === "not_configured") return true;
  return status.selected_model === null;
}

/** A default, never-probed snapshot must not suppress the automatic probe. */
export function gatesProbed(gates: GateSnapshot | null): boolean {
  if (!gates) return false;
  const probedAt = Date.parse(gates.probed_at);
  return Number.isFinite(probedAt) && probedAt > 0;
}

export function externalHarnessLabel(harness: ExternalHarness): string {
  return harness.kind === "claude-code" ? "Claude Code" : "opencode";
}

export function localAgentLabel(agent: LocalAgentStatus): string {
  switch (agent.state) {
    case "connected":
      return `Connected${agent.node ? ` · ${agent.node}` : ""}`;
    case "not_installed":
      return "Not installed on this Mac";
    case "not_enrolled":
      return "Not enrolled";
    case "connecting":
      return "Connecting";
    case "reconnecting":
      return "Reconnecting";
    case "revoked":
      return "Revoked";
    case "superseded":
      return "Superseded";
    case "stopped":
      return "Stopped";
    case "incompatible":
      return "Incompatible protocol";
    default:
      return "Unknown";
  }
}

export type GateAction = "local_orchestrator" | "asif_export" | "asif_import";

export function gateActionEnabled(
  gates: GateSnapshot | null,
  action: GateAction,
): boolean {
  if (!gates) return false;
  if (action === "local_orchestrator") {
    return (
      gates.features.local_orchestrator_actions &&
      gates.local_orchestrator.state === "ready" &&
      gates.local_agent.state === "connected"
    );
  }
  if (!gates.features.asif_actions || gates.asif.state !== "ready") return false;
  return action === "asif_export" ? gates.asif.export : gates.asif.import;
}

/** The visible reason next to a disabled action. Never a bare tooltip. */
export function gateActionReason(
  gates: GateSnapshot | null,
  action: GateAction,
): string {
  if (!gates) return "Probe the VOWP home to check this capability.";
  if (action === "local_orchestrator") {
    const gate = gates.local_orchestrator;
    if (gate.state === "waiting_for_vowp_phase3") {
      return `Waiting for VOWP Phase 2/3. Missing: ${gate.missing.join(" · ")}`;
    }
    if (gate.state === "incompatible") {
      return `Incompatible VOWP node surface: ${gate.detected} (required ${gate.required}).`;
    }
    if (gate.state === "unreachable") {
      return `The VOWP home could not be reached: ${gate.detail}`;
    }
    if (gate.state === "probing") return "Probing the VOWP home.";
    if (gate.state === "unknown") {
      return "Probe the VOWP home to check local orchestrator support.";
    }
    if (!gates.features.local_orchestrator_actions) {
      return GATE_NOT_IMPLEMENTED_LOCAL;
    }
    if (gates.local_agent.state !== "connected") {
      return "The local VOWP agent is not connected. Enroll it on this Mac first.";
    }
    return "";
  }

  const gate = gates.asif;
  if (gate.state === "waiting_for_vowp_asif") {
    return `Waiting for VOWP ASIF. Missing: ${gate.missing.join(" · ")}`;
  }
  if (gate.state === "incompatible") {
    return `Incompatible ASIF surface: ${gate.detected} (required ${gate.required}).`;
  }
  if (gate.state === "unreachable") {
    return `The VOWP home could not be reached: ${gate.detail}`;
  }
  if (gate.state === "probing") return "Probing the VOWP home.";
  if (gate.state === "unknown") {
    return "Probe the VOWP home to check ASIF support.";
  }
  if (!gates.features.asif_actions) return GATE_NOT_IMPLEMENTED_ASIF;
  const proven = action === "asif_export" ? gate.export : gate.import;
  if (!proven) {
    return action === "asif_export"
      ? "ASIF export capability is not proven."
      : "ASIF import capability is not proven.";
  }
  return "";
}

export function gateMissingMilestones(gates: GateSnapshot | null): {
  local: readonly string[];
  asif: readonly string[];
} {
  const probedLocal =
    gates?.local_orchestrator.state === "waiting_for_vowp_phase3"
      ? gates.local_orchestrator.missing
      : [];
  const probedAsif =
    gates?.asif.state === "waiting_for_vowp_asif" ? gates.asif.missing : [];
  return {
    local: [...new Set([...PHASE3_MILESTONES, ...probedLocal])],
    asif: [...new Set([...ASIF_MILESTONES, ...probedAsif])],
  };
}
