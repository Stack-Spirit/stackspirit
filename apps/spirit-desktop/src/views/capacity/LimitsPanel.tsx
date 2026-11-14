import type { CapacityView } from "../../bridge/wire";
import { Badge, Panel } from "../../components";
import { limitKindLabel, providerCaps } from "../../domain/capacity";
import { formatDateTime } from "../../domain/format";

export function LimitsPanel({ capacity }: { capacity: CapacityView }) {
  const caps = capacity.report_loaded ? providerCaps(capacity.provider_caps) : [];

  return (
    <Panel
      title="Provider limits and caps"
      actions={<Badge tone={capacity.limits.length > 0 ? "warn" : "unknown"}>{capacity.limits.length}</Badge>}
      className="limits-panel"
      data-testid="limits-panel"
      data-report-loaded={capacity.report_loaded ? "true" : "false"}
    >
      <p className="muted small">
        Provider limits are separate from disk, RAM and CPU. VOWP keeps the provider
        authoritative.
      </p>

      {capacity.limits.length === 0 ? (
        <p className="muted small">No provider limits reported.</p>
      ) : (
        <ul className="reason-list limits-panel__rows">
          {capacity.limits.map((limit) => (
            <li key={`${limit.provider}-${limit.kind}-${limit.harness ?? "any"}`} data-testid="limit-row">
              <Badge tone="warn">{limitKindLabel(limit.kind)}</Badge>
              <span>
                {limit.provider}
                {limit.harness ? ` · ${limit.harness}` : ""}
              </span>
              <span className="muted small">
                {limit.resets_at
                  ? `resets ${formatDateTime(limit.resets_at)}`
                  : "needs a human"}
              </span>
              <span className="muted small limits-panel__detail">{limit.detail}</span>
              {limit.concurrency_cap !== null ? (
                <Badge tone="unknown">cap {limit.concurrency_cap}</Badge>
              ) : null}
              {limit.probe_fail_streak > 0 ? (
                <span className="muted small">
                  {limit.probe_fail_streak} failed probe
                  {limit.probe_fail_streak === 1 ? "" : "s"}
                </span>
              ) : null}
            </li>
          ))}
        </ul>
      )}

      {capacity.report_loaded ? (
        <div className="limits-panel__caps" data-testid="provider-caps">
          <span className="eyebrow">Provider caps</span>
          {caps.length === 0 ? (
            <p className="muted small">No provider caps reported.</p>
          ) : (
            <div className="limits-panel__chips">
              {caps.map((cap) => (
                <Badge key={cap.provider} tone="info" mono>
                  {cap.provider} → {cap.concurrency}
                </Badge>
              ))}
            </div>
          )}
        </div>
      ) : (
        <p className="muted small">
          Provider caps load with the capacity report; this snapshot has no caps to show.
        </p>
      )}
    </Panel>
  );
}
