//! `vowp mcp` — the stdio MCP server. C0 stub; F6 fills the handler and the `mcp` module.

use std::path::PathBuf;

use clap::Args;

use crate::{CliError, Ctx, IoStreams};

/// Arguments for `vowp mcp`.
#[derive(Debug, Clone, Args)]
pub struct McpArgs {
    /// Session the server writes to.
    #[arg(long, value_name = "ID")]
    pub session: String,
    /// Surface id this server represents.
    #[arg(long, value_name = "ID")]
    pub surface: String,
    /// File that holds the lease token (read on every write).
    #[arg(long, value_name = "FILE")]
    pub token_file: PathBuf,
}

/// Runs `vowp mcp`.
pub fn run(_ctx: &Ctx, _io: &mut dyn IoStreams, _args: &McpArgs) -> Result<i32, CliError> {
    Err(crate::cmd::not_implemented())
}
