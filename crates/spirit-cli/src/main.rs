//! The `spirit` binary: parse, run, and map a typed failure to an exit code.

use std::io;
use std::process::ExitCode;

use clap::Parser;
use spirit_cli::{Cli, CliEnv, CliPaths};

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let paths = match CliPaths::from_env() {
        Ok(paths) => paths,
        Err(error) => {
            let mut stderr = io::stderr().lock();
            let _ = error.report(cli.json, &mut stderr);
            return ExitCode::from(error.exit);
        }
    };
    let env = CliEnv::system(paths);
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    match spirit_cli::run(&cli, &env, &mut stdout, &mut stderr).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = error.report(cli.json, &mut stderr);
            ExitCode::from(error.exit)
        }
    }
}
