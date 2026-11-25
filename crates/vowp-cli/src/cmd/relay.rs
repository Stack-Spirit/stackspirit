//! `stackspirit relay` — enroll this machine with the StackSpirit relay, show its state, run the
//! connector and rotate its relay key.
//!
//! Spec: `docs/design/relay-and-packaging.md` §6.2 and §6.3. The enrollment token is read from
//! `--token-file`, `--token-stdin` or `STACKSPIRIT_ENROLL_TOKEN`; it is never a flag value, so it
//! cannot reach a process list or a shell history. `run` connects the loopback daemon (found
//! through `[relay].daemon_url` or `<home>/web-window`) to the relay and never starts a daemon.
//! Nothing here prints a token, credential, private key or payload.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use clap::{Args, Subcommand};
use serde::Serialize;
use vowp_agent::relay::{
    ConnectorOutcome, EnrollRequest, RelayError, RelayPaths, StatusReport, enroll, rotate_key, status_report,
};
use vowp_agent::{Connector, ConnectorOptions};
use vowp_protocol::{NodeFile, RelayConfig};

use crate::output;
use crate::{CliError, Ctx, IoStreams};

/// Environment variable that can carry the enrollment token instead of a file or stdin.
pub const ENROLL_TOKEN_ENV: &str = "STACKSPIRIT_ENROLL_TOKEN";

/// Arguments for `stackspirit relay`.
#[derive(Debug, Clone, Args)]
pub struct RelayArgs {
    /// The relay subcommand.
    #[command(subcommand)]
    pub command: RelayCommand,
}

/// The `stackspirit relay` subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum RelayCommand {
    /// Enroll this machine with the relay using a one-time token and write relay.key.
    Enroll(RelayEnrollArgs),
    /// Show relay enrollment and connector state; never prints a credential or key.
    Status,
    /// Run the relay connector in the foreground until stopped.
    Run,
    /// Replace relay.key with a new key registered through a signed request.
    RotateKey,
}

/// Arguments for `stackspirit relay enroll`.
#[derive(Debug, Clone, Args)]
pub struct RelayEnrollArgs {
    /// Read the one-time enrollment token from this file.
    #[arg(long, value_name = "PATH", conflicts_with = "token_stdin")]
    pub token_file: Option<PathBuf>,
    /// Read the one-time enrollment token from stdin.
    #[arg(long)]
    pub token_stdin: bool,
    /// Machine display name; defaults to the host name.
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,
    /// Project this machine serves (not the global `--project` selector); defaults to
    /// `[relay].project`.
    #[arg(long, value_name = "PROJECT")]
    pub serve_project: Option<String>,
    /// API base URL; defaults to `[relay].api_url`.
    #[arg(long, value_name = "URL")]
    pub api_url: Option<String>,
    /// Relay machine endpoint; defaults to `[relay].relay_url`.
    #[arg(long, value_name = "URL")]
    pub relay_url: Option<String>,
}

/// Runs one `relay` subcommand.
pub fn run(ctx: &Ctx, io: &mut dyn IoStreams, args: &RelayArgs) -> Result<i32, CliError> {
    let home = ctx.paths().root().to_path_buf();
    match &args.command {
        RelayCommand::Enroll(enroll_args) => enroll_command(ctx, io, &home, enroll_args),
        RelayCommand::Status => status_command(ctx, io, &home),
        RelayCommand::Run => run_command(&home),
        RelayCommand::RotateKey => rotate_command(ctx, io, &home),
    }
}

/// Reads `<home>/config.toml` (absent means defaults) and returns the parsed `[relay]` table.
fn load_relay_config(home: &Path) -> Result<RelayConfig, CliError> {
    let path = home.join("config.toml");
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(NodeFile::from_toml(&text)?.relay),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(RelayConfig::default()),
        Err(error) => Err(CliError::Io(error)),
    }
}

/// Maps a relay error onto the CLI's exit-code classes. The messages carry no secret.
fn map_error(error: RelayError) -> CliError {
    match error {
        RelayError::Config(message) => CliError::Invalid(message),
        RelayError::NotEnrolled(message) => CliError::NotFound(format!(
            "not enrolled: {message}; run `stackspirit relay enroll --token-file <path>` first"
        )),
        RelayError::AlreadyEnrolled(message) => CliError::Conflict(format!("already enrolled: {message}")),
        RelayError::Key(message) | RelayError::Auth(message) => CliError::Guard(message),
        RelayError::Revoked => CliError::Guard("this machine was revoked by its owner".to_owned()),
        RelayError::Io(error) => CliError::Io(error),
        other => CliError::Remote(other.to_string()),
    }
}

/// Where the enrollment token comes from, in order: file, stdin, environment.
fn read_token(io: &mut dyn IoStreams, args: &RelayEnrollArgs) -> Result<String, CliError> {
    let raw = if let Some(path) = &args.token_file {
        std::fs::read_to_string(path)
            .map_err(|error| CliError::Invalid(format!("cannot read the token file: {}", error.kind())))?
    } else if args.token_stdin {
        crate::read_stdin(io)?
    } else if let Ok(value) = std::env::var(ENROLL_TOKEN_ENV) {
        value
    } else {
        return Err(CliError::Invalid(format!(
            "no enrollment token: use --token-file <path>, --token-stdin or {ENROLL_TOKEN_ENV}; \
             a token is never accepted as a flag value"
        )));
    };
    let token = raw.trim().to_owned();
    if token.is_empty() {
        return Err(CliError::Invalid("the enrollment token is empty".to_owned()));
    }
    Ok(token)
}

fn default_name() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "machine".to_owned())
}

#[derive(Serialize)]
struct EnrollReport<'a> {
    machine_id: &'a str,
    account_id: &'a str,
    key_id: &'a str,
    name: &'a str,
    api_url: &'a str,
    relay_url: &'a str,
    credential_expires_at: &'a str,
    config: String,
}

fn enroll_command(ctx: &Ctx, io: &mut dyn IoStreams, home: &Path, args: &RelayEnrollArgs) -> Result<i32, CliError> {
    let mut relay = load_relay_config(home)?;
    if let Some(url) = &args.api_url {
        relay.api_url = url.clone();
    }
    if let Some(url) = &args.relay_url {
        relay.relay_url = url.clone();
    }
    if let Some(project) = &args.serve_project {
        relay.project = project.clone();
    }
    let token = read_token(io, args)?;
    let name = args.name.clone().unwrap_or_else(default_name);
    let outcome = enroll(&EnrollRequest {
        home,
        config: &relay,
        token: &token,
        name: &name,
        now: ctx.clock().now(),
    })
    .map_err(map_error)?;
    relay.api_url.clone_from(&outcome.api_url);
    relay.relay_url.clone_from(&outcome.relay_url);
    let config_path = write_enabled_config(home, &relay)?;
    let report = EnrollReport {
        machine_id: &outcome.machine_id,
        account_id: &outcome.account_id,
        key_id: &outcome.key_id,
        name: &outcome.name,
        api_url: &outcome.api_url,
        relay_url: &outcome.relay_url,
        credential_expires_at: &outcome.credential_expires_at,
        config: config_path.display().to_string(),
    };
    if ctx.json() {
        output::print_json(io, &report)?;
    } else {
        writeln!(io.out(), "enrolled {} as {}", report.name, report.machine_id)?;
        writeln!(io.out(), "key id: {}", report.key_id)?;
        writeln!(io.out(), "relay:  {}", report.relay_url)?;
        writeln!(io.out(), "config: [relay] enabled = true in {}", report.config)?;
        writeln!(io.out(), "next:   stackspirit relay run")?;
    }
    Ok(0)
}

/// Sets `[relay] enabled = true` and the URLs/project in `config.toml`, keeping every other
/// setting. The file is validated before it replaces the old one, and replaced by rename.
fn write_enabled_config(home: &Path, relay: &RelayConfig) -> Result<PathBuf, CliError> {
    let path = home.join("config.toml");
    let mut table: toml::Table = match std::fs::read_to_string(&path) {
        Ok(text) => text
            .parse()
            .map_err(|error: toml::de::Error| CliError::Invalid(format!("config.toml is not valid TOML: {error}")))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
        Err(error) => return Err(CliError::Io(error)),
    };
    let section = table
        .entry("relay")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let toml::Value::Table(section) = section else {
        return Err(CliError::Invalid(
            "config.toml has a [relay] value that is not a table".to_owned(),
        ));
    };
    section.insert("enabled".to_owned(), toml::Value::Boolean(true));
    section.insert("api_url".to_owned(), toml::Value::String(relay.api_url.clone()));
    section.insert("relay_url".to_owned(), toml::Value::String(relay.relay_url.clone()));
    if !relay.project.is_empty() {
        section.insert("project".to_owned(), toml::Value::String(relay.project.clone()));
    }
    let text = toml::to_string_pretty(&table).map_err(|error| CliError::Invalid(error.to_string()))?;
    NodeFile::from_toml(&text)?;
    let temp = home.join(".config.toml.relay.tmp");
    std::fs::write(&temp, &text)?;
    std::fs::rename(&temp, &path)?;
    Ok(path)
}

fn status_command(ctx: &Ctx, io: &mut dyn IoStreams, home: &Path) -> Result<i32, CliError> {
    let relay = load_relay_config(home)?;
    let report = status_report(&RelayPaths::new(home, &relay), &relay, ctx.clock().now());
    if ctx.json() {
        output::print_json(io, &report)?;
    } else {
        print_status(io, &report)?;
    }
    Ok(0)
}

fn print_status(io: &mut dyn IoStreams, report: &StatusReport) -> Result<(), CliError> {
    let out = io.out();
    writeln!(out, "enabled:   {}", report.enabled)?;
    writeln!(out, "enrolled:  {}", report.enrolled)?;
    if let Some(id) = &report.machine_id {
        writeln!(out, "machine:   {id}")?;
    }
    if let Some(name) = &report.name {
        writeln!(out, "name:      {name}")?;
    }
    if let Some(id) = &report.key_id {
        writeln!(out, "key id:    {id}")?;
    }
    writeln!(out, "relay:     {}", report.relay_url)?;
    if let Some(expires) = &report.credential_expires_at {
        let note = if report.credential_expired == Some(true) {
            " (expired; `relay run` refreshes it)"
        } else {
            ""
        };
        writeln!(out, "credential expires: {expires}{note}")?;
    }
    let policy = &report.policy;
    writeln!(
        out,
        "policy:    http read={} write={}, mcp read={} write={}, ssh={}, exec={}",
        policy.http_read, policy.http_write, policy.mcp_read, policy.mcp_write, policy.ssh, policy.exec
    )?;
    match &report.connector {
        Some(connector) => {
            let stale = if report.connector_stale {
                " (process is gone)"
            } else {
                ""
            };
            writeln!(
                out,
                "connector: {:?}, {} channel(s), {} connection(s){stale}",
                connector.state, connector.channels, connector.connects
            )?;
            if let Some(error) = &connector.last_error {
                writeln!(out, "last error: {error}")?;
            }
        }
        None => writeln!(out, "connector: not started")?,
    }
    Ok(())
}

fn run_command(home: &Path) -> Result<i32, CliError> {
    let relay = load_relay_config(home)?;
    if !relay.enabled {
        return Err(CliError::Invalid(
            "relay is disabled: run `stackspirit relay enroll` or set [relay] enabled = true in config.toml".to_owned(),
        ));
    }
    let stop = Arc::new(AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
        signal_hook::flag::register(signal, Arc::clone(&stop)).map_err(CliError::Io)?;
    }
    let mut options = ConnectorOptions::new(home.to_path_buf(), relay);
    options.stop = stop;
    // Event lines carry ids and stable codes only; the connector never hands the log a payload.
    options.log = Arc::new(|line| eprintln!("{line}"));
    let report = Connector::new(options).run().map_err(map_error)?;
    match report.outcome {
        ConnectorOutcome::Stopped => Ok(0),
        ConnectorOutcome::Revoked => Err(CliError::Guard(
            "this machine was revoked; the connector stopped and will not reconnect".to_owned(),
        )),
        ConnectorOutcome::Superseded => Err(CliError::Conflict(
            "another connection for this machine superseded this one; the connector stopped".to_owned(),
        )),
    }
}

fn rotate_command(ctx: &Ctx, io: &mut dyn IoStreams, home: &Path) -> Result<i32, CliError> {
    let relay = load_relay_config(home)?;
    let key_id = rotate_key(&RelayPaths::new(home, &relay), ctx.clock().now()).map_err(map_error)?;
    if ctx.json() {
        output::print_json(io, &serde_json::json!({ "key_id": key_id }))?;
    } else {
        writeln!(io.out(), "rotated relay key; new key id: {key_id}")?;
        writeln!(io.out(), "restart `stackspirit relay run` to connect with the new key")?;
    }
    Ok(0)
}

// The package root is the workspace root, so Cargo does not auto-discover the
// `crates/vowp-cli/tests/` directory. Compile the owned relay suite as a library test
// instead; the shared root `Cargo.toml` stays untouched.
#[cfg(test)]
#[path = "../../tests/relay.rs"]
mod integration_tests;
