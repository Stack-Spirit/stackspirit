import { useEffect, useState } from "react";

import { useFleet, useFleetStore } from "../state/fleetStore";
import { FleetCanvas } from "../views/fleet/FleetCanvas";
import { JobRows } from "./JobRows";
import { NodeMap, type MapBranch } from "./NodeMap";
import { PageHeader } from "./PageHeader";
import { useShellUi } from "./shellUi";
import { jobName, jobTone, LANE_INFO, projectJobs, projectSummaries } from "./summary";

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
  // The board filters the shared store by project; clear that when leaving.
  useEffect(() => () => setProject(null), [setProject]);
  const jobs = projectJobs(fleet, project);
  const summary = projectSummaries(fleet).find((item) => item.name === project);
  const selectedId = panel?.kind === "job" && panel.project === project ? panel.jobId : null;

  const branches: MapBranch[] = LANE_INFO.map((lane) => {
    const inLane = jobs.filter((item) => item.lane === lane.key);
    const shown = inLane.slice(0, LEAVES_PER_LANE);
    return {
      node: { id: lane.key, label: lane.label, sub: `${inLane.length}`, tone: inLane.length ? lane.tone : "faint" },
      leaves: shown.map((item) => ({
        id: item.job.job_id,
        label: jobName(item.job),
        sub: `${item.job.kind} · ${item.job.role}`,
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
            ? `${summary.running} running · ${summary.ready} ready · ${summary.review} in review${
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
            <button type="button" className="nx-btn nx-btn--quiet nx-btn--sm" onClick={onOpenSession}>
              Open orchestrator
            </button>
          </>
        }
      />
      <div className="nx-page">
        {mode === "board" ? (
          <div className="nx-page__inner nx-page__inner--wide">
            <FleetCanvas />
          </div>
        ) : (
          <div className="nx-page__inner">
            <div className="nx-block">
              <div className="nx-block__head">
                <h2>Structure</h2>
                <span className="nx-legend">
                  {LANE_INFO.map((lane) => (
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
