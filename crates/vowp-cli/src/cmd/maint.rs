//! `reindex`, `backup`, `schema` and `version`. Owned by n09c; this build has the explicit stub.

use crate::cli::MaintCommand;
use crate::{CliError, Ctx, IoStreams};

/// Runs one maintenance command.
pub fn run(_ctx: &Ctx, _io: &mut dyn IoStreams, command: &MaintCommand) -> Result<i32, CliError> {
    let name = match command {
        MaintCommand::Reindex(_) => "reindex",
        MaintCommand::Backup(_) => "backup",
        MaintCommand::Schema(_) => "schema",
        MaintCommand::Version => "version",
    };
    Err(CliError::NotImplemented(name))
}
