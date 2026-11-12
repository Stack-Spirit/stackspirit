//! L3 verify: create a verifier job, render its prompt and turn its report into a verdict.
//!
//! Spec: `docs/design/loops.md` §5. A verify job is an ordinary job of kind `verify` that
//! runs in the original's worktree and reaches `done` through the normal gates path; it is
//! never merged. The review gate of the original is satisfied by the `GateResult` written
//! here, not by evaluating a gate in the verifier.

use std::path::PathBuf;

use vowp_graph::JobEvent;
use vowp_protocol::{
    Edge, EdgeKind, Evidence, Gate, GateResult, Job, JobId, JobKind, NewJob, Report, ReportGate, ReviewGate, Role,
    Verdict,
};
use vowp_store::Store;

use crate::CoreError;
use crate::deps::Deps;

/// Builds and inserts the verify job for `original`.
pub(crate) fn create_verify_job(
    store: &Store,
    deps: &Deps,
    original: &Job,
    evidence: &[GateResult],
    diff_summary: &str,
) -> Result<Job, CoreError> {
    create_verify_job_with_suffix(store, deps, original, evidence, diff_summary, "")
}

/// Creates the single retry of a verify job whose report had no usable verdict (T37).
pub(crate) fn create_verify_retry_job(
    store: &Store,
    deps: &Deps,
    original: &Job,
    evidence: &[GateResult],
    diff_summary: &str,
) -> Result<Job, CoreError> {
    create_verify_job_with_suffix(store, deps, original, evidence, diff_summary, "-r1")
}

/// Creates a verify job with an id suffix (`""` or `"-r1"`).
fn create_verify_job_with_suffix(
    store: &Store,
    deps: &Deps,
    original: &Job,
    evidence: &[GateResult],
    diff_summary: &str,
    suffix: &str,
) -> Result<Job, CoreError> {
    let id = JobId::parse(&format!("{}-verify-{}{suffix}", original.id, original.attempts))?;
    if store.job(&id)?.is_some() {
        // invariant: a retried supervisor tick reuses the existing verify job.
        return Ok(store.require_job(&id)?);
    }
    let brief = verify_prompt(original, &original.brief, evidence, diff_summary);
    let mut reads = original.owns.clone();
    reads.extend(original.reads.iter().cloned());
    let new = NewJob {
        id: id.clone(),
        title: format!("verify {}", original.id),
        kind: JobKind::Verify,
        role: Role::Verifier,
        profile: Some("verifier".to_owned()),
        needs: Vec::new(),
        owns: Vec::new(),
        reads,
        gates: Some(vec![Gate::Report(ReportGate::default())]),
        priority: Some(original.priority),
        loop_: Some(original.loop_.clone()),
        brief_text: Some(brief.clone()),
    };
    let verify = store.insert_job(&original.project, &new, "l3")?;
    store.add_edge(&Edge {
        src: verify.id.clone(),
        dst: original.id.clone(),
        kind: EdgeKind::Verifies,
    })?;
    store.set_job_workspace(&verify.id, original.branch.as_deref(), original.worktree.as_deref())?;
    let verify = store.require_job(&verify.id)?;
    let worktree = verify
        .worktree
        .as_ref()
        .map(PathBuf::from)
        .ok_or_else(|| CoreError::Invalid(format!("verify job {} has no worktree", verify.id)))?;
    let brief = vowp_protocol::Brief::from_job(&verify, &brief);
    let brief_path = worktree.join(&verify.brief);
    if let Some(dir) = brief_path.parent() {
        std::fs::create_dir_all(dir).map_err(|source| CoreError::Io {
            path: dir.display().to_string(),
            source,
        })?;
    }
    std::fs::write(&brief_path, brief.render()).map_err(|source| CoreError::Io {
        path: brief_path.display().to_string(),
        source,
    })?;
    let _ = deps;
    Ok(verify)
}

/// Reads the verifier report and writes the review `GateResult` for the original job.
///
/// Returns the event to feed into the original's state machine: `VerifyPassed` or
/// `VerifyFailed`. A report without a usable verdict is [`CoreError::Invalid`]; the caller
/// treats that as `VerifierFailed` (T37).
pub fn on_verify_done(store: &Store, verify_job: &Job) -> Result<JobEvent, CoreError> {
    Ok(on_verify_done_with_findings(store, verify_job)?.0)
}

/// Like [`on_verify_done`], but also returns the verifier findings so the caller can pass
/// them into the `CreateFixJob` effect.
pub fn on_verify_done_with_findings(store: &Store, verify_job: &Job) -> Result<(JobEvent, Vec<String>), CoreError> {
    let original_id = verify_parent(store, verify_job)?;
    let original = store.require_job(&original_id)?;
    let report = read_verify_report(verify_job)?;
    let (verdict, findings) = parse_verdict(&report)?;
    let review_index = original
        .gates
        .iter()
        .position(|gate| matches!(gate, Gate::Review(_)))
        .unwrap_or(0);
    let result = GateResult {
        id: None,
        job_id: original.id.clone(),
        run_id: None,
        attempt: original.attempts,
        gate_index: i32::try_from(review_index).unwrap_or(i32::MAX),
        gate: Gate::Review(ReviewGate::default()),
        passed: verdict == Verdict::Pass,
        evidence: Evidence::Review {
            verify_job: Some(verify_job.id.clone()),
            verdict: Some(verdict),
            findings: findings.clone(),
        },
        output_blob: None,
        duration_ms: 0,
        ts: store.clock().now(),
    };
    store.insert_gate_result(&result)?;
    let event = match verdict {
        Verdict::Pass => JobEvent::VerifyPassed,
        Verdict::Fail => JobEvent::VerifyFailed,
    };
    Ok((event, findings))
}

/// Parses a verifier report's verdict and findings.
///
/// `fail` without at least one finding is not a usable verdict; the caller retries or parks
/// the original (T37) rather than creating an empty fix prompt.
pub fn parse_verdict(report: &Report) -> Result<(Verdict, Vec<String>), CoreError> {
    match report.front_matter.verdict {
        Some(Verdict::Pass) => Ok((Verdict::Pass, Vec::new())),
        Some(Verdict::Fail) => {
            let findings = report.front_matter.findings.clone();
            if findings.is_empty() {
                Err(CoreError::Invalid(
                    "verify report has verdict: fail but no findings".to_owned(),
                ))
            } else {
                Ok((Verdict::Fail, findings))
            }
        }
        None => Err(CoreError::Invalid("verify report has no verdict".to_owned())),
    }
}

/// The verifier prompt (loops.md §5.1).
///
/// Names the exact report file (`job.worktree` joined with `job.report`) and tells the verifier
/// to write it with a tool instead of only printing it in chat.
pub fn verify_prompt(job: &Job, brief: &str, evidence: &[GateResult], diff_summary: &str) -> String {
    let mut lines = String::new();
    for result in evidence {
        lines.push_str(&format!(
            "- gate {} {}: {}\n",
            result.gate_index,
            if result.passed { "passed" } else { "failed" },
            summarise_evidence(&result.evidence),
        ));
    }
    if lines.is_empty() {
        lines.push_str("- (no gate evidence yet)\n");
    }
    let worktree = job.worktree.as_deref().unwrap_or(".");
    let report_path = PathBuf::from(worktree).join(&job.report);
    format!(
        "You are the verifier for job {job_id} of project {project}. The original brief is at {brief_path}.\n\
         The job worktree is {worktree}. The change under review: {diff}\n\
         Gate evidence for this attempt:\n{evidence}\
         Judge only against evidence and the brief; run the commands yourself; do not edit files.\n\
         Use the write tool to write the report file at {report_path}; do not only print it in chat.\n\
         The report needs front-matter `job`, `status: done`, `branch`, `commits`, `tests`, `verdict: pass|fail` and `findings` (on fail, a list of concrete, checkable problems: file, command, expected, actual). Report `status: done` when you finished judging.\n\
         Job: {job_id}",
        job_id = job.id,
        project = job.project,
        brief_path = brief,
        worktree = worktree,
        report_path = report_path.display(),
        evidence = lines,
        diff = diff_summary,
    )
}

/// Reads the verifier's own report file.
fn read_verify_report(job: &Job) -> Result<Report, CoreError> {
    let worktree = job
        .worktree
        .as_ref()
        .map(PathBuf::from)
        .ok_or_else(|| CoreError::Invalid(format!("verify job {} has no worktree", job.id)))?;
    let path = worktree.join(&job.report);
    let text = std::fs::read_to_string(&path).map_err(|source| CoreError::Io {
        path: path.display().to_string(),
        source,
    })?;
    Ok(Report::parse(&text)?)
}

/// Returns the original job id from the verify job's `verifies` edge.
pub(crate) fn verify_parent(store: &Store, verify_job: &Job) -> Result<JobId, CoreError> {
    for edge in store.edges_of(&verify_job.id)? {
        if edge.kind == EdgeKind::Verifies && edge.src == verify_job.id {
            return Ok(edge.dst);
        }
    }
    Err(CoreError::Invalid(format!(
        "verify job {} has no verifies edge",
        verify_job.id
    )))
}

/// Renders the important part of a gate evidence value for the verifier prompt.
fn summarise_evidence(evidence: &Evidence) -> String {
    match evidence {
        Evidence::Report {
            missing_fields,
            problems,
            ..
        } => {
            let mut parts = Vec::new();
            if !missing_fields.is_empty() {
                parts.push(format!("missing fields: {}", missing_fields.join(", ")));
            }
            parts.extend(problems.iter().cloned());
            if parts.is_empty() {
                "report has all required fields".to_owned()
            } else {
                parts.join("; ")
            }
        }
        Evidence::Cmd {
            command,
            exit_code,
            timed_out,
            tail,
            ..
        }
        | Evidence::Script {
            command,
            exit_code,
            timed_out,
            tail,
            ..
        } => {
            let tail = tail
                .lines()
                .rev()
                .take(40)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
            format!("`{command}` exited {exit_code:?} (timed_out={timed_out}):\n{tail}")
        }
        Evidence::OwnsRespected { violations, .. } => {
            if violations.is_empty() {
                "ownership respected".to_owned()
            } else {
                format!("ownership violations: {}", violations.join(", "))
            }
        }
        Evidence::Pushed { branch, remote, .. } => format!("{remote}/{branch}"),
        Evidence::Review { verdict, findings, .. } => format!("{verdict:?} {findings:?}"),
        Evidence::Deploy {
            command,
            exit_code,
            probe,
            ..
        } => format!("`{command}` exited {exit_code:?}; probe: {}", summarise_evidence(probe)),
        Evidence::SecretScan { hits, .. } => format!("{} secret hit(s)", hits.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::verify_prompt;
    use vowp_protocol::{Job, JobId, JobKind, JobLoop, JobState, ProjectName, Role, Timestamp};

    #[test]
    fn verify_prompt_names_the_report_file() {
        let ts = Timestamp::parse("2026-11-12T00:00:00Z").expect("timestamp");
        let job = Job {
            id: JobId::parse("demo-verify-0").expect("id"),
            project: ProjectName::parse("demo").expect("project"),
            title: "verify demo".to_owned(),
            kind: JobKind::Verify,
            role: Role::Verifier,
            profile: "verifier".to_owned(),
            state: JobState::Running,
            priority: 50,
            needs: Vec::new(),
            owns: Vec::new(),
            reads: Vec::new(),
            brief: ".vowp/jobs/demo-verify-0/brief.md".to_owned(),
            report: ".vowp/jobs/demo-verify-0/report.md".to_owned(),
            gates: Vec::new(),
            loop_: JobLoop::default(),
            branch: Some("job/demo".to_owned()),
            worktree: Some("/worktrees/demo".to_owned()),
            attempts: 0,
            continues: 0,
            hang_restarts: 0,
            parked_reason: None,
            note: String::new(),
            created_by: "l3".to_owned(),
            last_progress_at: None,
            state_changed_at: ts,
            created_at: ts,
            updated_at: ts,
        };

        let prompt = verify_prompt(
            &job,
            ".vowp/jobs/demo-verify-0/brief.md",
            &[],
            "git diff main...job/demo",
        );

        assert!(
            prompt.contains("/worktrees/demo/.vowp/jobs/demo-verify-0/report.md"),
            "prompt must name the exact report path: {prompt}"
        );
        assert!(
            prompt.contains("write the report file"),
            "prompt must say to write the report file: {prompt}"
        );
        for field in [
            "`job`",
            "`status: done`",
            "`branch`",
            "`commits`",
            "`tests`",
            "`verdict: pass|fail`",
            "`findings`",
        ] {
            assert!(
                prompt.contains(field),
                "prompt must name the front-matter field {field}: {prompt}"
            );
        }
    }
}
