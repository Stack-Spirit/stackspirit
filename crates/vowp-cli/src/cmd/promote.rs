//! `promote`, `rollback` and `releases`. Owned by n10b; this build has the explicit stub.

use crate::cli::PromoteCommand;
use crate::{CliError, Ctx, IoStreams};

/// Runs one release command.
pub fn run(_ctx: &Ctx, _io: &mut dyn IoStreams, command: &PromoteCommand) -> Result<i32, CliError> {
    let name = match command {
        PromoteCommand::Promote(_) => "promote",
        PromoteCommand::Rollback(_) => "rollback",
        PromoteCommand::Releases(_) => "releases",
    };
    Err(CliError::NotImplemented(name))
}
