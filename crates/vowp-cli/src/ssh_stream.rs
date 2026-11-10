//! The executor-side SSH ingest stream. C0 creates the types; F1 fills the client.
//!
//! Spec: `docs/design/v0.2/home-and-surfaces.md` §13.6.

use std::path::PathBuf;

use vowp_protocol::{IngestAck, NodeName, RemoteLine, RunId, SessionId};

use crate::CliError;

/// Number of unacknowledged events the client keeps in flight.
pub const INGEST_ACK_WINDOW: usize = 64;

/// The home a surface dials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshTarget {
    /// Home node name.
    pub node: NodeName,
    /// SSH destination.
    pub ssh_target: String,
    /// Absolute remote home path.
    pub remote_home: String,
}

/// A client that spools, sends and replays remote event lines.
pub struct IngestClient {
    _target: SshTarget,
    _session: SessionId,
    _run: RunId,
    _lease: String,
    _spool: PathBuf,
    revoked: Option<IngestAck>,
}

impl IngestClient {
    /// Creates a client. The caller starts the SSH child with [`crate::remote`].
    pub fn new(target: SshTarget, session: SessionId, run: RunId, lease: String, spool: PathBuf) -> Self {
        Self {
            _target: target,
            _session: session,
            _run: run,
            _lease: lease,
            _spool: spool,
            revoked: None,
        }
    }

    /// Spools an event and sends it, returning an ack when one arrived.
    pub fn send(&mut self, _line: &RemoteLine) -> Result<Option<IngestAck>, CliError> {
        Err(crate::cmd::not_implemented())
    }

    /// Resends every event from the first unacked one and returns how many were sent.
    pub fn replay(&mut self) -> Result<u32, CliError> {
        Err(crate::cmd::not_implemented())
    }

    /// Returns the takeover notice, when the home revoked the lease.
    pub fn revoked(&self) -> Option<&IngestAck> {
        self.revoked.as_ref()
    }

    /// Flushes the spool and closes the stream.
    pub fn finish(&mut self) -> Result<(), CliError> {
        Err(crate::cmd::not_implemented())
    }
}
