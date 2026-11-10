//! `vowp capacity` — measured, admitted capacity. C0 stub; E2 fills the handler.

use clap::{Args, Subcommand};

use crate::{CliError, Ctx, IoStreams};

/// Arguments for `vowp capacity`.
#[derive(Debug, Clone, Args)]
pub struct CapacityArgs {
    /// The capacity subcommand.
    #[command(subcommand)]
    pub command: CapacityCommand,
}

/// The `vowp capacity` subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum CapacityCommand {
    /// Show the current capacity report.
    Show(CapacityShowArgs),
    /// Show the report used by the advisor.
    Report(CapacityReportArgs),
    /// Write one capacity target row.
    Set(CapacitySetArgs),
}

/// Arguments for `vowp capacity show`.
#[derive(Debug, Clone, Default, Args)]
pub struct CapacityShowArgs {}

/// Arguments for `vowp capacity report`.
#[derive(Debug, Clone, Default, Args)]
pub struct CapacityReportArgs {
    /// Only include samples since this duration or date.
    #[arg(long, value_name = "DUR|DATE")]
    pub since: Option<String>,
    /// Maximum number of estimates.
    #[arg(long, value_name = "N")]
    pub limit: Option<u32>,
}

/// Arguments for `vowp capacity set`.
#[derive(Debug, Clone, Default, Args)]
pub struct CapacitySetArgs {
    /// Number of workers.
    #[arg(long, value_name = "N")]
    pub target: Option<u32>,
    /// `auto` or `fixed`.
    #[arg(long, value_name = "MODE")]
    pub mode: Option<String>,
    /// Allow a target above the supervisor's advice.
    #[arg(long = "override")]
    pub is_override: bool,
    /// Why the target changed.
    #[arg(long, value_name = "TEXT")]
    pub reason: Option<String>,
    /// Who set the target.
    #[arg(long, value_name = "ACTOR")]
    pub by: Option<String>,
}

/// Runs `vowp capacity`.
pub fn run(_ctx: &Ctx, _io: &mut dyn IoStreams, _args: &CapacityArgs) -> Result<i32, CliError> {
    Err(crate::cmd::not_implemented())
}
