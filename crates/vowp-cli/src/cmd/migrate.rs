//! `vowp migrate` — the v0.1 to v0.2 home migration. C0 stub; C2 fills the handler.

use std::path::PathBuf;

use clap::Args;

use crate::{CliError, Ctx, IoStreams};

/// Arguments for `vowp migrate`.
#[derive(Debug, Clone, Default, Args)]
pub struct MigrateArgs {
    /// Report what would change without writing.
    #[arg(long)]
    pub dry_run: bool,
    /// Restore from this pre-migration backup instead of migrating.
    #[arg(long, value_name = "BACKUP")]
    pub rollback: Option<PathBuf>,
    /// Confirm the migration.
    #[arg(long)]
    pub yes: bool,
}

/// Runs `vowp migrate`.
pub fn run(_ctx: &Ctx, _io: &mut dyn IoStreams, _args: &MigrateArgs) -> Result<i32, CliError> {
    Err(crate::cmd::not_implemented())
}
