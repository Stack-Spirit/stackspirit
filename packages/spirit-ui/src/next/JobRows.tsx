import { useId, useState } from "react";

import { formatAge } from "../domain/format";
import { jobName, jobTone, LANE_INFO, type LaneKey, type ProjectJob } from "./summary";

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
          {(["all", ...LANE_INFO.map((lane) => lane.key)] as Filter[]).map((key) => (
            <button key={key} type="button" aria-pressed={filter === key} onClick={() => setFilter(key)}>
              {key === "all" ? "All" : LANE_INFO.find((lane) => lane.key === key)?.label}
              <span className="nx-segment__count">{count(key)}</span>
            </button>
          ))}
        </div>
      </div>

      {visible.length === 0 ? (
        <p className="nx-rows__empty">{jobs.length === 0 ? "No jobs yet." : "No jobs match."}</p>
      ) : (
        <ul className="nx-rows__list">
          {visible.map((item) => {
            const { job } = item;
            const lane = LANE_INFO.find((entry) => entry.key === item.lane);
            const summary = [
              lane?.label,
              showProject ? job.project : null,
              `${job.kind} · ${job.role}`,
              job.executor?.display ?? null,
              `${formatAge(job.age_secs)} old`,
            ]
              .filter(Boolean)
              .join(" · ");
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
                    {job.reason ? <span className="nx-row__reason">{job.reason}</span> : null}
                  </span>
                  <span className="nx-row__id">{job.job_id}</span>
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
