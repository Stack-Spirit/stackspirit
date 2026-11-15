//! The thin `spirit` command line over [`spirit_core`].
//!
//! The CLI owns no VOWP, SSH or view logic: hosts, command execution, fixtures and
//! view models all come from `spirit-core`. It adds only argument parsing, a small
//! app-private config directory, human formatting, and typed process exit codes.
//!
//! This crate has no Tauri, React or desktop-app dependency and never reads a
//! provider key, a `.env` file, or `~/.vowp`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod args;
mod commands;
pub mod context;
pub mod error;
#[cfg(feature = "fixtures")]
mod fixture;
mod known_hosts;
mod output;

use std::io::Write;

pub use args::Cli;
pub use context::{CliEnv, CliPaths};
pub use error::{
    CliError, EXIT_CONFLICT, EXIT_FAILURE, EXIT_NOT_FOUND, EXIT_SUCCESS, EXIT_USAGE, exit_code_for,
};

/// Runs one parsed command.
///
/// `out` receives the command result and `err` receives warnings, the fixture
/// banner, and (from `main`) the final error. The function returns a typed
/// [`CliError`] instead of terminating the process so tests can call it directly.
pub async fn run(
    cli: &Cli,
    env: &CliEnv,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    let mut output = output::Output::new(cli.json, out, err);
    commands::dispatch(cli, env, &mut output).await
}
