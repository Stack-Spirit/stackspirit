import type { JobCardView } from "@stackspirit/ui";
import { Badge, Drawer } from "@stackspirit/ui";

import { GATE_FAILURES_UNAVAILABLE, JOB_DETAIL_UNAVAILABLE } from "../copy";

export function JobDrawer({
  job,
  onClose,
}: {
  job: JobCardView | null;
  onClose: () => void;
}) {
  return (
    <Drawer open={job !== null} title={job ? `Job ${job.job_id}` : "Job"} onClose={onClose}>
      {job ? (
        <div className="web-job-detail" data-testid="web-job-detail">
          <div className="web-job-detail__badges">
            <Badge tone="info">{job.kind}</Badge>
            <Badge tone="unknown">{job.state}</Badge>
            <Badge tone="verifier">{job.role}</Badge>
          </div>
          <h3>{job.title ?? job.job_id}</h3>
          <dl className="web-facts">
            <div>
              <dt>Project</dt>
              <dd className="mono">{job.project}</dd>
            </div>
            <div>
              <dt>Priority</dt>
              <dd>{job.priority}</dd>
            </div>
            <div>
              <dt>In this state</dt>
              <dd>{job.age_secs}s</dd>
            </div>
            <div>
              <dt>Session</dt>
              <dd className="mono">{job.session_id ?? "not matched"}</dd>
            </div>
            <div>
              <dt>Reason</dt>
              <dd>{job.reason ?? "None reported for this card"}</dd>
            </div>
          </dl>
          <p className="notice notice--warn">{GATE_FAILURES_UNAVAILABLE}</p>
          <p className="muted small">{JOB_DETAIL_UNAVAILABLE}</p>
          <p className="muted small">
            This is a read-only card. Retry, park, cancel, and inbox answers stay hidden until
            the local write proxy exists.
          </p>
        </div>
      ) : null}
    </Drawer>
  );
}
