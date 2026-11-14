//! The `vowp` command-line library: parsing, context, rendering, errors and command dispatch.
//!
//! The binary is a thin wrapper around [`run`], which tests call in-process with a buffer
//! [`IoStreams`]. Spec: `docs/design/crates/vowp-cli.md` §1–§2.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::io::{BufRead, BufReader, Cursor, IsTerminal, Write};

use clap::Parser;

pub mod chat;
pub mod cli;
pub mod cmd;
pub mod ctx;
pub mod errors;
pub mod framework;
pub mod mcp;
pub mod node_ctx;
pub mod output;
pub mod remote;
pub mod setup;
pub mod signals;
pub mod ssh_stream;

pub use cli::{Cli, Command, Global};
pub use ctx::Ctx;
pub use errors::CliError;

/// The input and output streams a command uses.
///
/// Commands never print directly to `std::io`; tests pass a [`BufferIo`] and the binary
/// passes [`RealIo`].
pub trait IoStreams {
    /// Returns the stdout writer.
    fn out(&mut self) -> &mut dyn Write;
    /// Returns the stderr writer.
    fn err(&mut self) -> &mut dyn Write;
    /// Returns the stdin reader.
    fn input(&mut self) -> &mut dyn BufRead;
    /// Returns whether stdout is a terminal.
    fn is_tty(&self) -> bool;
}

/// The real process streams.
pub struct RealIo {
    out: std::io::Stdout,
    err: std::io::Stderr,
    input: BufReader<std::io::Stdin>,
    tty: bool,
}

impl RealIo {
    /// Creates the real stream set.
    pub fn new() -> Self {
        Self {
            out: std::io::stdout(),
            err: std::io::stderr(),
            input: BufReader::new(std::io::stdin()),
            tty: std::io::stdout().is_terminal(),
        }
    }
}

impl Default for RealIo {
    fn default() -> Self {
        Self::new()
    }
}

impl IoStreams for RealIo {
    fn out(&mut self) -> &mut dyn Write {
        &mut self.out
    }

    fn err(&mut self) -> &mut dyn Write {
        &mut self.err
    }

    fn input(&mut self) -> &mut dyn BufRead {
        &mut self.input
    }

    fn is_tty(&self) -> bool {
        self.tty
    }
}

/// An in-memory [`IoStreams`] implementation for tests.
#[derive(Debug)]
pub struct BufferIo {
    out: Vec<u8>,
    err: Vec<u8>,
    input: Cursor<Vec<u8>>,
    tty: bool,
}

impl BufferIo {
    /// Creates a buffer stream with `input` as stdin and no TTY.
    pub fn new(input: impl Into<Vec<u8>>) -> Self {
        Self {
            out: Vec::new(),
            err: Vec::new(),
            input: Cursor::new(input.into()),
            tty: false,
        }
    }

    /// Creates a buffer stream with an explicit TTY flag.
    pub fn with_tty(input: impl Into<Vec<u8>>, tty: bool) -> Self {
        Self {
            tty,
            ..Self::new(input)
        }
    }

    /// Returns everything written to stdout as UTF-8.
    pub fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.out).into_owned()
    }

    /// Returns everything written to stderr as UTF-8.
    pub fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.err).into_owned()
    }
}

impl IoStreams for BufferIo {
    fn out(&mut self) -> &mut dyn Write {
        &mut self.out
    }

    fn err(&mut self) -> &mut dyn Write {
        &mut self.err
    }

    fn input(&mut self) -> &mut dyn BufRead {
        &mut self.input
    }

    fn is_tty(&self) -> bool {
        self.tty
    }
}

/// Runs one CLI invocation and returns the process exit code.
///
/// Parse errors keep clap's exit code (2 for usage, 0 for `--help`/`--version`); command
/// errors are printed by [`output::print_error`] and mapped through [`CliError::exit_code`].
pub fn run(args: Vec<String>, io: &mut dyn IoStreams) -> i32 {
    let mut args = if args.is_empty() { vec!["vowp".to_owned()] } else { args };
    inject_agent_chat_id(&mut args);
    let cli = match Cli::try_parse_from(args.iter().cloned()) {
        Ok(cli) => cli,
        Err(error) => {
            let code = error.exit_code();
            let rendered = error.render().to_string();
            if error.use_stderr() {
                let _ = io.err().write_all(rendered.as_bytes());
            } else {
                let _ = io.out().write_all(rendered.as_bytes());
            }
            return code;
        }
    };
    let json = cli.global.json;
    let command_args = args.get(1..).unwrap_or(&[]);
    match execute(cli, command_args, io) {
        Ok(code) => code,
        Err(error) => {
            output::print_error(io, &error, json);
            error.exit_code()
        }
    }
}

/// Inserts an agent chat's `--session` value as the required positional id.
///
/// The home spelling keeps `<ID>` as a required clap argument, so `vowp chat` without an
/// id still fails with clap's missing-argument error and the help tree is unchanged.
/// Agent mode accepts `--session <id>` instead; inserting the same id before parsing lets
/// both spellings share one argument definition without a hidden default.
fn inject_agent_chat_id(args: &mut Vec<String>) {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return;
    }
    let Some(chat) = chat_position(args) else {
        return;
    };
    let mut agent = false;
    let mut session = None;
    let mut has_id = false;
    let rest = args[chat + 1..].to_vec();
    let mut index = 0;
    while index < rest.len() {
        let arg = &rest[index];
        match arg.as_str() {
            "--agent" => agent = true,
            "--session" => {
                if let Some(value) = rest.get(index + 1) {
                    session = Some(value.clone());
                    index += 1;
                }
            }
            "--executor" | "--node" => index += 1,
            "--" => {
                has_id = index + 1 < rest.len();
                break;
            }
            value if value.starts_with("--session=") => {
                session = Some(value["--session=".len()..].to_owned());
            }
            value if value.starts_with('-') => {}
            _ => has_id = true,
        }
        index += 1;
    }
    if agent
        && !has_id
        && let Some(session) = session
    {
        args.push(session);
    }
}

/// Returns the index of the `chat` command token, including `session chat`.
fn chat_position(args: &[String]) -> Option<usize> {
    let mut index = 1;
    while index < args.len() {
        let arg = &args[index];
        if matches!(arg.as_str(), "--home" | "--project" | "--node") {
            index += 2;
            continue;
        }
        if arg.starts_with('-') {
            index += 1;
            continue;
        }
        if arg == "chat" {
            return Some(index);
        }
        if arg == "session" {
            let mut next = index + 1;
            while next < args.len() {
                if args[next].starts_with('-') {
                    next += 1;
                    continue;
                }
                return (args[next] == "chat").then_some(next);
            }
            return None;
        }
        return None;
    }
    None
}

/// Executes a parsed command, handling local-only commands and the `--node` redirect first.
///
/// `original_args` excludes argv[0]: the remote command must be `vowp <args>`, never
/// `vowp <local-binary-path> <args>`.
fn execute(cli: Cli, original_args: &[String], io: &mut dyn IoStreams) -> Result<i32, CliError> {
    let Cli { global, command } = cli;
    // `init` creates local files and node state; a remote cwd is not defined, so it stays
    // local even with `--node`. Every other command follows the global redirect.
    if let Command::Init(args) = &command {
        return cmd::init::run(&global, io, args);
    }
    // `reindex` is the documented recovery path when the index database is gone: open (or
    // create) it before `Ctx::build` applies its "run vowp init" guard to every other command.
    if matches!(command, Command::Reindex(_)) {
        let paths = ctx::node_paths(&global)?;
        if !paths.db_path().is_file() {
            vowp_store::Store::open(&paths)?;
        }
    }
    // The Mac agent has no store, project or `--node` meaning: it always runs locally against
    // `~/.vowp-agent`. Handle it before the generic remote redirect.
    if let Command::Agent { command } = &command {
        return cmd::agent::run(&global, io, command);
    }
    // `chat --agent` runs on the Mac against `~/.vowp-agent`; it must be handled before the
    // control-machine redirect and `Ctx::build`, neither of which may require a home database.
    if let Some(args) = agent_chat_args(&command) {
        return chat::agent::run_command(&global, io, args);
    }
    // `surface` is home-only and its `--node` flag names the node the surface runs on, not an
    // SSH redirect. Handle it before the generic `--node` routing.
    if let Command::Surface { command } = &command {
        let ctx = Ctx::build(&global, io)?;
        return cmd::surface::run(&ctx, io, command, global.node.as_deref());
    }
    // Refuse a command that still needs a v0.2 handler before the generic `--node` redirect,
    // which would otherwise silently run the v0.1 chat loop.
    if cmd::command_needs_v0_2_handler(&command) {
        return Err(cmd::not_implemented());
    }
    // Cross-node session export/import/switch stream a bundle instead of forwarding a local
    // id or path to the remote node (cli.md §7.5–§7.6).
    if let Some(code) = cmd::session::maybe_run_remote(&global, io, &command)? {
        return Ok(code);
    }
    if let Some(code) = remote::maybe_run(&global, &command, original_args)? {
        return Ok(code);
    }
    if let Command::Doctor(args) = &command {
        return cmd::doctor::run(&global, io, args);
    }
    // `schema` prints static protocol types. It needs no project, `vowp.toml` or
    // initialised home, so it runs before `Ctx::build` applies its "run vowp init" guard.
    if let Command::Schema(args) = &command {
        return cmd::schema::run(global.json, io, args);
    }
    // Node-scoped commands run before `Ctx::build`: a v0.1 home cannot open its store
    // until `vowp migrate` (or `vowp up`'s automatic migration) has split it, and the
    // registry commands need no checkout at all.
    if let Some(code) = execute_node_scoped(&global, &command, io)? {
        return Ok(code);
    }
    let ctx = Ctx::build(&global, io)?;
    cmd::dispatch(&ctx, io, command)
}

/// Dispatches `up`, `down`, `migrate` and `project` without a project context.
///
/// Returns `Ok(None)` for every other command. These four are node-scoped per
/// `docs/design/v0.2/home-and-surfaces.md` §3.9, §4.5 and §4.6: they must work from
/// `~/.vowp` itself (the unit's `WorkingDirectory`), on a v0.1 home and on a home with
/// several projects. [`Ctx::build`] requires a project config and an openable v0.2
/// database, so none of them may wait for it.
fn execute_node_scoped(global: &Global, command: &Command, io: &mut dyn IoStreams) -> Result<Option<i32>, CliError> {
    match command {
        Command::Up(args) => cmd::up::run_node(global, io, &crate::cli::UpCommand::Up(args.clone())).map(Some),
        Command::Down(args) => cmd::up::run_node(global, io, &crate::cli::UpCommand::Down(args.clone())).map(Some),
        Command::Migrate(args) => cmd::migrate::run_node(global, io, args).map(Some),
        Command::Project(args) => cmd::project::run_node(global, io, args).map(Some),
        _ => Ok(None),
    }
}

/// Returns the chat arguments when the command asks for agent mode.
fn agent_chat_args(command: &Command) -> Option<&cli::ChatArgs> {
    match command {
        Command::Chat(args) if args.agent => Some(args),
        Command::Session {
            command: cli::SessionCommand::Chat(args),
        } if args.agent => Some(args),
        _ => None,
    }
}

/// Reads all of stdin as UTF-8 text.
pub(crate) fn read_stdin(io: &mut dyn IoStreams) -> Result<String, CliError> {
    let mut text = String::new();
    io.input().read_to_string(&mut text)?;
    Ok(text)
}
