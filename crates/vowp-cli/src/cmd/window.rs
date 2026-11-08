//! Hidden `__window`. Owned by n09b; this build has the explicit stub.

use crate::cli::WindowArgs;
use crate::{CliError, Ctx, IoStreams};

/// Renders what a tmux window shows.
pub fn run(_ctx: &Ctx, _io: &mut dyn IoStreams, _args: &WindowArgs) -> Result<i32, CliError> {
    Err(CliError::NotImplemented("__window"))
}
