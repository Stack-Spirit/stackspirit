import { useState } from "react";

import type { JobAction, JobCardView } from "../../bridge/wire";
import { toSpiritError } from "../../bridge/errors";
import { Badge, Button, Drawer, Modal } from "../../components";
import {
  formatAgeParts,
  formatDateTime,
  truncateText,
} from "../../domain/format";
import {
  jobActionEligibility,
  jobStateChip,
  type JobActionName,
} from "../../domain/fleet";
import { useFleetStore } from "../../state/fleetStore";

const ACTION_LABELS: Record<JobActionName, string> = {
  retry: "Retry",
  park: "Park",
  cancel: "Cancel",
};

export function JobDrawer({
  job,
  open,
  onClose,
  mutationsAllowed,
}: {
  job: JobCardView | null;
  open: boolean;
  onClose: () => void;
  mutationsAllowed: boolean;
}) {
  const bridge = useFleetStore((state) => state.bridge);
  const refresh = useFleetStore((state) => state.refresh);
  const [confirm, setConfirm] = useState<JobActionName | null>(null);
  const [reason, setReason] = useState("Parked from StackSpirit.");
  const [busy, setBusy] = useState<JobActionName | null>(null);
  const [error, setError] = useState<string | null>(null);

  if (!job) {
    return (
      <Drawer open={open} title="Job" onClose={onClose}>
        <p className="muted">No job selected.</p>
      </Drawer>
    );
  }

  const chip = jobStateChip(job.state);
  const eligibility = jobActionEligibility(job, mutationsAllowed);
  const disabledReasons = [...new Set(
    (Object.keys(ACTION_LABELS) as JobActionName[])
      .map((action) => eligibility[action])
      .filter((entry) => !entry.enabled && entry.reason)
      .map((entry) => entry.reason as string),
  )];

  async function runAction(action: JobActionName): Promise<void> {
    if (!bridge) {
      setError("The bridge is not initialized.");
      return;
    }
    setBusy(action);
    setError(null);
    try {
      await bridge.fleet.jobAction(
        job!.job_id,
        action satisfies JobAction,
        action === "park" ? reason.trim() : null,
      );
      // A write always triggers a refresh; the drawer never invents the new state.
      await refresh();
      setConfirm(null);
      onClose();
    } catch (caught) {
      const spirit = toSpiritError(caught);
      if (spirit.code === "conflict") {
        setError("VOWP changed this job; refreshing.");
        await refresh();
      } else {
        setError(spirit.hint ? `${spirit.message} ${spirit.hint}` : spirit.message);
      }
    } finally {
      setBusy(null);
    }
  }

  return (
    <Drawer
      open={open}
      title={`Job ${job.job_id}`}
      onClose={onClose}
      footer={
        <div className="job-drawer__actions">
          {(Object.keys(ACTION_LABELS) as JobActionName[]).map((action) => {
            const entry = eligibility[action];
            const danger = action === "cancel";
            return (
              <Button
                key={action}
                variant={danger ? "danger" : "secondary"}
                size="sm"
                disabled={!entry.enabled || busy !== null}
                title={entry.reason ?? undefined}
                onClick={() => setConfirm(action)}
                data-testid={`job-action-${action}`}
              >
                {busy === action ? "Working…" : ACTION_LABELS[action]}
              </Button>
            );
          })}
        </div>
      }
    >
      <div className="job-drawer" data-testid="job-drawer">
        <div className="job-drawer__headline">
          <span className="mono">{job.job_id}</span>
          <Badge tone={chip.tone}>{chip.label}</Badge>
          <Badge tone="info">{job.kind}</Badge>
        </div>

        <dl className="job-drawer__facts">
          <div>
            <dt>Project</dt>
            <dd className="mono">{job.project}</dd>
          </div>
          <div>
            <dt>Kind</dt>
            <dd>{job.kind}</dd>
          </div>
          <div>
            <dt>Role</dt>
            <dd>{job.role}</dd>
          </div>
          <div>
            <dt>State</dt>
            <dd>{job.state}</dd>
          </div>
          <div>
            <dt>Priority</dt>
            <dd>{job.priority > 0 ? job.priority : "not reported"}</dd>
          </div>
          <div>
            <dt>Age</dt>
            <dd>{formatAgeParts(job.age_secs)}</dd>
          </div>
        </dl>

        {job.reason || job.detail ? (
          <section className="job-drawer__section" aria-label="Blocked or parked detail">
            <h3 className="section-title">Reason</h3>
            {job.reason ? <p>{job.reason}</p> : null}
            {job.detail ? (
              <p className="muted small">{truncateText(job.detail, 500)}</p>
            ) : null}
          </section>
        ) : null}

        {job.gate ? (
          <section className="job-drawer__section" aria-label="Gate failure">
            <h3 className="section-title">Gate failure</h3>
            <p>
              <Badge tone="verifier">{job.gate}</Badge>{" "}
              {job.gate_at ? (
                <span className="muted small">{formatDateTime(job.gate_at)}</span>
              ) : null}
            </p>
            {job.gate_evidence ? (
              <p className="small job-drawer__evidence">{job.gate_evidence}</p>
            ) : null}
          </section>
        ) : null}

        <section className="job-drawer__section" aria-label="Executor and session">
          <h3 className="section-title">Executor and session</h3>
          <p className="small">
            {job.executor ? (
              <>
                <span className="mono">{job.executor.raw}</span>
                <span className="muted"> · {job.executor.display}</span>
              </>
            ) : (
              "No executor recorded (shell, gate or merge run)."
            )}
          </p>
          <p className="small muted">
            Session: <span className="mono">{job.session_id ?? "not reported"}</span>
          </p>
        </section>

        {disabledReasons.length > 0 ? (
          <p className="muted small" data-testid="job-action-reason">
            {disabledReasons.join(" ")}
          </p>
        ) : null}
        {error ? (
          <p className="notice notice--danger" role="alert">
            {error}
          </p>
        ) : null}
        <p className="muted small">
          Full run logs and terminals are not part of the Desktop MVP. Actions ask VOWP,
          then refresh this snapshot; the drawer never assumes the job moved.
        </p>
      </div>

      <Modal
        open={confirm !== null}
        title={
          confirm === "cancel"
            ? `Cancel ${job.job_id}?`
            : confirm === "retry"
              ? `Retry ${job.job_id}?`
              : `Park ${job.job_id}?`
        }
        danger={confirm === "cancel"}
        onClose={() => setConfirm(null)}
        footer={
          <div className="inline-actions">
            <Button variant="ghost" size="sm" onClick={() => setConfirm(null)}>
              Keep job
            </Button>
            <Button
              variant={confirm === "cancel" ? "danger" : "primary"}
              size="sm"
              disabled={
                busy !== null ||
                (confirm === "park" && reason.trim().length === 0) ||
                (confirm !== null && !eligibility[confirm].enabled)
              }
              onClick={() => {
                if (confirm) void runAction(confirm);
              }}
            >
              {confirm === "cancel" ? "Cancel job" : confirm === "retry" ? "Retry job" : "Park job"}
            </Button>
          </div>
        }
      >
        <p className="small">
          {confirm === "cancel"
            ? "Cancelling is a VOWP command. The job leaves the active lanes after the next refresh."
            : confirm === "retry"
              ? "VOWP decides whether the job becomes ready again. This app does not optimistically move it."
              : "Parking keeps the job in Parked & Blocked until someone retries or cancels it."}
        </p>
        {confirm === "park" ? (
          <label className="field job-drawer__park-reason">
            <span className="field__label">Reason</span>
            <input
              className="input"
              value={reason}
              maxLength={200}
              onChange={(event) => setReason(event.target.value)}
            />
            <span className="field__hint">1–200 characters. Never include a secret.</span>
          </label>
        ) : null}
      </Modal>
    </Drawer>
  );
}
