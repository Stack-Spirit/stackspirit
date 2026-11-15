//! Command handlers. Every handler writes through [`Output`] and returns a typed
//! failure instead of terminating the process.

mod engine;
mod host;
mod read;
mod version;

use crate::args::{Cli, Command};
use crate::context::CliEnv;
use crate::error::CliError;
use crate::output::Output;

pub(crate) async fn dispatch(
    cli: &Cli,
    env: &CliEnv,
    output: &mut Output<'_>,
) -> Result<(), CliError> {
    if cli.fixture.is_some() && !cli.fixtures {
        return Err(CliError::usage("--fixture requires --fixtures"));
    }
    if cli.fixtures && !supports_fixtures(&cli.command) {
        return Err(CliError::usage(
            "--fixtures is only available for fleet, capacity and gates",
        ));
    }
    if cli.fixtures {
        output.note(
            "fixture data: no host is contacted; documents come from crates/spirit-core/fixtures/vowp",
        )?;
    }

    match &cli.command {
        Command::Host(command) => host::dispatch(command, cli, env, output).await,
        Command::Fleet(args) => read::fleet(args, cli, env, output).await,
        Command::Capacity => read::capacity(cli, env, output).await,
        Command::Gates => read::gates(cli, env, output).await,
        Command::Engine(command) => engine::dispatch(command, env, output),
        Command::Version => version::run(output),
    }
}

fn supports_fixtures(command: &Command) -> bool {
    matches!(
        command,
        Command::Fleet(_) | Command::Capacity | Command::Gates
    )
}
