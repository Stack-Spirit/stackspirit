//! `agent`: enroll, run and inspect the Mac-side v0.4 agent, and serve the hidden home gateway.
//!
//! Spec: `docs/design/v0.4/local-orchestrator.md` §4.2, §8.2–§8.5, §12.2–§12.3 and
//! `docs/design/cli.md` §12. The Mac commands use `~/.vowp-agent` (or `VOWP_AGENT_HOME`) and never
//! open a project store or `vowp-core`; the hidden `agent gateway` command is the home-side
//! exception and opens the store through [`Ctx`].

use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;

use serde::Serialize;
use vowp_agent::{
    AgentClient, AgentConfig, AgentError, AgentStatus, ConnectionState, NodeKey, RunOptions, RunOutcome, SshTransport,
    SurfacePairRequest, SystemAgentClock, fingerprint_of, load_public, pending_spool_bytes, surface_token_expiry,
    validate_enrollment_code,
};
use vowp_core::agent::gateway;
use vowp_protocol::{AGENT_PROTOCOL, AgentPathMap, AgentRole, ErrorKind, ProjectName, SessionId, SurfaceId};

use crate::cli::{
    AgentCommand, AgentEnrollArgs, AgentGatewayArgs, AgentPairArgs, AgentPathsAddArgs, AgentPathsArgs,
    AgentPathsCommand, AgentPathsRmArgs, AgentRunArgs, AgentStatusArgs,
};
use crate::node_ctx::NodeRole;
use crate::output::print_json;
use crate::{CliError, Ctx, Global, IoStreams};

/// Runs one agent subcommand.
pub fn run(global: &Global, io: &mut dyn IoStreams, command: &AgentCommand) -> Result<i32, CliError> {
    match command {
        AgentCommand::Enroll(args) => enroll(global, io, args),
        AgentCommand::Paths(args) => paths(global, io, args),
        AgentCommand::Pair(args) => pair(global, io, args),
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
    let path_maps = enrollment_path_maps(&state_dir, args)?;
    let code = read_code(io, &args.code_file)?;
    validate_enrollment_code(&code).map_err(map_agent_error)?;
    let key = NodeKey::load_or_generate(&state_dir).map_err(map_agent_error)?;
    let transport = Box::new(SshTransport::new(&args.target));
    let mut client =
        AgentClient::unenrolled(state_dir, key, transport, Arc::new(SystemAgentClock)).with_path_maps(path_maps);
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

/// Builds the map sent by enrollment: explicit `--project` flags win, else the saved map.
fn enrollment_path_maps(state_dir: &Path, args: &AgentEnrollArgs) -> Result<Vec<AgentPathMap>, CliError> {
    let path_maps = if args.project.is_empty() {
        if args.worktree_root.is_some() {
            return Err(CliError::Invalid(
                "--worktree-root needs at least one --project".to_owned(),
            ));
        }
        AgentConfig::load(state_dir)
            .map(|config| config.path_maps)
            .unwrap_or_default()
    } else {
        let mut path_maps = Vec::with_capacity(args.project.len());
        for project in &args.project {
            path_maps.push(path_map_from_checkout(
                Path::new(project),
                None,
                args.worktree_root.as_deref(),
                &[],
            )?);
        }
        path_maps
    };
    AgentConfig::validate_path_maps(state_dir, &path_maps).map_err(map_agent_error)?;
    Ok(path_maps)
}

fn paths(global: &Global, io: &mut dyn IoStreams, args: &AgentPathsArgs) -> Result<i32, CliError> {
    match &args.command {
        AgentPathsCommand::Add(args) => paths_add(global, io, args),
        AgentPathsCommand::Rm(args) => paths_rm(global, io, args),
        AgentPathsCommand::Ls(_) => paths_ls(global, io),
    }
}

fn paths_add(global: &Global, io: &mut dyn IoStreams, args: &AgentPathsAddArgs) -> Result<i32, CliError> {
    let state_dir = AgentConfig::default_state_dir().map_err(map_agent_error)?;
    let mut config = load_agent_config(&state_dir)?;
    let map = path_map_from_checkout(
        Path::new(&args.project),
        args.name.as_deref(),
        args.worktree_root.as_deref(),
        &args.allow,
    )?;
    config.set_path_map(map.clone()).map_err(map_agent_error)?;
    config.write().map_err(map_agent_error)?;
    if global.json {
        print_json(io, &map)?;
    } else {
        print_path_map(io, &map)?;
    }
    Ok(0)
}

fn paths_rm(global: &Global, io: &mut dyn IoStreams, args: &AgentPathsRmArgs) -> Result<i32, CliError> {
    let state_dir = AgentConfig::default_state_dir().map_err(map_agent_error)?;
    let mut config = load_agent_config(&state_dir)?;
    let project = ProjectName::parse(&args.project_name)
        .map_err(|_| CliError::Invalid(format!("invalid project name {:?}", args.project_name)))?;
    if !config.remove_path_map(&project) {
        return Err(CliError::NotFound(format!("project {project} is not mapped")));
    }
    config.write().map_err(map_agent_error)?;
    if global.json {
        print_json(io, &serde_json::json!({ "project": project.as_str(), "removed": true }))?;
    } else {
        writeln!(io.out(), "unmapped project {project}")?;
    }
    Ok(0)
}

fn paths_ls(global: &Global, io: &mut dyn IoStreams) -> Result<i32, CliError> {
    let state_dir = AgentConfig::default_state_dir().map_err(map_agent_error)?;
    let config = load_agent_config(&state_dir)?;
    if global.json {
        print_json(io, &config.path_maps)?;
    } else if config.path_maps.is_empty() {
        writeln!(io.out(), "no projects mapped")?;
    } else {
        for map in &config.path_maps {
            print_path_map(io, map)?;
        }
    }
    Ok(0)
}

fn load_agent_config(state_dir: &Path) -> Result<AgentConfig, CliError> {
    AgentConfig::load(state_dir).map_err(|error| match error {
        AgentError::NotEnrolled(_) => CliError::NotFound(format!(
            "agent is not enrolled under {}; run `vowp agent enroll --target <ssh-target> --code-file -`",
            state_dir.display()
        )),
        other => map_agent_error(other),
    })
}

/// Builds one validated map from a checkout and the CLI flags.
fn path_map_from_checkout(
    project: &Path,
    name: Option<&str>,
    worktree_root: Option<&Path>,
    allow: &[String],
) -> Result<AgentPathMap, CliError> {
    let local_root = std::fs::canonicalize(project).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            CliError::NotFound(format!("project checkout {}", project.display()))
        } else {
            CliError::Io(error)
        }
    })?;
    if !local_root.is_dir() {
        return Err(CliError::Invalid(format!(
            "project checkout {} is not a directory",
            local_root.display()
        )));
    }
    let project_name = match name {
        Some(name) => {
            ProjectName::parse(name).map_err(|_| CliError::Invalid(format!("invalid project name {name:?}")))?
        }
        None => project_name_for(&local_root)?,
    };
    let worktree_root = match worktree_root {
        Some(root) if root.is_absolute() => root.to_path_buf(),
        Some(root) => std::env::current_dir().map_err(CliError::Io)?.join(root),
        None => local_root.join(vowp_agent::DEFAULT_WORKTREE_DIR),
    };
    std::fs::create_dir_all(&worktree_root).map_err(CliError::Io)?;
    let worktree_root = std::fs::canonicalize(&worktree_root).map_err(CliError::Io)?;
    let local_root = local_root
        .to_str()
        .ok_or_else(|| CliError::Invalid("project checkout must be a UTF-8 path".to_owned()))?
        .to_owned();
    let worktree_root = worktree_root
        .to_str()
        .ok_or_else(|| CliError::Invalid("worktree root must be a UTF-8 path".to_owned()))?
        .to_owned();
    Ok(AgentPathMap {
        project: project_name,
        local_root,
        worktree_root: Some(worktree_root),
        allow: allow.to_vec(),
    })
}

/// Reads `[project].name` from a checkout's `vowp.toml`, else uses the directory name.
fn project_name_for(local_root: &Path) -> Result<ProjectName, CliError> {
    let config_path = local_root.join("vowp.toml");
    if config_path.is_file() {
        let text = std::fs::read_to_string(&config_path).map_err(CliError::Io)?;
        let value: toml::Value = toml::from_str(&text)
            .map_err(|error| CliError::Invalid(format!("invalid {}: {error}", config_path.display())))?;
        if let Some(name) = value
            .get("project")
            .and_then(|project| project.get("name"))
            .and_then(toml::Value::as_str)
        {
            return ProjectName::parse(name)
                .map_err(|_| CliError::Invalid(format!("invalid project name {name:?} in {}", config_path.display())));
        }
    }
    let name = local_root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CliError::Invalid(format!("cannot derive a project name from {}", local_root.display())))?;
    ProjectName::parse(name).map_err(|_| {
        CliError::Invalid(format!(
            "cannot derive a project name from {}; pass --name",
            local_root.display()
        ))
    })
}

fn print_path_map(io: &mut dyn IoStreams, map: &AgentPathMap) -> Result<(), CliError> {
    write!(io.out(), "{} {}", map.project, map.local_root)?;
    if let Some(root) = &map.worktree_root {
        write!(io.out(), " worktrees {root}")?;
    }
    if !map.allow.is_empty() {
        write!(io.out(), " allow {}", map.allow.join(","))?;
    }
    writeln!(io.out())?;
    Ok(())
}

fn pair(global: &Global, io: &mut dyn IoStreams, args: &AgentPairArgs) -> Result<i32, CliError> {
    let state_dir = AgentConfig::default_state_dir().map_err(map_agent_error)?;
    let code = read_code(io, &args.code_file)?;
    let session = SessionId::parse(&args.session)
        .map_err(|_| CliError::Invalid(format!("invalid session id {:?}", args.session)))?;
    let surface_id = SurfaceId(args.surface.clone());
    let app = args.app.clone().unwrap_or_else(|| "agent".to_owned());
    let request = SurfacePairRequest::new(code, surface_id, session, app);

    let mut client = AgentClient::from_state_dir(state_dir, Arc::new(SystemAgentClock)).map_err(map_agent_error)?;
    let paired = client.pair(&request).map_err(map_agent_error)?;
    if global.json {
        print_json(io, &paired)?;
    } else {
        writeln!(
            io.out(),
            "paired surface {}; token stored at {}; expires {}",
            paired.surface_id,
            paired.token_path.display(),
            paired.expires_at.as_rfc3339()
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
        print_json(
            io,
            &StatusDocument {
                status: &status,
                paths: &config.path_maps,
            },
        )?;
    } else {
        print_status(io, &status)?;
        if !config.path_maps.is_empty() {
            writeln!(io.out(), "paths:")?;
            for map in &config.path_maps {
                write!(io.out(), "  ")?;
                print_path_map(io, map)?;
            }
        }
    }
    Ok(0)
}

#[derive(Serialize)]
struct StatusDocument<'a> {
    #[serde(flatten)]
    status: &'a AgentStatus,
    /// Absolute roots, printed only by this owner-only command (never by `status.json`).
    paths: &'a [AgentPathMap],
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
        AgentError::Remote { kind, message, .. } => match kind {
            ErrorKind::Refused => CliError::Guard(message),
            ErrorKind::InvalidInput => CliError::Invalid(message),
            ErrorKind::Conflict => CliError::Conflict(message),
            ErrorKind::NotFound => CliError::NotFound(message),
            ErrorKind::Limit => CliError::Limited(message),
            other => CliError::Remote(format!("home error ({other:?}): {message}")),
        },
        AgentError::Transport(message) => CliError::Remote(message),
        AgentError::Io(error) => CliError::Io(error),
        AgentError::ReplayNonce => CliError::Guard("the home replayed a handshake nonce".to_owned()),
        AgentError::FrameTooLarge { .. } | AgentError::Protocol(_) | AgentError::Json(_) => {
            CliError::Remote(error.to_string())
        }
        AgentError::Config(_) | AgentError::Key(_) => CliError::Invalid(error.to_string()),
    }
}
