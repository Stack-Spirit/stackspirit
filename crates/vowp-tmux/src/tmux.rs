//! The real tmux implementation of [`WindowManager`].
//!
//! Every command runs without a shell, with an optional `-L <socket>` first,
//! and under a polling watchdog. The crate deliberately exposes no respawn or
//! keystroke capability; recycling means kill then create.
//!
//! Spec: `docs/design/crates/vowp-tmux.md` §3.

use std::io::Read;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use crate::error::TmuxError;
use crate::manager::{
    LayoutSpec, WindowInfo, WindowManager, WindowSpec, require_vowp_session, require_window_name,
    session_name_for_project,
};
use crate::quote::{validate_name, window_command};

const TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(20);
const WINDOW_FORMAT: &str = "#{window_id}|#{window_index}|#{window_name}|#{@vowp}|#{pane_pid}|#{window_active}";

/// Real tmux.
///
/// `socket = Some("name")` runs every command as `tmux -L name ...` (tests and
/// private servers); `None` uses the default server (CLI only).
#[derive(Debug, Clone)]
pub struct Tmux {
    socket: Option<String>,
    binary: String,
}

struct Completed {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

struct RawWindow {
    info: WindowInfo,
    id: String,
}

impl Tmux {
    /// Create a manager for the default tmux server, or for the named private
    /// socket when `socket` is `Some`.
    pub fn new(socket: Option<String>) -> Tmux {
        Tmux {
            socket,
            binary: "tmux".to_owned(),
        }
    }

    /// Create a manager with an overridden tmux binary (tests, CI).
    pub fn with_binary(socket: Option<String>, binary: impl Into<String>) -> Tmux {
        Tmux {
            socket,
            binary: binary.into(),
        }
    }

    /// Whether `tmux -V` runs successfully.
    pub fn has_tmux(&self) -> bool {
        let args = vec!["-V".to_owned()];
        match self.execute(&args) {
            Ok(output) => output.status.success(),
            Err(_) => false,
        }
    }

    /// Whether the session exists. Foreign sessions are refused.
    pub fn session_exists(&self, session: &str) -> Result<bool, TmuxError> {
        require_vowp_session(session)?;
        self.session_exists_raw(session)
    }

    /// Set the `@vowp_title` window option (never rename the window).
    pub fn set_window_title(&self, session: &str, name: &str, title: &str) -> Result<(), TmuxError> {
        require_vowp_session(session)?;
        require_window_name(name)?;
        validate_name("title", title)?;
        let target = self.window_target(session, name)?;
        self.set_option(&target, "@vowp_title", title)
    }

    /// Display only: capture the last `lines` lines of a pane.
    ///
    /// Decisions must never depend on this text; logs drive decisions.
    pub fn capture_pane(&self, session: &str, name: &str, lines: u32) -> Result<String, TmuxError> {
        require_vowp_session(session)?;
        require_window_name(name)?;
        let target = self.window_target(session, name)?;
        let lines = lines.clamp(1, 2000);
        let args = vec![
            "capture-pane".to_owned(),
            "-p".to_owned(),
            "-t".to_owned(),
            target,
            "-S".to_owned(),
            format!("-{lines}"),
        ];
        self.run_checked(&args)
    }

    /// Kill a session, but only when every window in it is tagged and the
    /// session is in the `vowp-` namespace.
    pub fn kill_session(&self, session: &str) -> Result<(), TmuxError> {
        require_vowp_session(session)?;
        if !self.session_exists_raw(session)? {
            return Ok(());
        }
        let windows = self.list_windows(session)?;
        if let Some(window) = windows.iter().find(|w| !w.tagged) {
            return Err(TmuxError::NotTagged {
                session: session.to_owned(),
                name: window.name.clone(),
            });
        }
        let args = vec!["kill-session".to_owned(), "-t".to_owned(), format!("={session}")];
        self.run_checked(&args).map(|_| ())
    }

    /// The argv a caller execs or spawns to attach to a session.
    ///
    /// `["tmux", ("-L", sock)?, "attach-session", "-t", session]`.
    pub fn attach_argv(&self, session: &str) -> Vec<String> {
        let mut argv = vec![self.binary.clone()];
        if let Some(socket) = &self.socket {
            argv.push("-L".to_owned());
            argv.push(socket.clone());
        }
        argv.push("attach-session".to_owned());
        argv.push("-t".to_owned());
        argv.push(session.to_owned());
        argv
    }

    /// Kill the private server owned by a test guard.
    ///
    /// Returns [`TmuxError::Refused`] when no socket is set, before spawning
    /// anything, so it can never touch the user's default server.
    pub fn kill_server_for_tests(&self) -> Result<(), TmuxError> {
        if self.socket.is_none() {
            return Err(TmuxError::Refused("refusing to kill the default tmux server"));
        }
        let args = vec!["kill-server".to_owned()];
        match self.execute(&args) {
            Ok(output) if output.status.success() => Ok(()),
            Ok(output) if output.stderr.contains("no server running") => Ok(()),
            Ok(output) => Err(Self::command_error(&args, &output)),
            Err(error) => Err(error),
        }
    }

    fn session_exists_raw(&self, session: &str) -> Result<bool, TmuxError> {
        let args = vec!["has-session".to_owned(), "-t".to_owned(), format!("={session}")];
        let output = self.execute(&args)?;
        Ok(output.status.success())
    }

    fn command(&self, args: &[String]) -> Command {
        let mut command = Command::new(&self.binary);
        if let Some(socket) = &self.socket {
            command.arg("-L").arg(socket);
        }
        command.args(args);
        command
            .env_remove("TMUX")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn execute(&self, args: &[String]) -> Result<Completed, TmuxError> {
        let mut child = self.command(args).spawn().map_err(spawn_error)?;
        let stdout_reader = child.stdout.take().map(spawn_reader);
        let stderr_reader = child.stderr.take().map(spawn_reader);
        let deadline = Instant::now() + TIMEOUT;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let stdout = join_reader(stdout_reader)?;
                    let stderr = join_reader(stderr_reader)?;
                    return Ok(Completed { status, stdout, stderr });
                }
                Ok(None) => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        discard_reader(stdout_reader);
                        discard_reader(stderr_reader);
                        return Err(TmuxError::Timeout(TIMEOUT));
                    }
                    std::thread::sleep(POLL_INTERVAL);
                }
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    discard_reader(stdout_reader);
                    discard_reader(stderr_reader);
                    return Err(TmuxError::Io(error));
                }
            }
        }
    }

    fn run_checked(&self, args: &[String]) -> Result<String, TmuxError> {
        self.run_checked_display(args, args)
    }

    fn run_checked_display(&self, display_args: &[String], args: &[String]) -> Result<String, TmuxError> {
        let output = self.execute(args)?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(Self::command_error(display_args, &output))
        }
    }

    fn command_error(display_args: &[String], output: &Completed) -> TmuxError {
        TmuxError::Command {
            args: display_args.to_vec(),
            status: output.status.code(),
            stderr: output.stderr.trim_end().to_owned(),
        }
    }

    fn list_windows_raw(&self, session: &str) -> Result<Vec<RawWindow>, TmuxError> {
        let args = vec![
            "list-windows".to_owned(),
            "-t".to_owned(),
            format!("={session}"),
            "-F".to_owned(),
            WINDOW_FORMAT.to_owned(),
        ];
        let output = self.execute(&args)?;
        if !output.status.success() {
            if self.session_exists_raw(session)? {
                return Err(Self::command_error(&args, &output));
            }
            return Ok(Vec::new());
        }
        let mut windows = Vec::new();
        for line in output.stdout.lines() {
            if line.is_empty() {
                continue;
            }
            let fields: Vec<&str> = line.split('|').collect();
            if fields.len() != 6 {
                return Err(TmuxError::Command {
                    args: args.clone(),
                    status: Some(0),
                    stderr: format!("unexpected list-windows output: {line:?}"),
                });
            }
            let index = fields[1].parse::<u32>().map_err(|_| TmuxError::Command {
                args: args.clone(),
                status: Some(0),
                stderr: format!("unexpected window index in {line:?}"),
            })?;
            let pane_pid = if fields[4].is_empty() {
                None
            } else {
                Some(fields[4].parse::<u32>().map_err(|_| TmuxError::Command {
                    args: args.clone(),
                    status: Some(0),
                    stderr: format!("unexpected pane pid in {line:?}"),
                })?)
            };
            windows.push(RawWindow {
                info: WindowInfo {
                    index,
                    name: fields[2].to_owned(),
                    tagged: fields[3] == "1",
                    pane_pid,
                    active: fields[5] == "1",
                },
                id: fields[0].to_owned(),
            });
        }
        Ok(windows)
    }

    fn window_target(&self, session: &str, name: &str) -> Result<String, TmuxError> {
        let windows = self.list_windows_raw(session)?;
        windows
            .into_iter()
            .find(|window| window.info.name == name)
            .map(|window| window.id)
            .ok_or_else(|| TmuxError::WindowNotFound {
                session: session.to_owned(),
                name: name.to_owned(),
            })
    }

    fn set_option(&self, target: &str, option: &str, value: &str) -> Result<(), TmuxError> {
        let args = vec![
            "set-option".to_owned(),
            "-w".to_owned(),
            "-t".to_owned(),
            target.to_owned(),
            option.to_owned(),
            value.to_owned(),
        ];
        self.run_checked(&args).map(|_| ())
    }

    fn create_session(
        &self,
        session: &str,
        window: &str,
        argv: &[String],
        cwd: Option<&Path>,
    ) -> Result<(), TmuxError> {
        if argv.is_empty() {
            return Err(TmuxError::EmptyCommand);
        }
        if let Some(cwd) = cwd {
            if !cwd.is_dir() {
                return Err(TmuxError::InvalidCwd(cwd.display().to_string()));
            }
        }
        let command = window_command(argv, &[])?;
        let args = session_args(session, window, cwd, command);
        let id = self.run_checked(&args)?.trim().to_owned();
        if id.is_empty() {
            return Err(TmuxError::Command {
                args,
                status: Some(0),
                stderr: "tmux new-session returned no window id".to_owned(),
            });
        }
        if let Err(error) = self.set_option(&id, "@vowp", "1") {
            let _ = self.execute(&["kill-session".to_owned(), "-t".to_owned(), format!("={session}")]);
            return Err(error);
        }
        Ok(())
    }

    fn check_cwd(cwd: Option<&Path>) -> Result<(), TmuxError> {
        if let Some(cwd) = cwd {
            if !cwd.is_dir() {
                return Err(TmuxError::InvalidCwd(cwd.display().to_string()));
            }
        }
        Ok(())
    }
}

fn spawn_error(error: std::io::Error) -> TmuxError {
    match error.kind() {
        std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied => TmuxError::NotInstalled,
        _ => TmuxError::Io(error),
    }
}

fn spawn_reader(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<std::io::Result<String>> {
    std::thread::spawn(move || {
        let mut buffer = Vec::new();
        pipe.read_to_end(&mut buffer)?;
        Ok(String::from_utf8_lossy(&buffer).into_owned())
    })
}

fn join_reader(handle: Option<std::thread::JoinHandle<std::io::Result<String>>>) -> Result<String, TmuxError> {
    match handle {
        Some(handle) => match handle.join() {
            Ok(Ok(text)) => Ok(text),
            Ok(Err(error)) => Err(TmuxError::Io(error)),
            Err(_) => Err(TmuxError::Io(std::io::Error::other("tmux output reader panicked"))),
        },
        None => Ok(String::new()),
    }
}

fn discard_reader(handle: Option<std::thread::JoinHandle<std::io::Result<String>>>) {
    if let Some(handle) = handle {
        let _ = handle.join();
    }
}

fn session_args(session: &str, window: &str, cwd: Option<&Path>, command: String) -> Vec<String> {
    let mut args = vec![
        "new-session".to_owned(),
        "-d".to_owned(),
        "-P".to_owned(),
        "-F".to_owned(),
        "#{window_id}".to_owned(),
        "-s".to_owned(),
        session.to_owned(),
        "-n".to_owned(),
        window.to_owned(),
    ];
    if let Some(cwd) = cwd {
        args.push("-c".to_owned());
        args.push(cwd.display().to_string());
    }
    args.push("--".to_owned());
    args.push(command);
    args
}

fn window_args(session: &str, name: &str, cwd: Option<&Path>, command: String) -> Vec<String> {
    let mut args = vec![
        "new-window".to_owned(),
        "-d".to_owned(),
        "-P".to_owned(),
        "-F".to_owned(),
        "#{window_id}".to_owned(),
        "-t".to_owned(),
        format!("{session}:"),
        "-n".to_owned(),
        name.to_owned(),
    ];
    if let Some(cwd) = cwd {
        args.push("-c".to_owned());
        args.push(cwd.display().to_string());
    }
    args.push("--".to_owned());
    args.push(command);
    args
}

impl WindowManager for Tmux {
    fn ensure_session(&self, session: &str) -> Result<(), TmuxError> {
        require_vowp_session(session)?;
        if self.session_exists_raw(session)? {
            return Ok(());
        }
        let argv = vec!["sh".to_owned(), "-c".to_owned(), "exec cat".to_owned()];
        self.create_session(session, "scratch", &argv, None)
    }

    fn new_window(&self, session: &str, name: &str, spec: &WindowSpec) -> Result<(), TmuxError> {
        require_vowp_session(session)?;
        require_window_name(name)?;
        Self::check_cwd(spec.cwd.as_deref())?;
        if self.window_exists(session, name)? {
            return Err(TmuxError::WindowExists {
                session: session.to_owned(),
                name: name.to_owned(),
            });
        }
        let command = window_command(&spec.argv, &spec.env)?;
        let display_command = window_command(&spec.argv, &[])?;
        let args = window_args(session, name, spec.cwd.as_deref(), command);
        let display_args = window_args(session, name, spec.cwd.as_deref(), display_command);
        let id = self.run_checked_display(&display_args, &args)?.trim().to_owned();
        if id.is_empty() {
            return Err(TmuxError::Command {
                args: display_args,
                status: Some(0),
                stderr: "tmux new-window returned no window id".to_owned(),
            });
        }
        if let Err(error) = self.set_option(&id, "@vowp", "1") {
            // A command that exits immediately can take its window with it
            // before the tag command runs. The window is already gone, so
            // there is nothing unsafe left behind; the command itself ran.
            if matches!(
                &error,
                TmuxError::Command { stderr, .. } if stderr.contains("no such window")
            ) {
                return Ok(());
            }
            let _ = self.execute(&["kill-window".to_owned(), "-t".to_owned(), id]);
            return Err(error);
        }
        Ok(())
    }

    fn kill_window(&self, session: &str, name: &str) -> Result<(), TmuxError> {
        require_vowp_session(session)?;
        require_window_name(name)?;
        let target = self.window_target(session, name)?;
        let args = vec![
            "show-options".to_owned(),
            "-w".to_owned(),
            "-v".to_owned(),
            "-t".to_owned(),
            target.clone(),
            "@vowp".to_owned(),
        ];
        let output = self.execute(&args)?;
        if !output.status.success() || output.stdout.trim() != "1" {
            return Err(TmuxError::NotTagged {
                session: session.to_owned(),
                name: name.to_owned(),
            });
        }
        let args = vec!["kill-window".to_owned(), "-t".to_owned(), target];
        self.run_checked(&args).map(|_| ())
    }

    fn list_windows(&self, session: &str) -> Result<Vec<WindowInfo>, TmuxError> {
        require_vowp_session(session)?;
        Ok(self
            .list_windows_raw(session)?
            .into_iter()
            .map(|window| window.info)
            .collect())
    }

    fn window_exists(&self, session: &str, name: &str) -> Result<bool, TmuxError> {
        require_vowp_session(session)?;
        require_window_name(name)?;
        Ok(self
            .list_windows_raw(session)?
            .iter()
            .any(|window| window.info.name == name))
    }

    fn ensure_layout(&self, project: &str, spec: &LayoutSpec) -> Result<(), TmuxError> {
        let session = session_name_for_project(project);
        require_vowp_session(&session)?;
        if !self.session_exists_raw(&session)? {
            self.create_session(&session, "board", &spec.board, spec.cwd.as_deref())?;
        } else {
            if !self.window_exists(&session, "board")? {
                self.new_window(
                    &session,
                    "board",
                    &WindowSpec {
                        argv: spec.board.clone(),
                        cwd: spec.cwd.clone(),
                        env: Vec::new(),
                    },
                )?;
            }
            if self.window_exists(&session, "scratch")? {
                self.kill_window(&session, "scratch")?;
            }
        }
        let windows = [
            ("supervisor", &spec.supervisor),
            ("planner", &spec.planner),
            ("keeper", &spec.keeper),
            ("inbox", &spec.inbox),
        ];
        for (name, argv) in windows {
            if !self.window_exists(&session, name)? {
                self.new_window(
                    &session,
                    name,
                    &WindowSpec {
                        argv: argv.clone(),
                        cwd: spec.cwd.clone(),
                        env: Vec::new(),
                    },
                )?;
            }
        }
        Ok(())
    }
}
