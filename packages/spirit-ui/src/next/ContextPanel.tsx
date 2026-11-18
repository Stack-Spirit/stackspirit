import { useState, type HTMLAttributes } from "react";

import { formatAge } from "../domain/format";
import { useFleet } from "../state/fleetStore";
import { Meter, Slots, useServerHealth } from "./health";
import { useShellUi, type PanelContent } from "./shellUi";
import { findJob, jobName, jobTone, LANE_INFO, openInbox } from "./summary";

export interface ContextPanelProps {
  /** The project in view, or null for the global orchestrator and server pages. */
  project: string | null;
  onOpenServer: () => void;
  onOpenWorkers: (project: string) => void;
  onOpenSession: (project: string) => void;
  /** Opens the full inbox, where questions can be answered. */
  onOpenInbox?: () => void;
  /** Drag handle on the panel's left edge. */
  resizeHandle?: HTMLAttributes<HTMLDivElement>;
}

function title(content: PanelContent): string {
  switch (content.kind) {
    case "overview":
      return "Details";
    case "server":
      return "Server";
    case "inbox":
      return "Inbox";
    case "job":
      return "Job";
  }
}

/**
 * The slide-in details panel. Closed by default; it opens from the header toggle,
 * when a node or row is selected, or when an agent reply points at something.
 * The open/closed width animates, so the chat makes room instead of being covered.
 */
export function ContextPanel(props: ContextPanelProps) {
  const panel = useShellUi((state) => state.panel);
  const closePanel = useShellUi((state) => state.closePanel);
  const open = panel !== null;

  return (
    <aside
      className={`nx-drawer${open ? " nx-drawer--open" : ""}`}
      aria-label="Details panel"
      aria-hidden={!open}
      data-testid="details-panel"
    >
      {panel && props.resizeHandle ? (
        <div className="nx-resizer nx-resizer--panel" aria-label="Resize details panel" {...props.resizeHandle} />
      ) : null}
      {panel ? (
        <div className="nx-drawer__inner">
          <header className="nx-drawer__head">
            <h2>{title(panel)}</h2>
            <button type="button" className="nx-icon-btn" aria-label="Close details panel" onClick={closePanel}>
              ✕
            </button>
          </header>
          <div className="nx-drawer__body">
            {panel.kind === "overview" ? <Overview {...props} /> : null}
            {panel.kind === "server" ? <ServerDetail onOpenServer={props.onOpenServer} /> : null}
            {panel.kind === "inbox" ? <InboxDetail project={props.project} onOpenInbox={props.onOpenInbox} /> : null}
            {panel.kind === "job" ? <JobDetail content={panel} {...props} /> : null}
          </div>
        </div>
      ) : null}
    </aside>
  );
}

function Overview({ project, onOpenServer, onOpenWorkers, onOpenInbox }: ContextPanelProps) {
  const fleet = useFleet();
  const openPanel = useShellUi((state) => state.openPanel);
  const running = (fleet?.running ?? []).filter((worker) => project === null || worker.project === project);
  return (
    <>
      <section className="nx-panel__section" aria-label="Running now">
        <div className="nx-panel__title">
          <span>Running now{project ? ` · ${project}` : ""}</span>
          <span>{running.length}</span>
        </div>
        {running.length === 0 ? (
          <p className="nx-panel__empty">Nothing running right now.</p>
        ) : (
          <ul className="nx-inbox">
            {running.slice(0, 8).map((worker) => (
              <li key={`${worker.project}:${worker.job_id}`}>
                <button
                  type="button"
                  className="nx-inbox__item nx-inbox__item--button"
                  onClick={() => openPanel({ kind: "job", project: worker.project, jobId: worker.job_id })}
                >
                  <span>
                    <span className={`nx-row__dot nx-tone--${worker.stalled ? "warn" : "ok"}`} aria-hidden="true" />{" "}
                    {worker.job_id}
                  </span>
                  <span className="nx-inbox__meta">
                    {project ? null : <span>{worker.project} ·</span>}
                    <span>{worker.role}</span>
                    <span>·</span>
                    <span>{worker.executor?.display ?? worker.kind}</span>
                    <span>·</span>
                    <span>{worker.stalled ? "stalled" : formatAge(worker.age_secs)}</span>
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
        {project ? (
          <button type="button" className="nx-link" onClick={() => onOpenWorkers(project)}>
            All {project} workers →
          </button>
        ) : null}
      </section>
      <InboxDetail project={project} onOpenInbox={onOpenInbox} />
      <ServerDetail onOpenServer={onOpenServer} compact />
    </>
  );
}

function ServerDetail({ onOpenServer, compact = false }: { onOpenServer: () => void; compact?: boolean }) {
  const health = useServerHealth();
  return (
    <section className="nx-panel__section" aria-label="Server health">
      <div className="nx-panel__title">
        <span>Server</span>
        {health.advised != null ? <span>advised {health.advised}</span> : null}
      </div>
      <div className="nx-workers">
        <span className="nx-workers__big">{health.running}</span>
        <span className="nx-workers__of">of {health.target ?? "?"} workers active</span>
      </div>
      <Slots running={health.running} slots={health.slots} />
      <Meter label="Memory" ratio={health.ramRatio} value={health.ramText} compact={compact} />
      <Meter label="Disk" ratio={null} value={health.diskText} compact={compact} />
      <Meter label="CPU load" ratio={health.cpuRatio} value={health.cpuText} compact={compact} />
      {health.brake ? <span className="nx-chip nx-chip--warn">Brake on: new workers paused</span> : null}
      <button type="button" className="nx-link" onClick={onOpenServer}>
        Open server page →
      </button>
    </section>
  );
}

function InboxDetail({ project, onOpenInbox }: { project: string | null; onOpenInbox?: () => void }) {
  const fleet = useFleet();
  const [scope, setScope] = useState<"project" | "all">(project ? "project" : "all");
  const active = project ? scope : "all";
  const items = openInbox(fleet, active === "project" ? project : undefined);
  return (
    <section className="nx-panel__section" aria-label="Inbox">
      <div className="nx-panel__title">
        <span>Inbox</span>
        <span>{openInbox(fleet).length} open</span>
      </div>
      {project ? (
        <div className="nx-tabs" role="tablist" aria-label="Inbox scope">
          <button type="button" role="tab" aria-selected={active === "project"} onClick={() => setScope("project")}>
            {project}
          </button>
          <button type="button" role="tab" aria-selected={active === "all"} onClick={() => setScope("all")}>
            All projects
          </button>
        </div>
      ) : null}
      {items.length === 0 ? (
        <p className="nx-panel__empty">Nothing waiting for you.</p>
      ) : (
        <ul className="nx-inbox">
          {items.slice(0, 8).map((item) => (
            <li key={item.id} className="nx-inbox__item">
              <span>{item.title}</span>
              {item.options.length ? (
                <span className="nx-inbox__options">
                  {item.options.map((option) => (
                    <span key={option} className="nx-chip">
                      {option}
                    </span>
                  ))}
                </span>
              ) : null}
              <span className="nx-inbox__meta">
                <span>{item.kind === "question" ? "Question" : "Review"}</span>
                <span>·</span>
                <span>{item.project ?? "global"}</span>
                <span>·</span>
                <span>{formatAge(item.age_secs)} ago</span>
              </span>
            </li>
          ))}
        </ul>
      )}
      {onOpenInbox ? (
        <button type="button" className="nx-link" onClick={onOpenInbox}>
          Answer in inbox →
        </button>
      ) : null}
    </section>
  );
}

function JobDetail({
  content,
  onOpenWorkers,
  onOpenSession,
}: { content: Extract<PanelContent, { kind: "job" }> } & ContextPanelProps) {
  const fleet = useFleet();
  const item = findJob(fleet, content.project, content.jobId);
  const worker = fleet?.running.find((entry) => entry.project === content.project && entry.job_id === content.jobId);
  if (!item) {
    return (
      <section className="nx-panel__section">
        <p className="nx-panel__empty">
          {content.jobId} is no longer in the latest snapshot. It may have finished or moved.
        </p>
      </section>
    );
  }
  const { job } = item;
  const lane = LANE_INFO.find((entry) => entry.key === item.lane);
  const tone = jobTone(item);
  return (
    <section className="nx-panel__section" aria-label={`Job ${job.job_id}`}>
      <div className="nx-job-head">
        <span className={`nx-chip nx-chip--tone nx-tone-chip--${tone}`}>{lane?.label ?? job.state}</span>
        <span className="nx-row__id">{job.job_id}</span>
      </div>
      <h3 className="nx-job-title">{jobName(job)}</h3>
      <dl className="nx-kv">
        <dt>Project</dt>
        <dd>{job.project}</dd>
        <dt>Kind · role</dt>
        <dd>
          {job.kind} · {job.role}
        </dd>
        <dt>State</dt>
        <dd>{job.state}</dd>
        <dt>Age</dt>
        <dd>{formatAge(job.age_secs)}</dd>
        {job.executor ? (
          <>
            <dt>Model</dt>
            <dd>{job.executor.display}</dd>
          </>
        ) : null}
        {worker ? (
          <>
            <dt>Last activity</dt>
            <dd>{worker.last_event_age_secs != null ? `${formatAge(worker.last_event_age_secs)} ago` : "unknown"}</dd>
          </>
        ) : null}
        {job.gate ? (
          <>
            <dt>Gate</dt>
            <dd>{job.gate}</dd>
          </>
        ) : null}
      </dl>
      {job.reason || job.detail ? (
        <div className="nx-secure__note">
          <span>
            {job.reason ? <strong>{job.reason}. </strong> : null}
            {job.detail}
          </span>
        </div>
      ) : null}
      <div className="nx-job-actions">
        <button type="button" className="nx-btn nx-btn--sm" onClick={() => onOpenSession(job.project)}>
          Ask the orchestrator
        </button>
        <button type="button" className="nx-btn nx-btn--quiet nx-btn--sm" onClick={() => onOpenWorkers(job.project)}>
          {job.project} workers
        </button>
      </div>
    </section>
  );
}
