import { useEffect, useState } from "react";

import { useFleet, useFleetStore } from "../state/fleetStore";
import { FleetCanvas } from "../views/fleet/FleetCanvas";
import { InboxView } from "./InboxView";
import { JobRows } from "./JobRows";
import { NodeMap, type MapBranch } from "./NodeMap";
import { PageHeader } from "./PageHeader";
import { useShellUi } from "./shellUi";
import { historyJobs, openInbox, jobName, jobPhase, jobTone, LANE_INFO, projectJobs, projectSummaries, providerModel, waitReason } from "./summary";

const LEAVES_PER_LANE = 6;

/**
 * Full-page view of one project's workers: a node map from the project's
 * orchestrator through each lane to its jobs, then the jobs as rows. The classic
 * lane board (with its job actions) stays available as "Board".
 */
export function WorkersPage({ project, onOpenSession }: { project: string; onOpenSession: () => void }) {
  const fleet = useFleet();
  const setProject = useFleetStore((state) => state.setProject);
  const panel = useShellUi((state) => state.panel);
  const openPanel = useShellUi((state) => state.openPanel);
  const [mode, setMode] = useState<"map" | "board">("map");
  // Active work by default; history is one explicit click away.
  const [tab, setTab] = useState<"active" | "history" | "inbox">("active");
  // The board filters the shared store by project; clear that when leaving.
  useEffect(() => () => setProject(null), [setProject]);
  const jobs = projectJobs(fleet, project);
  const summary = projectSummaries(fleet).find((item) => item.name === project);
  const selectedId = panel?.kind === "job" && panel.project === project ? panel.jobId : null;

  const history = historyJobs(fleet, project);
  const inboxCount = openInbox(fleet, project).length;
  const activeLanes = LANE_INFO.filter(
    (lane) =>
      lane.key !== "done" && (lane.key !== "parked_blocked" || jobs.some((item) => item.lane === "parked_blocked")),
  );

  const branches: MapBranch[] = activeLanes.map((lane) => {
    const inLane = jobs.filter((item) => item.lane === lane.key);
    const shown = inLane.slice(0, LEAVES_PER_LANE);
    return {
      node: { id: lane.key, label: lane.label, sub: `${inLane.length}`, tone: inLane.length ? lane.tone : "faint" },
      leaves: shown.map((item) => ({
        id: item.job.job_id,
        label: jobName(item.job),
        sub:
          item.lane === "ready"
            ? `Waiting: ${waitReason(item.job)}`
            : [jobPhase(item), providerModel(item.job)].filter(Boolean).join(" · ") ||
              (item.job.kind === "unknown" ? (item.job.reason ?? "needs attention") : `${item.job.kind} · ${item.job.role}`),
        tone: jobTone(item),
        selected: selectedId === item.job.job_id,
        onSelect: () => openPanel({ kind: "job", project, jobId: item.job.job_id }),
      })),
      more: inLane.length - shown.length || undefined,
    };
  });

  return (
    <section className="nx-main" aria-label={`${project} workers`}>
      <PageHeader
        title={`${project} · Workers`}
        sub={
          summary
            ? `${summary.running} running · ${summary.ready} queued · ${summary.review} in review${
                summary.blocked ? ` · ${summary.blocked} need attention` : ""
              }`
            : undefined
        }
        actions={
          <>
            <div className="nx-segment" role="group" aria-label="View">
              <button type="button" aria-pressed={mode === "map"} onClick={() => setMode("map")}>
                Map
              </button>
              <button
                type="button"
                aria-pressed={mode === "board"}
                onClick={() => {
                  setProject(project);
                  setMode("board");
                }}
              >
                Board
              </button>
            </div>
            <div className="nx-segment" role="group" aria-label="Jobs shown">
              <button type="button" aria-pressed={tab === "active"} onClick={() => setTab("active")}>
                Active
              </button>
              <button type="button" aria-pressed={tab === "history"} onClick={() => setTab("history")}>
                History
                <span className="nx-segment__count" data-testid="history-count">
                  {summary?.done ?? 0}
                </span>
              </button>
              <button type="button" aria-pressed={tab === "inbox"} onClick={() => setTab("inbox")} data-testid="project-inbox-tab">
                Inbox
                <span className="nx-count nx-segment__badge" data-testid="project-inbox-count" aria-label={`${inboxCount} open`}>
                  {inboxCount}
                </span>
              </button>
            </div>
            <button type="button" className="nx-btn nx-btn--quiet nx-btn--sm" onClick={onOpenSession}>
              Open orchestrator
            </button>
          </>
        }
      />
      <div className="nx-page">
        {tab === "inbox" ? (
          <div className="nx-page__inner" data-testid="project-inbox">
            <InboxView
              scope={{ project }}
              onOpenSession={() => onOpenSession()}
            />
          </div>
        ) : tab === "history" ? (
          <div className="nx-page__inner" data-testid="history-view">
            <div className="nx-block">
              <div className="nx-block__head">
                <h2>History</h2>
              </div>
              <p className="nx-rows__empty" data-testid="history-summary">
                {summary?.done ?? 0} completed · {summary?.cancelled ?? 0} cancelled · {history.length} past gate
                failures and cancellations listed
              </p>
              {summary?.lastMerge ? (
                <p className="nx-rows__empty" data-testid="history-last-merge">
                  Last merge: {summary.lastMerge.job_id} · {summary.lastMerge.branch} ·{" "}
                  {summary.lastMerge.commit.slice(0, 8)}
                </p>
              ) : null}
              {history.length === 0 ? null : (
                <ul className="nx-rows__list">
                  {history.map((job, index) => (
                    <li key={`${job.job_id}:${index}`}>
                      <div className="nx-row">
                        <span className="nx-row__dot nx-tone--faint" aria-hidden="true" />
                        <span className="nx-row__text">
                          <span className="nx-row__title">{jobName(job)}</span>
                          <span className="nx-row__sub">
                            {job.gate ? `Gate failed: ${job.gate}` : job.state}
                            {job.gate_evidence ? ` · ${job.gate_evidence}` : ""}
                          </span>
                        </span>
                        <span className="nx-row__id">{job.job_id}</span>
                      </div>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </div>
        ) : mode === "board" ? (
          <div className="nx-page__inner nx-page__inner--wide">
            <FleetCanvas />
          </div>
        ) : (
          <div className="nx-page__inner">
            <div className="nx-block">
              <div className="nx-block__head">
                <h2>Structure</h2>
                <span className="nx-legend">
                  {activeLanes.map((lane) => (
                    <span key={lane.key}>
                      <i className={`nx-tone--${lane.tone}`} />
                      {lane.label}
                    </span>
                  ))}
                </span>
              </div>
              <NodeMap
                label={`${project} structure`}
                root={{ id: "orchestrator", label: `${project} orchestrator`, sub: summary?.state ?? "", tone: "ok", onSelect: onOpenSession }}
                branches={branches}
              />
            </div>
            <div className="nx-block">
              <div className="nx-block__head">
                <h2>Jobs</h2>
              </div>
              <JobRows
                jobs={jobs}
                selectedId={selectedId}
                showProject={false}
                onSelect={(item) => openPanel({ kind: "job", project, jobId: item.job.job_id })}
              />
            </div>
          </div>
        )}
      </div>
    </section>
  );
}
