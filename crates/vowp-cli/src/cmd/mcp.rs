//! `vowp mcp` — the stdio MCP server and the local MCP proxy.
//!
//! Spec: `docs/design/v0.2/home-and-surfaces.md` §7.7 and
//! `docs/design/v0.4/local-orchestrator.md` §5.4. The handler only parses the frozen
//! flags; the home server lives in [`crate::mcp`] and the Mac proxy in
//! [`crate::mcp::proxy`]. The proxy is dispatched before `Ctx::build`, because a Mac has no
//! local VOWP database.

use std::path::PathBuf;

use clap::Args;
use vowp_protocol::{SessionId, SurfaceId};

use crate::mcp::McpOptions;
use crate::mcp::proxy::ProxyOptions;
use crate::{CliError, Ctx, IoStreams};

/// Arguments for `vowp mcp`.
///
/// With none of `--session`, `--surface`, `--token-file` and `--agent-socket`, the command
/// serves the read-only no-session server (`docs/design/relay-and-packaging.md` §3.7).
/// Supplying any one of the three home flags requires all three.
#[derive(Debug, Clone, Args)]
pub struct McpArgs {
    /// Session the server writes to (home mode; not with --agent-socket).
    #[arg(long, value_name = "ID", conflicts_with = "agent_socket")]
    pub session: Option<String>,
    /// Surface id this server represents.
    #[arg(long, value_name = "ID")]
    pub surface: Option<String>,
    /// File that holds the lease token (home mode; not with --agent-socket).
    #[arg(long, value_name = "FILE", conflicts_with = "agent_socket")]
    pub token_file: Option<PathBuf>,
    /// Unix socket served by `vowp agent run`; selects the local MCP proxy.
    #[arg(long, value_name = "PATH", requires = "surface")]
    pub agent_socket: Option<PathBuf>,
}

/// Runs the home `vowp mcp` server: full session mode, or the read-only mode with no flags.
pub fn run(ctx: &Ctx, io: &mut dyn IoStreams, args: &McpArgs) -> Result<i32, CliError> {
    if args.session.is_none() && args.surface.is_none() && args.token_file.is_none() {
        return crate::mcp::serve_read_only_with_ctx(ctx, io);
    }
    let missing: Vec<&str> = [
        ("--session", args.session.is_none()),
        ("--surface", args.surface.is_none()),
        ("--token-file", args.token_file.is_none()),
    ]
    .iter()
    .filter(|(_, absent)| *absent)
    .map(|(name, _)| *name)
    .collect();
    let (Some(session_text), Some(surface), Some(token_file)) =
        (args.session.as_deref(), args.surface.as_ref(), args.token_file.as_ref())
    else {
        return Err(CliError::Invalid(format!(
            "the full MCP server needs --session, --surface and --token-file together; missing {}",
            missing.join(", ")
        )));
    };
    let session = SessionId::parse(session_text)?;
    if surface.trim().is_empty() {
        return Err(CliError::Invalid("--surface must not be empty".to_owned()));
    }
    let opts = McpOptions {
        session,
        surface: SurfaceId(surface.clone()),
        token_file: token_file.clone(),
    };
    crate::mcp::serve_with_ctx(ctx, io, &opts)
}

/// Runs the Mac-side proxy; it must be dispatched before [`Ctx`] exists.
pub fn run_proxy(io: &mut dyn IoStreams, args: &McpArgs) -> Result<i32, CliError> {
    let Some(agent_socket) = args.agent_socket.as_ref() else {
        return Err(CliError::Invalid(
            "--agent-socket selects the local MCP proxy".to_owned(),
        ));
    };
    if args.session.is_some() || args.token_file.is_some() {
        return Err(CliError::Invalid(
            "--session and --token-file are not used with --agent-socket".to_owned(),
        ));
    }
    let surface_text = args
        .surface
        .as_deref()
        .ok_or_else(|| CliError::Invalid("--surface is required with --agent-socket".to_owned()))?;
    if surface_text.trim().is_empty() {
        return Err(CliError::Invalid("--surface must not be empty".to_owned()));
    }
    let surface = SurfaceId(surface_text.to_owned());
    vowp_agent::validate_surface_id(&surface)
        .map_err(|_| CliError::Invalid("--surface must be a 26-character ULID".to_owned()))?;
    let opts = ProxyOptions {
        agent_socket: agent_socket.clone(),
        surface,
    };
    crate::mcp::proxy::serve(io, &opts)
}
