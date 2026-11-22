import type { ExecutorView } from "../bridge/wire";

const KNOWN_HARNESSES = new Set(["claude-code", "opencode", "cli", "acp", "vowp"]);

const FRIENDLY_MODELS: Record<string, string> = {
  "deepseek/deepseek-flash": "DeepSeek V4.1 Flash",
  "deepseek/deepseek-chat": "DeepSeek Chat",
  "anthropic/claude-sonnet-4": "Claude Sonnet 4",
  "anthropic/claude-opus-4": "Claude Opus 4",
  "openai/gpt-5": "GPT-5",
  "openai/gpt-5-mini": "GPT-5 mini",
  "claude-sonnet-5-5": "Claude Sonnet 5.5",
  "claude-opus-5-5": "Claude Opus 5.5",
  "claude-haiku-5-5": "Claude Haiku 5.5",
  "claude-fable-5-1": "Claude Fable 5.1",
};

const PROVIDER_LABELS: Record<string, string> = {
  claude: "Anthropic",
  anthropic: "Anthropic",
  deepseek: "DeepSeek",
  openrouter: "OpenRouter",
  openai: "OpenAI",
  google: "Google",
  gemini: "Google",
  opencode: "OpenCode",
};

const HARNESS_LABELS: Record<string, string> = {
  "claude-code": "Claude Code",
  opencode: "OpenCode",
  cli: "CLI",
  acp: "ACP",
};

/** Mirrors `spirit-core::view::executor::provider_label`. */
export function executorProviderLabel(provider: string): string {
  if (!provider) return "";
  return PROVIDER_LABELS[provider.toLowerCase()] ?? provider.charAt(0).toUpperCase() + provider.slice(1);
}

/** Mirrors `spirit-core::view::executor::harness_label`. */
export function harnessLabel(harness: string): string {
  return HARNESS_LABELS[harness] ?? harness;
}

function splitModifier(
  input: string,
  needle: string,
): { base: string; value: string | null } | null {
  const index = input.indexOf(needle);
  if (index === -1) return { base: input, value: null };
  const base = input.slice(0, index);
  const value = input.slice(index + needle.length);
  if (value.includes(needle)) return null;
  return { base, value };
}

function isModifier(input: string): boolean {
  return /^[a-z0-9_-]+$/.test(input);
}

function unknownExecutor(raw: string): ExecutorView {
  return {
    raw,
    harness: "unknown",
    provider: "",
    model: "",
    effort: null,
    variant: null,
    display: raw,
  };
}

/**
 * Mirrors `spirit-core::view::executor` and the canonical
 * `harness:model[@effort][#variant]` grammar. A malformed value is never normalized.
 */
export function parseExecutor(raw: string): ExecutorView {
  const colon = raw.indexOf(":");
  if (colon === -1) return unknownExecutor(raw);
  const harness = raw.slice(0, colon);
  if (!KNOWN_HARNESSES.has(harness)) return unknownExecutor(raw);
  const rest = raw.slice(colon + 1);

  const withoutVariant = splitModifier(rest, "#");
  if (!withoutVariant) return unknownExecutor(raw);
  const withoutEffort = splitModifier(withoutVariant.base, "@");
  if (!withoutEffort) return unknownExecutor(raw);
  const model = withoutEffort.base;
  if (!model || !/^[A-Za-z0-9._:/-]+$/.test(model)) return unknownExecutor(raw);

  const effort = withoutEffort.value;
  const variant = withoutVariant.value;
  if (effort !== null && (!isModifier(effort) || harness !== "claude-code")) {
    return unknownExecutor(raw);
  }
  if (variant !== null && (!isModifier(variant) || harness !== "opencode")) {
    return unknownExecutor(raw);
  }

  const provider =
    harness === "claude-code"
      ? "claude"
      : harness === "opencode"
        ? (model.split("/")[0] ?? model)
        : model;
  const friendly = friendlyModelName(model);
  const extras = [variant, effort].filter((value): value is string => Boolean(value));
  return {
    raw,
    harness,
    provider,
    model,
    effort,
    variant,
    display: extras.length > 0 ? `${friendly} · ${extras.join(" · ")}` : friendly,
  };
}

export function friendlyModelName(model: string): string {
  return FRIENDLY_MODELS[model] ?? model;
}

export function executorLabel(executor: ExecutorView | null): string {
  if (!executor) return "shell";
  if (executor.harness === "unknown") return executor.raw;
  return `${executor.harness} · ${executor.display}`;
}

export interface ExecutorBadges {
  harness: string;
  provider: string | null;
  model: string | null;
  modifier: string | null;
  raw: string;
  unknown: boolean;
  shell: boolean;
}

/** Splits an executor into the worker-card badge fields. */
export function executorBadges(executor: ExecutorView | null): ExecutorBadges {
  if (!executor) {
    return {
      harness: "shell",
      provider: null,
      model: null,
      modifier: null,
      raw: "",
      unknown: false,
      shell: true,
    };
  }
  if (executor.harness === "unknown") {
    return {
      harness: "Unknown harness",
      provider: null,
      model: executor.raw,
      modifier: null,
      raw: executor.raw,
      unknown: true,
      shell: false,
    };
  }
  const modifier = executor.variant ?? executor.effort ?? null;
  return {
    harness: executor.harness,
    provider: executor.provider || null,
    model: friendlyModelName(executor.model),
    modifier,
    raw: executor.raw,
    unknown: false,
    shell: false,
  };
}
