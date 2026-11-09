//! `vowp up` and `vowp down`: tmux layout, supervisor lifecycle and the stop file.
//!
//! Spec: `docs/design/cli.md` §2, `docs/design/loops.md` §1 steps 12–13 and
//! `docs/design/crates/vowp-cli.md` §1. `up --once` runs startup plus exactly one
//! supervisor tick (tests and cron); `--foreground` runs the loop in this process and
//! `--detach` (the default) re-executes the binary with `--foreground`.
//!
//! `down` writes `<home>/stop` and waits for the lock to disappear. The foreground
//! supervisor watches that file and interrupts its runners.

#[path = "../signals.rs"]
pub mod signals;

use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use vowp_core::Deps;
use vowp_core::loops::{Supervisor, TickReport};
use vowp_protocol::{Clock, Level, Log, ProjectName, Timestamp};
use vowp_store::NodePaths;
use vowp_tmux::{LayoutSpec, WindowManager, session_name_for_project};

use crate::cli::{DownArgs, UpArgs, UpCommand};
use crate::{CliError, Ctx, IoStreams};

/// How long `down` waits for a cooperative stop before reporting a conflict.
const DOWN_WAIT: Duration = Duration::from_secs(30);

/// How long `down --force` waits after SIGTERM before giving up.
const FORCE_WAIT: Duration = Duration::from_secs(5);

/// Runs `vowp up` or `vowp down`.
pub fn run(ctx: &Ctx, io: &mut dyn IoStreams, command: &UpCommand) -> Result<i32, CliError> {
    match command {
        UpCommand::Up(args) => run_up(ctx, io, args),
        UpCommand::Down(args) => run_down(ctx, io, args),
    }
}

/// Runs one supervisor tick with the supplied dependencies.
///
/// This is the seam `up --once` uses and the one the hermetic test drives with fake
/// adapters, a temporary node home and a temporary git repository.
pub fn tick_once(deps: &Deps, project: &ProjectName) -> Result<TickReport, CliError> {
    let mut supervisor = Supervisor::new(deps.clone(), project.clone())?;
    supervisor.startup()?;
    Ok(supervisor.tick()?)
}

/// Creates the fixed tmux layout for `project`.
///
/// `program` is the binary each window runs (`vowp`, or an absolute path in the detached
/// supervisor). Tests pass a harmless placeholder command on a private tmux socket.
pub fn ensure_project_windows(
    windows: &dyn WindowManager,
    project: &str,
    cwd: Option<&Path>,
    program: &[String],
) -> Result<(), CliError> {
    let window = |name: &str| {
        let mut argv = program.to_vec();
        argv.push("__window".to_owned());
        argv.push(name.to_owned());
        argv
    };
    let spec = LayoutSpec {
        board: window("board"),
        supervisor: window("supervisor"),
        planner: window("planner"),
        keeper: window("keeper"),
        inbox: window("inbox"),
        cwd: cwd.map(Path::to_path_buf),
    };
    windows
        .ensure_layout(project, &spec)
        .map_err(|error| CliError::Core(vowp_core::CoreError::Tmux(error)))
}

/// What `up --json` prints.
#[derive(Debug, Serialize)]
struct UpOutput {
    running: bool,
    pid: Option<u32>,
    mode: &'static str,
}

/// What `up --once --json` prints after the tick.
#[derive(Debug, Serialize)]
struct TickOutput {
    at: String,
    ok: bool,
    steps: Vec<StepOutput>,
    dispatched: Vec<String>,
    transitions: Vec<TransitionOutput>,
    merged: Vec<String>,
}

/// One tick step in JSON output.
#[derive(Debug, Serialize)]
struct StepOutput {
    name: String,
    ok: bool,
    detail: String,
}

/// One state transition in JSON output.
#[derive(Debug, Serialize)]
struct TransitionOutput {
    job: String,
    from: String,
    to: String,
}

/// Runs `vowp up`.
fn run_up(ctx: &Ctx, io: &mut dyn IoStreams, args: &UpArgs) -> Result<i32, CliError> {
    let paths = ctx.paths();
    if let Some(pid) = crate::ctx::supervisor_pid(paths) {
        return Err(CliError::Conflict(format!(
            "supervisor already running (pid {pid}); `vowp status` shows it"
        )));
    }
    signals::clear_stop(paths)?;

    if !args.no_tmux {
        let exe = std::env::current_exe()?;
        let program = vec![exe.to_string_lossy().into_owned()];
        match ensure_project_windows(
            ctx.deps().windows.as_ref(),
            ctx.project().as_str(),
            Some(ctx.project_root()),
            &program,
        ) {
            Ok(()) => {}
            Err(CliError::Core(vowp_core::CoreError::Tmux(vowp_tmux::TmuxError::NotInstalled))) => {
                writeln!(io.err(), "warning: tmux is not installed; continuing without windows")?;
            }
            Err(error) => return Err(error),
        }
    }

    if args.once {
        return run_once(ctx, io);
    }
    if args.foreground {
        return run_foreground(ctx, io);
    }
    run_detached(ctx, io)
}

/// Runs `startup()` plus one tick, prints the report and releases the lock.
fn run_once(ctx: &Ctx, io: &mut dyn IoStreams) -> Result<i32, CliError> {
    let paths = ctx.paths();
    let pid = std::process::id();
    write_lock(paths, pid, ctx.clock().now())?;
    let result = tick_once(&supervisor_deps(ctx), ctx.project());
    remove_lock_if_own(paths, pid);
    let report = result?;

    let ok = report.steps.iter().all(|step| step.ok);
    if ctx.json() {
        let output = TickOutput {
            at: report.at.as_rfc3339(),
            ok,
            steps: report
                .steps
                .iter()
                .map(|step| StepOutput {
                    name: step.name.to_owned(),
                    ok: step.ok,
                    detail: step.detail.clone(),
                })
                .collect(),
            dispatched: report.dispatched.iter().map(ToString::to_string).collect(),
            transitions: report
                .transitions
                .iter()
                .map(|(job, from, to)| TransitionOutput {
                    job: job.to_string(),
                    from: from.as_str().to_owned(),
                    to: to.as_str().to_owned(),
                })
                .collect(),
            merged: report.merged.iter().map(ToString::to_string).collect(),
        };
        crate::output::print_json(io, &output)?;
    } else {
        let completed = report.steps.iter().filter(|step| step.ok).count();
        writeln!(
            io.out(),
            "tick {}: {}/{} steps ok, {} dispatched",
            report.at.as_rfc3339(),
            completed,
            report.steps.len(),
            report.dispatched.len()
        )?;
        for step in report.steps.iter().filter(|step| !step.ok) {
            writeln!(io.out(), "  degraded {}: {}", step.name, step.detail)?;
        }
    }
    Ok(0)
}

/// Runs the supervisor loop in this process until the stop file appears.
fn run_foreground(ctx: &Ctx, io: &mut dyn IoStreams) -> Result<i32, CliError> {
    let paths = ctx.paths();
    let pid = std::process::id();
    write_lock(paths, pid, ctx.clock().now())?;

    let watcher = signals::SignalWatcher::install(paths);
    let mut supervisor = match Supervisor::new(supervisor_deps(ctx), ctx.project().clone()) {
        Ok(supervisor) => supervisor,
        Err(error) => {
            remove_lock_if_own(paths, pid);
            return Err(error.into());
        }
    };
    if !ctx.json() {
        writeln!(io.out(), "supervisor running (pid {pid}); stop it with `vowp down`")?;
    }
    // `Supervisor::run` waits up to `tick_secs` between ticks; a cancellation alone
    // would keep `vowp down` waiting for that whole interval. Wake it as soon as the
    // stop watcher cancels so shutdown is prompt.
    let cancel = watcher.cancel_token();
    let wake = supervisor.wake_handle();
    let wake_cancel = cancel.clone();
    let wake_thread = std::thread::spawn(move || {
        while !wake_cancel.is_cancelled() {
            std::thread::sleep(Duration::from_millis(200));
        }
        wake.wake();
    });
    let result = supervisor.run(&cancel);
    // A failed startup returns before the stop watcher cancels; release the wake thread.
    watcher.cancel();
    let _ = wake_thread.join();
    remove_lock_if_own(paths, pid);
    result?;
    if !ctx.json() {
        writeln!(io.out(), "supervisor stopped (pid {pid})")?;
    }
    Ok(0)
}

/// Re-executes this binary with `--foreground` and returns as soon as it owns the lock.
fn run_detached(ctx: &Ctx, io: &mut dyn IoStreams) -> Result<i32, CliError> {
    let paths = ctx.paths();
    std::fs::create_dir_all(paths.logs_dir())?;
    let log_path = paths.logs_dir().join("supervisor.log");
    let log = std::fs::OpenOptions::new().create(true).append(true).open(&log_path)?;

    let exe = std::env::current_exe()?;
    let configure = |command: &mut Command| {
        command
            .arg("--home")
            .arg(paths.root())
            .arg("--project")
            .arg(ctx.project_root())
            .arg("up")
            .arg("--foreground")
            .arg("--no-tmux");
    };

    // `setsid` detaches the supervisor from the caller's terminal; when it is not
    // installed the child still gets its own process group and inherited stdio is gone.
    let mut detached = Command::new("setsid");
    detached.arg(&exe);
    configure(&mut detached);
    detached
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log.try_clone()?);
    let child = match detached.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut direct = Command::new(&exe);
            configure(&mut direct);
            direct
                .stdin(Stdio::null())
                .stdout(log.try_clone()?)
                .stderr(log)
                .process_group(0)
                .spawn()?
        }
        Err(error) => return Err(CliError::Io(error)),
    };

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut started_pid = None;
    while Instant::now() < deadline {
        if let Some(pid) = crate::ctx::supervisor_pid(paths) {
            started_pid = Some(pid);
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let pid = started_pid.unwrap_or(child.id());

    if ctx.json() {
        crate::output::print_json(
            io,
            &UpOutput {
                running: true,
                pid: Some(pid),
                mode: "detached",
            },
        )?;
    } else {
        writeln!(io.out(), "supervisor started (pid {pid})")?;
    }
    Ok(0)
}

/// Runs `vowp down`.
fn run_down(ctx: &Ctx, io: &mut dyn IoStreams, args: &DownArgs) -> Result<i32, CliError> {
    let paths = ctx.paths();
    let stop = signals::stop_file(paths);
    std::fs::write(&stop, ctx.clock().now().unix_secs().to_string())?;

    let Some(pid) = crate::ctx::supervisor_pid(paths) else {
        remove_lock(paths);
        if args.kill_windows {
            kill_windows(ctx)?;
        }
        if ctx.json() {
            crate::output::print_json(
                io,
                &UpOutput {
                    running: false,
                    pid: None,
                    mode: "down",
                },
            )?;
        } else {
            writeln!(io.out(), "supervisor is not running")?;
        }
        return Ok(0);
    };

    let deadline = Instant::now() + DOWN_WAIT;
    while Instant::now() < deadline && crate::ctx::supervisor_pid(paths) == Some(pid) {
        std::thread::sleep(Duration::from_millis(100));
    }

    if crate::ctx::supervisor_pid(paths) == Some(pid) {
        if args.force {
            send_term(pid)?;
            let force_deadline = Instant::now() + FORCE_WAIT;
            while Instant::now() < force_deadline && crate::ctx::supervisor_pid(paths) == Some(pid) {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        if crate::ctx::supervisor_pid(paths) == Some(pid) {
            return Err(CliError::Conflict(format!(
                "supervisor (pid {pid}) did not stop; use `vowp down --force`"
            )));
        }
    }

    remove_lock(paths);
    if args.kill_windows {
        kill_windows(ctx)?;
    }
    if ctx.json() {
        crate::output::print_json(
            io,
            &UpOutput {
                running: false,
                pid: Some(pid),
                mode: "down",
            },
        )?;
    } else {
        writeln!(io.out(), "supervisor stopped (pid {pid})")?;
    }
    Ok(0)
}

/// Sends SIGTERM through `/bin/kill` (no `unsafe`, no libc dependency).
fn send_term(pid: u32) -> Result<(), CliError> {
    let status = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status()
        .map_err(CliError::Io)?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::Conflict(format!("cannot signal supervisor pid {pid}")))
    }
}

/// Kills every tagged window in the project's session.
fn kill_windows(ctx: &Ctx) -> Result<(), CliError> {
    let session = session_name_for_project(ctx.project().as_str());
    let windows = match ctx.deps().windows.list_windows(&session) {
        Ok(windows) => windows,
        Err(vowp_tmux::TmuxError::NotInstalled) => return Ok(()),
        Err(error) => return Err(CliError::Core(vowp_core::CoreError::Tmux(error))),
    };
    for window in windows {
        let _ = ctx.deps().windows.kill_window(&session, &window.name);
    }
    Ok(())
}

/// Writes the supervisor lock: pid on line 1, RFC 3339 start time on line 2.
fn write_lock(paths: &NodePaths, pid: u32, now: Timestamp) -> Result<(), CliError> {
    let text = format!("{pid}\n{}\n", now.as_rfc3339());
    let path = paths.root().join("supervisor.lock");
    let temp = paths.tmp_dir().join("supervisor.lock.tmp");
    std::fs::create_dir_all(paths.tmp_dir())?;
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

/// Removes the lock when it still names `pid`.
fn remove_lock_if_own(paths: &NodePaths, pid: u32) {
    let path = paths.root().join("supervisor.lock");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    if text.lines().next().and_then(|line| line.trim().parse::<u32>().ok()) == Some(pid) {
        let _ = std::fs::remove_file(path);
    }
}

/// Removes the lock file unconditionally (stale lock after a stopped supervisor).
fn remove_lock(paths: &NodePaths) {
    let _ = std::fs::remove_file(paths.root().join("supervisor.lock"));
}

/// Clones the context dependencies and replaces the log with the node's supervisor log.
fn supervisor_deps(ctx: &Ctx) -> Deps {
    let mut deps = ctx.deps().clone();
    deps.log = Arc::new(FileLog::new(
        ctx.paths().logs_dir().join("supervisor.log"),
        Arc::clone(&ctx.deps().clock),
    ));
    deps
}

/// Appends supervisor lines to `<home>/logs/supervisor.log`.
///
/// Lines are `<rfc3339> <LEVEL> <target> <message>`, matching `conventions.md` §4.
struct FileLog {
    path: std::path::PathBuf,
    clock: Arc<dyn Clock>,
}

impl FileLog {
    /// Creates a sink for `path`; the directory is created by [`vowp_store::NodePaths`].
    fn new(path: std::path::PathBuf, clock: Arc<dyn Clock>) -> Self {
        Self { path, clock }
    }
}

impl Log for FileLog {
    fn log(&self, level: Level, target: &str, msg: &str) {
        let level = match level {
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        };
        let line = format!(
            "{} {level} {target} {}\n",
            self.clock.now().as_rfc3339(),
            msg.replace('\n', " ")
        );
        let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&self.path) else {
            return;
        };
        let _ = file.write_all(line.as_bytes());
    }
}
