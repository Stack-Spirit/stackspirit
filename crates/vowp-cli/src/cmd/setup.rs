//! `vowp setup` — AI-first setup. C0 stub; I1 fills the handler and the `setup` module.

use clap::{Args, Subcommand};

use crate::{CliError, Ctx, IoStreams};

/// Arguments for `vowp setup`.
#[derive(Debug, Clone, Args)]
pub struct SetupArgs {
    /// The setup subcommand.
    #[command(subcommand)]
    pub command: SetupCommand,
}

/// The `vowp setup` subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum SetupCommand {
    /// Print every phase, command and file without changing anything.
    Plan(SetupPlanArgs),
    /// Set up the home on a fresh machine.
    Home(SetupHomeArgs),
    /// Set up this machine as the control machine.
    Control(SetupControlArgs),
    /// Run the fixture verification on the home.
    Verify(SetupVerifyArgs),
}

/// Arguments for `vowp setup plan`.
#[derive(Debug, Clone, Args)]
pub struct SetupPlanArgs {
    /// SSH target of the home to set up.
    #[arg(long, value_name = "SSH-TARGET")]
    pub target: String,
}

/// Arguments for `vowp setup home`.
#[derive(Debug, Clone, Args)]
pub struct SetupHomeArgs {
    /// SSH target of the home to set up.
    #[arg(long, value_name = "SSH-TARGET")]
    pub target: String,
    /// Apply the plan instead of only printing it.
    #[arg(long)]
    pub apply: bool,
    /// Do not ask before applying.
    #[arg(long)]
    pub yes: bool,
}

/// Arguments for `vowp setup control`.
#[derive(Debug, Clone, Default, Args)]
pub struct SetupControlArgs {
    /// Apply the plan instead of only printing it.
    #[arg(long)]
    pub apply: bool,
}

/// Arguments for `vowp setup verify`.
#[derive(Debug, Clone, Default, Args)]
pub struct SetupVerifyArgs {}

/// Runs `vowp setup`.
pub fn run(_ctx: &Ctx, _io: &mut dyn IoStreams, _args: &SetupArgs) -> Result<i32, CliError> {
    Err(crate::cmd::not_implemented())
}
