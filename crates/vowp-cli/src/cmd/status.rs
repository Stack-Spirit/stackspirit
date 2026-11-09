//! `status`. Owned by n09b; this build has the explicit stub.

use crate::cli::StatusArgs;
use crate::{CliError, Ctx, IoStreams};

/// Prints the supervisor, job, slot, limit, cost and inbox summary.
pub fn run(_ctx: &Ctx, _io: &mut dyn IoStreams, _args: &StatusArgs) -> Result<i32, CliError> {
    Err(CliError::NotImplemented("status"))
}
