//! `up`, `down`, `status` and `board`. Owned by n09b; this build has the explicit stub.

use crate::cli::UpCommand;
use crate::{CliError, Ctx, IoStreams};

/// Runs one of the supervisor lifecycle commands.
pub fn run(_ctx: &Ctx, _io: &mut dyn IoStreams, command: &UpCommand) -> Result<i32, CliError> {
    let name = match command {
        UpCommand::Up(_) => "up",
        UpCommand::Down(_) => "down",
        UpCommand::Status(_) => "status",
        UpCommand::Board(_) => "board",
    };
    Err(CliError::NotImplemented(name))
}
