import { useEffect, useId, useRef, useState } from "react";

import type { ChatUsageView } from "../bridge/wire";

/** The ring geometry from the brief: a 14 px SVG circle with r=5.5 and a 2 px stroke. */
const RING_SIZE = 14;
const RING_RADIUS = 5.5;
const RING_CIRCUMFERENCE = 2 * Math.PI * RING_RADIUS;

/** A safe context reading: both a used count and a positive limit are required. */
export interface ContextReading {
  /** Rounded percentage, clamped to 0–100. */
  percent: number;
  used: number;
  limit: number;
}

/**
 * Reads `view.usage` honestly: a ring only means something when the engine reported
 * both the tokens used and the model's context limit. A missing value stays missing
 * instead of becoming a zero or a guessed percentage.
 */
export function contextReading(
  usage: ChatUsageView | null | undefined,
): ContextReading | null {
  if (!usage) return null;
  const { context_tokens_used: used, context_limit: limit } = usage;
  if (used === null || limit === null) return null;
  if (!Number.isFinite(used) || !Number.isFinite(limit) || limit <= 0 || used < 0) {
    return null;
  }
  return {
    percent: Math.min(100, Math.max(0, Math.round((used / limit) * 100))),
    used,
    limit,
  };
}

/** Joins exact counts with thousands separators for the popover. */
export function formatExactTokens(value: number): string {
  return Math.round(value).toLocaleString("en-US");
}

/** Compact counts for the quiet stats pills. */
export function formatCompactTokens(value: number): string {
  if (value >= 1_000_000) {
    return `${trimZero(value / 1_000_000, 1)}m`;
  }
  if (value >= 1_000) {
    return `${trimZero(value / 1_000, 1)}k`;
  }
  return String(Math.round(value));
}

function trimZero(value: number, digits: number): string {
  return value.toFixed(digits).replace(/\.0$/, "");
}

function tokenTotal(usage: ChatUsageView): number | null {
  if (usage.input_tokens === null || usage.output_tokens === null) return null;
  return usage.input_tokens + usage.output_tokens;
}

/**
 * Context occupancy beside the composer: a small ring and percentage, with a
 * click-open popover for exact token counts. It renders nothing until `view.usage`
 * carries both used tokens and a context limit.
 */
export function ContextMeter({ usage }: { usage?: ChatUsageView | null }) {
  const reading = contextReading(usage);
  const [open, setOpen] = useState(false);
  const id = useId();
  const root = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onPointer = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("pointerdown", onPointer);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onPointer);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  if (!reading) return null;
  const offset = RING_CIRCUMFERENCE * (1 - reading.percent / 100);
  const input = usage?.input_tokens ?? null;
  const output = usage?.output_tokens ?? null;
  const cache = usage?.cache_read_tokens ?? null;
  const turns = usage?.turns ?? 0;

  return (
    <div className="nx-context" ref={root}>
      <button
        type="button"
        className="nx-context-meter"
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-controls={id}
        aria-label={`Context ${reading.percent}% used`}
        title={`${formatExactTokens(reading.used)} of ${formatExactTokens(reading.limit)} context tokens used`}
        data-testid="context-meter"
        onClick={() => setOpen((value) => !value)}
      >
        <svg
          width={RING_SIZE}
          height={RING_SIZE}
          viewBox={`0 0 ${RING_SIZE} ${RING_SIZE}`}
          aria-hidden="true"
        >
          <circle
            className="nx-context-meter__track"
            cx={RING_SIZE / 2}
            cy={RING_SIZE / 2}
            r={RING_RADIUS}
            fill="none"
            strokeWidth="2"
          />
          <circle
            className="nx-context-meter__value"
            cx={RING_SIZE / 2}
            cy={RING_SIZE / 2}
            r={RING_RADIUS}
            fill="none"
            strokeWidth="2"
            strokeDasharray={RING_CIRCUMFERENCE}
            strokeDashoffset={offset}
            transform={`rotate(-90 ${RING_SIZE / 2} ${RING_SIZE / 2})`}
          />
        </svg>
        <span className="nx-context-meter__percent">{reading.percent}%</span>
      </button>
      {open ? (
        <div
          id={id}
          className="nx-context__pop"
          role="group"
          aria-label="Context usage"
          data-testid="context-popover"
        >
          <div className="nx-context__title">Context</div>
          <dl className="nx-context__rows">
            <div>
              <dt>Used</dt>
              <dd>
                {formatExactTokens(reading.used)} / {formatExactTokens(reading.limit)} tokens
              </dd>
            </div>
            {input !== null ? (
              <div>
                <dt>Input</dt>
                <dd>{formatExactTokens(input)} tokens</dd>
              </div>
            ) : null}
            {output !== null ? (
              <div>
                <dt>Output</dt>
                <dd>{formatExactTokens(output)} tokens</dd>
              </div>
            ) : null}
            {cache !== null ? (
              <div>
                <dt>Cache read</dt>
                <dd>{formatExactTokens(cache)} tokens</dd>
              </div>
            ) : null}
            {turns > 0 ? (
              <div>
                <dt>Turns</dt>
                <dd>{turns}</dd>
              </div>
            ) : null}
          </dl>
        </div>
      ) : null}
    </div>
  );
}

/**
 * Quiet secondary pills beside the ring. A counter renders only when the engine
 * reported it; a zero cache count is not shown as a "cache hit".
 */
export function UsageStats({ usage }: { usage?: ChatUsageView | null }) {
  if (!usage) return null;
  const total = tokenTotal(usage);
  const cache = usage.cache_read_tokens;
  const turns = usage.turns;
  const showTurns = turns > 0;
  const showTotal = total !== null && total > 0;
  const showCache = cache !== null && cache > 0;
  if (!showTurns && !showTotal && !showCache) return null;

  return (
    <div className="nx-usage-pills" data-testid="usage-stats">
      {showTurns ? (
        <span className="nx-usage-pill" data-testid="usage-pill-turns">
          {turns} {turns === 1 ? "turn" : "turns"}
        </span>
      ) : null}
      {showTotal && total !== null ? (
        <span className="nx-usage-pill" data-testid="usage-pill-tokens">
          {formatCompactTokens(total)} tokens
        </span>
      ) : null}
      {showCache && cache !== null ? (
        <span className="nx-usage-pill" data-testid="usage-pill-cache">
          cache {formatCompactTokens(cache)}
        </span>
      ) : null}
    </div>
  );
}
