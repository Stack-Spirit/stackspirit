import { useEffect, useId, useRef, useState, type ReactNode } from "react";

export interface MenuProps {
  /** Renders the trigger; spread the props onto a button. */
  trigger: (props: {
    "aria-haspopup": "menu";
    "aria-expanded": boolean;
    "aria-controls": string;
    onClick: () => void;
  }) => ReactNode;
  /** Where the popover opens relative to the trigger. */
  placement?: "above" | "below";
  align?: "start" | "end";
  label: string;
  children: (close: () => void) => ReactNode;
}

/**
 * A small popover menu: opens on click, closes on outside click, Escape, or when
 * an item calls `close`. Focus moves into the menu and back to the trigger.
 */
export function Menu({ trigger, placement = "below", align = "start", label, children }: MenuProps) {
  const [open, setOpen] = useState(false);
  const id = useId();
  const root = useRef<HTMLDivElement>(null);
  const popover = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onPointer = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setOpen(false);
        root.current?.querySelector<HTMLElement>("[aria-haspopup]")?.focus();
      }
    };
    document.addEventListener("pointerdown", onPointer);
    document.addEventListener("keydown", onKey);
    popover.current?.querySelector<HTMLElement>("button:not([disabled])")?.focus();
    return () => {
      document.removeEventListener("pointerdown", onPointer);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  return (
    <div className="nx-menu" ref={root}>
      {trigger({
        "aria-haspopup": "menu",
        "aria-expanded": open,
        "aria-controls": id,
        onClick: () => setOpen((value) => !value),
      })}
      {open ? (
        <div
          id={id}
          ref={popover}
          role="menu"
          aria-label={label}
          className={`nx-menu__pop nx-menu__pop--${placement} nx-menu__pop--${align}`}
        >
          {children(() => setOpen(false))}
        </div>
      ) : null}
    </div>
  );
}

export function MenuItem({
  icon,
  children,
  meta,
  onSelect,
  disabled = false,
}: {
  icon?: ReactNode;
  children: ReactNode;
  meta?: ReactNode;
  onSelect: () => void;
  disabled?: boolean;
}) {
  return (
    <button type="button" role="menuitem" className="nx-menu__item" onClick={onSelect} disabled={disabled}>
      {/* The icon slot is always reserved so labels line up. */}
      <span className="nx-menu__icon" aria-hidden="true">
        {icon}
      </span>
      <span className="nx-menu__text">{children}</span>
      {meta ? <span className="nx-menu__meta">{meta}</span> : null}
    </button>
  );
}
