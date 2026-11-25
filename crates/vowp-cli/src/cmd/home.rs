//! `stackspirit home migrate` — the link bridge and the offline move.
//!
//! Spec: `docs/design/stackspirit-engine-rebrand.md` §3.3 and §5.2. Discovery never writes;
//! this command is the only place that creates the compatibility symlinks or moves a home.
//! `--link` never moves data. `--move` copies to a sibling staging directory, verifies the copy
//! with a read-only open, swaps it into place and leaves the old directory beside it under a
//! `.premove` name. It never edits a database and never deletes the original.

use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use serde::Serialize;
use vowp_store::{NodePaths, StateKind};

use crate::output::print_json;
use crate::{CliError, Ctx, IoStreams};

/// Arguments for `vowp home`.
#[derive(Debug, Clone, Args)]
pub struct HomeArgs {
    /// The home subcommand.
    #[command(subcommand)]
    pub command: HomeCommand,
}

/// `vowp home` subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum HomeCommand {
    /// Create compatibility symlinks between the StackSpirit and legacy directory names, or move a
    /// stopped home to a new directory.
    Migrate(HomeMigrateArgs),
}

/// Arguments for `vowp home migrate`.
#[derive(Debug, Clone, Args)]
pub struct HomeMigrateArgs {
    /// Create the compatibility symlinks without moving data.
    #[arg(long, conflicts_with = "move_", required_unless_present = "move_")]
    pub link: bool,
    /// Copy a stopped home to `--to` and leave the old path as a symlink.
    #[arg(long = "move", requires = "to")]
    pub move_: bool,
    /// Destination directory for `--move` (must not exist).
    #[arg(long, value_name = "DIR")]
    pub to: Option<PathBuf>,
    /// Which state directory `--move` moves: `home` (default), `agent` or `control`.
    #[arg(long, value_name = "KIND", default_value = "home")]
    pub kind: String,
    /// Print the plan and write nothing.
    #[arg(long)]
    pub dry_run: bool,
    /// Confirm the changes.
    #[arg(long)]
    pub yes: bool,
    /// With `--move`: do not leave a symlink at the old path.
    #[arg(long)]
    pub no_link: bool,
}

/// One planned or performed symlink.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinkStep {
    /// State directory kind (`home`, `agent`, `control`).
    pub kind: String,
    /// The symlink that is (or would be) created.
    pub link: String,
    /// What the symlink points at.
    pub target: String,
    /// `create`, `already-linked` or `skip`.
    pub action: String,
    /// Why, for `skip`.
    pub note: String,
}

/// Machine-readable result of `home migrate`.
#[derive(Debug, Clone, Serialize)]
pub struct HomeMigrateReport {
    /// `link` or `move`.
    pub mode: String,
    /// True when nothing was written.
    pub dry_run: bool,
    /// Link plan (link mode).
    pub links: Vec<LinkStep>,
    /// Source of a move.
    pub from: Option<String>,
    /// Destination of a move.
    pub to: Option<String>,
    /// Where the original was left after a move.
    pub backup: Option<String>,
    /// Old tmux socket name (move mode).
    pub old_tmux_socket: Option<String>,
    /// New tmux socket name (move mode).
    pub new_tmux_socket: Option<String>,
}

/// Runs `vowp home` with a built context.
pub fn run(ctx: &Ctx, io: &mut dyn IoStreams, args: &HomeArgs) -> Result<i32, CliError> {
    let HomeCommand::Migrate(args) = &args.command;
    migrate(ctx.json(), io, args, None)
}

/// Runs `home migrate` against an explicit `$HOME` (tests) or the process environment.
pub fn migrate(
    json: bool,
    io: &mut dyn IoStreams,
    args: &HomeMigrateArgs,
    home_override: Option<&Path>,
) -> Result<i32, CliError> {
    let home = match home_override {
        Some(home) => home.to_path_buf(),
        None => std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| CliError::NotFound("HOME is not set".to_owned()))?,
    };
    let mut report = HomeMigrateReport {
        mode: if args.move_ { "move" } else { "link" }.to_owned(),
        dry_run: args.dry_run,
        links: Vec::new(),
        from: None,
        to: None,
        backup: None,
        old_tmux_socket: None,
        new_tmux_socket: None,
    };
    if args.move_ {
        move_state(&home, args, &mut report)?;
    } else {
        link_state(&home, args, &mut report)?;
    }
    if json {
        print_json(io, &report)?;
    } else {
        print_human(io, &report)?;
    }
    Ok(0)
}

/// Plans and (with `--yes`) creates the compatibility symlinks.
fn link_state(home: &Path, args: &HomeMigrateArgs, report: &mut HomeMigrateReport) -> Result<(), CliError> {
    if !args.dry_run && !args.yes {
        return Err(CliError::Guard(
            "home migrate --link changes the filesystem; run with --dry-run to see the plan, or --yes to apply"
                .to_owned(),
        ));
    }
    for kind in [StateKind::Home, StateKind::Agent, StateKind::Control] {
        report.links.push(plan_link(home, kind)?);
    }
    if args.dry_run {
        return Ok(());
    }
    for step in &report.links {
        if step.action == "create" {
            symlink(&step.target, &step.link)
                .map_err(|error| CliError::Invalid(format!("cannot create {}: {error}", step.link)))?;
        }
    }
    Ok(())
}

/// Returns true when a path exists, including a dangling symlink.
fn exists_any(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// Plans the bridge for one state directory kind.
fn plan_link(home: &Path, kind: StateKind) -> Result<LinkStep, CliError> {
    let new = home.join(kind.new_dir_name());
    let old = home.join(kind.legacy_dir_name());
    let step = |link: &Path, target: &Path, action: &str, note: &str| LinkStep {
        kind: kind_name(kind).to_owned(),
        link: link.display().to_string(),
        target: target.display().to_string(),
        action: action.to_owned(),
        note: note.to_owned(),
    };
    let real = |path: &Path| path.join(kind.marker()).is_file();
    let (new_real, old_real) = (real(&new), real(&old));
    match (new_real, old_real) {
        (true, true) => {
            let same = matches!((std::fs::canonicalize(&new), std::fs::canonicalize(&old)), (Ok(a), Ok(b)) if a == b);
            if same {
                Ok(step(
                    &new,
                    &old,
                    "already-linked",
                    "both names resolve to one directory",
                ))
            } else {
                Err(CliError::Conflict(format!(
                    "{} and {} are both real {} directories; choose one with --home or remove the stray copy",
                    new.display(),
                    old.display(),
                    kind_name(kind)
                )))
            }
        }
        (false, true) => {
            if exists_any(&new) {
                Err(CliError::Conflict(format!(
                    "{} already exists but is not a {} directory; refusing to replace it",
                    new.display(),
                    kind_name(kind)
                )))
            } else {
                Ok(step(&new, Path::new(kind.legacy_dir_name()), "create", ""))
            }
        }
        (true, false) => {
            if exists_any(&old) {
                Err(CliError::Conflict(format!(
                    "{} already exists but is not a {} directory; refusing to replace it",
                    old.display(),
                    kind_name(kind)
                )))
            } else {
                Ok(step(&old, Path::new(kind.new_dir_name()), "create", ""))
            }
        }
        (false, false) => Ok(step(&new, &old, "skip", "not present on this machine")),
    }
}

/// Returns the lowercase kind name.
fn kind_name(kind: StateKind) -> &'static str {
    match kind {
        StateKind::Home => "home",
        StateKind::Agent => "agent",
        StateKind::Control => "control",
    }
}

/// Parses `--kind`.
fn parse_kind(text: &str) -> Result<StateKind, CliError> {
    match text {
        "home" => Ok(StateKind::Home),
        "agent" => Ok(StateKind::Agent),
        "control" => Ok(StateKind::Control),
        other => Err(CliError::Invalid(format!(
            "unknown --kind {other}; expected home, agent or control"
        ))),
    }
}

/// Plans and (with `--yes`) performs the offline move.
fn move_state(home: &Path, args: &HomeMigrateArgs, report: &mut HomeMigrateReport) -> Result<(), CliError> {
    if !args.dry_run && !args.yes {
        return Err(CliError::Guard(
            "home migrate --move changes the filesystem; run with --dry-run to see the plan, or --yes to apply"
                .to_owned(),
        ));
    }
    let kind = parse_kind(&args.kind)?;
    let to = args
        .to
        .clone()
        .ok_or_else(|| CliError::Invalid("--move needs --to DIR".to_owned()))?;
    let resolved = crate::ctx::resolve_state(kind, None)?;
    // An explicit environment path is used as given; a fresh machine has nothing to move.
    let source_spelling = resolved.path.clone();
    if !source_spelling.join(kind.marker()).is_file() {
        return Err(CliError::NotFound(format!(
            "{} has no {}; nothing to move",
            source_spelling.display(),
            kind.marker()
        )));
    }
    let real_source = std::fs::canonicalize(&source_spelling)?;
    if exists_any(&to) {
        return Err(CliError::Conflict(format!("{} already exists", to.display())));
    }
    let parent = to
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| CliError::Invalid(format!("{} has no parent directory", to.display())))?;
    let parent = std::fs::canonicalize(parent)
        .map_err(|error| CliError::NotFound(format!("parent of {}: {error}", to.display())))?;
    let name = to
        .file_name()
        .ok_or_else(|| CliError::Invalid(format!("{} has no file name", to.display())))?;
    let real_dest = parent.join(name);
    if real_dest.starts_with(&real_source) || real_source.starts_with(&real_dest) {
        return Err(CliError::Invalid(
            "source and destination must not contain each other".to_owned(),
        ));
    }

    refuse_if_live(kind, &real_source)?;

    report.from = Some(real_source.display().to_string());
    report.to = Some(real_dest.display().to_string());
    let backup = backup_path(&real_source);
    report.backup = Some(backup.display().to_string());
    if kind == StateKind::Home {
        report.old_tmux_socket = Some(vowp_tmux::socket_name_for_home(&real_source));
        // The destination does not exist yet; its socket name hashes the canonical path it will have.
        report.new_tmux_socket = Some(vowp_tmux::socket_name_for_home(&real_dest));
    }
    if args.dry_run {
        return Ok(());
    }

    // Every spelling under $HOME that currently resolves to the source must keep working.
    let spellings: Vec<PathBuf> = [kind.new_dir_name(), kind.legacy_dir_name()]
        .into_iter()
        .map(|name| home.join(name))
        .filter(|path| std::fs::canonicalize(path).is_ok_and(|real| real == real_source))
        .collect();

    let staging = parent.join(format!(".{}.staging-{}", name.to_string_lossy(), std::process::id()));
    if exists_any(&staging) {
        return Err(CliError::Conflict(format!("{} already exists", staging.display())));
    }
    if let Err(error) = copy_tree(&real_source, &staging).and_then(|()| verify_copy(kind, &staging)) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    // Re-check immediately before the swap: a supervisor may have started during the copy.
    if let Err(error) = refuse_if_live(kind, &real_source) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    std::fs::rename(&staging, &real_dest)?;
    if let Err(error) = std::fs::rename(&real_source, &backup) {
        // Roll back: the original is still in place, so remove only our copy.
        let _ = std::fs::remove_dir_all(&real_dest);
        return Err(CliError::Io(error));
    }
    for spelling in spellings {
        let is_link = std::fs::symlink_metadata(&spelling).is_ok_and(|meta| meta.file_type().is_symlink());
        if is_link {
            std::fs::remove_file(&spelling)?;
        } else if args.no_link {
            continue;
        }
        symlink(&real_dest, &spelling)?;
    }
    Ok(())
}

/// Returns `<dir>.premove`, with a numeric suffix when that name is taken.
fn backup_path(dir: &Path) -> PathBuf {
    let base = format!("{}.premove", dir.display());
    let mut candidate = PathBuf::from(&base);
    let mut index = 1;
    while exists_any(&candidate) {
        candidate = PathBuf::from(format!("{base}-{index}"));
        index += 1;
    }
    candidate
}

/// Refuses while anything is using the state directory.
fn refuse_if_live(kind: StateKind, root: &Path) -> Result<(), CliError> {
    match kind {
        StateKind::Home => {
            if let Some(pid) = lock_pid(&NodePaths::new(root).supervisor_lock_path()) {
                return Err(CliError::Conflict(format!(
                    "supervisor.lock names live process {pid}; run `vowp down` first"
                )));
            }
            let orchestrators = root.join("orchestrators");
            if let Ok(entries) = std::fs::read_dir(&orchestrators) {
                for entry in entries.flatten() {
                    let pid_file = entry.path().join(vowp_core::surface::orchestrator::KEEPER_PID_FILE);
                    if let Some(pid) = lock_pid(&pid_file) {
                        return Err(CliError::Conflict(format!(
                            "orchestrator host {} is running (pid {pid}); stop it first",
                            entry.file_name().to_string_lossy()
                        )));
                    }
                }
            }
        }
        StateKind::Agent => {
            let socket = root.join(vowp_agent::socket::AGENT_SOCKET_FILE);
            if socket.exists() && std::os::unix::net::UnixStream::connect(&socket).is_ok() {
                return Err(CliError::Conflict(format!(
                    "agent socket {} is live; stop `vowp agent run` first",
                    socket.display()
                )));
            }
        }
        StateKind::Control => {}
    }
    Ok(())
}

/// Reads a pid file and returns the pid when that process is alive.
fn lock_pid(path: &Path) -> Option<u32> {
    let text = std::fs::read_to_string(path).ok()?;
    let pid: u32 = text.lines().next()?.trim().parse().ok()?;
    crate::ctx::pid_alive(pid).then_some(pid)
}

/// Copies a directory tree, keeping symlinks, permissions and modification times.
fn copy_tree(from: &Path, to: &Path) -> Result<(), CliError> {
    std::fs::create_dir(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            symlink(std::fs::read_link(&source)?, &target)?;
        } else if file_type.is_dir() {
            copy_tree(&source, &target)?;
        } else if file_type.is_file() {
            std::fs::copy(&source, &target)?;
            if let Ok(modified) = std::fs::metadata(&source).and_then(|meta| meta.modified()) {
                if let Ok(file) = std::fs::File::options().write(true).open(&target) {
                    let _ = file.set_modified(modified);
                }
            }
        }
        // Sockets and fifos are runtime artefacts of a stopped node; they are not copied.
    }
    std::fs::set_permissions(to, std::fs::metadata(from)?.permissions())?;
    Ok(())
}

/// Opens the copy read-only and runs the database checks `doctor` runs.
fn verify_copy(kind: StateKind, copy: &Path) -> Result<(), CliError> {
    if !copy.join(kind.marker()).is_file() {
        return Err(CliError::Invalid(format!("copy is missing {}", kind.marker())));
    }
    match kind {
        StateKind::Home => crate::cmd::doctor::verify_home_read_only(&NodePaths::new(copy))
            .map_err(|detail| CliError::Invalid(format!("copy failed verification: {detail}"))),
        StateKind::Agent | StateKind::Control => {
            let text = std::fs::read_to_string(copy.join(kind.marker()))?;
            serde_json::from_str::<serde_json::Value>(&text)
                .map(|_| ())
                .map_err(|error| CliError::Invalid(format!("copy failed verification: {}: {error}", kind.marker())))
        }
    }
}

/// Prints the human report.
fn print_human(io: &mut dyn IoStreams, report: &HomeMigrateReport) -> Result<(), CliError> {
    let verb = if report.dry_run { "would" } else { "did" };
    if report.mode == "link" {
        for step in &report.links {
            match step.action.as_str() {
                "create" => writeln!(io.out(), "{verb} link {} -> {}", step.link, step.target)?,
                "already-linked" => writeln!(io.out(), "{} already linked to {}", step.link, step.target)?,
                _ => writeln!(io.out(), "skip {}: {}", step.kind, step.note)?,
            }
        }
        if report.dry_run {
            writeln!(io.out(), "dry run: nothing was written")?;
        }
        return Ok(());
    }
    let (Some(from), Some(to), Some(backup)) = (&report.from, &report.to, &report.backup) else {
        return Ok(());
    };
    writeln!(io.out(), "{verb} copy {from} -> {to}")?;
    writeln!(
        io.out(),
        "{verb} keep the original at {backup}; delete it when you are satisfied"
    )?;
    if let (Some(old), Some(new)) = (&report.old_tmux_socket, &report.new_tmux_socket) {
        writeln!(io.out(), "tmux socket: old {old}, new {new}")?;
        writeln!(
            io.out(),
            "to keep the old tmux view: VOWP_TMUX_SOCKET={old} STACKSPIRIT_TMUX_SOCKET={old}"
        )?;
    }
    if report.dry_run {
        writeln!(io.out(), "dry run: nothing was written")?;
    }
    Ok(())
}
