//! `vowp orchestrator` — orchestrator sessions and app hosts. C0 stub; F4 fills the handler.

use clap::{Args, Subcommand};

use crate::{CliError, Ctx, IoStreams};

/// Arguments for `vowp orchestrator`.
#[derive(Debug, Clone, Args)]
pub struct OrchestratorArgs {
    /// The orchestrator subcommand.
    #[command(subcommand)]
    pub command: OrchestratorCommand,
}

/// The `vowp orchestrator` subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum OrchestratorCommand {
    /// Create and start an orchestrator.
    New(OrchestratorNewArgs),
    /// List orchestrators.
    Ls,
    /// Stop an orchestrator's host and release its lease.
    Stop(OrchestratorNameArgs),
    /// Start an orchestrator's host again.
    Resume(OrchestratorNameArgs),
}

/// Arguments for `vowp orchestrator new`.
#[derive(Debug, Clone, Args)]
pub struct OrchestratorNewArgs {
    /// Orchestrator name.
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,
    /// Project to bind, when any.
    #[arg(long, value_name = "NAME")]
    pub project: Option<String>,
    /// App to run: `opencode`, `claude-code` or `chat`.
    #[arg(long, value_name = "APP")]
    pub app: String,
    /// Executor to use.
    #[arg(long, value_name = "EXECUTOR")]
    pub executor: Option<String>,
}

/// Arguments carrying only an orchestrator name.
#[derive(Debug, Clone, Args)]
pub struct OrchestratorNameArgs {
    /// Orchestrator name.
    pub name: String,
}

/// Runs `vowp orchestrator`.
pub fn run(_ctx: &Ctx, _io: &mut dyn IoStreams, _args: &OrchestratorArgs) -> Result<i32, CliError> {
    Err(crate::cmd::not_implemented())
}
