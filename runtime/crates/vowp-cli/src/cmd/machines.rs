//! `stackspirit machines ls`: the cloud account's machines and their presence.
//!
//! Spec: `docs/design/relay-and-packaging.md` §6.4 and §6.7. The command reads the stored
//! session, calls `GET /v1/machines`, and prints the id, name, online/offline presence and last
//! seen time. `--json` prints one document with the same normalised fields.

use serde::Serialize;

use crate::cli::{MachinesArgs, MachinesCommand, MachinesLsArgs};
use crate::cloud::{Client, Machine, list_machines};
use crate::output::{self, Table};
use crate::{CliError, Global, IoStreams};

/// The `--json` document for `machines ls`.
#[derive(Debug, Serialize)]
struct MachinesReport {
    api_url: String,
    machines: Vec<Machine>,
}

/// Runs one `stackspirit machines` subcommand.
pub fn run(global: &Global, io: &mut dyn IoStreams, args: &MachinesArgs) -> Result<i32, CliError> {
    match &args.command {
        MachinesCommand::Ls(ls) => run_ls(global, io, ls),
    }
}

fn run_ls(global: &Global, io: &mut dyn IoStreams, _args: &MachinesLsArgs) -> Result<i32, CliError> {
    let home = crate::ctx::node_paths(global)?.root().to_path_buf();
    let mut client = Client::load(&home)?;
    let machines = list_machines(&mut client)?;
    if global.json {
        output::print_json(
            io,
            &MachinesReport {
                api_url: client.api_url().to_owned(),
                machines,
            },
        )?;
        return Ok(0);
    }
    let mut table = Table::new(&["ID", "NAME", "STATE", "LAST SEEN"]);
    for machine in &machines {
        table.row(vec![
            machine.id.clone(),
            machine.name.clone(),
            machine.state.clone(),
            machine.last_seen_at.clone().unwrap_or_else(|| "never".to_owned()),
        ]);
    }
    let rendered = table.render(io.is_tty());
    write!(io.out(), "{rendered}")?;
    Ok(0)
}
