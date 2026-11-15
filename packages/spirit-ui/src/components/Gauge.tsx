import type { ReactNode } from "react";

import type { ResourceState } from "../bridge/wire";
import { cx } from "./cx";

export interface GaugeProps {
  label: string;
  value: number;
  max: number;
  min?: number;
  valueLabel: string;
  state?: ResourceState;
  marker?: number | null;
  detail?: ReactNode;
  ariaLabel?: string;
}

export function Gauge({
  label,
  value,
  max,
  min = 0,
  valueLabel,
  state = "unknown",
  marker = null,
  detail,
  ariaLabel,
}: GaugeProps) {
  const span = Math.max(1, max - min);
  const ratio = Math.min(1, Math.max(0, (value - min) / span));
  const markerRatio = marker === null ? null : Math.min(1, Math.max(0, (marker - min) / span));
  return (
    <div className={cx("gauge", `gauge--${state}`)}>
      <div className="gauge__head">
        <span className="gauge__label">{label}</span>
        <span className="gauge__value">{valueLabel}</span>
      </div>
      <div
        className="gauge__track"
        role="meter"
        aria-label={ariaLabel ?? label}
        aria-valuemin={min}
        aria-valuemax={max}
        aria-valuenow={value}
        aria-valuetext={valueLabel}
      >
        <span className="gauge__fill" style={{ width: `${(ratio * 100).toFixed(2)}%` }} />
        {markerRatio !== null ? (
          <span
            className="gauge__marker"
            style={{ left: `${(markerRatio * 100).toFixed(2)}%` }}
            aria-hidden="true"
          />
        ) : null}
      </div>
      {detail ? <div className="gauge__detail">{detail}</div> : null}
    </div>
  );
}
