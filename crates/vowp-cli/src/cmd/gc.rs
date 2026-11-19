//! `vowp gc` — lifecycle classes and garbage collection.
//!
//! Spec: `docs/design/v0.2/home-and-surfaces.md` §10.2. The dry run is the default; `--apply`
//! deletes only the disposable classes and compacts closed sessions. `--now` exists for tests.

use clap::Args;
use vowp_core::lifecycle::{GcOptions, class_name, gc, parse_class, parse_older_than};
use vowp_protocol::Timestamp;

use crate::output::{Table, print_json};
use crate::{CliError, Ctx, IoStreams};

/// Arguments for `vowp gc`.
#[derive(Debug, Clone, Default, Args)]
pub struct GcArgs {
    /// Delete eligible files instead of only reporting.
    #[arg(long)]
    pub apply: bool,
    /// Restrict to one lifecycle class.
    #[arg(long, value_name = "CLASS")]
    pub class: Option<String>,
    /// Collect closed sessions and run output older than this window (for example 7d, 36h, 2w).
    ///
    /// Applies only to `closed_session` and `run_output`; every other class keeps its default.
    #[arg(long, value_name = "DURATION")]
    pub older_than: Option<String>,
    /// Clock override for tests (RFC 3339).
    #[arg(long, value_name = "RFC3339")]
    pub now: Option<String>,
}

/// Runs `vowp gc`.
pub fn run(ctx: &Ctx, io: &mut dyn IoStreams, args: &GcArgs) -> Result<i32, CliError> {
    let class = args.class.as_deref().map(parse_class).transpose()?;
    let older_than = args.older_than.as_deref().map(parse_older_than).transpose()?;
    let now = args.now.as_deref().map(Timestamp::parse).transpose()?;
    let opts = GcOptions {
        apply: args.apply,
        class,
        older_than,
        now,
    };
    let store = ctx.store()?;
    let report = gc(ctx.deps(), &store, &opts)?;

    if ctx.json() {
        print_json(io, &report)?;
        for error in &report.errors {
            writeln!(io.err(), "warning: {error}")?;
        }
        return Ok(0);
    }

    if report.classes.is_empty() {
        writeln!(io.out(), "no lifecycle files to collect")?;
    } else {
        let mut table = Table::new(&["CLASS", "FILES", "BYTES", "ELIGIBLE", "ELIGIBLE BYTES"]);
        for class in &report.classes {
            table.row(vec![
                class_name(class.class).to_owned(),
                class.files.to_string(),
                class.bytes.to_string(),
                class.eligible_files.to_string(),
                class.eligible_bytes.to_string(),
            ]);
        }
        writeln!(io.out(), "{}", table.render(ctx.color()))?;
        for class in &report.classes {
            if class.sample_paths.is_empty() {
                continue;
            }
            writeln!(
                io.out(),
                "{}: {}",
                class_name(class.class),
                class.sample_paths.join(", ")
            )?;
        }
    }

    if !report.refused.is_empty() {
        writeln!(io.out(), "refused {} candidate(s):", report.refused.len())?;
        for entry in &report.refused {
            let class = class_name(entry.class);
            match &entry.session_id {
                Some(session) => writeln!(io.out(), "  {class} session {session}: {}", entry.reason)?,
                None => writeln!(io.out(), "  {class} {}: {}", entry.path, entry.reason)?,
            }
        }
    }

    if report.dry_run {
        writeln!(
            io.out(),
            "dry run: nothing deleted; pass --apply to collect eligible files"
        )?;
    } else {
        writeln!(
            io.out(),
            "applied: deleted {} files, freed {} bytes, compacted {} sessions",
            report.deleted_files,
            report.freed_bytes,
            report.compacted_sessions
        )?;
    }
    for error in &report.errors {
        writeln!(io.err(), "warning: {error}")?;
    }
    Ok(0)
}
