import { useCallback, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";

export interface ResizeOptions {
  /** localStorage key for the saved width. */
  storageKey: string;
  initial: number;
  min: number;
  max: number;
  /** "right": dragging right makes it wider (left sidebar). "left": the opposite (right panel). */
  grows: "right" | "left";
}

function readWidth({ storageKey, initial, min, max }: ResizeOptions): number {
  try {
    const saved = Number(window.localStorage.getItem(storageKey));
    return Number.isFinite(saved) && saved >= min && saved <= max ? saved : initial;
  } catch {
    return initial;
  }
}

/**
 * A draggable, keyboard-adjustable width for the sidebar and the details panel.
 * Double-click the handle to reset. The width is remembered per computer.
 */
export function useResizable(options: ResizeOptions) {
  const [width, setWidth] = useState(() => readWidth(options));
  const [dragging, setDragging] = useState(false);
  const start = useRef<{ x: number; width: number } | null>(null);
  const { storageKey, initial, min, max, grows } = options;

  const commit = useCallback(
    (next: number) => {
      const clamped = Math.round(Math.min(max, Math.max(min, next)));
      setWidth(clamped);
      try {
        window.localStorage.setItem(storageKey, String(clamped));
      } catch {
        // Not saved; the width still applies for this session.
      }
    },
    [max, min, storageKey],
  );

  const handleProps = {
    role: "separator" as const,
    "aria-orientation": "vertical" as const,
    "aria-valuemin": min,
    "aria-valuemax": max,
    "aria-valuenow": width,
    tabIndex: 0,
    onPointerDown: (event: PointerEvent<HTMLElement>) => {
      event.preventDefault();
      event.currentTarget.setPointerCapture(event.pointerId);
      start.current = { x: event.clientX, width };
      setDragging(true);
    },
    onPointerMove: (event: PointerEvent<HTMLElement>) => {
      if (!start.current) return;
      const delta = event.clientX - start.current.x;
      commit(start.current.width + (grows === "right" ? delta : -delta));
    },
    onPointerUp: () => {
      start.current = null;
      setDragging(false);
    },
    onPointerCancel: () => {
      start.current = null;
      setDragging(false);
    },
    onDoubleClick: () => commit(initial),
    onKeyDown: (event: KeyboardEvent<HTMLElement>) => {
      const step = event.shiftKey ? 48 : 16;
      const wider = grows === "right" ? "ArrowRight" : "ArrowLeft";
      const narrower = grows === "right" ? "ArrowLeft" : "ArrowRight";
      if (event.key === wider) {
        event.preventDefault();
        commit(width + step);
      } else if (event.key === narrower) {
        event.preventDefault();
        commit(width - step);
      } else if (event.key === "Home") {
        event.preventDefault();
        commit(initial);
      }
    },
  };

  return { width, dragging, handleProps };
}
