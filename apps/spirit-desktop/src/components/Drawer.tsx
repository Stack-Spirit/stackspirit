import { useId, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";

import { Button } from "./Button";
import { useDialogFocus } from "./useDialogFocus";

export interface DrawerProps {
  open: boolean;
  title: string;
  children: ReactNode;
  footer?: ReactNode;
  onClose: () => void;
}

export function Drawer({ open, title, children, footer, onClose }: DrawerProps) {
  const titleId = useId();
  const ref = useRef<HTMLDivElement>(null);
  useDialogFocus(open, onClose, ref);
  if (!open) return null;
  return createPortal(
    <div
      className="scrim scrim--drawer"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        ref={ref}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className="drawer"
        tabIndex={-1}
      >
        <header className="drawer__header">
          <h2 id={titleId} className="drawer__title">
            {title}
          </h2>
          <Button variant="ghost" size="sm" aria-label="Close drawer" onClick={onClose}>
            ✕
          </Button>
        </header>
        <div className="drawer__body">{children}</div>
        {footer ? <footer className="drawer__footer">{footer}</footer> : null}
      </div>
    </div>,
    document.body,
  );
}
