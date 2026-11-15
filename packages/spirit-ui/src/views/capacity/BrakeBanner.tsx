import type { BrakeView } from "../../bridge/wire";
import { Badge, Panel } from "../../components";
import { brakeReasonLabel, brakeTone } from "../../domain/capacity";
import { formatClock } from "../../domain/format";

export function BrakeBanner({ brake }: { brake: BrakeView }) {
  if (!brake.active) {
    return (
      <p className="brake-idle" data-testid="brake-banner" data-active="false">
        <Badge tone="ok">No brake active</Badge> VOWP is admitting runs subject to target
        and provider limits.
      </p>
    );
  }
  const tone = brakeTone(brake);
  return (
    <Panel
      title="VOWP brake"
      tone={tone === "danger" ? "danger" : "warn"}
      className="brake-banner"
      data-testid="brake-banner"
      data-active="true"
      data-tone={tone}
    >
      <p className="brake-title">
        <Badge tone={tone === "danger" ? "danger" : "warn"}>brake active</Badge>
        <span>since {formatClock(brake.since)}</span>
      </p>
      <ul className="reason-list">
        {brake.reasons.map((reason) => (
          <li key={`${reason.code}-${reason.detail}`} data-testid="brake-reason">
            <span className="mono">{brakeReasonLabel(reason.code)}</span>
            <span className="muted">{reason.detail}</span>
          </li>
        ))}
      </ul>
      <p className="muted small">New runs wait; running runs continue.</p>
    </Panel>
  );
}
