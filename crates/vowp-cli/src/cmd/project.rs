//! `vowp project` — the project registry. C0 stub; C2 fills the handler.

use std::path::PathBuf;

use clap::{Args, Subcommand};

use crate::{CliError, Ctx, IoStreams};

/// Arguments for `vowp project`.
#[derive(Debug, Clone, Args)]
pub struct ProjectArgs {
    /// The project subcommand.
    #[command(subcommand)]
    pub command: ProjectCommand,
}

/// The `vowp project` subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum ProjectCommand {
    /// Register a git checkout as a project.
    Add(ProjectAddArgs),
    /// List registered projects.
    Ls(ProjectLsArgs),
    /// Stop new work in a project.
    Pause(ProjectNameArgs),
    /// Resume a paused project.
    Resume(ProjectNameArgs),
    /// Archive a project.
    Archive(ProjectNameArgs),
    /// Archive a project's database and remove its registry row.
    Rm(ProjectRmArgs),
    /// Set a project's fairness weight.
    Weight(ProjectWeightArgs),
}

/// Arguments for `vowp project add`.
#[derive(Debug, Clone, Args)]
pub struct ProjectAddArgs {
    /// Path of the checkout on the home.
    pub path: PathBuf,
    /// Project name; defaults to the directory name.
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,
    /// Fairness weight, `1..=100`.
    #[arg(long, value_name = "N")]
    pub weight: Option<u8>,
}

/// Arguments for `vowp project ls`.
#[derive(Debug, Clone, Args)]
pub struct ProjectLsArgs {
    /// Include the reserved `_node` project.
    #[arg(long)]
    pub all: bool,
}

/// Arguments carrying only a project name.
#[derive(Debug, Clone, Args)]
pub struct ProjectNameArgs {
    /// Project name.
    pub name: String,
}

/// Arguments for `vowp project rm`.
#[derive(Debug, Clone, Args)]
pub struct ProjectRmArgs {
    /// Project name.
    pub name: String,
    /// Confirm the archive.
    #[arg(long)]
    pub yes: bool,
}

/// Arguments for `vowp project weight`.
#[derive(Debug, Clone, Args)]
pub struct ProjectWeightArgs {
    /// Project name.
    pub name: String,
    /// Fairness weight, `1..=100`.
    pub weight: u8,
}

/// Runs `vowp project`.
pub fn run(_ctx: &Ctx, _io: &mut dyn IoStreams, _args: &ProjectArgs) -> Result<i32, CliError> {
    Err(crate::cmd::not_implemented())
}
