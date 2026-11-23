import { useId, useState } from "react";

import { formatAge } from "../domain/format";
import { jobName, jobPhase, jobTone, LANE_INFO, providerModel, type LaneKey, type ProjectJob, waitReason } from "./summary";

export interface JobRowsProps {
  jobs: ProjectJob[];
  selectedId: string | null;
  onSelect: (item: ProjectJob) => void;
  showProject: boolean;
}

type Filter = "all" | LaneKey;

/**
 * Jobs as rows, like DeepSeek Harness's Automation page: search, a status filter,
 * and one summary line per row. Selecting a row opens it in the details panel.
 */
export function JobRows({ jobs, selectedId, onSelect, showProject }: JobRowsProps) {
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const searchId = useId();
  const needle = query.trim().toLowerCase();

  const visible = jobs.filter((item) => {
    if (filter !== "all" && item.lane !== filter) return false;
    if (!needle) return true;
    const { job } = item;
    return [job.job_id, job.title ?? "", job.project, job.kind, job.role, job.reason ?? ""]
      .join(" ")
      .toLowerCase()
      .includes(needle);
  });

  // Only lanes that have jobs get a filter, so an empty "Needs attention" never shows.
  const filterKeys = LANE_INFO.map((lane) => lane.key).filter((key) => jobs.some((item) => item.lane === key));
  const count = (key: Filter) => (key === "all" ? jobs.length : jobs.filter((item) => item.lane === key).length);

  return (
    <section className="nx-rows" aria-label="Jobs">
      <div className="nx-rows__tools">
        <label htmlFor={searchId} className="nx-sr">
          Search jobs
        </label>
        <input
          id={searchId}
          className="nx-search"
          type="search"
          placeholder="Search jobs"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
        <div className="nx-segment" role="group" aria-label="Status">
          {(["all", ...filterKeys] as Filter[]).map((key) => (
            <button key={key} type="button" aria-pressed={filter === key} onClick={() => setFilter(key)}>
              {key === "all" ? "All" : LANE_INFO.find((lane) => lane.key === key)?.label}
              <span className="nx-segment__count">{count(key)}</span>
            </button>
          ))}
        </div>
      </div>

      {visible.length === 0 ? (
        <p className="nx-rows__empty">{jobs.length === 0 ? "No active or queued jobs." : "No jobs match."}</p>
      ) : (
        <ul className="nx-rows__list">
          {visible.map((item) => {
            const { job } = item;
            const lane = LANE_INFO.find((entry) => entry.key === item.lane);
            const summary = [
              lane?.label,
              showProject ? job.project : null,
              job.kind === "unknown" && job.role === "unknown" ? null : `${job.kind} · ${job.role}`,
              `${formatAge(job.age_secs)} old`,
            ]
              .filter(Boolean)
              .join(" · ");
            const model = providerModel(job);
            const phase = jobPhase(item);
            return (
              <li key={`${job.project}:${job.job_id}`}>
                <button
                  type="button"
                  className="nx-row"
                  aria-pressed={selectedId === job.job_id}
                  onClick={() => onSelect(item)}
                >
                  <span className={`nx-row__dot nx-tone--${jobTone(item)}`} aria-hidden="true" />
                  <span className="nx-row__text">
                    <span className="nx-row__title">{jobName(job)}</span>
                    <span className="nx-row__sub">{summary}</span>
                    {phase || model ? (
                      <span className="nx-row__badges" style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
                        {phase ? (
                          <span className="nx-chip nx-chip--muted" data-testid="job-phase">
                            {phase}
                          </span>
                        ) : null}
                        {model ? (
                          <span className="nx-chip nx-chip--muted" data-testid="job-model">
                            {model}
                          </span>
                        ) : null}
                      </span>
                    ) : null}
                    {item.lane === "ready" ? (
                      <span className="nx-row__reason" data-testid="job-wait-reason">
                        Waiting: {waitReason(job)}
                      </span>
                    ) : job.reason ? (
                      <span className="nx-row__reason">{job.reason}</span>
                    ) : null}
                  </span>
                  {jobName(job) !== job.job_id ? (
                    <span className="nx-row__id" title={job.job_id}>
                      {job.job_id}
                    </span>
                  ) : null}
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
