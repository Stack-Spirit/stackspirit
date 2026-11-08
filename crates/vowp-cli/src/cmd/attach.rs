//! `attach`. Owned by n09b; this build has the explicit stub.

use crate::cli::AttachArgs;
use crate::{CliError, Ctx, IoStreams};

/// Attaches to a job's tmux window.
pub fn run(_ctx: &Ctx, _io: &mut dyn IoStreams, _args: &AttachArgs) -> Result<i32, CliError> {
    Err(CliError::NotImplemented("attach"))
}
