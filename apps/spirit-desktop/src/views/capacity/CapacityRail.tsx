import { useEffect, useState } from "react";

import { Badge, EmptyState, Panel } from "../../components";
import { formatClock } from "../../domain/format";
import {
  useCapacity,
  useConnection,
  useDataDimmed,
  useFleet,
  useNextRetryAt,
  useProjectFilter,
} from "../../state/fleetStore";
import { BrakeBanner } from "./BrakeBanner";
import { CpuPanel } from "./CpuPanel";
import { DiskPanel } from "./DiskPanel";
import { LimitsPanel } from "./LimitsPanel";
import { RamPanel } from "./RamPanel";
import { RawDocumentDrawer } from "./RawDocumentDrawer";
import { TargetAdvisor } from "./TargetAdvisor";
import { WaitingPanel } from "./WaitingPanel";
import "./capacity.css";

function RetryCountdown({ retryAt }: { retryAt: number }) {
  const [remaining, setRemaining] = useState<number | null>(null);
  useEffect(() => {
    const update = () => setRemaining(Math.max(0, Math.ceil((retryAt - Date.now()) / 1000)));
    const first = window.setTimeout(update, 0);
    const timer = window.setInterval(update, 1000);
    return () => {
      window.clearTimeout(first);
      window.clearInterval(timer);
    };
  }, [retryAt]);
  if (remaining === null) return null;
  return (
    <span data-testid="capacity-retry-countdown">
      {remaining > 0 ? `retrying in ${remaining}s` : "retrying now"}
    </span>
  );
}

export function CapacityRail({ onOpenHosts }: { onOpenHosts?: () => void }) {
  const capacity = useCapacity();
  const fleet = useFleet();
  const connection = useConnection();
  const dimmed = useDataDimmed();
  const projectFilter = useProjectFilter();
  const nextRetryAt = useNextRetryAt();
  const [rawOpen, setRawOpen] = useState(false);

  if (!capacity || !fleet) {
    return (
      <div className="view-stack" data-testid="capacity-rail">
        <Panel title="Capacity & VPS Storage">
          <EmptyState
            icon="▥"
            title="Capacity unavailable"
            body="Connect a verified VOWP host to read disk, RAM and CPU headroom."
            actionLabel={onOpenHosts ? "Open Hosts" : undefined}
            onAction={onOpenHosts}
          />
        </Panel>
      </div>
    );
  }

  const stale = fleet.stale || connection.state === "stale";
  const freshness = stale ? "stale" : "fresh";

  return (
    <div
      className={`capacity-rail view-stack${dimmed ? " data-dimmed" : ""}`}
      data-testid="capacity-rail"
      data-stale={stale ? "true" : "false"}
      data-report-loaded={capacity.report_loaded ? "true" : "false"}
    >
      <div className="capacity-rail__head">
        <div>
          <span className="eyebrow">Capacity &amp; VPS Storage</span>
          <p className="muted small">
            {capacity.report_loaded
              ? "Node-wide capacity from VOWP."
              : "Snapshot capacity; the detailed report is not loaded."}
          </p>
        </div>
        {projectFilter ? <Badge tone="info">node-wide</Badge> : null}
      </div>

      <BrakeBanner brake={capacity.brake} />
      <TargetAdvisor />
      <DiskPanel capacity={capacity} />
      <RamPanel capacity={capacity} running={fleet.running} />
      <CpuPanel capacity={capacity} />
      <WaitingPanel capacity={capacity} />
      <LimitsPanel capacity={capacity} />

      <button
        type="button"
        className={`capacity-freshness${stale ? " capacity-freshness--stale" : ""}`}
        onClick={() => setRawOpen(true)}
        data-testid="capacity-freshness"
        aria-label="Open raw capacity document"
      >
        <span>
          Capacity data from VOWP · {formatClock(capacity.generated_at)} · {freshness}
        </span>
        {stale && nextRetryAt !== null ? <RetryCountdown retryAt={nextRetryAt} /> : null}
        {!capacity.report_loaded ? (
          <span className="capacity-freshness__detail">
            capacity detail unavailable — retrying
          </span>
        ) : null}
      </button>

      <RawDocumentDrawer open={rawOpen} onClose={() => setRawOpen(false)} />
    </div>
  );
}
