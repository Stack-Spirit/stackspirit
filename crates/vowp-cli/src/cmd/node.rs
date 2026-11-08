//! `node`. Owned by n09c; this build has the explicit stub.

use crate::cli::NodeCommand;
use crate::{CliError, Ctx, IoStreams};

/// Runs one node subcommand.
pub fn run(_ctx: &Ctx, _io: &mut dyn IoStreams, _command: &NodeCommand) -> Result<i32, CliError> {
    Err(CliError::NotImplemented("node"))
}
