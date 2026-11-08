//! `schedule`. Owned by n09c; this build has the explicit stub.

use crate::cli::ScheduleCommand;
use crate::{CliError, Ctx, IoStreams};

/// Runs one schedule subcommand.
pub fn run(_ctx: &Ctx, _io: &mut dyn IoStreams, _command: &ScheduleCommand) -> Result<i32, CliError> {
    Err(CliError::NotImplemented("schedule"))
}
