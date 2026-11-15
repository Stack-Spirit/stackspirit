import { useEffect, useState } from "react";

import type {
  JobCardView,
  JobDetailView,
  SpiritBridge,
  SpiritError,
} from "@stackspirit/ui";
import {
  Badge,
  Button,
  Drawer,
  EmptyState,
  executorLabel,
  formatAgeParts,
  formatDateTime,
} from "@stackspirit/ui";

import { asSpiritError, webError, webErrorTitle } from "../bridge/errors";
import {
  JOB_DETAIL_LOADING,
  JOB_DETAIL_NOT_FOUND,
  JOB_DETAIL_STALE,
  JOB_DETAIL_UNAVAILABLE,
  JOB_GATES_EMPTY,
  JOB_GATES_TRUNCATED,
  JOB_RUNS_EMPTY,
  JOB_RUNS_TRUNCATED,
  READ_ONLY_NOTICE,
} from "../copy";

type DetailState =
  | { status: "loading"; detail: null; error: null }
  | { status: "loaded"; detail: JobDetailView; error: null }
  | { status: "error"; detail: null; error: SpiritError };

function detailErrorText(error: SpiritError): string {
  if (error.code === "not_found") return JOB_DETAIL_NOT_FOUND;
  if (error.code === "daemon_incompatible" && error.message.includes("does not serve")) {
    return JOB_DETAIL_UNAVAILABLE;
  }
  return error.message;
}

function JobDetailBody({
  job,
  bridge,
  stale,
  onRetry,
}: {
  job: JobCardView;
  bridge: SpiritBridge;
  stale: boolean;
  onRetry: () => void;
}) {
  const [state, setState] = useState<DetailState>(() => {
    if (!bridge.fleet.job) {
      return {
        status: "error",
        detail: null,
        error: webError("daemon_incompatible", JOB_DETAIL_UNAVAILABLE),
      };
    }
    return { status: "loading", detail: null, error: null };
  });

  useEffect(() => {
    const read = bridge.fleet.job;
    if (!read) return;
    let cancelled = false;
    read(job.job_id).then(
      (detail) => {
        if (!cancelled) setState({ status: "loaded", detail, error: null });
      },
      (caught: unknown) => {
        if (!cancelled) {
          setState({ status: "error", detail: null, error: asSpiritError(caught) });
        }
      },
    );
    return () => {
      cancelled = true;
    };
  }, [job.job_id, bridge]);

  const current = state.status === "loaded" ? state.detail : null;
  const error = state.status === "error" ? state.error : null;
  const busy = state.status === "loading";
  const title = current?.title ?? job.title ?? job.job_id;

  return (
    <div className="web-job-detail" data-testid="web-job-detail">
      <div className="web-job-detail__badges">
        <Badge tone="info">{job.kind}</Badge>
        <Badge tone="unknown">{job.state}</Badge>
        <Badge tone="verifier">{job.role}</Badge>
        {stale ? (
          <Badge tone="warn" title={JOB_DETAIL_STALE}>
            stale snapshot
          </Badge>
        ) : null}
      </div>
      <h3>{title}</h3>

      {stale ? (
        <p className="notice notice--warn" data-testid="job-detail-stale">
          {JOB_DETAIL_STALE}
        </p>
      ) : null}

      <dl className="web-facts" data-testid="job-card-facts">
        <div>
          <dt>Project</dt>
          <dd className="mono">{job.project}</dd>
        </div>
        <div>
          <dt>State</dt>
          <dd>{job.state}</dd>
        </div>
        <div>
          <dt>Priority</dt>
          <dd>{job.priority}</dd>
        </div>
        <div>
          <dt>In this state</dt>
          <dd>{formatAgeParts(job.age_secs)}</dd>
        </div>
        <div>
          <dt>Session</dt>
          <dd className="mono">{job.session_id ?? "not matched"}</dd>
        </div>
      </dl>

      {busy ? (
        <p className="muted" role="status" data-testid="job-detail-loading">
          {JOB_DETAIL_LOADING}
        </p>
      ) : null}

      {error ? (
        <div className="notice notice--danger" role="alert" data-testid="job-detail-error">
          <strong>{webErrorTitle(error)}</strong>
          <p>{detailErrorText(error)}</p>
          {error.hint ? <p className="muted small">{error.hint}</p> : null}
          {error.retryable ? (
            <Button size="sm" variant="secondary" onClick={onRetry}>
              Retry
            </Button>
          ) : null}
        </div>
      ) : null}

      {current ? (
        <>
          <section className="web-job-detail__section" data-testid="job-detail-fields">
            <h3 className="web-subtitle">Job detail</h3>
            <dl className="web-facts">
              <div>
                <dt>Job</dt>
                <dd className="mono">{current.id}</dd>
              </div>
              <div>
                <dt>Project</dt>
                <dd className="mono">{current.project}</dd>
              </div>
              <div>
                <dt>Kind / role</dt>
                <dd>
                  {current.kind} / {current.role}
                </dd>
              </div>
              <div>
                <dt>State</dt>
                <dd>{current.state}</dd>
              </div>
              <div>
                <dt>Priority</dt>
                <dd>{current.priority}</dd>
              </div>
              <div>
                <dt>Needs</dt>
                <dd className="mono">
                  {current.needs.length > 0 ? current.needs.join(", ") : "none"}
                </dd>
              </div>
              <div>
                <dt>Attempts</dt>
                <dd>{current.attempts}</dd>
              </div>
              <div>
                <dt>Parked reason</dt>
                <dd>{current.parked_reason ?? "not parked"}</dd>
              </div>
              <div>
                <dt>Note</dt>
                <dd>{current.note || "No note reported."}</dd>
              </div>
              <div>
                <dt>Report path</dt>
                <dd className="mono">
                  {current.report_path ?? "No repo-relative report path reported."}
                </dd>
              </div>
              <div>
                <dt>Created</dt>
                <dd>{formatDateTime(current.created_at)}</dd>
              </div>
              <div>
                <dt>State changed</dt>
                <dd>{formatDateTime(current.state_changed_at)}</dd>
              </div>
              <div>
                <dt>Updated</dt>
                <dd>{formatDateTime(current.updated_at)}</dd>
              </div>
              <div>
                <dt>Last progress</dt>
                <dd>
                  {current.last_progress_at
                    ? formatDateTime(current.last_progress_at)
                    : "not reported"}
                </dd>
              </div>
            </dl>
            <p className="muted small">
              Read {formatDateTime(current.fetched_at)} from the daemon. This view omits the job
              worktree and every home path.
            </p>
          </section>

          <section className="web-job-detail__section" data-testid="job-runs">
            <h3 className="web-subtitle">Runs ({current.runs.length})</h3>
            {current.runs_truncated ? (
              <p className="notice notice--warn" data-testid="job-runs-truncated">
                {JOB_RUNS_TRUNCATED}
              </p>
            ) : null}
            {current.runs.length === 0 ? (
              <EmptyState icon="▷" title="No runs recorded" body={JOB_RUNS_EMPTY} />
            ) : (
              <ol className="web-job-runs">
                {current.runs.map((run) => (
                  <li key={run.id} className="web-job-run" data-testid={`job-run-${run.id}`}>
                    <div className="web-job-run__head">
                      <Badge tone="info">{run.role}</Badge>
                      <Badge tone="unknown">{run.purpose}</Badge>
                      <span className="mono small">{run.id}</span>
                      {run.outcome ? (
                        <Badge tone={run.outcome === "ok" ? "ok" : "danger"}>
                          {run.outcome}
                        </Badge>
                      ) : (
                        <Badge tone="warn">open</Badge>
                      )}
                      {run.exit_code !== null ? (
                        <span className="muted small">exit {run.exit_code}</span>
                      ) : null}
                    </div>
                    <p className="muted small">
                      {formatDateTime(run.started_at)} →{" "}
                      {run.ended_at ? formatDateTime(run.ended_at) : "still open"}
                    </p>
                    <p className="muted small">Executor: {executorLabel(run.executor)}</p>
                    {run.detail ? (
                      <p className="web-job-run__detail">{run.detail}</p>
                    ) : (
                      <p className="muted small">No run detail reported.</p>
                    )}
                  </li>
                ))}
              </ol>
            )}
          </section>

          <section className="web-job-detail__section" data-testid="job-gates">
            <h3 className="web-subtitle">Gates — newest attempt</h3>
            {current.gates_truncated ? (
              <p className="notice notice--warn" data-testid="job-gates-truncated">
                {JOB_GATES_TRUNCATED}
              </p>
            ) : null}
            {current.gates.length === 0 ? (
              <EmptyState icon="◍" title="No gate results" body={JOB_GATES_EMPTY} />
            ) : (
              <ul className="web-job-gates">
                {current.gates.map((gate) => (
                  <li
                    key={`${gate.gate_index}:${gate.checked_at}`}
                    className={`web-job-gate web-job-gate--${gate.passed ? "passed" : "failed"}`}
                    data-testid={`job-gate-${gate.gate_index}`}
                    data-passed={gate.passed ? "true" : "false"}
                  >
                    <div className="web-job-gate__head">
                      <Badge tone={gate.passed ? "ok" : "danger"}>
                        {gate.passed ? "passed" : "failed"}
                      </Badge>
                      <Badge tone="verifier">{gate.kind}</Badge>
                      <span className="muted small">
                        gate {gate.gate_index} · attempt {gate.attempt}
                      </span>
                      <span className="muted small">{gate.duration_ms} ms</span>
                    </div>
                    <p className="web-job-gate__evidence">
                      {gate.evidence_summary ?? "No safe evidence summary was reported."}
                    </p>
                    <p className="muted small">Checked {formatDateTime(gate.checked_at)}</p>
                  </li>
                ))}
              </ul>
            )}
          </section>
        </>
      ) : null}

      <p className="muted small">{READ_ONLY_NOTICE}</p>
    </div>
  );
}

export function JobDrawer({
  job,
  bridge,
  stale = false,
  onClose,
}: {
  job: JobCardView | null;
  bridge: SpiritBridge;
  /** The board snapshot itself is stale; the detail read happens when the drawer opens. */
  stale?: boolean;
  onClose: () => void;
}) {
  const [attempt, setAttempt] = useState(0);
  return (
    <Drawer open={job !== null} title={job ? `Job ${job.job_id}` : "Job"} onClose={onClose}>
      {job ? (
        <JobDetailBody
          key={`${job.job_id}:${attempt}`}
          job={job}
          bridge={bridge}
          stale={stale}
          onRetry={() => setAttempt((value) => value + 1)}
        />
      ) : null}
    </Drawer>
  );
}
