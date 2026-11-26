import { useState, type HTMLAttributes } from "react";

import { toSpiritError } from "../bridge/errors";
import type { JobCardView } from "../bridge/wire";
import { harnessLabel, executorProviderLabel, parseExecutor } from "../domain/executor";
import { formatAge } from "../domain/format";
import { useFleet, useFleetStore, useMutationsAllowed } from "../state/fleetStore";
import { Meter, Slots, useServerHealth } from "./health";
import { useShellUi, type PanelContent } from "./shellUi";
import { activeRunningWorkers, findJob, jobName, jobTone, LANE_INFO, openInbox } from "./summary";

export interface ContextPanelProps {
  /** The project in view, or null for the global orchestrator and server pages. */
  project: string | null;
  onOpenServer: () => void;
  onOpenWorkers: (project: string) => void;
  onOpenSession: (project: string) => void;
  /** Opens the live read-only view of a worker's session. */
  onOpenWorkerSession?: (project: string, jobId: string, sessionId: string) => void;
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
  const running = activeRunningWorkers(fleet, project);
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
  onOpenWorkerSession,
}: { content: Extract<PanelContent, { kind: "job" }> } & ContextPanelProps) {
  const fleet = useFleet();
  const openPanel = useShellUi((state) => state.openPanel);
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
  const sessionId = job.session_id ?? worker?.session_id ?? null;
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
        {item.stageTrail ? (
          <>
            <dt>Stage trail</dt>
            <dd data-testid="job-detail-stage-trail">{item.stageTrail}</dd>
          </>
        ) : null}
        {item.promotedFrom ? (
          <>
            <dt>Replaces</dt>
            <dd>
              <button
                type="button"
                className="nx-link"
                data-testid="job-detail-promoted-from"
                onClick={() =>
                  openPanel({ kind: "job", project: job.project, jobId: item.promotedFrom ?? "" })
                }
              >
                {item.promotedFrom} (finished)
              </button>
            </dd>
          </>
        ) : null}
        <dt>Age</dt>
        <dd>{formatAge(job.age_secs)}</dd>
        {job.executor ? (
          <>
            {job.executor.provider ? (
              <>
                <dt>Provider</dt>
                <dd>{executorProviderLabel(job.executor.provider)}</dd>
              </>
            ) : null}
            <dt>Harness</dt>
            <dd>{harnessLabel(job.executor.harness)}</dd>
            <dt>Model</dt>
            <dd>{job.executor.display}</dd>
          </>
        ) : null}
        {!job.executor && job.planned_executor ? (
          <>
            {job.planned_executor.provider ? (
              <>
                <dt>Planned provider</dt>
                <dd>{executorProviderLabel(job.planned_executor.provider)}</dd>
              </>
            ) : null}
            <dt>Planned harness</dt>
            <dd>{harnessLabel(job.planned_executor.harness)}</dd>
            <dt>Planned model</dt>
            <dd>{job.planned_executor.display}</dd>
          </>
        ) : null}
        <UsageRows job={job} />
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
      {canChangeModel(item.lane, job) ? <ChangeModel job={job} running={item.lane === "running"} /> : null}
      <div className="nx-job-actions">
        <button type="button" className="nx-btn nx-btn--sm" onClick={() => onOpenSession(job.project)}>
          Ask the orchestrator
        </button>
        {sessionId && onOpenWorkerSession ? (
          <button
            type="button"
            className="nx-btn nx-btn--sm"
            onClick={() => onOpenWorkerSession(job.project, job.job_id, sessionId)}
          >
            Open session
          </button>
        ) : null}
        <button type="button" className="nx-btn nx-btn--quiet nx-btn--sm" onClick={() => onOpenWorkers(job.project)}>
          {job.project} workers
        </button>
      </div>
    </section>
  );
}

/** Compact token count: `842`, `142K`, `1.2M`. */
export function formatTokens(count: number): string {
  if (count < 1000) return String(Math.round(count));
  if (count < 1_000_000) return `${Math.round(count / 1000)}K`;
  return `${(count / 1_000_000).toFixed(1)}M`;
}

/** Context-window fill in whole percent (input + output over the window), or null when unknown. */
export function contextPercent(job: JobCardView): number | null {
  const window = job.context_window;
  if (!window || window <= 0) return null;
  const used = (job.tokens_input ?? 0) + (job.tokens_output ?? 0);
  if (job.tokens_input == null && job.tokens_output == null) return null;
  return Math.min(100, Math.max(0, Math.round((used / window) * 100)));
}

/** Total tokens spent as `142K tok`, with a cache-read hint when reported. */
export function tokenSummary(job: JobCardView): string | null {
  if (job.tokens_input == null && job.tokens_output == null) return null;
  const total = (job.tokens_input ?? 0) + (job.tokens_output ?? 0);
  const cache = job.tokens_cache_read ? ` · ${formatTokens(job.tokens_cache_read)} cached` : "";
  return `${formatTokens(total)} tok${cache}`;
}

/**
 * Usage label by billing mode. A subscription never shows an API dollar amount, and an
 * unknown or missing mode shows nothing rather than a guess.
 */
export function costLabel(job: JobCardView): string | null {
  switch (job.billing_mode) {
    case "api":
      return job.cost_usd != null ? `$${job.cost_usd.toFixed(2)}` : null;
    case "subscription":
      return "Subscription";
    case "free":
      return "Free";
    default:
      return null;
  }
}

/** Mini circular progress ring for the context window. */
export function ContextRing({ percent }: { percent: number }) {
  const radius = 7;
  const circumference = 2 * Math.PI * radius;
  return (
    <svg
      className="nx-ring"
      width="18"
      height="18"
      viewBox="0 0 18 18"
      role="img"
      aria-label={`Context window ${percent}% used`}
    >
      <circle className="nx-ring__track" cx="9" cy="9" r={radius} fill="none" strokeWidth="2.5" />
      <circle
        className="nx-ring__fill"
        cx="9"
        cy="9"
        r={radius}
        fill="none"
        strokeWidth="2.5"
        strokeDasharray={`${(percent / 100) * circumference} ${circumference}`}
        transform="rotate(-90 9 9)"
      />
    </svg>
  );
}

/** Subtle one-line usage summary for an active job row; null when nothing is reported. */
export function UsageInline({ job }: { job: JobCardView }) {
  const percent = contextPercent(job);
  const tokens = tokenSummary(job);
  const cost = costLabel(job);
  if (percent === null && !tokens && !cost) return null;
  return (
    <span className="nx-usage" data-testid="job-usage">
      {percent !== null ? (
        <span className="nx-usage__item">
          <ContextRing percent={percent} />
          <span>{percent}%</span>
        </span>
      ) : null}
      {tokens ? <span className="nx-usage__item">{tokens}</span> : null}
      {cost ? <span className="nx-usage__item">{cost}</span> : null}
    </span>
  );
}

function UsageRows({ job }: { job: JobCardView }) {
  const percent = contextPercent(job);
  const tokens = tokenSummary(job);
  const cost = costLabel(job);
  return (
    <>
      {percent !== null ? (
        <>
          <dt>Context window</dt>
          <dd className="nx-usage__item" data-testid="job-context">
            <ContextRing percent={percent} />
            <span>{percent}%</span>
          </dd>
        </>
      ) : null}
      {tokens ? (
        <>
          <dt>Tokens</dt>
          <dd data-testid="job-tokens">{tokens}</dd>
        </>
      ) : null}
      {cost ? (
        <>
          <dt>Usage / cost</dt>
          <dd data-testid="job-cost">{cost}</dd>
        </>
      ) : null}
    </>
  );
}

// ---- Change model -------------------------------------------------------------------

interface ModelOption {
  id: string;
  reasoning: boolean;
}
interface ProviderOption {
  key: string;
  models: ModelOption[];
}
interface HarnessOption {
  key: "claude-code" | "opencode";
  /** `@effort` for Claude Code, `#variant` for OpenCode. */
  levels: string[];
  providers: ProviderOption[];
}

const CATALOG: HarnessOption[] = [
  {
    key: "claude-code",
    levels: ["low", "medium", "high"],
    providers: [
      {
        key: "anthropic",
        models: [
          { id: "claude-fable-5-1", reasoning: true },
          { id: "claude-opus-5-5", reasoning: true },
          { id: "claude-sonnet-5-5", reasoning: true },
          { id: "claude-haiku-5-5", reasoning: false },
        ],
      },
    ],
  },
  {
    key: "opencode",
    levels: ["high", "max"],
    providers: [
      {
        key: "anthropic",
        models: [
          { id: "anthropic/claude-sonnet-4", reasoning: true },
          { id: "anthropic/claude-opus-4", reasoning: true },
        ],
      },
      {
        key: "deepseek",
        models: [
          { id: "deepseek/deepseek-flash", reasoning: true },
          { id: "deepseek/deepseek-chat", reasoning: false },
        ],
      },
      {
        key: "openrouter",
        models: [
          { id: "openrouter/deepseek/deepseek-chat-v3.1", reasoning: false },
          { id: "openrouter/meta-llama/llama-4-maverick", reasoning: false },
        ],
      },
      {
        key: "openai",
        models: [
          { id: "openai/gpt-5", reasoning: true },
          { id: "openai/gpt-5-mini", reasoning: false },
        ],
      },
    ],
  },
];

/** Queued (pending/ready/parked) and running jobs can change model; finished or gating ones cannot. */
function canChangeModel(lane: string, job: JobCardView): boolean {
  return lane === "running" || lane === "ready" || job.state === "parked";
}

function buildExecutor(harness: HarnessOption, model: string, level: string): string {
  if (!level) return `${harness.key}:${model}`;
  return `${harness.key}:${model}${harness.key === "claude-code" ? "@" : "#"}${level}`;
}

function ChangeModel({ job, running }: { job: JobCardView; running: boolean }) {
  const bridge = useFleetStore((state) => state.bridge);
  const mutationsAllowed = useMutationsAllowed();
  const current = job.executor ?? job.planned_executor ?? null;
  const [open, setOpen] = useState(false);
  const [harnessKey, setHarnessKey] = useState<HarnessOption["key"]>(
    current?.harness === "opencode" ? "opencode" : "claude-code",
  );
  const harness = CATALOG.find((entry) => entry.key === harnessKey) ?? CATALOG[0]!;
  const [providerKey, setProviderKey] = useState(() => {
    const key = current?.provider === "claude" ? "anthropic" : (current?.provider ?? "");
    const home = CATALOG.find((entry) => entry.key === (current?.harness === "opencode" ? "opencode" : "claude-code"));
    return home?.providers.some((entry) => entry.key === key) ? key : "";
  });
  const provider = harness.providers.find((entry) => entry.key === providerKey) ?? harness.providers[0]!;
  const [modelId, setModelId] = useState(current?.model ?? "");
  const model = provider.models.find((entry) => entry.id === modelId) ?? provider.models[0]!;
  const [level, setLevel] = useState(current?.effort ?? current?.variant ?? "");
  const levelValue = model.reasoning && harness.levels.includes(level) ? level : "";
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ kind: "ok" | "error"; text: string } | null>(null);

  const executor = buildExecutor(harness, model.id, levelValue);
  const target = parseExecutor(executor).display;

  const apply = async () => {
    if (!bridge) return;
    setBusy(true);
    setMessage(null);
    try {
      await bridge.fleet.setJobExecutor(job.project, job.job_id, executor);
      await useFleetStore.getState().refresh();
      setMessage({
        kind: "ok",
        text: running ? `Switching to ${target} via session handoff.` : `Planned model changed to ${target}.`,
      });
      setConfirming(false);
      setOpen(false);
    } catch (error) {
      setMessage({ kind: "error", text: toSpiritError(error).message });
      setConfirming(false);
    } finally {
      setBusy(false);
    }
  };

  const reset = () => {
    setConfirming(false);
    setOpen(false);
  };

  return (
    <div className="nx-change" data-testid="change-model">
      {!open ? (
        <button
          type="button"
          className="nx-btn nx-btn--sm"
          disabled={!mutationsAllowed}
          onClick={() => {
            setMessage(null);
            setOpen(true);
          }}
        >
          Change model
        </button>
      ) : (
        <form
          className="nx-change__form"
          aria-label="Change model"
          onSubmit={(event) => {
            event.preventDefault();
            if (running) setConfirming(true);
            else void apply();
          }}
        >
          <label>
            <span>Harness</span>
            <select
              value={harnessKey}
              onChange={(event) => {
                setHarnessKey(event.target.value as HarnessOption["key"]);
                setProviderKey("");
                setModelId("");
                setLevel("");
                setConfirming(false);
              }}
            >
              {CATALOG.map((entry) => (
                <option key={entry.key} value={entry.key}>
                  {harnessLabel(entry.key)}
                </option>
              ))}
            </select>
          </label>
          <label>
            <span>Provider</span>
            <select
              value={provider.key}
              onChange={(event) => {
                setProviderKey(event.target.value);
                setModelId("");
                setLevel("");
                setConfirming(false);
              }}
            >
              {harness.providers.map((entry) => (
                <option key={entry.key} value={entry.key}>
                  {executorProviderLabel(entry.key)}
                </option>
              ))}
            </select>
          </label>
          <label>
            <span>Model</span>
            <select
              value={model.id}
              onChange={(event) => {
                setModelId(event.target.value);
                setLevel("");
                setConfirming(false);
              }}
            >
              {provider.models.map((entry) => (
                <option key={entry.id} value={entry.id}>
                  {parseExecutor(`${harness.key}:${entry.id}`).display}
                </option>
              ))}
            </select>
          </label>
          {model.reasoning ? (
            <label>
              <span>Reasoning</span>
              <select
                value={levelValue}
                onChange={(event) => {
                  setLevel(event.target.value);
                  setConfirming(false);
                }}
              >
                <option value="">Default</option>
                {harness.levels.map((entry) => (
                  <option key={entry} value={entry}>
                    {entry}
                  </option>
                ))}
              </select>
            </label>
          ) : null}
          {confirming ? (
            <div className="nx-change__confirm" role="alert">
              <p>
                Switch this running worker via universal session handoff? It will drain the current step, save a
                capsule, and resume on {target}.
              </p>
              <div className="nx-job-actions">
                <button type="button" className="nx-btn nx-btn--sm" disabled={busy} onClick={() => void apply()}>
                  Switch worker
                </button>
                <button type="button" className="nx-btn nx-btn--quiet nx-btn--sm" onClick={() => setConfirming(false)}>
                  Keep current
                </button>
              </div>
            </div>
          ) : (
            <div className="nx-job-actions">
              <button type="submit" className="nx-btn nx-btn--sm" disabled={busy}>
                {running ? "Review switch" : "Apply"}
              </button>
              <button type="button" className="nx-btn nx-btn--quiet nx-btn--sm" onClick={reset}>
                Cancel
              </button>
            </div>
          )}
        </form>
      )}
      {message ? (
        <p
          className={`nx-change__msg${message.kind === "error" ? " nx-change__msg--error" : ""}`}
          role={message.kind === "error" ? "alert" : "status"}
          data-testid="change-model-result"
        >
          {message.text}
        </p>
      ) : null}
    </div>
  );
}
