//! Adapter for the opencode CLI harness.
//!
//! n06z leaves the shared crate compiling; n06a replaces this module with the real
//! `opencode run --format json` adapter and parser described in `docs/design/adapters.md` §3.

use vowp_protocol::{
    Adapter, AdapterCapabilities, AdapterError, AdapterRun, Cost, Executor, HarnessEvent, HarnessKind, ProbeResult,
    RunOutcome, RunSummary, StartInput, StopReason, Timestamp,
};

use crate::stream::StreamParser;

/// Configuration for [`OpencodeAdapter`].
#[derive(Debug, Clone, Default)]
pub struct OpencodeConfig {
    /// Binary to run; defaults to `opencode` on `PATH`, then `$HOME/.opencode/bin/opencode`.
    pub binary: Option<String>,
    /// Extra arguments appended to the run command.
    pub extra_args: Vec<String>,
    /// Optional `--title` value.
    pub title: Option<String>,
    /// Fixed offset used to interpret clock-time reset texts.
    pub tz_offset_minutes: i32,
    /// Prompts larger than this go through a file under `<cwd>/.vowp/prompts/`.
    pub max_arg_prompt_bytes: usize,
}

/// The opencode adapter.
pub struct OpencodeAdapter {
    config: OpencodeConfig,
}

impl OpencodeAdapter {
    /// Creates an adapter from `config`.
    pub fn new(config: OpencodeConfig) -> Self {
        Self { config }
    }
}

impl Adapter for OpencodeAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Opencode
    }

    fn probe(&self) -> ProbeResult {
        ProbeResult {
            ok: false,
            version: None,
            path: None,
            detail: "opencode adapter is implemented by n06a".to_owned(),
        }
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities::default()
    }

    fn start(&self, _input: StartInput) -> Result<Box<dyn AdapterRun>, AdapterError> {
        let _ = &self.config;
        Err(AdapterError::Unavailable(
            "opencode adapter is implemented by n06a".to_owned(),
        ))
    }
}

/// Parser for opencode `--format json` lines.
pub struct OpencodeParser {
    now: Timestamp,
    tz_offset_minutes: i32,
    executor: Executor,
}

impl OpencodeParser {
    /// Creates a parser for one run.
    pub fn new(now: Timestamp, tz_offset_minutes: i32, executor: Executor) -> Self {
        Self {
            now,
            tz_offset_minutes,
            executor,
        }
    }
}

impl StreamParser for OpencodeParser {
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
