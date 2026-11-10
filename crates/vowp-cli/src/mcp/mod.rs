//! The stdio MCP server. C0 creates the module; F6 fills the server.

use std::path::PathBuf;

use vowp_protocol::{SessionId, SurfaceId};

use crate::{CliError, IoStreams};

/// Options for `vowp mcp`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpOptions {
    /// Session the server writes to.
    pub session: SessionId,
    /// Surface id this server represents.
    pub surface: SurfaceId,
    /// File that holds the lease token.
    pub token_file: PathBuf,
}

/// Serves MCP JSON-RPC until stdin closes.
pub fn serve(_io: &mut dyn IoStreams, _opts: &McpOptions) -> Result<i32, CliError> {
    Err(crate::cmd::not_implemented())
}
