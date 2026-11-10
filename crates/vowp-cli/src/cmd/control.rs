//! `vowp control` — the control-machine role. C0 stub; D2 fills the handler.

use clap::{Args, Subcommand};

use crate::{CliError, Ctx, IoStreams};

/// Arguments for `vowp control`.
#[derive(Debug, Clone, Args)]
pub struct ControlArgs {
    /// The control subcommand.
    #[command(subcommand)]
    pub command: ControlCommand,
}

/// The `vowp control` subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum ControlCommand {
    /// Record the home this machine controls.
    Init(ControlInitArgs),
    /// Print the control configuration and cache metadata.
    Status,
}

/// Arguments for `vowp control init`.
#[derive(Debug, Clone, Args)]
pub struct ControlInitArgs {
    /// Name of the home node.
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,
    /// SSH destination of the home.
    pub ssh_target: String,
    /// Fail when the SSH probe fails.
    #[arg(long)]
    pub strict: bool,
    /// Overwrite an existing control configuration.
    #[arg(long)]
    pub force: bool,
}

/// Runs `vowp control`.
pub fn run(_ctx: &Ctx, _io: &mut dyn IoStreams, _args: &ControlArgs) -> Result<i32, CliError> {
    Err(crate::cmd::not_implemented())
}
