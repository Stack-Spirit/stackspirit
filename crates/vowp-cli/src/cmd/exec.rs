//! Hidden `vowp __exec` — the remote executor client. C0 stub; F1 fills the handler.

use std::path::PathBuf;

use clap::Args;

use crate::{CliError, Ctx, IoStreams};

/// Arguments for the hidden `vowp __exec`.
#[derive(Debug, Clone, Args)]
pub struct ExecArgs {
    /// Grant file written by `session turn-begin` (mode 0600).
    #[arg(long, value_name = "FILE")]
    pub grant: PathBuf,
    /// Directory for the executor-side spool.
    #[arg(long, value_name = "DIR")]
    pub spool_dir: Option<PathBuf>,
}

/// Runs the hidden `vowp __exec`.
pub fn run(_ctx: &Ctx, _io: &mut dyn IoStreams, _args: &ExecArgs) -> Result<i32, CliError> {
    Err(crate::cmd::not_implemented())
}
