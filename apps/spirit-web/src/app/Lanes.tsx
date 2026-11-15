import type { JobCardView, WebFleetView } from "@stackspirit/ui";
import { Badge, EmptyState, Panel } from "@stackspirit/ui";

import {
  GATE_FAILURES_UNAVAILABLE,
  JOB_DETAIL_UNAVAILABLE,
  JOB_PAGE_TRUNCATED,
} from "../copy";
import { LANES, laneFooterText, sortWorkerCards } from "../shared/domain";
import { JobLane, WorkerCard } from "../shared/views";

function titleForJob(snapshot: WebFleetView, jobId: string): string | null {
  for (const cards of [
    snapshot.lanes.running,
    snapshot.lanes.ready,
    snapshot.lanes.review_fix,
    snapshot.lanes.parked_blocked,
    snapshot.lanes.done,
  ]) {
    const card = cards.find((candidate) => candidate.job_id === jobId);
    if (card?.title) return card.title;
  }
  return null;
}

export function RunningWorkers({ snapshot }: { snapshot: WebFleetView }) {
  const sorted = sortWorkerCards(snapshot.running);
  return (
    <Panel title="Running workers" data-testid="running-workers">
      {sorted.length === 0 ? (
        <EmptyState
          icon="○"
          title="No running workers"
          body="The board reports no running worker cards in this snapshot."
        />
      ) : (
        <div className="running-grid__cards">
          {sorted.map((card) => (
            <div key={`${card.project}:${card.job_id}`} className="web-worker">
              <WorkerCard
                card={card}
                title={titleForJob(snapshot, card.job_id)}
                showProject={false}
              />
              {card.project === "unknown" ? (
                <p className="muted small" data-testid={`job-detail-unavailable-${card.job_id}`}>
                  Job detail unavailable for this board run.
                </p>
              ) : null}
            </div>
          ))}
        </div>
      )}
    </Panel>
  );
}

export function Lanes({
  snapshot,
  onOpenJob,
}: {
  snapshot: WebFleetView;
  onOpenJob: (job: JobCardView) => void;
}) {
  const truncated = snapshot.lanes.queue_truncated;
  return (
    <section className="fleet-canvas__lanes" aria-label="Job lanes" data-testid="web-lanes">
      {truncated ? (
        <p className="notice notice--warn web-lanes-notice" data-testid="job-page-truncated">
          {JOB_PAGE_TRUNCATED}
        </p>
      ) : null}
      {LANES.map((lane) => {
        const cards = snapshot.lanes[lane.key];
        const count = snapshot.lanes.counts[lane.key] ?? 0;
        if (lane.key === "review_fix") {
          return (
            <section
              key={lane.key}
              className={`job-lane job-lane--${lane.key}`}
              aria-label={`${lane.label} lane`}
              data-testid={`lane-${lane.key}`}
              data-count={count}
            >
              <header className={`job-lane__head job-lane__head--${lane.headerTone}`}>
                <h3 className="job-lane__title">{lane.label}</h3>
                <span className="job-lane__count" data-testid={`lane-count-${lane.key}`}>
                  {count}
                </span>
              </header>
              <div className="job-lane__cards">
                <p className="notice notice--warn" data-testid="gate-failures-unavailable">
                  {GATE_FAILURES_UNAVAILABLE}
                </p>
              </div>
              <footer className="job-lane__foot">
                <span>{laneFooterText(lane.key, count)}</span>
              </footer>
            </section>
          );
        }
        return (
          <JobLane
            key={lane.key}
            lane={lane}
            cards={cards}
            count={count}
            queueCap={Number.MAX_SAFE_INTEGER}
            truncated={
              truncated && (lane.key === "ready" || lane.key === "parked_blocked")
            }
            showProject={false}
            onOpenJob={onOpenJob}
          />
        );
      })}
    </section>
  );
}

export function UnavailableDetails({ snapshot }: { snapshot: WebFleetView }) {
  const shown = snapshot.unavailable.filter(
    (notice) =>
      notice.feature !== "capacity_report" &&
      notice.feature !== "supervisor_state" &&
      notice.feature !== "project_state" &&
      notice.feature !== "gate_failures",
  );
  return (
    <Panel title="Unavailable detail" data-testid="unavailable-details">
      <p className="muted small">
        This build does not render empty states for features the daemon cannot supply.
      </p>
      <ul className="web-list">
        {shown.map((notice) => (
          <li key={notice.feature}>
            <Badge tone="unknown">{notice.feature}</Badge> {notice.detail}
          </li>
        ))}
      </ul>
      <p className="muted small">{JOB_DETAIL_UNAVAILABLE}</p>
    </Panel>
  );
}
