//! On-demand job history (`job ls --all`) and the safe history clean-up (`gc`).
//!
//! Both are explicit user actions and never run on connect or render. The views carry
//! no home-side paths: GC sample paths and job worktree/brief paths are dropped at parse
//! time. GC only ever reports and removes VOWP's own closed-session, run-output and
//! worktree classes; production, truth, open work and secrets stay untouched.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::SpiritError;
use crate::wire::ProjectName;

/// Retention window the History tab uses for clean-up.
pub const HISTORY_GC_OLDER_THAN: &str = "1d";

/// Job states the History browser lists. Active and queued work stays in the live lanes.
const HISTORIC_STATES: [&str; 4] = ["done", "failed", "cancelled", "replanned"];

/// One historic job, as shown in the History browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryJobView {
    /// Job id.
    pub job_id: String,
    /// Owning project.
    pub project: String,
    /// Job title, empty when VOWP has none.
    pub title: String,
    /// Job kind (`build`, `fix`, `verify`, ...).
    pub kind: String,
    /// Job role.
    pub role: String,
    /// Terminal state (`done`, `failed`, `cancelled`, `replanned`).
    pub state: String,
    /// When the job row last changed, as VOWP printed it.
    pub updated_at: Option<String>,
}

/// Per-class totals of one GC pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GcClassView {
    /// Lifecycle class (`closed_sessions`, `run_output`, `worktrees`, ...).
    pub class: String,
    /// Files in the class.
    pub files: u64,
    /// Bytes in the class.
    pub bytes: u64,
    /// Files an apply would remove now.
    pub eligible_files: u64,
    /// Bytes an apply would free now.
    pub eligible_bytes: u64,
}

/// A GC preview or the outcome of an applied GC, path-free.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GcReportView {
    /// True for a preview; nothing was deleted.
    pub dry_run: bool,
    /// Per-class totals; only classes with something to report are kept.
    pub classes: Vec<GcClassView>,
    /// Candidates protected because they belong to active work.
    pub refused: u64,
    /// Files deleted (zero on a preview).
    pub deleted_files: u64,
    /// Bytes freed (zero on a preview).
    pub freed_bytes: u64,
    /// Closed sessions compacted.
    pub compacted_sessions: u64,
    /// Total files an apply would remove now.
    pub eligible_files: u64,
    /// Total bytes an apply would free now.
    pub eligible_bytes: u64,
    /// Errors VOWP reported while applying.
    pub errors: Vec<String>,
}

/// Classes the History clean-up may report; everything else is dropped from the view.
const GC_CLASSES: [&str; 3] = ["closed_sessions", "run_output", "worktrees"];

/// Validates the project for a history call.
pub fn validate_history_project(project: &str) -> Result<(), SpiritError> {
    ProjectName::parse(project)
        .map(|_| ())
        .map_err(|_| SpiritError::invalid_input("project name", "must be a valid slug"))
}

/// Parses `vowp --project <p> --json job ls --all`, keeping historic jobs newest first.
pub fn parse_history_jobs(
    project: &str,
    stdout: &[u8],
) -> Result<Vec<HistoryJobView>, SpiritError> {
    let value: Value =
        serde_json::from_slice(stdout).map_err(|err| SpiritError::parse("job ls", err))?;
    let rows = value
        .as_array()
        .ok_or_else(|| SpiritError::parse("job ls", "expected a JSON array"))?;
    let text = |row: &Value, key: &str| -> String {
        row.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let mut jobs: Vec<HistoryJobView> = rows
        .iter()
        .filter_map(|row| {
            let state = text(row, "state");
            let job_id = text(row, "id");
            if job_id.is_empty() || !HISTORIC_STATES.contains(&state.as_str()) {
                return None;
            }
            // A project-scoped listing only ever contains that project's jobs.
            let row_project = text(row, "project");
            Some(HistoryJobView {
                job_id,
                project: if row_project.is_empty() {
                    project.to_owned()
                } else {
                    row_project
                },
                title: text(row, "title"),
                kind: text(row, "kind"),
                role: text(row, "role"),
                state,
                updated_at: row
                    .get("updated_at")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        })
        .collect();
    // RFC 3339 timestamps in one zone sort lexicographically.
    jobs.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then(a.job_id.cmp(&b.job_id))
    });
    Ok(jobs)
}

/// Parses `vowp gc --json` output into a path-free report.
pub fn parse_gc_report(stdout: &[u8]) -> Result<GcReportView, SpiritError> {
    let value: Value =
        serde_json::from_slice(stdout).map_err(|err| SpiritError::parse("gc", err))?;
    let number = |value: &Value, key: &str| value.get(key).and_then(Value::as_u64).unwrap_or(0);
    let classes: Vec<GcClassView> = value
        .get("classes")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let class = row.get("class").and_then(Value::as_str)?;
                    if !GC_CLASSES.contains(&class) {
                        return None;
                    }
                    Some(GcClassView {
                        class: class.to_owned(),
                        files: number(row, "files"),
                        bytes: number(row, "bytes"),
                        eligible_files: number(row, "eligible_files"),
                        eligible_bytes: number(row, "eligible_bytes"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(GcReportView {
        dry_run: value
            .get("dry_run")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        refused: value
            .get("refused")
            .and_then(Value::as_array)
            .map_or(0, |rows| rows.len() as u64),
        deleted_files: number(&value, "deleted_files"),
        freed_bytes: number(&value, "freed_bytes"),
        compacted_sessions: number(&value, "compacted_sessions"),
        eligible_files: classes.iter().map(|c| c.eligible_files).sum(),
        eligible_bytes: classes.iter().map(|c| c.eligible_bytes).sum(),
        errors: value
            .get("errors")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(|line| line.chars().take(300).collect())
                    .collect()
            })
            .unwrap_or_default(),
        classes,
    })
}
