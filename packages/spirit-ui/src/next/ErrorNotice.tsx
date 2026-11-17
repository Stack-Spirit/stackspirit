import { useEffect, useState } from "react";

import { useFleetStore, useLastError, useNextRetryAt } from "../state/fleetStore";

/**
 * The last bridge error, shown as a slim bar above the main view. It replaces the
 * classic status strip: message, hint, the automatic retry countdown, Retry and
 * Dismiss. Nothing is shown while things are fine.
 */
export function ErrorNotice() {
  const error = useLastError();
  const nextRetryAt = useNextRetryAt();
  const clearError = useFleetStore((state) => state.clearError);
  const refresh = useFleetStore((state) => state.refresh);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!nextRetryAt) return;
    const handle = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(handle);
  }, [nextRetryAt]);

  if (!error) return null;
  const seconds = nextRetryAt ? Math.max(0, Math.ceil((nextRetryAt - now) / 1000)) : null;

  return (
    <div className="nx-error" role="alert" data-testid="error-notice">
      <span className="nx-dot nx-dot--danger" aria-hidden="true" />
      <span className="nx-error__text">
        <strong>{error.message}</strong>
        {error.hint ? <span>{error.hint}</span> : null}
      </span>
      {seconds != null ? <span className="nx-error__retry">Retrying in {seconds}s</span> : null}
      {error.retryable ? (
        <button type="button" className="nx-btn nx-btn--quiet nx-btn--sm" onClick={() => void refresh()}>
          Retry now
        </button>
      ) : null}
      <button type="button" className="nx-icon-btn nx-icon-btn--sm" aria-label="Dismiss error" onClick={clearError}>
        ✕
      </button>
    </div>
  );
}
