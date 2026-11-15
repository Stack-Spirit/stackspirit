import type { ReactNode } from "react";

import { cx } from "./cx";

export type BadgeTone =
  | "ok"
  | "info"
  | "warn"
  | "danger"
  | "verifier"
  | "unknown"
  | "accent";

export interface BadgeProps {
  tone?: BadgeTone;
  mono?: boolean;
  uppercase?: boolean;
  title?: string;
  children: ReactNode;
  className?: string;
}

export function Badge({
  tone = "unknown",
  mono = false,
  uppercase = false,
  title,
  children,
  className,
}: BadgeProps) {
  return (
    <span
      className={cx(
        "badge",
        `badge--${tone}`,
        mono && "badge--mono",
        uppercase && "badge--upper",
        className,
      )}
      title={title}
    >
      {children}
    </span>
  );
}
