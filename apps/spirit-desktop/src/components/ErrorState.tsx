import { useState } from "react";

import { errorTitle, type SpiritError } from "../bridge/errors";
import { Button } from "./Button";

export interface ErrorStateProps {
  error: SpiritError;
  onRetry?: () => void;
  retryLabel?: string;
}

export function ErrorState({ error, onRetry, retryLabel = "Retry" }: ErrorStateProps) {
  const [open, setOpen] = useState(false);
  return (
    <div className="error-state" role="alert">
      <div className="error-state__icon" aria-hidden="true">
        ⚠
      </div>
      <div className="error-state__content">
        <h3 className="error-state__title">{errorTitle(error)}</h3>
        <p className="error-state__body">{error.message}</p>
        {error.hint ? <p className="error-state__hint">{error.hint}</p> : null}
        <div className="error-state__actions">
          {onRetry ? (
            <Button variant="primary" size="sm" onClick={onRetry}>
              {retryLabel}
            </Button>
          ) : null}
          <Button variant="ghost" size="sm" onClick={() => setOpen((value) => !value)}>
            {open ? "Hide details" : "Details"}
          </Button>
        </div>
        {open ? (
          <pre className="error-state__details">
            {error.code}
            {"\n"}
            {error.message}
          </pre>
        ) : null}
      </div>
    </div>
  );
}
