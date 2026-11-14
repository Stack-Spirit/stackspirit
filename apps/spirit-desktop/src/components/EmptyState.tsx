import type { ReactNode } from "react";

import { Button } from "./Button";

export interface EmptyStateProps {
  icon?: ReactNode;
  title: string;
  body: string;
  actionLabel?: string;
  onAction?: () => void;
  actionDisabled?: boolean;
}

export function EmptyState({
  icon = "◇",
  title,
  body,
  actionLabel,
  onAction,
  actionDisabled = false,
}: EmptyStateProps) {
  return (
    <div className="empty-state">
      <div className="empty-state__icon" aria-hidden="true">
        {icon}
      </div>
      <h3 className="empty-state__title">{title}</h3>
      <p className="empty-state__body">{body}</p>
      {actionLabel && onAction ? (
        <Button variant="primary" onClick={onAction} disabled={actionDisabled}>
          {actionLabel}
        </Button>
      ) : null}
    </div>
  );
}
