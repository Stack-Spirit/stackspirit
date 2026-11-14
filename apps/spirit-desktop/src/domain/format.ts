const GIB = 1024 ** 3;

export function formatBytes(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined) return "Unknown";
  if (bytes === 0) return "0 B";
  if (bytes < GIB) return `${Math.round(bytes / 1024 ** 2)} MiB`;
  return `${(bytes / GIB).toFixed(1)} GiB`;
}

export function formatAge(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined) return "unknown";
  if (seconds < 60) return `${Math.floor(seconds)}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  return `${hours}h ${minutes}m`;
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

export function shortId(id: string, length = 8): string {
  return id.length <= length ? id : id.slice(0, length);
}

export function latencyLabel(last: number | null | undefined): string {
  return last === null || last === undefined ? "—" : `${Math.round(last)} ms`;
}
