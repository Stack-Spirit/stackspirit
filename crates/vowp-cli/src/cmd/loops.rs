//! `loops`. Owned by n09c; this build has the explicit stub.

use crate::cli::LoopsArgs;
use crate::{CliError, Ctx, IoStreams};

/// Lists loops or pauses/resumes one.
pub fn run(_ctx: &Ctx, _io: &mut dyn IoStreams, _args: &LoopsArgs) -> Result<i32, CliError> {
    Err(CliError::NotImplemented("loops"))
}
