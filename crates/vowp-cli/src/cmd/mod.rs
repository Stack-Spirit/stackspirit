//! Command dispatch. Spec: `docs/design/crates/vowp-cli.md` §1.
//!
//! `init` and `doctor` run before a [`crate::Ctx`] exists; every other command receives one.
//! A command whose handler is not implemented yet is an explicit `NotImplemented` stub here,
//! never a silent no-op.

use crate::cli::{BackupArgs, ChatArgs, Command, MaintCommand, PromoteCommand, SessionCommand, UpCommand};
use crate::{CliError, Ctx, IoStreams};

pub mod attach;
pub mod board;
pub mod capacity;
pub mod control;
pub mod cost;
pub mod doctor;
pub mod exec;
pub mod fleet;
pub mod gc;
pub mod graph;
pub mod inbox;
pub mod init;
pub mod job;
pub mod lease;
pub mod loops;
pub mod maint;
pub mod mcp;
pub mod migrate;
pub mod node;
pub mod orchestrator;
pub mod plan;
pub mod project;
pub mod promote;
pub mod schedule;
pub mod schema;
pub mod secret;
pub mod session;
pub mod setup;
pub mod status;
pub mod tunnel;
pub mod up;
pub mod window;

/// Returns the shared C0 stub error: the command exists but is not implemented in this build.
pub(crate) fn not_implemented() -> CliError {
    CliError::NotImplemented("not implemented in this build")
}

/// Returns whether a chat invocation uses a v0.2 flag C0 does not implement yet.
///
/// The v0.1 handler is still the chat implementation in this build. F3 replaces it with
/// the lease/remote handler that consumes these flags.
fn chat_needs_v0_2_handler(args: &ChatArgs) -> bool {
    args.node.is_some() || args.follow || args.takeover
}

/// Returns whether `command` is a chat invocation with a v0.2 flag C0 does not implement.
///
/// Checked before the generic `--node` redirect: chat's own `--node` names the executor
/// node, not a CLI redirect target, so it must not silently run the v0.1 chat loop.
pub(crate) fn command_needs_v0_2_handler(command: &Command) -> bool {
    match command {
        Command::Chat(args) => chat_needs_v0_2_handler(args),
        Command::Session {
            command: SessionCommand::Chat(args),
        } => chat_needs_v0_2_handler(args),
        _ => false,
    }
}

/// Returns whether a backup invocation asks for a v0.2 form C0 does not implement yet.
///
/// The v0.1 handler only writes the legacy single-DB copy. H2 fills the directory,
/// offsite, encrypted and restore forms.
fn backup_needs_v0_2_handler(args: &BackupArgs) -> bool {
    args.offsite.is_some() || args.encrypt.is_some() || args.restore.is_some() || args.to_home.is_some() || args.force
}

/// Runs one command with a built context.
pub fn dispatch(ctx: &Ctx, io: &mut dyn IoStreams, command: Command) -> Result<i32, CliError> {
    match command {
        Command::Init(_) | Command::Doctor(_) => Err(CliError::Invalid(
            "init and doctor run without a project context".to_owned(),
        )),
        Command::Up(args) => {
            if args.no_migrate {
                Err(not_implemented())
            } else {
                up::run(ctx, io, &UpCommand::Up(args))
            }
        }
        Command::Down(args) => up::run(ctx, io, &UpCommand::Down(args)),
        Command::Status(args) => status::run(ctx, io, &args),
        Command::Board(args) => board::run(ctx, io, &args),
        Command::Plan(args) => plan::run(ctx, io, &args),
        Command::Job { command } => job::run(ctx, io, &command),
        Command::Graph(args) => graph::run(ctx, io, &args),
        Command::Attach(args) => attach::run(ctx, io, &args),
        Command::Session { command } => match &command {
            SessionCommand::Chat(args) if chat_needs_v0_2_handler(args) => Err(not_implemented()),
            _ => session::run(ctx, io, &command),
        },
        Command::Chat(args) => {
            if chat_needs_v0_2_handler(&args) {
                Err(not_implemented())
            } else {
                session::run(ctx, io, &SessionCommand::Chat(args))
            }
        }
        Command::Inbox { command } => inbox::run(ctx, io, &command),
        Command::Cost(args) => cost::run(ctx, io, &args),
        Command::Node { command } => node::run(ctx, io, &command),
        Command::Loops(args) => {
            if args.all {
                Err(not_implemented())
            } else {
                loops::run(ctx, io, &args)
            }
        }
        Command::Schedule(args) => {
            if args.all {
                Err(not_implemented())
            } else {
                schedule::run(ctx, io, &args)
            }
        }
        Command::Promote(args) => promote::run(ctx, io, &PromoteCommand::Promote(args)),
        Command::Rollback(args) => promote::run(ctx, io, &PromoteCommand::Rollback(args)),
        Command::Releases(args) => promote::run(ctx, io, &PromoteCommand::Releases(args)),
        Command::Reindex(args) => maint::run(ctx, io, &MaintCommand::Reindex(args)),
        Command::Backup(args) => {
            if backup_needs_v0_2_handler(&args) {
                Err(not_implemented())
            } else {
                maint::run(ctx, io, &MaintCommand::Backup(args))
            }
        }
        Command::Schema(args) => schema::run(ctx, io, &args),
        Command::Project(args) => project::run(ctx, io, &args),
        Command::Fleet(args) => fleet::run(ctx, io, &args),
        Command::Capacity(args) => capacity::run(ctx, io, &args),
        Command::Secret(args) => secret::run(ctx, io, &args),
        Command::Gc(args) => gc::run(ctx, io, &args),
        Command::Tunnel(args) => tunnel::run(ctx, io, &args),
        Command::Setup(args) => setup::run(ctx, io, &args),
        Command::Control(args) => control::run(ctx, io, &args),
        Command::Orchestrator(args) => orchestrator::run(ctx, io, &args),
        Command::Migrate(args) => migrate::run(ctx, io, &args),
        Command::Mcp(args) => mcp::run(ctx, io, &args),
        Command::Version => maint::run(ctx, io, &MaintCommand::Version),
        Command::Window(args) => window::run(ctx, io, &args),
        Command::Exec(args) => exec::run(ctx, io, &args),
        Command::VaultExec(args) => secret::vault_exec(ctx, io, &args),
        Command::VaultAdmin(args) => secret::vault_admin(ctx, io, &args),
    }
}
