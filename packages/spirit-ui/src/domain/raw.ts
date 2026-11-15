import type { RawDocument, WireWarning } from "../bridge/wire";

const SECRET_KEY =
  /^(api[_-]?key|apikey|private[_-]?key|secret|client[_-]?secret|password|passphrase|token|access[_-]?token|refresh[_-]?token|authorization|credential)s?$/i;

const SECRET_VALUE =
  /(-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----|\b(?:sk|pk)_[A-Za-z0-9]{8,}\b|\bgh[pousr]_[A-Za-z0-9]{8,}\b|\bxox[baprs]-[A-Za-z0-9-]{8,}\b|\bAKIA[0-9A-Z]{12,}\b)/;

export const REDACTED = "[redacted]";

/**
 * Redacts secret-shaped keys and values from parsed documents. The drawer only ever
 * shows parsed fields, never raw command lines or stderr.
 */
export function redactParsed(value: unknown, depth = 0): unknown {
  if (depth > 20) return "[depth limit]";
  if (value === null || value === undefined) return value;
  if (typeof value === "string") return SECRET_VALUE.test(value) ? REDACTED : value;
  if (typeof value === "number" || typeof value === "boolean") return value;
  if (Array.isArray(value)) return value.map((entry) => redactParsed(entry, depth + 1));
  if (typeof value === "object") {
    const output: Record<string, unknown> = {};
    for (const [key, entry] of Object.entries(value as Record<string, unknown>)) {
      output[key] = SECRET_KEY.test(key) ? REDACTED : redactParsed(entry, depth + 1);
    }
    return output;
  }
  return String(value);
}

export function prettyParsed(value: unknown): string {
  if (value === null || value === undefined) return "No parsed document.";
  return JSON.stringify(redactParsed(value), null, 2) ?? "No parsed document.";
}

export function allWarnings(raw: RawDocument | null): WireWarning[] {
  if (!raw) return [];
  return [...raw.status_warnings, ...raw.capacity_warnings];
}

export type WarningTone = "warn" | "info" | "danger";

export function warningTone(code: WireWarning["code"]): WarningTone {
  switch (code) {
    case "schema":
      return "danger";
    case "unknown_enum":
    case "missing_optional":
      return "warn";
    default:
      return "info";
  }
}

export function warningCountLabel(count: number): string {
  return `${count} parse warning${count === 1 ? "" : "s"}`;
}
