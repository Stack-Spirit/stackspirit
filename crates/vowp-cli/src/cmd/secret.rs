//! `vowp secret` and the hidden vault helpers.
//!
//! Values are read from a hidden prompt or from stdin, never from argv. The CLI only
//! renders the core vault API; storage, injection and redaction live in `vowp-core`.
//!
//! Spec: `docs/design/v0.2/home-and-surfaces.md` §9 and §12.

use std::io::IsTerminal;
use std::os::unix::process::CommandExt;
use std::process::Command;

use clap::{Args, Subcommand};
use vowp_core::vault::{self, REQUEST_TTL, SecretRequestServer, Upgrade};
use vowp_protocol::{SecretInfo, SecretName, VaultMode};

use crate::output::{Table, local_time_at, print_json};
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
    /// Check that the helper context and vault directory are usable.
    Probe,
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
pub fn run(ctx: &Ctx, io: &mut dyn IoStreams, args: &SecretArgs) -> Result<i32, CliError> {
    match &args.command {
        SecretCommand::Set(set) => set_secret(ctx, io, set),
        SecretCommand::Ls => list_secrets(ctx, io),
        SecretCommand::Rm(remove) => remove_secret(ctx, io, remove),
        SecretCommand::Upgrade => upgrade(ctx, io),
        SecretCommand::Request(request) => request_secret(ctx, io, request),
    }
}

/// Runs the hidden `vowp __vault-exec`.
pub fn vault_exec(ctx: &Ctx, _io: &mut dyn IoStreams, args: &VaultExecArgs) -> Result<i32, CliError> {
    let name = SecretName::parse(&args.env)?;
    let value =
        vault::read_value_for_exec(ctx.deps(), &name)?.ok_or_else(|| CliError::NotFound(format!("secret {name}")))?;
    let Some(program) = args.argv.first() else {
        return Err(CliError::Invalid("__vault-exec needs a program after --".to_owned()));
    };
    // `exec` replaces this process; only a spawn failure returns.
    let error = Command::new(program)
        .args(&args.argv[1..])
        .env(name.as_str(), value)
        .exec();
    Err(CliError::Io(error))
}

/// Runs the hidden `vowp __vault-admin`.
pub fn vault_admin(ctx: &Ctx, io: &mut dyn IoStreams, args: &VaultAdminArgs) -> Result<i32, CliError> {
    match &args.command {
        VaultAdminCommand::Probe => {
            vault::admin_probe(ctx.deps())?;
            Ok(0)
        }
        VaultAdminCommand::Set(set) => {
            let name = SecretName::parse(&set.name)?;
            let value = crate::read_stdin(io)?;
            if value.is_empty() {
                return Err(CliError::Invalid("secret value must not be empty".to_owned()));
            }
            let info = vault::admin_set(ctx.deps(), &name, &value)?;
            writeln!(io.out(), "saved {} ends in …{}", info.name, info.last4)?;
            Ok(0)
        }
        VaultAdminCommand::Rm(remove) => {
            let name = SecretName::parse(&remove.name)?;
            if vault::admin_remove(ctx.deps(), &name)? {
                writeln!(io.out(), "removed {name}")?;
            } else {
                writeln!(io.out(), "missing {name}")?;
            }
            Ok(0)
        }
    }
}

/// Stores a secret after reading it from a hidden terminal prompt.
fn set_secret(ctx: &Ctx, io: &mut dyn IoStreams, args: &SecretSetArgs) -> Result<i32, CliError> {
    let name = SecretName::parse(&args.name)?;
    if !std::io::stdin().is_terminal() {
        return Err(CliError::Guard(
            "run it in a terminal or use `vowp secret request`".to_owned(),
        ));
    }
    let guard = EchoGuard::hide();
    write!(io.err(), "value for {name}: ")?;
    io.err().flush()?;
    let mut line = String::new();
    io.input().read_line(&mut line)?;
    drop(guard);
    writeln!(io.err())?;
    let value = line.trim_end_matches(&['\r', '\n'][..]).to_owned();
    let info = vault::write_secret(ctx.deps(), &name, &value)?;
    if ctx.json() {
        print_json(io, &info)?;
    } else {
        writeln!(io.out(), "{} saved, ends in …{}", info.name, info.last4)?;
    }
    Ok(0)
}

/// Lists secret names, last four characters and update times.
fn list_secrets(ctx: &Ctx, io: &mut dyn IoStreams) -> Result<i32, CliError> {
    let infos = vault::list_secrets(ctx.deps())?;
    if ctx.json() {
        print_json(io, &infos)?;
        return Ok(0);
    }
    let now = ctx.clock().now();
    let mut table = Table::new(&["NAME", "LAST4", "UPDATED"]);
    for info in &infos {
        table.row(vec![
            info.name.to_string(),
            info.last4.clone(),
            local_time_at(info.updated_at, now, ctx.tz_offset_minutes()),
        ]);
    }
    writeln!(io.out(), "{}", table.render(ctx.color()))?;
    Ok(0)
}

/// Removes one secret after the explicit `--yes` guard.
fn remove_secret(ctx: &Ctx, io: &mut dyn IoStreams, args: &SecretRmArgs) -> Result<i32, CliError> {
    if !args.yes {
        return Err(CliError::Guard(format!("removing secret {} requires --yes", args.name)));
    }
    let name = SecretName::parse(&args.name)?;
    if !vault::remove_secret(ctx.deps(), &name)? {
        return Err(CliError::NotFound(format!("secret {name}")));
    }
    if ctx.json() {
        print_json(io, &serde_json::json!({"name": name.to_string(), "removed": true}))?;
    } else {
        writeln!(io.out(), "{name} removed")?;
    }
    Ok(0)
}

/// Prints or completes the basic-to-vault upgrade.
fn upgrade(ctx: &Ctx, io: &mut dyn IoStreams) -> Result<i32, CliError> {
    match vault::upgrade(ctx.deps())? {
        Upgrade::AlreadyVault => {
            if ctx.json() {
                print_json(io, &serde_json::json!({"mode": VaultMode::VaultUser}))?;
            } else {
                writeln!(io.out(), "secrets: vault user")?;
            }
            Ok(0)
        }
        Upgrade::Switched => {
            if ctx.json() {
                print_json(io, &serde_json::json!({"mode": VaultMode::VaultUser}))?;
            } else {
                writeln!(io.out(), "secrets: vault user (switched from basic mode)")?;
            }
            Ok(0)
        }
        Upgrade::NeedsCommand(command) => Err(CliError::Guard(format!(
            "run this command, then re-run `vowp secret upgrade`:\n{command}"
        ))),
        Upgrade::NoSudo(reason) => Err(CliError::Guard(reason)),
    }
}

/// Starts the one-time paste page and waits for the owner's value.
fn request_secret(ctx: &Ctx, io: &mut dyn IoStreams, args: &SecretRequestArgs) -> Result<i32, CliError> {
    let name = SecretName::parse(&args.name)?;
    let server = SecretRequestServer::bind(ctx.deps(), &name, ctx.clock().now())?;
    let request = server.request().clone();
    if ctx.json() {
        print_json(io, &request)?;
    } else {
        writeln!(io.out(), "open {}", request.url)?;
        writeln!(
            io.out(),
            "expires {} (single use, 10 minutes)",
            request.expires_at.as_rfc3339()
        )?;
    }
    io.out().flush()?;
    let info: SecretInfo = server.wait(REQUEST_TTL)?;
    if !ctx.json() {
        writeln!(io.out(), "{} saved, ends in …{}", info.name, info.last4)?;
    }
    Ok(0)
}

/// Restores terminal echo on every return path, including errors.
struct EchoGuard {
    active: bool,
}

impl EchoGuard {
    /// Disables echo when the process has a terminal on stdin.
    fn hide() -> Self {
        let active = Command::new("stty")
            .arg("-echo")
            .status()
            .is_ok_and(|status| status.success());
        Self { active }
    }
}

impl Drop for EchoGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = Command::new("stty").arg("echo").status();
        }
    }
}
