import { latencyLabel } from "../domain/format";
import { useConnection, useFleet, useFleetStore } from "../state/fleetStore";
import { CapacityRail } from "../views/capacity/CapacityRail";
import { ClusterSection } from "./ClusterSection";
import { Meter, Slots, useServerHealth } from "./health";
import { NodeMap, type MapBranch } from "./NodeMap";
import { PageHeader } from "./PageHeader";
import { useShellUi } from "./shellUi";
import { jobName, openInbox, jobPhase, projectJobs, projectSummaries, providerModel, waitReason } from "./summary";

const LEAVES_PER_PROJECT = 4;

/**
 * Full-page server view: health in detail, then a node map of every project and
 * what its workers are doing, then the projects as rows.
 */
export function ServerPage({ serverLabel, onOpenProject, onFormCluster, onPromote, onOpenInbox, onOpenFiles }: { serverLabel: string; onOpenProject: (project: string) => void; onFormCluster: () => void; onPromote: (target: string | null) => void; onOpenInbox?: () => void; onOpenFiles?: () => void }) {
  const fleet = useFleet();
  const connection = useConnection();
  const refresh = useFleetStore((state) => state.refresh);
  const health = useServerHealth();
  const panel = useShellUi((state) => state.panel);
  const openPanel = useShellUi((state) => state.openPanel);
  const projects = projectSummaries(fleet);
  const supervisor = fleet?.supervisor;
  const globalInbox = openInbox(fleet, null);
  const totalInbox = openInbox(fleet).length;

  const status = [
    health.stale ? "Data is stale" : "Connected",
    connection.latency ? latencyLabel(connection.latency.last_ms) : null,
    supervisor ? (supervisor.running ? (supervisor.paused ? "supervisor paused" : "supervisor running") : "supervisor stopped") : null,
  ]
    .filter(Boolean)
    .join(" · ");

  const branches: MapBranch[] = projects.map((project) => {
    const running = projectJobs(fleet, project.name).filter((item) => item.lane === "running");
    const attention = projectJobs(fleet, project.name).filter((item) => item.lane === "parked_blocked");
    const reviewing = projectJobs(fleet, project.name).filter((item) => item.lane === "review_fix");
    const ready = projectJobs(fleet, project.name).filter((item) => item.lane === "ready");
    const queuedJobs = [...reviewing, ...ready];
    const shown = [...running, ...reviewing, ...attention, ...ready].slice(0, LEAVES_PER_PROJECT);
    const queued = project.ready + project.review;
    return {
      node: {
        id: project.name,
        label: project.name,
        sub:
          project.state !== "active"
            ? project.state
            : `${project.running} running · ${queued} queued${project.inbox ? ` · ${project.inbox} inbox` : ""}`,
        tone: project.state !== "active" ? "faint" : project.blocked ? "warn" : project.running ? "ok" : "neutral",
        onSelect: () => onOpenProject(project.name),
      },
      leaves: shown.map((item) => ({
        id: `${project.name}:${item.job.job_id}`,
        label: jobName(item.job),
        sub:
          item.lane === "running"
            ? [jobPhase(item), providerModel(item.job)].filter(Boolean).join(" · ")
            : item.lane === "ready"
              ? `queued · ${waitReason(item.job)}`
              : item.lane === "review_fix"
                ? [jobPhase(item), providerModel(item.job)].filter(Boolean).join(" · ")
                : (item.job.reason ?? "needs attention"),
        tone: item.lane === "running" ? "ok" : item.lane === "parked_blocked" ? "warn" : "neutral",
        selected: panel?.kind === "job" && panel.jobId === item.job.job_id,
        onSelect: () => openPanel({ kind: "job", project: project.name, jobId: item.job.job_id }),
      })),
      more: Math.max(0, running.length + queuedJobs.length + attention.length - shown.length) || undefined,
      onMore: () => onOpenProject(project.name),
    };
  });

  return (
    <section className="nx-main" aria-label="Server">
      <PageHeader
        title={serverLabel}
        sub={status}
        actions={
          <>
            {onOpenFiles ? (
              <button type="button" className="nx-btn nx-btn--quiet nx-btn--sm" onClick={onOpenFiles} data-testid="server-browse-files">
                Browse files
              </button>
            ) : null}
            <button type="button" className="nx-btn nx-btn--quiet nx-btn--sm" onClick={() => void refresh()}>
              Refresh
            </button>
          </>
        }
      />
      <div className="nx-page">
        <div className="nx-page__inner">
          {health.brake ? (
            <div className="nx-banner nx-banner--warn" role="status">
              <strong>Brake on:</strong> new workers are paused.
              {health.brakeReasons.length ? ` ${health.brakeReasons.join(" · ")}` : ""}
            </div>
          ) : null}

          <div className="nx-cards">
            <div className="nx-card">
              <span className="nx-card__label">Workers</span>
              <div className="nx-workers">
                <span className="nx-workers__big">{health.running}</span>
                <span className="nx-workers__of">of {health.target ?? "?"} active</span>
              </div>
              <Slots running={health.running} slots={health.slots} />
              {health.advised != null ? <span className="nx-card__foot">StackSpirit advises {health.advised}</span> : null}
            </div>
            <div className="nx-card">
              <span className="nx-card__label">Memory</span>
              <Meter label="Used" ratio={health.ramRatio} value={health.ramText} />
            </div>
            <div className="nx-card">
              <span className="nx-card__label">Disk</span>
              <Meter label="Home" ratio={null} value={health.diskText} />
              {health.diskFloorText ? <span className="nx-card__foot">{health.diskFloorText}</span> : null}
            </div>
            <div className="nx-card">
              <span className="nx-card__label">CPU</span>
              <Meter label="Load" ratio={health.cpuRatio} value={health.cpuText} />
            </div>
          </div>

          {onOpenInbox ? (
            <div className="nx-block" data-testid="server-inbox">
              <div className="nx-block__head">
                <h2>Needs you</h2>
                <button type="button" className="nx-btn nx-btn--quiet nx-btn--sm" onClick={onOpenInbox} data-testid="server-inbox-link">
                  Open Inbox{totalInbox > 0 ? ` (${totalInbox})` : ""} →
                </button>
              </div>
              {globalInbox.length === 0 ? (
                <p className="nx-rows__empty">No server-level items waiting. Project items are in the Inbox.</p>
              ) : (
                <ul className="nx-rows__list">
                  {globalInbox.slice(0, 3).map((item) => (
                    <li key={item.id}>
                      <button type="button" className="nx-row" onClick={onOpenInbox}>
                        <span className="nx-row__dot nx-tone--warn" aria-hidden="true" />
                        <span className="nx-row__text">
                          <span className="nx-row__title">{item.title}</span>
                          <span className="nx-row__sub">{item.body ?? item.kind}</span>
                        </span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          ) : null}

          <ClusterSection onFormCluster={onFormCluster} onPromote={onPromote} />

          <div className="nx-block">
            <div className="nx-block__head">
              <h2>Structure</h2>
              <span className="nx-legend">
                <span><i className="nx-tone--ok" />Running</span>
                <span><i className="nx-tone--warn" />Needs attention</span>
                <span><i className="nx-tone--neutral" />Idle</span>
                <span><i className="nx-tone--faint" />Paused</span>
              </span>
            </div>
            {projects.length === 0 ? (
              <p className="nx-rows__empty">No projects on this server yet.</p>
            ) : (
              <NodeMap
                label="Server structure"
                root={{ id: "server", label: serverLabel, sub: `${projects.length} projects`, tone: health.stale ? "warn" : "ok" }}
                branches={branches}
              />
            )}
          </div>

          <details className="nx-block nx-disclosure">
            <summary>
              <h2>Capacity controls</h2>
              <span className="nx-row-item__meta">Target workers, brake, limits and waiting jobs</span>
            </summary>
            <div className="nx-classic">
              <CapacityRail />
            </div>
          </details>

          <div className="nx-block">
            <div className="nx-block__head">
              <h2>Projects</h2>
            </div>
            <ul className="nx-rows__list">
              {projects.map((project) => (
                <li key={project.name}>
                  <button type="button" className="nx-row" onClick={() => onOpenProject(project.name)}>
                    <span
                      className={`nx-row__dot nx-tone--${
                        project.state !== "active" ? "faint" : project.blocked ? "warn" : project.running ? "ok" : "neutral"
                      }`}
                      aria-hidden="true"
                    />
                    <span className="nx-row__text">
                      <span className="nx-row__title">{project.name}</span>
                      <span className="nx-row__sub">
                        {project.state} · {project.running} running · {project.ready} queued · {project.review} in review
                        {project.blocked ? ` · ${project.blocked} need attention` : ""}
                        {project.inbox ? ` · ${project.inbox} in inbox` : ""}
                      </span>
                    </span>
                    <span className="nx-row__id">Workers →</span>
                  </button>
                </li>
              ))}
            </ul>
          </div>
        </div>
      </div>
    </section>
  );
}
