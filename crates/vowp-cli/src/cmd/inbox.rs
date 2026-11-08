//! `inbox`. Owned by n09c; this build has the explicit stub.

use crate::cli::InboxCommand;
use crate::{CliError, Ctx, IoStreams};

/// Runs one inbox subcommand.
pub fn run(_ctx: &Ctx, _io: &mut dyn IoStreams, _command: &InboxCommand) -> Result<i32, CliError> {
    Err(CliError::NotImplemented("inbox"))
}
