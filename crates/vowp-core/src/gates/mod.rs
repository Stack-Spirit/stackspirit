//! Deterministic job gates: report, command, ownership, push, review, deploy and secret scan.
//! Spec: `docs/design/loops.md` §4 and `docs/design/crates/vowp-core.md` §4.
//!
//! Gates are evaluated in the job worktree, in `jobs.gates_json` order. A bad gate definition
//! or an unexpected environment never panics: it produces a failed [`GateResult`] with
//! evidence describing what happened.

use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;
use vowp_protocol::{CmdGate, Evidence, Gate, Job, RunId};
use vowp_store::Store;

use crate::CoreError;
use crate::deps::Deps;

pub mod cmd;
pub mod deploy;
pub mod owns;
pub mod pushed;
pub mod report;
pub mod review;
pub mod runner;
pub mod secret_scan;

pub use runner::{GateRunner, GatesOutcome};

/// Everything one gate evaluation may read.
pub struct GateCtx<'a> {
    /// Injected dependencies (clock, command runner, git, config, log).
    pub deps: &'a Deps,
    /// Job whose gates are being evaluated.
    pub job: &'a Job,
    /// Run that produced the work, when there was one.
    pub run_id: Option<RunId>,
    /// Job worktree the commands and scans run in.
    pub worktree: &'a Path,
    /// Project repository (main checkout or worktree root).
    pub repo: &'a Path,
    /// Store used for gate rows, verifier jobs and their reports.
    pub store: &'a Store,
    /// Fix-round attempt number.
    pub attempt: u32,
}

/// One gate's outcome before it is stamped into a [`GateResult`].
pub(crate) struct Outcome {
    /// Whether the gate passed.
    pub passed: bool,
    /// Evidence describing the evaluation.
    pub evidence: Evidence,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
    /// Blob reference when output was spilled.
    pub output_blob: Option<String>,
}

impl Outcome {
    /// Builds a zero-duration outcome with no spilled output.
    pub(crate) fn new(passed: bool, evidence: Evidence) -> Self {
        Self {
            passed,
            evidence,
            duration_ms: 0,
            output_blob: None,
        }
    }
}

/// Reads and parses a report file.
///
/// Used by the `report` gate, the review gate and the loops. A missing file is an
/// [`CoreError::Io`]; malformed front matter is [`vowp_protocol::ProtocolError::FrontMatter`].
pub fn parse_report_file(path: &Path) -> Result<vowp_protocol::Report, CoreError> {
    let text = std::fs::read_to_string(path).map_err(|source| CoreError::Io {
        path: path.display().to_string(),
        source,
    })?;
    Ok(vowp_protocol::Report::parse(&text)?)
}

/// Validates a gate definition, including the anti-pattern ban from `loops.md` §4.
///
/// Command and script gates may not merely check that a file exists (`test -f`, `[ -e ]`,
/// `ls`, `stat`, `cat`, bare `true` or `:`) because that does not show the work runs. A
/// command gate that expects a nonzero exit must also assert an expected pattern, so a
/// command that fails for an unrelated reason cannot satisfy it. Deploy gates must run a
/// real command and end with a real probe. Protocol-level checks (non-empty command,
/// compilable pattern, positive timeout) run first.
pub fn validate_gate(gate: &Gate) -> Result<(), CoreError> {
    gate.validate()?;
    match gate {
        Gate::Cmd(cmd) | Gate::Script(cmd) => {
            reject_nonzero_exit_without_pattern(cmd)?;
            reject_file_existence_only(&cmd.run)
        }
        Gate::Deploy(deploy) => {
            if is_file_existence_only(&deploy.run) {
                return Err(CoreError::Invalid(format!(
                    "anti-pattern: gate must execute and check results: {}",
                    deploy.run
                )));
            }
            reject_nonzero_exit_without_pattern(&deploy.probe)?;
            reject_file_existence_only(&deploy.probe.run)
        }
        Gate::Report(_) | Gate::OwnsRespected(_) | Gate::Pushed(_) | Gate::Review(_) | Gate::SecretScan(_) => Ok(()),
    }
}

/// Rejects a command gate that expects a nonzero exit without a pattern to match.
fn reject_nonzero_exit_without_pattern(cmd: &CmdGate) -> Result<(), CoreError> {
    if cmd.expect_exit != 0 && cmd.expect_pattern.is_none() {
        return Err(CoreError::Invalid(format!(
            "anti-pattern: command gate with expect_exit {} needs an expect_pattern: {}",
            cmd.expect_exit, cmd.run
        )));
    }
    Ok(())
}

/// Rejects a command that only checks whether a path exists.
fn reject_file_existence_only(run: &str) -> Result<(), CoreError> {
    if is_file_existence_only(run) {
        return Err(CoreError::Invalid(format!(
            "anti-pattern: gate must execute and check results: {run}"
        )));
    }
    Ok(())
}

/// Returns true for the file-existence-only command shapes banned by the spec.
fn is_file_existence_only(run: &str) -> bool {
    static BANNED: OnceLock<Regex> = OnceLock::new();
    let banned = BANNED.get_or_init(|| {
        // invariant: this literal is a valid regex, exercised by
        // file_exists_only_gate_rejected_by_validate.
        Regex::new(r"^\s*(?:test\s+-[fed]\b|\[\s+-[fed]\b|ls\b|stat\b|cat\b|true\b|:)")
            .expect("invariant: anti-pattern regex is valid")
    });
    banned.is_match(run)
}
