import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from "react";

import { cx } from "./cx";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "primary" | "secondary" | "danger" | "ghost";
  size?: "sm" | "md" | "lg";
  loading?: boolean;
  icon?: ReactNode;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  {
    variant = "secondary",
    size = "md",
    loading = false,
    icon,
    className,
    children,
    disabled = false,
    ...rest
  },
  ref,
) {
  return (
    <button
      {...rest}
      ref={ref}
      type={rest.type ?? "button"}
      className={cx("btn", `btn--${variant}`, `btn--${size}`, className)}
      aria-busy={loading || undefined}
      disabled={disabled || loading}
    >
      {loading ? <span className="btn__spinner" aria-hidden="true" /> : icon}
      <span>{children}</span>
    </button>
  );
});
