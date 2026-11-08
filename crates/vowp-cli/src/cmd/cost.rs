//! `cost`. Owned by n09c; this build has the explicit stub.

use crate::cli::CostArgs;
use crate::{CliError, Ctx, IoStreams};

/// Prints cost totals.
pub fn run(_ctx: &Ctx, _io: &mut dyn IoStreams, _args: &CostArgs) -> Result<i32, CliError> {
    Err(CliError::NotImplemented("cost"))
}
