//! Remote execution: begin a turn on the home and ingest the executor's lines.
//!
//! Spec: `docs/design/v0.2/home-and-surfaces.md` §7.2 and §13.2. F1 fills this module;
//! C0 writes no remote framing and ingests no events.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use vowp_protocol::{ExecutorRef, NodeName, RunId, RunPurpose, SessionId, TurnGrant};
use vowp_store::Store;

use crate::CoreError;
use crate::deps::Deps;

/// The grant plus the executor-side spool directory.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteTurn {
    /// The grant returned to the surface.
    pub grant: TurnGrant,
    /// Directory that holds `events.jsonl` and `acked.json`.
    pub spool_dir: PathBuf,
}

/// A request to begin a remote turn.
#[derive(Debug, Clone, PartialEq)]
pub struct BeginTurn {
    /// Session to continue.
    pub session: SessionId,
    /// Lease token presented by the surface.
    pub lease: String,
    /// Executor the surface wants to run.
    pub executor: ExecutorRef,
    /// Purpose of the run.
    pub purpose: RunPurpose,
    /// Executor node.
    pub node: NodeName,
    /// Prompt to send.
    pub prompt: String,
}

/// The home-side grant used while ingesting one run.
#[derive(Debug, Clone, PartialEq)]
pub struct IngestGrant {
    /// Session the run belongs to.
    pub session: SessionId,
    /// Lease token carried by `RemoteLine::Hello`.
    pub lease: String,
    /// Run id.
    pub run: RunId,
}

/// What one ingest pass processed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IngestReport {
    /// Events appended.
    pub events: u64,
    /// Events skipped as duplicates.
    pub duplicates: u64,
    /// Whether the run summary was accepted.
    pub done: bool,
}

/// Writes the framing events for a remote turn and returns its grant.
pub fn begin_turn(_deps: &Deps, _store: &Store, _req: &BeginTurn) -> Result<TurnGrant, CoreError> {
    Err(CoreError::Invalid(
        "remote turns are not implemented in this build".to_owned(),
    ))
}

/// Reads remote lines and appends their events to the home session log.
pub fn ingest_lines(
    _deps: &Deps,
    _store: &Store,
    _grant: &IngestGrant,
    _reader: &mut dyn BufRead,
    _ack: &mut dyn Write,
) -> Result<IngestReport, CoreError> {
    Ok(IngestReport::default())
}
