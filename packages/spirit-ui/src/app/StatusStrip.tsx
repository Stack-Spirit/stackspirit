import { useEffect, useState } from "react";

import { Button } from "../components";
import { errorTitle } from "../bridge/errors";
import { formatClock } from "../domain/format";
import {
  useConnection,
  useFleet,
  useFleetStore,
  useLastError,
  useNextRetryAt,
} from "../state/fleetStore";

/** Ticks the stale-backoff countdown once per second without touching the store. */
export function BackoffCountdown({ retryAt }: { retryAt: number }) {
  const [remaining, setRemaining] = useState<number | null>(null);
  useEffect(() => {
    const update = () =>
      setRemaining(Math.max(0, Math.ceil((retryAt - Date.now()) / 1000)));
    const first = window.setTimeout(update, 0);
    const timer = window.setInterval(update, 1000);
    return () => {
      window.clearTimeout(first);
      window.clearInterval(timer);
    };
  }, [retryAt]);
  if (remaining === null) return null;
  return (
    <span className="status-strip__item status-strip__item--warn" data-testid="backoff-countdown">
      {remaining > 0 ? `retrying in ${remaining}s` : "retrying now"}
    </span>
  );
}

export function StatusStrip({ onOpenRaw }: { onOpenRaw: () => void }) {
  const connection = useConnection();
  const fleet = useFleet();
  const lastError = useLastError();
  const nextRetryAt = useNextRetryAt();
  const clearError = useFleetStore((state) => state.clearError);
  const [expanded, setExpanded] = useState(false);

  return (
    <footer className="status-strip" data-testid="status-strip">
      <span className={`status-dot status-strip__dot status-dot--${connection.state === "connected" ? (fleet?.stale ? "warn" : "ok") : "unknown"}`} aria-hidden="true" />
      <span className="status-strip__item">
        {connection.state === "connected"
          ? fleet?.stale
            ? "Connected · stale data"
            : "Connected"
          : connection.state === "failed"
            ? "Connection failed"
            : connection.state}
      </span>
      {fleet ? (
        <>
          <span className="status-strip__item">
            source <span className="mono">{fleet.source}</span>
          </span>
          <span className="status-strip__item">
            generated <span className="mono">{formatClock(fleet.generated_at)}</span>
          </span>
          <span className="status-strip__item">
            fetched <span className="mono">{formatClock(fleet.fetched_at)}</span>
          </span>
          {fleet.stale ? (
            <span className="status-strip__item status-strip__item--warn">
              stale · writes need a refresh
            </span>
          ) : null}
        </>
      ) : (
        <span className="status-strip__item">no snapshot yet</span>
      )}

      {connection.state === "stale" && nextRetryAt !== null ? (
        <BackoffCountdown retryAt={nextRetryAt} />
      ) : null}

      <span className="status-strip__spacer" />

      {lastError ? (
        <>
          <span className="status-strip__item status-strip__item--danger" title={lastError.message}>
            {lastError.code}: {lastError.message}
          </span>
          <Button variant="ghost" size="sm" onClick={() => setExpanded((value) => !value)}>
            {expanded ? "Hide error" : "Details"}
          </Button>
          <Button variant="ghost" size="sm" onClick={clearError}>
            Dismiss
          </Button>
        </>
      ) : (
        <span className="status-strip__item muted">no errors</span>
      )}

      <Button variant="ghost" size="sm" onClick={onOpenRaw} disabled={!fleet}>
        Raw document
      </Button>

      {expanded && lastError ? (
        <div className="status-strip__details" role="region" aria-label="Last error details">
          <strong>{errorTitle(lastError)}</strong>
          <p className="mono">{lastError.message}</p>
          {lastError.hint ? <p className="muted">{lastError.hint}</p> : null}
          <p className="muted small">
            {lastError.code} · {lastError.retryable ? "retryable" : "not retryable"}
          </p>
        </div>
      ) : null}
    </footer>
  );
}
