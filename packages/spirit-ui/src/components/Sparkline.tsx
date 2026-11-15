export interface SparklineProps {
  samples: number[];
  p50?: number | null;
  p95?: number | null;
  label?: string;
  width?: number;
  height?: number;
}

/** 64 × 18 SVG sparkline of the last 20 latency samples, no axis labels. */
export function Sparkline({
  samples,
  p50 = null,
  p95 = null,
  label = "Latency samples",
  width = 64,
  height = 18,
}: SparklineProps) {
  const points = samples.slice(-20);
  const max = Math.max(1, ...points);
  const min = Math.min(...points, 0);
  const span = Math.max(1, max - min);
  const polyline =
    points.length === 0
      ? ""
      : points
          .map((sample, index) => {
            const x = points.length === 1 ? width : (index / (points.length - 1)) * width;
            const y = height - 1 - ((sample - min) / span) * (height - 2);
            return `${x.toFixed(2)},${y.toFixed(2)}`;
          })
          .join(" ");
  const accessible = [
    label,
    points.length > 0 ? `last ${points[points.length - 1] ?? 0} ms` : "no samples yet",
    p50 !== null ? `p50 ${Math.round(p50)} ms` : null,
    p95 !== null ? `p95 ${Math.round(p95)} ms` : null,
  ]
    .filter(Boolean)
    .join(", ");

  return (
    <svg
      className="sparkline"
      width={width}
      height={height}
      viewBox={`0 0 ${width} ${height}`}
      role="img"
      aria-label={accessible}
    >
      <title>{accessible}</title>
      {points.length > 1 ? (
        <polyline points={polyline} fill="none" stroke="currentColor" strokeWidth="1.5" />
      ) : (
        <line
          x1="0"
          y1={height / 2}
          x2={width}
          y2={height / 2}
          stroke="currentColor"
          strokeDasharray="2 2"
          strokeWidth="1"
        />
      )}
    </svg>
  );
}
