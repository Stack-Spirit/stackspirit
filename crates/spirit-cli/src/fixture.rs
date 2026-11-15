//! Explicit fixture-mode helpers.
//!
//! Fixture data is used only when `--fixtures` is passed. There is no silent fallback:
//! a live command uses the system SSH runner and fails normally.

use spirit_core::fixtures::{FixtureVowpClient, Scenario};

use crate::args::Cli;
use crate::error::CliError;

/// Parses the requested scenario, or the command-specific default.
pub(crate) fn scenario(cli: &Cli, default_name: &str) -> Result<Scenario, CliError> {
    let name = cli.fixture.as_deref().unwrap_or(default_name);
    Scenario::from_name(name).ok_or_else(|| {
        let names = Scenario::ALL
            .iter()
            .map(|scenario| scenario.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        CliError::usage(format!("unknown fixture scenario {name:?}"))
            .with_hint(format!("Available scenarios: {names}."))
    })
}

/// Builds the core fixture client for a scenario.
pub(crate) fn client(cli: &Cli, default_name: &str) -> Result<FixtureVowpClient, CliError> {
    let scenario = scenario(cli, default_name)?;
    FixtureVowpClient::new(scenario).map_err(CliError::from)
}
