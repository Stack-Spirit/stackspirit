import type { FleetView, WorkerCardView } from "../../bridge/wire";
import { Badge } from "../../components";
import { workerRingModel } from "../../domain/fleet";

const CENTER_X = 140;
const CENTER_Y = 128;
const RADIUS = 96;
const INNER_RADIUS = 74;
const START_ANGLE = 160;
const SWEEP = 220;

function polar(angle: number, radius: number): { x: number; y: number } {
  const radians = (angle * Math.PI) / 180;
  return {
    x: CENTER_X + radius * Math.cos(radians),
    y: CENTER_Y + radius * Math.sin(radians),
  };
}

function arcPath(radius: number): string {
  const start = polar(START_ANGLE, radius);
  const end = polar(START_ANGLE + SWEEP, radius);
  return `M ${start.x.toFixed(2)} ${start.y.toFixed(2)} A ${radius} ${radius} 0 1 1 ${end.x.toFixed(
    2,
  )} ${end.y.toFixed(2)}`;
}

export function WorkerSlotRing({
  fleet,
  running = fleet.running,
}: {
  fleet: FleetView;
  running?: readonly WorkerCardView[];
}) {
  const model = workerRingModel(fleet, running);
  const step = SWEEP / Math.max(1, model.slots.length);
  const verifierCount = Math.min(model.verifierRuns, 12);
  const overflowPoint = polar(START_ANGLE + SWEEP - 8, RADIUS + 16);

  return (
    <article
      className="slot-ring"
      aria-label="Worker slots"
      data-testid="worker-slot-ring"
      data-target={model.target}
      data-busy={model.busy}
      data-over-target={model.overTarget}
      data-stalled={model.stalled}
      data-verifier={model.verifierRuns}
    >
      <header className="slot-ring__head">
        <span className="eyebrow">Worker slots</span>
        <div className="slot-ring__chips">
          {model.stalled > 0 ? <Badge tone="danger">Stalled</Badge> : null}
          {model.overTarget > 0 ? (
            <Badge tone="warn">
              over target +{model.overTarget}
            </Badge>
          ) : null}
          {model.otherRuns > 0 ? (
            <Badge tone="unknown">+{model.otherRuns} other runs</Badge>
          ) : null}
          {model.verifierRuns > 0 ? (
            <Badge tone="verifier">{model.verifierRuns} verifier</Badge>
          ) : null}
        </div>
      </header>

      <div className="slot-ring__stage">
        <svg
          className="slot-ring__svg"
          viewBox="0 0 280 236"
          role="presentation"
          aria-hidden="true"
          focusable="false"
        >
          <path className="slot-ring__arc" d={arcPath(RADIUS)} />
          {model.targetZero ? (
            <circle
              className="slot-ring__slot slot-ring__slot--empty"
              cx={CENTER_X}
              cy={CENTER_Y}
              r={44}
            />
          ) : null}
          {model.slots.map((slot) => {
            const point = polar(START_ANGLE + step * (slot.index + 0.5), RADIUS);
            return (
              <circle
                key={slot.index}
                className={`slot-ring__slot slot-ring__slot--${slot.state}`}
                cx={point.x}
                cy={point.y}
                r={9}
                data-slot-state={slot.state}
              />
            );
          })}
          {Array.from({ length: verifierCount }, (_, index) => {
            const point = polar(
              START_ANGLE + step * (index + 0.5),
              INNER_RADIUS,
            );
            return (
              <circle
                key={`verifier-${index}`}
                className="slot-ring__verifier"
                cx={point.x}
                cy={point.y}
                r={4}
                data-testid="verifier-run"
              />
            );
          })}
          {model.aggregateOverflow > 0 ? (
            <text
              className="slot-ring__overflow"
              x={overflowPoint.x}
              y={overflowPoint.y}
              textAnchor="middle"
              data-testid="ring-overflow"
            >
              +{model.aggregateOverflow}
            </text>
          ) : null}
        </svg>

        <div className="slot-ring__center" aria-hidden="true">
          <strong>
            {model.busy} / {model.target}
          </strong>
          <span>workers</span>
          <span className="slot-ring__sub">
            {model.targetZero
              ? "target 0"
              : `target ${model.target} · advised ${model.advised ?? "—"}`}
          </span>
          {model.overTarget > 0 ? (
            <span className="slot-ring__over">+{model.overTarget} over target</span>
          ) : null}
        </div>
      </div>

      <p className="slot-ring__summary" data-testid="ring-summary">
        {model.summary}
      </p>
    </article>
  );
}
