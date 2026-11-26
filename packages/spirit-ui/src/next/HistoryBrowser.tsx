import { useId, useState } from "react";

import type { SpiritBridge } from "../bridge";
import { toSpiritError } from "../bridge/errors";
import type { GcReportView, HistoryJobView } from "../bridge/wire";
import { Modal } from "../components";
import { formatBytes, formatDateTime } from "../domain/format";

type HistoryApi = NonNullable<SpiritBridge["history"]>;
type Filter = "all" | "completed" | "cancelled" | "failed";

const FILTERS: ReadonlyArray<{ key: Filter; label: string }> = [
  { key: "all", label: "All" },
  { key: "completed", label: "Completed" },
  { key: "cancelled", label: "Cancelled" },
  { key: "failed", label: "Failed" },
];

const CLASS_LABEL: Record<string, string> = {
  closed_session: "Closed sessions",
  run_output: "Run outputs",
  worktree: "Worktrees",
  cache: "Caches",
  sample: "Host samples",
  job_window: "Job windows",
};

/** `done` and `replanned` both read as completed work; `failed` and `cancelled` stand alone. */
export function historyFilterOf(state: string): Exclude<Filter, "all"> {
  if (state === "cancelled") return "cancelled";
  if (state === "failed") return "failed";
  return "completed";
}

export function filterHistory(
  jobs: HistoryJobView[],
  filter: Filter,
  query: string,
): HistoryJobView[] {
  const needle = query.trim().toLowerCase();
  return jobs.filter((job) => {
    if (filter !== "all" && historyFilterOf(job.state) !== filter) return false;
    if (!needle) return true;
    return [job.job_id, job.title, job.kind, job.role, job.state]
      .join(" ")
      .toLowerCase()
      .includes(needle);
  });
}

function message(error: unknown): string {
  const normalized = toSpiritError(error);
  return normalized.hint
    ? `${normalized.message} ${normalized.hint}`
    : normalized.message;
}

/**
 * On-demand history for one project: nothing is requested until the user clicks
 * **Load history**, and clean-up previews first and deletes only after confirmation.
 */
export function HistoryBrowser({
  project,
  api,
  onLoaded,
}: {
  project: string;
  api: HistoryApi | undefined;
  onLoaded?: (jobs: HistoryJobView[]) => void;
}) {
  const [jobs, setJobs] = useState<HistoryJobView[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<Filter>("all");
  const [query, setQuery] = useState("");
  const [preview, setPreview] = useState<GcReportView | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const [applying, setApplying] = useState(false);
  const [cleanError, setCleanError] = useState<string | null>(null);
  const [freed, setFreed] = useState<GcReportView | null>(null);
  const searchId = useId();

  if (!api) {
    return (
      <p className="nx-rows__empty" data-testid="history-unavailable">
        Job history is not available on this connection.
      </p>
    );
  }

  async function load() {
    if (!api) return;
    setLoading(true);
    setError(null);
    try {
      const next = await api.load(project);
      setJobs(next);
      onLoaded?.(next);
    } catch (caught) {
      setError(message(caught));
    } finally {
      setLoading(false);
    }
  }

  async function openCleanup() {
    if (!api) return;
    setPreviewing(true);
    setCleanError(null);
    setFreed(null);
    try {
      setPreview(await api.gcPreview(project));
    } catch (caught) {
      setCleanError(message(caught));
    } finally {
      setPreviewing(false);
    }
  }

  async function apply() {
    if (!api) return;
    setApplying(true);
    setCleanError(null);
    try {
      setFreed(await api.gcApply(project));
      setPreview(null);
    } catch (caught) {
      setCleanError(message(caught));
    } finally {
      setApplying(false);
    }
  }

  const visible = jobs ? filterHistory(jobs, filter, query) : [];
  const count = (key: Filter) =>
    (jobs ?? []).filter(
      (job) => key === "all" || historyFilterOf(job.state) === key,
    ).length;

  return (
    <div className="nx-history" data-testid="history-browser">
      <div className="nx-rows__tools">
        <button
          type="button"
          className="nx-btn nx-btn--sm"
          onClick={() => void load()}
          disabled={loading}
        >
          {loading ? "Loading…" : jobs ? "Reload history" : "Load history"}
        </button>
        <button
          type="button"
          className="nx-btn nx-btn--quiet nx-btn--sm"
          onClick={() => void openCleanup()}
          disabled={previewing}
        >
          {previewing ? "Checking…" : "Clean up history"}
        </button>
      </div>
      {error ? (
        <p className="nx-rows__empty" role="alert" data-testid="history-error">
          {error}
        </p>
      ) : null}
      {cleanError && !preview ? (
        <p className="nx-rows__empty" role="alert" data-testid="cleanup-error">
          {cleanError}
        </p>
      ) : null}
      {freed ? (
        <p
          className="nx-rows__empty"
          role="status"
          data-testid="cleanup-result"
        >
          Freed {freed.deleted_files}{" "}
          {freed.deleted_files === 1 ? "file" : "files"} (
          {formatBytes(freed.freed_bytes)}).
          {freed.errors.length > 0
            ? ` ${freed.errors.length} could not be removed.`
            : ""}
        </p>
      ) : null}

      {jobs ? (
        <section aria-label="Historic jobs">
          <div className="nx-rows__tools">
            <label htmlFor={searchId} className="nx-sr">
              Search history
            </label>
            <input
              id={searchId}
              className="nx-search"
              type="search"
              placeholder="Search history"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
            />
            <div
              className="nx-segment"
              role="group"
              aria-label="History status"
            >
              {FILTERS.map(({ key, label }) => (
                <button
                  key={key}
                  type="button"
                  aria-pressed={filter === key}
                  onClick={() => setFilter(key)}
                >
                  {label}
                  <span className="nx-segment__count">{count(key)}</span>
                </button>
              ))}
            </div>
          </div>
          {visible.length === 0 ? (
            <p className="nx-rows__empty" data-testid="history-list-empty">
              {jobs.length === 0 ? "No historic jobs." : "No jobs match."}
            </p>
          ) : (
            <ul className="nx-rows__list" data-testid="history-list">
              {visible.map((job) => (
                <li key={job.job_id}>
                  <div className="nx-row">
                    <span
                      className={`nx-row__dot nx-tone--${job.state === "failed" ? "danger" : "faint"}`}
                      aria-hidden="true"
                    />
                    <span className="nx-row__text">
                      <span className="nx-row__title">
                        {job.title.trim() || job.job_id}
                      </span>
                      <span className="nx-row__sub">
                        {job.kind} · {job.role} ·{" "}
                        {job.updated_at
                          ? formatDateTime(job.updated_at)
                          : "unknown time"}
                      </span>
                      <span className="nx-row__badges">
                        <span
                          className="nx-chip nx-chip--muted"
                          data-testid="history-state"
                        >
                          {job.state}
                        </span>
                      </span>
                    </span>
                    {job.title.trim() ? (
                      <span className="nx-row__id">{job.job_id}</span>
                    ) : null}
                  </div>
                </li>
              ))}
            </ul>
          )}
        </section>
      ) : null}

      <Modal
        open={preview !== null}
        title="Clean up history"
        onClose={() => (applying ? undefined : setPreview(null))}
        danger
        footer={
          <>
            <button
              type="button"
              className="nx-btn nx-btn--quiet"
              onClick={() => setPreview(null)}
              disabled={applying}
            >
              Cancel
            </button>
            <button
              type="button"
              className="nx-btn"
              data-testid="cleanup-confirm"
              onClick={() => void apply()}
              disabled={applying || (preview?.eligible_files ?? 0) === 0}
            >
              {applying ? "Cleaning…" : "Clean up"}
            </button>
          </>
        }
      >
        {preview ? (
          <div data-testid="cleanup-preview">
            <p>
              Old closed sessions, run outputs, worktrees, caches, host samples and
              job windows are listed below.{" "}
              <strong>
                {preview.eligible_files}{" "}
                {preview.eligible_files === 1 ? "file" : "files"} (
                {formatBytes(preview.eligible_bytes)})
              </strong>{" "}
              would be freed.
            </p>
            <ul>
              {preview.classes.map((item) => (
                <li key={item.class}>
                  {CLASS_LABEL[item.class] ?? item.class}: {item.eligible_files}{" "}
                  of {item.files} files ({formatBytes(item.eligible_bytes)})
                </li>
              ))}
            </ul>
            <p>
              Active work and the job database are never touched
              {preview.refused > 0
                ? `; ${preview.refused} in-use ${preview.refused === 1 ? "item is" : "items are"} protected`
                : ""}
              .
            </p>
            {preview.eligible_files === 0 ? <p>Nothing to clean up.</p> : null}
            {cleanError ? (
              <p role="alert" data-testid="cleanup-error">
                {cleanError}
              </p>
            ) : null}
          </div>
        ) : null}
      </Modal>
    </div>
  );
}
