const GIB = 1024 ** 3;

export function formatBytes(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined) return "Unknown";
  if (bytes === 0) return "0 B";
  if (bytes < GIB) return `${Math.round(bytes / 1024 ** 2)} MiB`;
  return `${(bytes / GIB).toFixed(1)} GiB`;
}

/** Binary GiB with one decimal, as required by the capacity rail. */
export function formatGiB(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined) return "Unknown";
  return `${(bytes / GIB).toFixed(1)} GiB`;
}

/** Exact byte count for a tooltip. Never used as the primary value. */
export function formatExactBytes(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined) return "Unknown";
  return `${Math.round(bytes).toLocaleString("en-US")} bytes`;
}

export function formatAge(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined) return "unknown";
  if (seconds < 60) return `${Math.floor(seconds)}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  return `${hours}h ${minutes}m`;
}

/** Compact age with both units, for example `2m 14s` or `3h 02m`. */
export function formatAgeParts(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined) return "unknown";
  const total = Math.max(0, Math.floor(seconds));
  if (total < 60) return `${total}s`;
  if (total < 3600) {
    const minutes = Math.floor(total / 60);
    const remainder = total % 60;
    return remainder === 0 ? `${minutes}m` : `${minutes}m ${remainder}s`;
  }
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  return minutes === 0 ? `${hours}h` : `${hours}h ${String(minutes).padStart(2, "0")}m`;
}

/** Last-event copy: live under 30 s, quiet afterwards, stalled when flagged. */
export function eventAgeLabel(
  seconds: number | null | undefined,
  stalled = false,
): string {
  if (stalled) return "stalled";
  if (seconds === null || seconds === undefined) return "no events";
  if (seconds < 30) return "live";
  return `quiet ${formatAgeParts(seconds)}`;
}

export function formatClock(timestamp: string | null | undefined): string {
  if (!timestamp) return "never";
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) return "unknown";
  return date.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

export function formatDateTime(timestamp: string | null | undefined): string {
  if (!timestamp) return "never";
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) return "unknown";
  return date.toLocaleString();
}

export function formatPercent(value: number): string {
  return `${value.toFixed(0)}%`;
}

export function formatPercentOne(value: number): string {
  return `${value.toFixed(1)}%`;
}

export function shortId(id: string, length = 8): string {
  return id.length <= length ? id : id.slice(0, length);
}

export function latencyLabel(last: number | null | undefined): string {
  return last === null || last === undefined ? "—" : `${Math.round(last)} ms`;
}

/** Truncates to `limit` characters, appending an ellipsis when cut. */
export function truncateText(text: string, limit = 200): string {
  const chars = Array.from(text);
  if (chars.length <= limit) return text;
  return `${chars.slice(0, limit).join("")}…`;
}

export function formatCount(value: number): string {
  return value.toLocaleString("en-US");
}
