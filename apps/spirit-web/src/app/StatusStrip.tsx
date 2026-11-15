import { useEffect, useState } from "react";

import { Badge } from "@stackspirit/ui";
import { formatClock, latencyLabel, type ConnectionSnapshot, type WebBridgeStatus, type WebFleetView } from "@stackspirit/ui";

function Countdown({ retryAt }: { retryAt: number }) {
  const [remaining, setRemaining] = useState<number | null>(null);
  useEffect(() => {
    const update = () => setRemaining(Math.max(0, Math.ceil((retryAt - Date.now()) / 1000)));
    update();
    const timer = window.setInterval(update, 1000);
    return () => window.clearInterval(timer);
  }, [retryAt]);
  if (remaining === null) return null;
  return (
    <span className="status-strip__item status-strip__item--warn" data-testid="web-retry-countdown">
      {remaining > 0 ? `retrying in ${remaining}s` : "retrying now"}
    </span>
  );
}

export function WebStatusStrip({
  snapshot,
  connection,
  status,
}: {
  snapshot: WebFleetView | null;
  connection: ConnectionSnapshot;
  status: WebBridgeStatus | null;
}) {
  const stale = snapshot?.stale === true || connection.state === "stale";
  return (
    <footer className="status-strip" data-testid="web-status-strip">
      <span
        className={`status-dot status-strip__dot status-dot--${
          stale ? "warn" : connection.state === "connected" ? "ok" : "unknown"
        }`}
        aria-hidden="true"
      />
      <span className="status-strip__item" aria-live="polite">
        {connection.state === "connected"
          ? stale
            ? "Connected · no current data"
            : "Connected"
          : connection.state === "reconnecting"
            ? "Reconnecting · polling fallback"
            : connection.state === "failed"
              ? "Connection failed"
              : connection.state}
      </span>
      {snapshot ? (
        <>
          <span className="status-strip__item">
            source <span className="mono">{snapshot.source === "daemon" ? "live from daemon" : "preview"}</span>
          </span>
          <span className="status-strip__item">
            received <span className="mono">{formatClock(snapshot.fetched_at)}</span>
          </span>
          <span className="status-strip__item">
            snapshot changed at <span className="mono">{formatClock(snapshot.generated_at)}</span>
          </span>
          <span className="status-strip__item">
            daemon RTT <span className="mono">{latencyLabel(snapshot.latency.last_ms)}</span>
          </span>
        </>
      ) : (
        <span className="status-strip__item">no snapshot yet</span>
      )}

      {status ? (
        <span className="status-strip__item">
          stream <span className="mono">{status.stream_state}</span>
        </span>
      ) : null}
      {status?.polling_only ? (
        <span className="status-strip__item status-strip__item--warn" data-testid="polling-only">
          polling only
        </span>
      ) : null}
      {stale && status?.next_retry_at ? <Countdown retryAt={status.next_retry_at} /> : null}

      <span className="status-strip__spacer" />
      <Badge tone="unknown" uppercase>
        read-only
      </Badge>
      <span className="muted small">
        Job and inbox actions require the local write proxy.
      </span>
    </footer>
  );
}
