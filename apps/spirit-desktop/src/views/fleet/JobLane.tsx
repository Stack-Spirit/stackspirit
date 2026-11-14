import type { JobCardView } from "../../bridge/wire";
import { laneFooterText, queueTruncationText, type LaneDefinition } from "../../domain/fleet";
import { JobCard } from "./JobCard";

export function JobLane({
  lane,
  cards,
  count,
  truncated = false,
  queueCap = 200,
  showProject = false,
  onOpenJob,
}: {
  lane: LaneDefinition;
  cards: readonly JobCardView[];
  count: number;
  truncated?: boolean;
  queueCap?: number;
  showProject?: boolean;
  onOpenJob: (job: JobCardView) => void;
}) {
  // The queue cap is VOWP's row bound; repeat it in the DOM so a changed cap never
  // turns into an unbounded card list (PLAN §9.5).
  const visibleCards = cards.slice(0, Math.max(0, queueCap));
  const capped = visibleCards.length < cards.length;
  const truncation = truncated || capped ? queueTruncationText(queueCap) : null;

  return (
    <section
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
        {lane.key === "done" && cards.length === 0 ? (
          <p className="job-lane__summary">
            Done jobs are summarized from jobs_by_state; live cards are not loaded.
          </p>
        ) : cards.length === 0 ? (
          <p className="job-lane__empty">No jobs</p>
        ) : (
          visibleCards.map((job) => (
            <JobCard
              key={`${job.project}:${job.job_id}`}
              job={job}
              lane={lane.key}
              showProject={showProject}
              onOpen={onOpenJob}
            />
          ))
        )}
      </div>

      <footer className="job-lane__foot">
        <span>{laneFooterText(lane.key, count)}</span>
        {truncation ? (
          <span className="job-lane__truncated" data-testid={`lane-truncated-${lane.key}`}>
            {truncation}
          </span>
        ) : null}
      </footer>
    </section>
  );
}
