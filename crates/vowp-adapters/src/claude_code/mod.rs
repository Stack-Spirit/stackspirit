//! Adapter for the Claude Code CLI harness.
//!
//! n06z leaves the shared crate compiling; n06b replaces this module with the real
//! `stream-json` adapter and parser described in `docs/design/adapters.md` §2.

use std::path::PathBuf;

use vowp_protocol::{
    Adapter, AdapterCapabilities, AdapterError, AdapterRun, Cost, Executor, HarnessEvent, HarnessKind, ProbeResult,
    RunOutcome, RunSummary, StartInput, StopReason, Timestamp,
};

use crate::stream::StreamParser;

/// Configuration for [`ClaudeCodeAdapter`].
#[derive(Debug, Clone, Default)]
pub struct ClaudeConfig {
    /// Binary to run; defaults to the first `claude` on `PATH`.
    pub binary: Option<String>,
    /// Extra arguments appended after VOWP's flags and before the prompt.
    pub extra_args: Vec<String>,
    /// Directories passed through `--add-dir`.
    pub add_dirs: Vec<PathBuf>,
    /// Permission mode; defaults to `"auto"`.
    pub permission_mode: Option<String>,
    /// Pass the prompt as the last argument instead of stdin.
    pub prompt_via_arg: bool,
    /// Fixed offset used to interpret clock-time reset texts.
    pub tz_offset_minutes: i32,
}

/// The Claude Code adapter.
pub struct ClaudeCodeAdapter {
    config: ClaudeConfig,
}

impl ClaudeCodeAdapter {
    /// Creates an adapter from `config`.
    pub fn new(config: ClaudeConfig) -> Self {
        Self { config }
    }
}

impl Adapter for ClaudeCodeAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::ClaudeCode
    }

    fn probe(&self) -> ProbeResult {
        ProbeResult {
            ok: false,
            version: None,
            path: None,
            detail: "claude-code adapter is implemented by n06b".to_owned(),
        }
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities::default()
    }

    fn start(&self, _input: StartInput) -> Result<Box<dyn AdapterRun>, AdapterError> {
        let _ = &self.config;
        Err(AdapterError::Unavailable(
            "claude-code adapter is implemented by n06b".to_owned(),
        ))
    }
}

/// Parser for Claude Code `stream-json` lines.
pub struct ClaudeParser {
    now: Timestamp,
    tz_offset_minutes: i32,
    executor: Executor,
}

impl ClaudeParser {
    /// Creates a parser for one run.
    pub fn new(now: Timestamp, tz_offset_minutes: i32, executor: Executor) -> Self {
        Self {
            now,
            tz_offset_minutes,
            executor,
        }
    }
}

impl StreamParser for ClaudeParser {
    fn feed_line(&mut self, _line: &str) -> Vec<HarnessEvent> {
        let _ = (self.now, self.tz_offset_minutes, &self.executor);
        Vec::new()
    }

    fn finish(self: Box<Self>, exit: Option<i32>, killed: bool, _stderr_tail: &str) -> (Vec<HarnessEvent>, RunSummary) {
        (Vec::new(), stub_summary(exit, killed))
    }
}

fn stub_summary(exit: Option<i32>, killed: bool) -> RunSummary {
    RunSummary {
        exit_code: exit,
        outcome: if killed { RunOutcome::Killed } else { RunOutcome::Error },
        native_session_id: None,
        cost: Cost::unknown(),
        limit: None,
        result_text: None,
        stop_reason: if killed {
            StopReason::Interrupted
        } else {
            StopReason::Crash
        },
    }
}
