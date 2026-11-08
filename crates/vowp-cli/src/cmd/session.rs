//! `session` and the `chat` alias. Owned by n09c; this build has the explicit stub.

use crate::cli::SessionCommand;
use crate::{CliError, Ctx, IoStreams};

/// Runs one session subcommand.
pub fn run(_ctx: &Ctx, _io: &mut dyn IoStreams, _command: &SessionCommand) -> Result<i32, CliError> {
    Err(CliError::NotImplemented("session"))
}
