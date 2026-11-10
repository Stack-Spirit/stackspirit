//! The `vowp chat` fallback surface. C0 creates the module; F3 fills the handler.

use vowp_protocol::{ExecutorRef, NodeName, SessionId};

use crate::{CliError, Ctx, IoStreams};

/// Options for one `vowp chat` run.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatOptions {
    /// Session to talk to.
    pub session: SessionId,
    /// Executor to switch to before the first turn.
    pub executor: Option<ExecutorRef>,
    /// Executor node; defaults to this surface's machine.
    pub node: Option<NodeName>,
    /// Follow the log read-only.
    pub follow: bool,
    /// Take the lease from a live surface.
    pub takeover: bool,
}

/// Runs one chat surface.
pub fn run(_ctx: &mut Ctx, _io: &mut dyn IoStreams, _opts: &ChatOptions) -> Result<i32, CliError> {
    Err(crate::cmd::not_implemented())
}
