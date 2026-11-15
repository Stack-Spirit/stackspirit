//! Command-line grammar for `spirit`.
//!
//! Parsing is pure: it never reads the host store, the network or the environment.
//! Command handlers resolve a host and produce output later.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// The `spirit` command line.
#[derive(Debug, Parser)]
#[command(
    name = "spirit",
    version,
    about = "StackSpirit command line over spirit-core",
    long_about = "Read and manage StackSpirit hosts, fleet views, capacity, gates and the \
                  local engine status. Every command is a one-shot process; use --fixtures \
                  to preview against the core's documented golden documents."
)]
pub struct Cli {
    /// Print machine-readable JSON instead of a human summary.
    #[arg(long, global = true)]
    pub json: bool,

    /// Use golden fixture documents instead of a live host.
    #[arg(long, global = true)]
    pub fixtures: bool,

    /// Fixture scenario name. Requires `--fixtures`.
    #[arg(long, global = true, value_name = "SCENARIO")]
    pub fixture: Option<String>,

    /// Host id, unique id prefix, or label.
    #[arg(long, global = true, value_name = "HOST")]
    pub host: Option<String>,

    /// The command to run.
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Add, inspect, trust and connect saved hosts.
    #[command(subcommand)]
    Host(HostCommand),

    /// Read the fleet view.
    Fleet(FleetArgs),

    /// Read the capacity rail.
    Capacity,

    /// Read the local-orchestrator and ASIF capability gates.
    Gates,

    /// Read the local engine status without starting or changing it.
    #[command(subcommand)]
    Engine(EngineCommand),

    /// Print the CLI and core version.
    Version,
}

/// Host management commands.
#[derive(Debug, Subcommand)]
pub enum HostCommand {
    /// Save a host profile with a confirmed host-key fingerprint.
    Add(AddHostArgs),

    /// List saved hosts.
    Ls,

    /// Show one saved host.
    Show(HostRefArgs),

    /// Remove one saved host.
    Remove(HostRefArgs),

    /// Re-scan a host key, verify a fingerprint, and install it in the app-private
    /// known_hosts file.
    Trust(TrustArgs),

    /// Run the read-only host probe.
    Probe(HostRefArgs),

    /// Connect once, show the connection snapshot, and exit.
    Connect(HostRefArgs),

    /// Report a disconnected CLI connection.
    Disconnect(HostRefArgs),
}

/// Engine commands.
#[derive(Debug, Subcommand)]
pub enum EngineCommand {
    /// Show the saved local engine state and preferences. Read-only.
    Status,
}

/// A host reference used by show/remove/probe/connect/disconnect.
#[derive(Debug, Args)]
pub struct HostRefArgs {
    /// Host id, unique id prefix, or label. Defaults to `--host`, then to the only
    /// saved host.
    #[arg(value_name = "HOST")]
    pub host: Option<String>,
}

/// Arguments for `host add`.
#[derive(Debug, Args)]
pub struct AddHostArgs {
    /// Display label, 1-64 characters.
    #[arg(long)]
    pub label: String,

    /// SSH destination, `USER@HOST` or `USER@HOST:PORT`.
    #[arg(long, value_name = "USER@HOST[:PORT]")]
    pub ssh: String,

    /// SSH port. Conflicts with an embedded `:PORT`.
    #[arg(long, value_name = "PORT")]
    pub port: Option<u16>,

    /// Identity file path. The CLI records the path and never reads the key.
    #[arg(long, value_name = "PATH")]
    pub identity_file: Option<PathBuf>,

    /// Remote VOWP home path, when it is not the default.
    #[arg(long, value_name = "PATH")]
    pub vowp_home: Option<String>,

    /// Default project for project-scoped actions.
    #[arg(long, value_name = "NAME")]
    pub default_project: Option<String>,

    /// SSH host-key algorithm for the confirmed fingerprint.
    #[arg(long, default_value = "ssh-ed25519")]
    pub algorithm: String,

    /// Confirmed SHA-256 fingerprint, exactly as `ssh-keygen -lf` prints it.
    #[arg(long, value_name = "SHA256:...")]
    pub fingerprint: String,
}

/// Arguments for `host trust`.
#[derive(Debug, Args)]
pub struct TrustArgs {
    /// Host id, unique id prefix, or label.
    #[command(flatten)]
    pub target: HostRefArgs,

    /// SSH host-key algorithm for the confirmed fingerprint.
    #[arg(long, default_value = "ssh-ed25519")]
    pub algorithm: String,

    /// Confirmed SHA-256 fingerprint, exactly as `ssh-keygen -lf` prints it.
    #[arg(long, value_name = "SHA256:...")]
    pub fingerprint: String,
}

/// Arguments for `fleet`.
#[derive(Debug, Args)]
pub struct FleetArgs {
    /// Show only one project and its cards.
    #[arg(long, value_name = "NAME")]
    pub project: Option<String>,
}
