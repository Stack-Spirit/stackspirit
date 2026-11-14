//! `agent`: enroll, run and inspect the Mac-side v0.4 agent, and serve the hidden home gateway.
//!
//! Spec: `docs/design/v0.4/local-orchestrator.md` §4.2, §8.2–§8.5, §12.2–§12.3 and
//! `docs/design/cli.md` §12. The Mac commands use `~/.vowp-agent` (or `VOWP_AGENT_HOME`) and never
//! open a project store or `vowp-core`; the hidden `agent gateway` command is the home-side
//! exception and opens the store through [`Ctx`].

use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;

use vowp_agent::{
    AgentClient, AgentConfig, AgentError, AgentStatus, ConnectionState, NodeKey, RunOptions, RunOutcome, SshTransport,
    SystemAgentClock, fingerprint_of, load_public, pending_spool_bytes, surface_token_expiry, validate_enrollment_code,
};
use vowp_core::agent::gateway;
use vowp_protocol::{AGENT_PROTOCOL, AgentRole};

use crate::cli::{AgentCommand, AgentEnrollArgs, AgentGatewayArgs, AgentRunArgs, AgentStatusArgs};
use crate::node_ctx::NodeRole;
use crate::output::print_json;
use crate::{CliError, Ctx, Global, IoStreams};

/// Runs one agent subcommand.
pub fn run(global: &Global, io: &mut dyn IoStreams, command: &AgentCommand) -> Result<i32, CliError> {
    match command {
        AgentCommand::Enroll(args) => enroll(global, io, args),
        AgentCommand::Run(args) => run_agent(global, io, args),
        AgentCommand::Status(args) => status(global, io, args),
        AgentCommand::Gateway(args) => gateway(global, io, args),
    }
}

/// Serves one Phase 0 agent connection on stdin/stdout.
fn gateway(global: &Global, io: &mut dyn IoStreams, args: &AgentGatewayArgs) -> Result<i32, CliError> {
    if !args.stdio {
        return Err(CliError::Invalid("`vowp agent gateway` requires --stdio".to_owned()));
    }
    let ctx = Ctx::build(global, io)?;
    if ctx.role() != NodeRole::Home {
        return Err(CliError::Guard(
            "the gateway needs the home store; run it on the home machine".to_owned(),
        ));
    }
    let store = ctx.store()?;
    // Frames are the only stdout output; the CLI prints any error to stderr.
    gateway::serve(&store, ctx.deps(), BufReader::new(std::io::stdin()), std::io::stdout())?;
    Ok(0)
}

fn enroll(global: &Global, io: &mut dyn IoStreams, args: &AgentEnrollArgs) -> Result<i32, CliError> {
    let state_dir = AgentConfig::default_state_dir().map_err(map_agent_error)?;
    let code = read_code(io, &args.code_file)?;
    validate_enrollment_code(&code).map_err(map_agent_error)?;
    let key = NodeKey::load_or_generate(&state_dir).map_err(map_agent_error)?;
    let transport = Box::new(SshTransport::new(&args.target));
    let mut client = AgentClient::unenrolled(state_dir, key, transport, Arc::new(SystemAgentClock));
    let enrollment = client.enroll(&args.target, &code).map_err(map_agent_error)?;

    if global.json {
        print_json(io, &enrollment)?;
    } else {
        writeln!(
            io.out(),
            "enrolled node {} in home {} ({})",
            enrollment.node,
            enrollment.home,
            enrollment.protocol
        )?;
    }
    Ok(0)
}

fn run_agent(global: &Global, io: &mut dyn IoStreams, args: &AgentRunArgs) -> Result<i32, CliError> {
    let state_dir = AgentConfig::default_state_dir().map_err(map_agent_error)?;
    let config = AgentConfig::load(&state_dir).map_err(map_agent_error)?;
    let key = NodeKey::load(&state_dir).map_err(map_agent_error)?;
    let target = config.ssh_target().map_err(map_agent_error)?.to_owned();
    let transport = Box::new(SshTransport::new(target));
    let mut client = AgentClient::new(state_dir, key, config, transport, Arc::new(SystemAgentClock));
    let (role, role_name) = parse_role(args.role.as_deref())?;
    if !global.quiet {
        writeln!(
            io.err(),
            "agent running through ssh as {role_name}; press Ctrl-C to stop"
        )?;
        if !args.foreground {
            writeln!(io.err(), "note: Phase 0 always runs in the foreground")?;
        }
    }
    match client
        .run(&RunOptions {
            role,
            foreground: args.foreground,
        })
        .map_err(map_agent_error)?
    {
        RunOutcome::Revoked { reason } => Err(CliError::Guard(format!("agent revoked: {reason}"))),
        RunOutcome::Superseded { connection_id } => {
            Err(CliError::Conflict(format!("agent superseded by {connection_id}")))
        }
    }
}

fn status(global: &Global, io: &mut dyn IoStreams, _args: &AgentStatusArgs) -> Result<i32, CliError> {
    let state_dir = AgentConfig::default_state_dir().map_err(map_agent_error)?;
    let config = AgentConfig::load(&state_dir).map_err(|error| match error {
        AgentError::NotEnrolled(_) => CliError::NotFound(format!(
            "agent is not enrolled under {}; run `vowp agent enroll --target <ssh-target> --code-file -`",
            state_dir.display()
        )),
        other => map_agent_error(other),
    })?;

    let mut status = AgentStatus::load(&state_dir)
        .map_err(map_agent_error)?
        .unwrap_or_default();
    if status.connection == ConnectionState::Unknown {
        status.connection = ConnectionState::Stopped;
    }
    if status.node.is_none() {
        status.node = Some(config.node.clone());
    }
    if status.home.is_none() {
        status.home = Some(config.home.clone());
    }
    if status.protocol.is_none() {
        status.protocol = Some(AGENT_PROTOCOL.to_owned());
    }
    if status.key_fingerprint.is_none() {
        status.key_fingerprint = load_public(&state_dir).ok().map(|key| fingerprint_of(&key));
    }
    status.pending_spool_bytes = pending_spool_bytes(&state_dir);
    if let Some(expiry) = surface_token_expiry(&state_dir) {
        status.surface_token_expires_at = Some(expiry);
    }

    if global.json {
        print_json(io, &status)?;
    } else {
        print_status(io, &status)?;
    }
    Ok(0)
}

fn print_status(io: &mut dyn IoStreams, status: &AgentStatus) -> Result<(), CliError> {
    writeln!(io.out(), "connection: {}", connection_name(status.connection))?;
    writeln!(
        io.out(),
        "node: {}",
        status
            .node
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "-".to_owned())
    )?;
    writeln!(io.out(), "home: {}", status.home.as_deref().unwrap_or("-"))?;
    writeln!(io.out(), "protocol: {}", status.protocol.as_deref().unwrap_or("-"))?;
    writeln!(
        io.out(),
        "last heartbeat: {}",
        status
            .last_heartbeat
            .map(|at| at.as_rfc3339())
            .unwrap_or_else(|| "-".to_owned())
    )?;
    writeln!(io.out(), "pending spool: {} bytes", status.pending_spool_bytes)?;
    writeln!(io.out(), "key: {}", status.key_fingerprint.as_deref().unwrap_or("-"))?;
    if let Some(expiry) = status.surface_token_expires_at {
        writeln!(io.out(), "surface token expires: {}", expiry.as_rfc3339())?;
    }
    if let Some(error) = &status.last_error {
        writeln!(io.out(), "last error: {error}")?;
    }
    Ok(())
}

fn read_code(io: &mut dyn IoStreams, code_file: &str) -> Result<String, CliError> {
    let text = if code_file == "-" {
        crate::read_stdin(io)?
    } else {
        std::fs::read_to_string(Path::new(code_file)).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                CliError::NotFound(format!("code file {code_file}"))
            } else {
                CliError::Io(error)
            }
        })?
    };
    Ok(text.trim().to_owned())
}

fn parse_role(value: Option<&str>) -> Result<(AgentRole, &'static str), CliError> {
    match value.unwrap_or("both") {
        "surface" => Ok((AgentRole::Surface, "surface")),
        "executor" => Ok((AgentRole::Executor, "executor")),
        "both" => Ok((AgentRole::Both, "both")),
        other => Err(CliError::Invalid(format!(
            "invalid role {other:?}; use surface, executor or both"
        ))),
    }
}

fn connection_name(state: ConnectionState) -> &'static str {
    match state {
        ConnectionState::Unknown => "unknown",
        ConnectionState::Connecting => "connecting",
        ConnectionState::Connected => "connected",
        ConnectionState::Reconnecting => "reconnecting",
        ConnectionState::Revoked => "revoked",
        ConnectionState::Superseded => "superseded",
        ConnectionState::Stopped => "stopped",
    }
}

fn map_agent_error(error: AgentError) -> CliError {
    match error {
        AgentError::NotEnrolled(message) => CliError::NotFound(message),
        AgentError::Revoked(reason) => CliError::Guard(format!("agent revoked: {reason}")),
        AgentError::Superseded(connection_id) => CliError::Conflict(format!("agent superseded by {connection_id}")),
        AgentError::Auth(message) => CliError::Remote(format!("authentication rejected: {message}")),
        AgentError::Remote { message, .. } | AgentError::Transport(message) => CliError::Remote(message),
        AgentError::Io(error) => CliError::Io(error),
        AgentError::ReplayNonce => CliError::Guard("the home replayed a handshake nonce".to_owned()),
        AgentError::FrameTooLarge { .. } | AgentError::Protocol(_) | AgentError::Json(_) => {
            CliError::Remote(error.to_string())
        }
        AgentError::Config(_) | AgentError::Key(_) => CliError::Invalid(error.to_string()),
    }
}
