//! Hidden `vowp session lease` and `session takeover`. C0 stub; F2 fills both.

use clap::Args;

use crate::{CliError, Ctx, IoStreams};

/// Arguments for the hidden `vowp session lease`.
#[derive(Debug, Clone, Args)]
pub struct LeaseArgs {
    /// Session id or unique prefix.
    pub session: String,
    /// Acquire the lease for this surface kind (`chat`, `opencode`, ...).
    #[arg(long, value_name = "KIND")]
    pub acquire: Option<String>,
    /// Human-visible holder name.
    #[arg(long, value_name = "NAME")]
    pub holder: Option<String>,
    /// Node the surface runs on.
    #[arg(long, value_name = "NAME")]
    pub node: Option<String>,
}

/// Arguments for the hidden `vowp session takeover`.
#[derive(Debug, Clone, Args)]
pub struct TakeoverArgs {
    /// Session id or unique prefix.
    pub session: String,
    /// Surface kind taking over (`chat`, `opencode`, ...).
    #[arg(long, value_name = "KIND")]
    pub kind: String,
    /// Human-visible holder name.
    #[arg(long, value_name = "NAME")]
    pub holder: String,
    /// Node the new surface runs on.
    #[arg(long, value_name = "NAME")]
    pub node: String,
}

/// Runs the hidden `vowp session lease`.
pub fn lease(_ctx: &Ctx, _io: &mut dyn IoStreams, _args: &LeaseArgs) -> Result<i32, CliError> {
    Err(crate::cmd::not_implemented())
}

/// Runs the hidden `vowp session takeover`.
pub fn takeover(_ctx: &Ctx, _io: &mut dyn IoStreams, _args: &TakeoverArgs) -> Result<i32, CliError> {
    Err(crate::cmd::not_implemented())
}
