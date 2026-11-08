//! Generic CLI harness adapter driven by a mapping file.
//!
//! n06z leaves the shared crate compiling; n06c replaces this module with the real mapping
//! loader, parser and adapter described in `docs/design/adapters.md` §4.

use std::path::Path;

use vowp_protocol::{
    Adapter, AdapterCapabilities, AdapterError, AdapterRun, Cost, HarnessEvent, HarnessKind, ProbeResult, RunOutcome,
    RunSummary, StartInput, StopReason, Timestamp,
};

use crate::stream::StreamParser;

/// Deserialised mapping file. Placeholder until n06c implements the full schema.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mapping {
    /// Harness entry name.
    pub name: String,
}

/// The generic CLI adapter.
pub struct CliAdapter {
    mapping: Mapping,
}

impl CliAdapter {
    /// Loads a mapping from a TOML file.
    pub fn from_mapping_file(_path: &Path) -> Result<CliAdapter, AdapterError> {
        Err(AdapterError::Unavailable(
            "cli adapter is implemented by n06c".to_owned(),
        ))
    }

    /// Creates an adapter from an already parsed mapping.
    pub fn from_mapping(mapping: Mapping) -> Result<CliAdapter, AdapterError> {
        Ok(Self { mapping })
    }

    /// Returns the mapping this adapter uses.
    pub fn mapping(&self) -> &Mapping {
        &self.mapping
    }
}

impl Adapter for CliAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Cli
    }

    fn probe(&self) -> ProbeResult {
        ProbeResult {
            ok: false,
            version: None,
            path: None,
            detail: "cli adapter is implemented by n06c".to_owned(),
        }
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities::default()
    }

    fn start(&self, _input: StartInput) -> Result<Box<dyn AdapterRun>, AdapterError> {
        let _ = &self.mapping;
        Err(AdapterError::Unavailable(
            "cli adapter is implemented by n06c".to_owned(),
        ))
    }
}

/// Parser for a mapped CLI harness.
pub struct CliParser {
    mapping: Mapping,
    now: Timestamp,
}

impl CliParser {
    /// Creates a parser from a validated mapping.
    pub fn new(mapping: Mapping, now: Timestamp) -> Result<Self, AdapterError> {
        Ok(Self { mapping, now })
    }
}

impl StreamParser for CliParser {
    fn feed_line(&mut self, _line: &str) -> Vec<HarnessEvent> {
        let _ = (&self.mapping, self.now);
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
