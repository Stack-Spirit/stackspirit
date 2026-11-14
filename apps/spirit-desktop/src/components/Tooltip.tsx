import { useId, type ReactNode } from "react";

export interface TooltipProps {
  content: string;
  children: ReactNode;
}

/** A focusable wrapper so keyboard users can read the tooltip text too. */
export function Tooltip({ content, children }: TooltipProps) {
  const id = useId();
  return (
    <span className="tooltip" tabIndex={0} aria-describedby={id} title={content}>
      {children}
      <span id={id} role="tooltip" className="tooltip__bubble">
        {content}
      </span>
    </span>
  );
}
