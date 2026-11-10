//! `vowp gc` — lifecycle classes and garbage collection. C0 stub; H1 fills the handler.

use clap::Args;

use crate::{CliError, Ctx, IoStreams};

/// Arguments for `vowp gc`.
#[derive(Debug, Clone, Default, Args)]
pub struct GcArgs {
    /// Delete eligible files instead of only reporting.
    #[arg(long)]
    pub apply: bool,
    /// Restrict to one lifecycle class.
    #[arg(long, value_name = "CLASS")]
    pub class: Option<String>,
    /// Clock override for tests (RFC 3339).
    #[arg(long, value_name = "RFC3339")]
    pub now: Option<String>,
}

/// Runs `vowp gc`.
pub fn run(_ctx: &Ctx, _io: &mut dyn IoStreams, _args: &GcArgs) -> Result<i32, CliError> {
    Err(crate::cmd::not_implemented())
}
