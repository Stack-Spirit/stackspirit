//! `vowp chat --agent`: the local chat surface on the Mac.
//!
//! The home log stays authoritative. This handler loads `~/.vowp-agent/agent.json`, the node
//! key and the paired surface token, then runs the adapter on this machine while the agent
//! client acquires the lease, spools events and streams them to the home gateway. It never
//! takes the home lease and never writes a second session log.

use std::path::Path;
use std::sync::Arc;

use vowp_agent::{AgentConfig, AgentError, ChatClient, ChatTurnRequest, ChatTurnStatus, SystemAgentClock};
use vowp_protocol::{EventBody, HarnessEvent, PathRef, ProjectName, RunPurpose, SessionId};
use vowp_store::session_log::SessionLog;

use crate::chat::{ChatOptions, write_wrapped};
use crate::{CliError, Ctx, IoStreams};

/// Runs the local agent chat bridge for one session.
pub fn run(ctx: &Ctx, io: &mut dyn IoStreams, opts: &ChatOptions) -> Result<i32, CliError> {
    if opts.follow {
        return Err(CliError::Invalid(
            "`chat --agent --follow` is not supported; the agent bridge owns the live turn".to_owned(),
        ));
    }
    if opts.takeover {
        return Err(CliError::Invalid(
            "`chat --agent --takeover` is not supported; ask the owner to take the session over".to_owned(),
        ));
    }
    let state_dir = AgentConfig::default_state_dir().map_err(map_agent_error)?;
    let mut chat = ChatClient::from_state_dir(state_dir, opts.session.clone(), Arc::new(SystemAgentClock))
        .map_err(map_agent_error)?;
    let store = ctx.store()?;
    let session = store.require_session(&opts.session)?;
    let executor = opts
        .executor
        .as_ref()
        .map(|executor| executor.executor.clone())
        .or_else(|| session.executor.clone())
        .ok_or_else(|| CliError::Invalid("session has no executor; pass --executor".to_owned()))?;
    let cwd = if Path::new(&session.cwd).is_dir() {
        Path::new(&session.cwd).to_path_buf()
    } else {
        ctx.project_root().to_path_buf()
    };
    let cwd_ref = path_ref_for(ctx, ctx.project(), &cwd);
    let adapter = ctx
        .deps()
        .adapters
        .get(&executor)
        .ok_or_else(|| CliError::Invalid(format!("harness for {executor} is not installed on this machine")))?;
    let purpose = if first_turn(ctx, &session.id)? {
        RunPurpose::Start
    } else {
        RunPurpose::Continue
    };
    let watcher = if io.is_tty() {
        crate::signals::SignalWatcher::install(ctx.paths()).ok()
    } else {
        None
    };
    let cancel = watcher.as_ref().map(crate::signals::SignalWatcher::cancel_token);
    writeln!(
        io.out(),
        "attached session {} on {} through the agent; press Ctrl-C to stop",
        session.id,
        ctx.config().node.name
    )?;
    loop {
        if cancel.as_ref().is_some_and(vowp_protocol::CancelToken::is_cancelled) {
            break;
        }
        let mut line = String::new();
        let read = io.input().read_line(&mut line)?;
        if read == 0 {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if matches!(line, "/quit" | "/q") {
            break;
        }
        if line.starts_with('/') {
            writeln!(io.err(), "slash commands are not available in --agent mode")?;
            continue;
        }
        let request = ChatTurnRequest {
            executor: executor.clone(),
            prompt: line.to_owned(),
            cwd: cwd.clone(),
            cwd_ref: cwd_ref.clone(),
            job: session.job_id.clone(),
            purpose,
        };
        let mut print_error: Option<CliError> = None;
        let status = chat.turn(
            &request,
            adapter.as_ref(),
            &mut |event| {
                if print_error.is_none()
                    && let Err(error) = print_event(io.out(), event)
                {
                    print_error = Some(error);
                }
            },
            cancel.as_ref(),
        );
        if let Some(error) = print_error {
            return Err(error);
        }
        match status.map_err(map_agent_error)? {
            ChatTurnStatus::Done => {}
            ChatTurnStatus::ReadOnly { reason } => {
                writeln!(io.err(), "This surface is read-only: {reason}")?;
                break;
            }
            ChatTurnStatus::SpoolFull { bytes, cap } => {
                return Err(CliError::Remote(format!(
                    "spool is full ({bytes} bytes would exceed the {cap}-byte cap); the turn stopped"
                )));
            }
            ChatTurnStatus::Interrupted => break,
        }
        if cancel.as_ref().is_some_and(vowp_protocol::CancelToken::is_cancelled) {
            break;
        }
    }
    let _ = chat.release();
    Ok(0)
}

/// Returns whether the session has no started turn yet.
fn first_turn(ctx: &Ctx, session: &SessionId) -> Result<bool, CliError> {
    let Ok(log) = SessionLog::open_read(ctx.paths(), session) else {
        return Ok(true);
    };
    let events = log.read_all().map_err(vowp_core::CoreError::from)?;
    Ok(!events
        .iter()
        .any(|event| event.event_type == vowp_protocol::EventType::TurnStarted))
}

/// Builds the project-relative path for a local working directory.
fn path_ref_for(ctx: &Ctx, project: &ProjectName, cwd: &Path) -> PathRef {
    let root = ctx.project_root();
    if cwd == root {
        return PathRef::Project {
            project: project.clone(),
        };
    }
    let Ok(relative) = cwd.strip_prefix(root) else {
        return PathRef::Project {
            project: project.clone(),
        };
    };
    let relative = relative.to_string_lossy().replace('\\', "/");
    if relative.is_empty() {
        return PathRef::Project {
            project: project.clone(),
        };
    }
    PathRef::parse(&format!("subdir:{project}/{relative}")).unwrap_or_else(|_| PathRef::Project {
        project: project.clone(),
    })
}

/// Prints one adapter event in the home chat's narrow format.
fn print_event(out: &mut dyn std::io::Write, event: &HarnessEvent) -> Result<(), CliError> {
    match &event.body {
        EventBody::AssistantMessage(payload) => write_wrapped(out, &payload.text)?,
        EventBody::ToolCall(payload) => write_wrapped(out, &format!("· tool: {}", payload.name))?,
        EventBody::Error(payload) => write_wrapped(out, &format!("! {}", payload.message))?,
        EventBody::Note(payload) => write_wrapped(out, &format!("— {}", payload.text))?,
        _ => {}
    }
    Ok(())
}

/// Maps an agent failure onto the CLI's stable errors.
fn map_agent_error(error: AgentError) -> CliError {
    match error {
        AgentError::NotEnrolled(message) => CliError::NotFound(message),
        AgentError::Revoked(reason) => CliError::Guard(format!("agent revoked: {reason}")),
        AgentError::Superseded(connection_id) => CliError::Conflict(format!("agent superseded by {connection_id}")),
        AgentError::Auth(message) => CliError::Remote(format!("authentication rejected: {message}")),
        AgentError::Remote { kind, message, .. } => match kind {
            vowp_protocol::ErrorKind::Refused => CliError::Guard(message),
            vowp_protocol::ErrorKind::InvalidInput => CliError::Invalid(message),
            vowp_protocol::ErrorKind::Conflict => CliError::Conflict(message),
            vowp_protocol::ErrorKind::NotFound => CliError::NotFound(message),
            vowp_protocol::ErrorKind::Limit => CliError::Limited(message),
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
