//! `plan`. Owned by n09b; this build has the explicit stub.

use crate::cli::PlanArgs;
use crate::{CliError, Ctx, IoStreams};

/// Starts a planner turn.
pub fn run(_ctx: &Ctx, _io: &mut dyn IoStreams, _args: &PlanArgs) -> Result<i32, CliError> {
    Err(CliError::NotImplemented("plan"))
}
