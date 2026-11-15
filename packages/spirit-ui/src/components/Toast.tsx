import type { ReactNode } from "react";

import { Button } from "./Button";
import { cx } from "./cx";

export interface ToastProps {
  tone?: "ok" | "info" | "warn" | "danger";
  title: string;
  children?: ReactNode;
  onDismiss?: () => void;
}

export function Toast({ tone = "info", title, children, onDismiss }: ToastProps) {
  return (
    <div
      className={cx("toast", `toast--${tone}`)}
      role={tone === "danger" ? "alert" : "status"}
      aria-live={tone === "danger" ? "assertive" : "polite"}
    >
      <div className="toast__body">
        <strong className="toast__title">{title}</strong>
        {children ? <div className="toast__message">{children}</div> : null}
      </div>
      {onDismiss ? (
        <Button variant="ghost" size="sm" aria-label="Dismiss notification" onClick={onDismiss}>
          ✕
        </Button>
      ) : null}
    </div>
  );
}

export function ToastViewport({ children }: { children: ReactNode }) {
  return (
    <div className="toast-viewport" role="region" aria-label="Notifications">
      {children}
    </div>
  );
}
