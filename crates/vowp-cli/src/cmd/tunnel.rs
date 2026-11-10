//! `vowp tunnel` — control-machine SSH tunnels. C0 stub; D2 fills the handler.

use clap::{Args, Subcommand};

use crate::{CliError, Ctx, IoStreams};

/// Arguments for `vowp tunnel`.
#[derive(Debug, Clone, Args)]
pub struct TunnelArgs {
    /// The tunnel subcommand.
    #[command(subcommand)]
    pub command: TunnelCommand,
}

/// The `vowp tunnel` subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum TunnelCommand {
    /// Open a local port forward to the home.
    Open(TunnelOpenArgs),
    /// List open tunnels.
    Ls,
    /// Close one tunnel.
    Close(TunnelCloseArgs),
}

/// Arguments for `vowp tunnel open`.
#[derive(Debug, Clone, Args)]
pub struct TunnelOpenArgs {
    /// Tunnel name.
    pub name: String,
    /// Remote loopback port on the home.
    #[arg(long, value_name = "N")]
    pub remote_port: u16,
    /// Local loopback port; a free port is chosen when omitted.
    #[arg(long, value_name = "N")]
    pub local_port: Option<u16>,
    /// Remote host the tunnel forwards to.
    #[arg(long, value_name = "HOST", default_value = "127.0.0.1")]
    pub host: String,
    /// Allow a local port below 1024.
    #[arg(long)]
    pub allow_privileged: bool,
}

/// Arguments for `vowp tunnel close`.
#[derive(Debug, Clone, Args)]
pub struct TunnelCloseArgs {
    /// Tunnel name.
    pub name: String,
}

/// Runs `vowp tunnel`.
pub fn run(_ctx: &Ctx, _io: &mut dyn IoStreams, _args: &TunnelArgs) -> Result<i32, CliError> {
    Err(crate::cmd::not_implemented())
}
