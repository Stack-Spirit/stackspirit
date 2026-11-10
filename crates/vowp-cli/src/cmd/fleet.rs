//! `vowp fleet` — the node read model. C0 stub; D1 fills the handler.

use clap::{Args, Subcommand};

use crate::{CliError, Ctx, IoStreams};

/// Arguments for `vowp fleet`.
#[derive(Debug, Clone, Args)]
pub struct FleetArgs {
    /// The fleet subcommand.
    #[command(subcommand)]
    pub command: FleetCommand,
}

/// The `vowp fleet` subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum FleetCommand {
    /// Show the node summary from `snapshot.json`.
    Status(FleetStatusArgs),
    /// List queued jobs across projects.
    Queue(FleetQueueArgs),
    /// List open inbox items across the node and its projects.
    Inbox(FleetInboxArgs),
    /// Alias of `vowp capacity show`.
    Capacity(FleetCapacityArgs),
}

/// Arguments for `vowp fleet status`.
#[derive(Debug, Clone, Args)]
pub struct FleetStatusArgs {
    /// Recompute the snapshot instead of reading the file.
    #[arg(long)]
    pub refresh: bool,
    /// Read only the local cache, never SSH.
    #[arg(long)]
    pub cached: bool,
}

/// Arguments for `vowp fleet queue`.
#[derive(Debug, Clone, Args)]
pub struct FleetQueueArgs {
    /// Restrict to one project.
    #[arg(long, value_name = "NAME")]
    pub project: Option<String>,
    /// Restrict to one job state.
    #[arg(long, value_name = "STATE")]
    pub state: Option<String>,
}

/// Arguments for `vowp fleet inbox`.
#[derive(Debug, Clone, Args)]
pub struct FleetInboxArgs {
    /// Restrict to one project.
    #[arg(long, value_name = "NAME")]
    pub project: Option<String>,
    /// Include answered items.
    #[arg(long)]
    pub all: bool,
}

/// Arguments for `vowp fleet capacity`.
#[derive(Debug, Clone, Default, Args)]
pub struct FleetCapacityArgs {}

/// Runs `vowp fleet`.
pub fn run(_ctx: &Ctx, _io: &mut dyn IoStreams, _args: &FleetArgs) -> Result<i32, CliError> {
    Err(crate::cmd::not_implemented())
}
