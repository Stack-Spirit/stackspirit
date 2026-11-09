//! `board`. Owned by n09b; this build has the explicit stub.

use crate::cli::BoardArgs;
use crate::{CliError, Ctx, IoStreams};

/// Prints or refreshes the board.
pub fn run(_ctx: &Ctx, _io: &mut dyn IoStreams, _args: &BoardArgs) -> Result<i32, CliError> {
    Err(CliError::NotImplemented("board"))
}
