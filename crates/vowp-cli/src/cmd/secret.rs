//! `vowp secret` and the hidden vault helpers. C0 stub; G1 fills the handlers.

use clap::{Args, Subcommand};

use crate::{CliError, Ctx, IoStreams};

/// Arguments for `vowp secret`.
#[derive(Debug, Clone, Args)]
pub struct SecretArgs {
    /// The secret subcommand.
    #[command(subcommand)]
    pub command: SecretCommand,
}

/// The `vowp secret` subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum SecretCommand {
    /// Store a secret through a hidden prompt.
    Set(SecretSetArgs),
    /// List secret names and their last four characters.
    Ls,
    /// Remove a secret.
    Rm(SecretRmArgs),
    /// Move basic-mode secrets to the vault user.
    Upgrade,
    /// Open a one-time paste page for a secret.
    Request(SecretRequestArgs),
}

/// Arguments for `vowp secret set`.
#[derive(Debug, Clone, Args)]
pub struct SecretSetArgs {
    /// Secret name (`^[A-Z][A-Z0-9_]{0,63}$`).
    pub name: String,
}

/// Arguments for `vowp secret rm`.
#[derive(Debug, Clone, Args)]
pub struct SecretRmArgs {
    /// Secret name.
    pub name: String,
    /// Confirm the removal.
    #[arg(long)]
    pub yes: bool,
}

/// Arguments for `vowp secret request`.
#[derive(Debug, Clone, Args)]
pub struct SecretRequestArgs {
    /// Secret name.
    pub name: String,
}

/// Arguments for the hidden `vowp __vault-exec`.
#[derive(Debug, Clone, Args)]
pub struct VaultExecArgs {
    /// Name of the one variable to inject.
    #[arg(long, value_name = "NAME")]
    pub env: String,
    /// Program and arguments, after `--`.
    #[arg(last = true, num_args = 1.., required = true)]
    pub argv: Vec<String>,
}

/// Arguments for the hidden `vowp __vault-admin`.
#[derive(Debug, Clone, Args)]
pub struct VaultAdminArgs {
    /// The admin action.
    #[command(subcommand)]
    pub command: VaultAdminCommand,
}

/// The hidden `__vault-admin` actions.
#[derive(Debug, Clone, Subcommand)]
pub enum VaultAdminCommand {
    /// Read one value from stdin and write it as the vault user.
    Set(VaultNameArgs),
    /// Remove one secret as the vault user.
    Rm(VaultNameArgs),
}

/// Arguments carrying only a secret name.
#[derive(Debug, Clone, Args)]
pub struct VaultNameArgs {
    /// Secret name.
    pub name: String,
}

/// Runs `vowp secret`.
pub fn run(_ctx: &Ctx, _io: &mut dyn IoStreams, _args: &SecretArgs) -> Result<i32, CliError> {
    Err(crate::cmd::not_implemented())
}

/// Runs the hidden `vowp __vault-exec`.
pub fn vault_exec(_ctx: &Ctx, _io: &mut dyn IoStreams, _args: &VaultExecArgs) -> Result<i32, CliError> {
    Err(crate::cmd::not_implemented())
}

/// Runs the hidden `vowp __vault-admin`.
pub fn vault_admin(_ctx: &Ctx, _io: &mut dyn IoStreams, _args: &VaultAdminArgs) -> Result<i32, CliError> {
    Err(crate::cmd::not_implemented())
}
