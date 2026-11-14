import { useState } from "react";

import type { JobCardView } from "../../bridge/wire";
import { Badge, Button, EmptyState } from "../../components";
import { formatAge, formatClock } from "../../domain/format";
import { LANES, selectFleet } from "../../domain/fleet";
import {
  useConnection,
  useDataDimmed,
  useFleet,
  useFleetStore,
  useMutationsAllowed,
  useProjectFilter,
} from "../../state/fleetStore";
import { JobDrawer } from "./JobDrawer";
import { JobLane } from "./JobLane";
import { ProjectSwitcher } from "./ProjectSwitcher";
import { RunningWorkerGrid } from "./RunningWorkerGrid";
import { VpsRootCard } from "./VpsRootCard";
import { WorkerSlotRing } from "./WorkerSlotRing";
import "./fleet.css";

export function FleetCanvas({ onOpenHosts }: { onOpenHosts?: () => void }) {
  const fleet = useFleet();
  const connection = useConnection();
  const dimmed = useDataDimmed();
  const projectFilter = useProjectFilter();
  const mutationsAllowed = useMutationsAllowed();
  const refresh = useFleetStore((state) => state.refresh);
  const [selectedJob, setSelectedJob] = useState<JobCardView | null>(null);

  if (!fleet) {
    return (
      <div className="view-stack" data-testid="fleet-canvas">
        <EmptyState
          icon="◈"
          title="No fleet snapshot yet"
          body={
            connection.state === "connecting"
              ? "Connecting to the host and reading its first VOWP snapshot…"
              : "Connect a verified VOWP host to see its workers, capacity and job lanes."
          }
          actionLabel={onOpenHosts ? "Open Hosts" : undefined}
          onAction={onOpenHosts}
        />
      </div>
    );
  }

  const selection = selectFleet(fleet, projectFilter);
  const refreshDisabled =
    connection.state !== "connected" && connection.state !== "stale";

  return (
    <div
      className={`fleet-canvas view-stack${dimmed ? " data-dimmed" : ""}`}
      data-testid="fleet-canvas"
      data-stale={fleet.stale ? "true" : "false"}
    >
      <header className="fleet-canvas__header">
        <div className="fleet-canvas__title">
          <span className="eyebrow">Fleet &amp; Worker Canvas</span>
          <strong className="mono">{fleet.home}</strong>
        </div>
        <div className="fleet-canvas__controls">
          <ProjectSwitcher />
          <Button
            variant="secondary"
            size="sm"
            onClick={() => void refresh()}
            disabled={refreshDisabled}
            data-testid="fleet-refresh"
            title={
              connection.state === "stale"
                ? "Retry the poll and clear the stale state"
                : undefined
            }
          >
            Refresh
          </Button>
        </div>
      </header>

      {connection.state === "disconnected" ? (
        <div className="notice" role="status">
          <Badge tone="unknown">disconnected</Badge> Showing data from{" "}
          {formatClock(fleet.generated_at)}. Fleet actions are disabled until you reconnect.
        </div>
      ) : null}
      {fleet.stale ? (
        <div className="notice notice--warn" role="status">
          <Badge tone="warn">stale</Badge> As of {formatClock(fleet.generated_at)} (
          {formatAge(fleet.supervisor.last_tick_age_secs)} old). Refresh before changing
          capacity or jobs.
        </div>
      ) : null}

      <div className="fleet-canvas__top" aria-label="VPS and live workers">
        <VpsRootCard fleet={fleet} />
        <WorkerSlotRing fleet={fleet} running={fleet.running} />
        <RunningWorkerGrid
          fleet={fleet}
          cards={selection.running}
          showProject={projectFilter === null}
        />
      </div>

      <section className="fleet-canvas__lanes" aria-label="Job lanes">
        {LANES.map((lane) => (
          <JobLane
            key={lane.key}
            lane={lane}
            cards={selection.lanes[lane.key]}
            count={selection.counts[lane.key]}
            queueCap={selection.queueCap}
            truncated={
              selection.queueTruncated &&
              (lane.key === "ready" || lane.key === "parked_blocked")
            }
            showProject={projectFilter === null}
            onOpenJob={setSelectedJob}
          />
        ))}
      </section>

      <JobDrawer
        key={selectedJob?.job_id ?? "closed"}
        job={selectedJob}
        open={selectedJob !== null}
        onClose={() => setSelectedJob(null)}
        mutationsAllowed={mutationsAllowed}
      />
    </div>
  );
}
