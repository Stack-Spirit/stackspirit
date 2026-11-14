import type { ExecutorView } from "../bridge/wire";

const FRIENDLY_MODELS: Record<string, string> = {
  "deepseek/deepseek-flash": "DeepSeek V4.1 Flash",
  "openai/gpt-5-mini": "GPT-5 mini",
};

/**
 * Mirrors `spirit-core::view::executor` for display only. A malformed executor is
 * never normalized; the exact VOWP string is retained.
 */
export function parseExecutor(raw: string): ExecutorView {
  const match = /^([^:@#]+):([^@#]+)(?:@([^#]+))?(?:#(.+))?$/.exec(raw);
  if (!match || !match[1] || !match[2]) {
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
  const harness = match[1];
  const model = match[2];
  const effort = match[3] ?? null;
  const variant = match[4] ?? null;
  const provider = harness === "claude-code" ? "claude" : (model.split("/")[0] ?? model);
  const friendly = FRIENDLY_MODELS[model] ?? model;
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

export function executorLabel(executor: ExecutorView | null): string {
  if (!executor) return "shell";
  if (executor.harness === "unknown") return executor.raw;
  return `${executor.harness} · ${executor.display}`;
}
