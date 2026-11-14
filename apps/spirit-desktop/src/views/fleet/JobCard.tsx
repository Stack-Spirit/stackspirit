import type { JobCardView } from "../../bridge/wire";
import { Badge } from "../../components";
import { formatAgeParts, truncateText } from "../../domain/format";
import { jobStateChip } from "../../domain/fleet";

export function JobCard({
  job,
  onOpen,
  showProject = false,
  lane = "running",
}: {
  job: JobCardView;
  onOpen: (job: JobCardView) => void;
  showProject?: boolean;
  lane?: string;
}) {
  const chip = jobStateChip(job.state);
  const unknown = chip.label === "Unknown";
  const detail = job.reason ?? job.detail;
  const evidence = job.gate_evidence ? truncateText(job.gate_evidence, 200) : null;

  return (
    <button
      type="button"
      className={`job-card${job.gate ? " job-card--gate" : ""}${
        unknown ? " job-card--unknown" : ""
      }`}
      onClick={() => onOpen(job)}
      aria-label={`Open job ${job.job_id}`}
      data-testid={`job-card-${job.job_id}`}
      data-state={job.state}
      data-lane={lane}
      data-gate={job.gate ?? undefined}
      data-unknown-state={unknown ? "true" : undefined}
      title={unknown ? `Unknown VOWP state: ${job.state}` : undefined}
    >
      <span className="job-card__head">
        <span className="mono job-card__id">{job.job_id}</span>
        {job.priority >= 80 ? (
          <span className="job-card__priority" title={`Priority ${job.priority}`}>
            <span aria-hidden="true">●</span>
            <span className="sr-only">High priority</span>
          </span>
        ) : null}
        <Badge tone="info">{job.kind}</Badge>
        <Badge tone={chip.tone}>{chip.label}</Badge>
      </span>

      <span className="job-card__title">{job.title ?? job.job_id}</span>

      {job.gate ? (
        <span className="job-card__gate">
          <Badge tone="verifier">{job.gate}</Badge>
          {evidence ? (
            <span className="muted small" title={job.gate_evidence ?? undefined}>
              {evidence}
            </span>
          ) : null}
        </span>
      ) : null}

      {!job.gate && detail ? (
        <span className="job-card__detail">
          {job.reason ? <span className="job-card__reason">{job.reason}</span> : null}
          {job.detail && job.detail !== job.reason ? (
            <span className="muted small">{truncateText(job.detail, 200)}</span>
          ) : null}
        </span>
      ) : null}

      <span className="job-card__foot">
        <span className="muted small">{formatAgeParts(job.age_secs)} old</span>
        {showProject ? <Badge tone="unknown">{job.project}</Badge> : null}
      </span>
    </button>
  );
}
