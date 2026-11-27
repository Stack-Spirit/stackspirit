import { useEffect, useId, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";

import { useDialogFocus } from "../components/useDialogFocus";

export interface SettingsModalProps {
  open: boolean;
  onClose: () => void;
  children: ReactNode;
}

/**
 * The centered Settings popup used by the Meadow shell.
 *
 * It is deliberately a modal dialog rather than a main view: the shell stays where it
 * was, the conversation keeps its scroll position, and Esc or a backdrop click returns
 * to it. `useDialogFocus` owns Esc, the Tab loop and focus restoration to the control
 * that opened Settings. The blurred backdrop is a Meadow token, so light and dark mode
 * both stay legible.
 */
export function SettingsModal({ open, onClose, children }: SettingsModalProps) {
  const titleId = useId();
  const hintId = useId();
  const dialogRef = useRef<HTMLDivElement>(null);
  useDialogFocus(open, onClose, dialogRef);

  useEffect(() => {
    if (!open) return;
    const previous = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      document.body.style.overflow = previous;
    };
  }, [open]);

  if (!open) return null;

  return createPortal(
    <div
      className="nx-settings-backdrop"
      data-testid="settings-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={hintId}
        className="nx-settings-modal"
        data-testid="settings-modal"
        tabIndex={-1}
      >
        <header className="nx-settings-modal__head">
          <div>
            <h2 id={titleId} className="nx-settings-modal__title">
              Settings
            </h2>
            <p id={hintId} className="nx-settings-modal__hint">
              The app, your account, the Home machine and every harness in one place.
            </p>
          </div>
          <button
            type="button"
            className="nx-icon-btn nx-settings-modal__close"
            aria-label="Close settings"
            title="Close settings"
            onClick={onClose}
            data-testid="settings-close"
          >
            <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
              <path
                d="m4 4 8 8M12 4l-8 8"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.5"
                strokeLinecap="round"
              />
            </svg>
          </button>
        </header>
        <div className="nx-settings-modal__body">{children}</div>
      </div>
    </div>,
    document.body,
  );
}
