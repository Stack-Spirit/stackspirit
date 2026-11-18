import type { ReactNode } from "react";

import { useShellUi } from "./shellUi";

/** The button that slides the details panel in from the right, or closes it. */
export function PanelToggle() {
  const open = useShellUi((state) => state.panel !== null);
  const toggle = useShellUi((state) => state.togglePanel);
  return (
    <button
      type="button"
      className="nx-icon-btn nx-panel-toggle"
      aria-label={open ? "Hide details panel" : "Show details panel"}
      aria-pressed={open}
      title={open ? "Hide details" : "Show details"}
      onClick={toggle}
      data-testid="panel-toggle"
    >
      <svg width="18" height="18" viewBox="0 0 18 18" aria-hidden="true">
        <rect x="2" y="3" width="14" height="12" rx="2.5" fill="none" stroke="currentColor" strokeWidth="1.5" />
        <rect x="10.5" y="3.75" width="4.75" height="10.5" rx="1.5" fill="currentColor" opacity={open ? 0.9 : 0.25} />
      </svg>
    </button>
  );
}

export function PageHeader({
  title,
  sub,
  children,
  actions,
}: {
  title: string;
  sub?: string;
  /** Chips next to the title. */
  children?: ReactNode;
  /** Buttons before the panel toggle. */
  actions?: ReactNode;
}) {
  return (
    <header className="nx-chat-head">
      <div className="nx-chat-head__titles">
        <h1 className="nx-chat-head__title">{title}</h1>
        {sub ? <span className="nx-chat-head__sub">{sub}</span> : null}
      </div>
      {children}
      <span className="nx-chat-head__spacer" />
      {actions}
      <PanelToggle />
    </header>
  );
}
