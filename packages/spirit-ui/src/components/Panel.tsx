import type { HTMLAttributes, ReactNode } from "react";

import { cx } from "./cx";

export interface PanelProps extends Omit<HTMLAttributes<HTMLElement>, "title"> {
  title?: ReactNode;
  actions?: ReactNode;
  tone?: "default" | "warn" | "danger";
  children: ReactNode;
}

export function Panel({
  title,
  actions,
  tone = "default",
  className,
  children,
  ...rest
}: PanelProps) {
  return (
    <section className={cx("panel", `panel--${tone}`, className)} {...rest}>
      {(title || actions) && (
        <header className="panel__header">
          {title ? <h2 className="panel__title">{title}</h2> : <span />}
          {actions ? <div className="panel__actions">{actions}</div> : null}
        </header>
      )}
      <div className="panel__body">{children}</div>
    </section>
  );
}
