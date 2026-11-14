import { useId, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";

import { Button } from "./Button";
import { useDialogFocus } from "./useDialogFocus";

export interface ModalProps {
  open: boolean;
  title: string;
  children: ReactNode;
  footer?: ReactNode;
  onClose: () => void;
  danger?: boolean;
}

export function Modal({ open, title, children, footer, onClose, danger = false }: ModalProps) {
  const titleId = useId();
  const ref = useRef<HTMLDivElement>(null);
  useDialogFocus(open, onClose, ref);
  if (!open) return null;
  return createPortal(
    <div
      className="scrim"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        ref={ref}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className={danger ? "modal modal--danger" : "modal"}
        tabIndex={-1}
      >
        <header className="modal__header">
          <h2 id={titleId} className="modal__title">
            {title}
          </h2>
          <Button variant="ghost" size="sm" aria-label="Close dialog" onClick={onClose}>
            ✕
          </Button>
        </header>
        <div className="modal__body">{children}</div>
        {footer ? <footer className="modal__footer">{footer}</footer> : null}
      </div>
    </div>,
    document.body,
  );
}
