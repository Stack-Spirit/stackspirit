//! `vowp migrate` — the v0.1 to v0.2 home migration.
//!
//! Spec: `docs/design/v0.2/home-and-surfaces.md` §3.9. The split itself lives in
//! `vowp_store::migrate`; this wrapper only picks the mode, prints the report and maps
//! store refusals onto the CLI's exit codes. `vowp up` calls the same store function
//! automatically after its pre-migration backup, unless `--no-migrate` is given.

use std::path::PathBuf;

use clap::Args;
use vowp_store::MigrationReport;

use crate::output::{Table, print_json};
use crate::{CliError, Ctx, Global, IoStreams};

/// Arguments for `vowp migrate`.
#[derive(Debug, Clone, Default, Args)]
pub struct MigrateArgs {
    /// Report what would change without writing.
    #[arg(long)]
    pub dry_run: bool,
    /// Restore from this pre-migration backup instead of migrating.
    #[arg(long, value_name = "BACKUP")]
    pub rollback: Option<PathBuf>,
    /// Confirm the migration.
    #[arg(long)]
    pub yes: bool,
}

/// Runs `vowp migrate` with a project context.
pub fn run(ctx: &Ctx, io: &mut dyn IoStreams, args: &MigrateArgs) -> Result<i32, CliError> {
    migrate(ctx.paths(), ctx.json(), ctx.color(), io, args)
}

/// Runs `vowp migrate` without needing a `vowp.toml` in the current directory.
pub fn run_home(global: &Global, io: &mut dyn IoStreams, args: &MigrateArgs) -> Result<i32, CliError> {
    let paths = crate::ctx::node_paths(global)?;
    let color = !global.no_color && io.is_tty() && std::env::var_os("NO_COLOR").is_none();
    migrate(&paths, global.json, color, io, args)
}

/// Runs one migration mode and prints its report.
fn migrate(
    paths: &vowp_store::NodePaths,
    json: bool,
    color: bool,
    io: &mut dyn IoStreams,
    args: &MigrateArgs,
) -> Result<i32, CliError> {
    let report = match (&args.rollback, args.dry_run) {
        (Some(backup), false) => vowp_store::migrate_rollback(paths, backup)?,
        (Some(_), true) => {
            return Err(CliError::Invalid(
                "--dry-run and --rollback cannot be combined".to_owned(),
            ));
        }
        (None, true) => vowp_store::migrate_v01_dry_run(paths)?,
        (None, false) => {
            let _ = args.yes;
            vowp_store::migrate_v01(paths)?
        }
    };
    print_report(json, color, io, &report)?;
    Ok(0)
}

/// Prints one migration report as JSON or a short human summary with per-table counts.
fn print_report(json: bool, color: bool, io: &mut dyn IoStreams, report: &MigrationReport) -> Result<(), CliError> {
    if json {
        print_json(io, report)?;
        return Ok(());
    }
    let verb = if report.dry_run {
        "would migrate"
    } else if report.to_version == 1 {
        "rolled back"
    } else {
        "migrated"
    };
    writeln!(
        io.out(),
        "{verb} schema {} -> {} ({} project(s)); backup: {}",
        report.from_version,
        report.to_version,
        report.projects.len(),
        report.backup
    )?;
    if !report.projects.is_empty() {
        let names = report
            .projects
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(io.out(), "projects: {names}")?;
    }
    if !report.tables.is_empty() {
        let mut table = Table::new(&["TABLE", "BEFORE", "AFTER"]);
        for (name, count) in &report.tables {
            table.row(vec![name.clone(), count.before.to_string(), count.after.to_string()]);
        }
        writeln!(io.out(), "{}", table.render(color))?;
    }
    if report.dropped_workers > 0 {
        writeln!(
            io.out(),
            "dropped {} unassignable worker row(s): {}",
            report.dropped_workers,
            report.dropped_worker_rows.join(", ")
        )?;
    }
    if !report.primary_runs.is_empty() {
        writeln!(
            io.out(),
            "job-less runs moved to the primary project: {}",
            report.primary_runs.len()
        )?;
    }
    for warning in &report.warnings {
        writeln!(io.out(), "warning: {warning}")?;
    }
    if report.dry_run {
        writeln!(io.out(), "nothing was written")?;
    }
    Ok(())
}
