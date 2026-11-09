//! `session` and the `chat` alias.
//!
//! Spec: `docs/design/cli.md` §4 and `docs/design/sessions.md`. Session content lives in the
//! append-only log; the rows here are the store index. Ids are accepted by unique prefix of
//! at least six characters.

use std::path::PathBuf;
use std::time::Duration;

use time::{OffsetDateTime, macros::format_description};
use vowp_core::session::bundle;
use vowp_core::session::service::SessionService;
use vowp_protocol::{
    AttachAction, CancelToken, CapsuleTrigger, EventBody, EventEnvelope, Executor, ExecutorRef, NewSession, RunOutcome,
    RunPurpose, RunRole, Session, SessionId, SessionRole, SessionState, SwitchReason, Timestamp, TurnRequest,
};
use vowp_store::session_log::SessionLog;
use vowp_store::{SessionFilter, Store};

use crate::cli::{
    ChatArgs, SessionCapsuleArgs, SessionCommand, SessionExportArgs, SessionImportArgs, SessionLsArgs, SessionNewArgs,
    SessionShowArgs, SessionSwitchArgs, SessionTailArgs, SessionWatchArgs,
};
use crate::output::{Table, local_time_at, print_json};
use crate::{CliError, Ctx, IoStreams};

/// Runs one session subcommand.
pub fn run(ctx: &Ctx, io: &mut dyn IoStreams, command: &SessionCommand) -> Result<i32, CliError> {
    match command {
        SessionCommand::Ls(args) => list(ctx, io, args),
        SessionCommand::Show(args) => show(ctx, io, args),
        SessionCommand::New(args) => new_session(ctx, io, args),
        SessionCommand::Chat(args) => chat(ctx, io, args),
        SessionCommand::Switch(args) => switch(ctx, io, args),
        SessionCommand::Capsule(args) => capsule(ctx, io, args),
        SessionCommand::Export(args) => export(ctx, io, args),
        SessionCommand::Import(args) => import(ctx, io, args),
        SessionCommand::Watch(args) => watch(ctx, io, args),
        SessionCommand::Tail(args) => tail(ctx, io, args),
    }
}

/// Lists sessions with the documented columns.
fn list(ctx: &Ctx, io: &mut dyn IoStreams, args: &SessionLsArgs) -> Result<i32, CliError> {
    let store = ctx.store()?;
    let project = match args.project.as_deref() {
        Some(value) => match vowp_protocol::ProjectName::parse(value) {
            Ok(name) => Some(name),
            Err(error) => {
                let path = std::path::Path::new(value);
                if path.is_dir() && path.join("vowp.toml").is_file() {
                    Some(
                        crate::ctx::load_project_config(&path.join("vowp.toml"), ctx.paths())?
                            .project
                            .name,
                    )
                } else {
                    return Err(error.into());
                }
            }
        },
        None => Some(ctx.project().clone()),
    };
    let filter = SessionFilter {
        project,
        job_id: args.job.as_deref().map(vowp_protocol::JobId::parse).transpose()?,
        state: args.state.as_deref().map(parse_state).transpose()?,
        limit: args.limit,
    };
    let sessions = store.list_sessions(&filter)?;
    if ctx.json() {
        print_json(io, &sessions)?;
        return Ok(0);
    }

    let now = ctx.clock().now();
    let mut table = Table::new(&["ID", "TITLE", "EXECUTOR", "NODE", "STATE", "LAST", "UPDATED", "EVENTS"]);
    for session in &sessions {
        let last = last_event(&store, session)
            .map(|event| local_time_at(event.ts, now, ctx.tz_offset_minutes()))
            .unwrap_or_else(|| "-".to_owned());
        table.row(vec![
            session.id.to_string(),
            session.title.clone(),
            session
                .executor
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "-".to_owned()),
            session.node.to_string(),
            state_name(session.state).to_owned(),
            last,
            local_time_at(session.updated_at, now, ctx.tz_offset_minutes()),
            session.last_seq.to_string(),
        ]);
    }
    writeln!(io.out(), "{}", table.render(ctx.color()))?;
    Ok(0)
}

/// Shows one session, its executor history, switches and optional capsule/events.
fn show(ctx: &Ctx, io: &mut dyn IoStreams, args: &SessionShowArgs) -> Result<i32, CliError> {
    let store = ctx.store()?;
    let session = resolve_session(&store, &args.id)?;
    let events = SessionLog::open_read(ctx.paths(), &session.id)
        .map_err(vowp_core::CoreError::from)?
        .read_all()
        .map_err(vowp_core::CoreError::from)?;
    let executor_history: Vec<EventEnvelope> = events
        .iter()
        .filter(|event| event.event_type == vowp_protocol::EventType::SessionExecutor)
        .cloned()
        .collect();
    let switches = store.list_switches(&session.id)?;
    let capsule = if args.capsule {
        store.latest_capsule(&session.id)?
    } else {
        None
    };
    let last_events = match args.events {
        Some(count) => {
            let start = events.len().saturating_sub(count);
            events[start..].to_vec()
        }
        None => Vec::new(),
    };

    if ctx.json() {
        #[derive(serde::Serialize)]
        struct Show<'a> {
            #[serde(flatten)]
            session: &'a Session,
            #[serde(skip_serializing_if = "Option::is_none")]
            capsule: Option<&'a vowp_protocol::Capsule>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            switches: Vec<&'a vowp_protocol::Switch>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            events: Vec<EventEnvelope>,
        }
        print_json(
            io,
            &Show {
                session: &session,
                capsule: capsule.as_ref(),
                switches: if args.lineage {
                    switches.iter().collect()
                } else {
                    Vec::new()
                },
                events: last_events,
            },
        )?;
        return Ok(0);
    }

    let now = ctx.clock().now();
    writeln!(io.out(), "id: {}", session.id)?;
    writeln!(io.out(), "title: {}", session.title)?;
    writeln!(io.out(), "state: {}", state_name(session.state))?;
    writeln!(
        io.out(),
        "project: {}",
        session
            .project
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "-".to_owned())
    )?;
    writeln!(
        io.out(),
        "job: {}",
        session
            .job_id
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "-".to_owned())
    )?;
    writeln!(io.out(), "node: {}", session.node)?;
    writeln!(io.out(), "cwd: {}", session.cwd)?;
    writeln!(
        io.out(),
        "executor: {}",
        session
            .executor
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "-".to_owned())
    )?;
    writeln!(
        io.out(),
        "created: {}",
        local_time_at(session.created_at, now, ctx.tz_offset_minutes())
    )?;
    writeln!(
        io.out(),
        "updated: {}",
        local_time_at(session.updated_at, now, ctx.tz_offset_minutes())
    )?;
    writeln!(io.out(), "events: {}", session.last_seq)?;

    if !executor_history.is_empty() {
        writeln!(io.out(), "\nexecutor history:")?;
        for event in &executor_history {
            if let Ok(EventBody::SessionExecutor(payload)) = event.body() {
                let action = match payload.action {
                    AttachAction::Attach => "attach",
                    AttachAction::Detach => "detach",
                };
                writeln!(
                    io.out(),
                    "  {} {} {} on {}",
                    clock_time(event.ts, ctx.tz_offset_minutes()),
                    action,
                    payload.executor,
                    payload.node
                )?;
            }
        }
    }
    if args.lineage && !switches.is_empty() {
        writeln!(io.out(), "\nswitches:")?;
        for switch in &switches {
            writeln!(
                io.out(),
                "  {} {} -> {} ({:?}, {:?})",
                switch.id,
                switch
                    .from_executor
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "-".to_owned()),
                switch.to_executor,
                switch.reason,
                switch.state
            )?;
        }
    }
    if let Some(capsule) = &capsule {
        writeln!(
            io.out(),
            "\ncapsule: {} (trigger {:?}, at seq {})",
            capsule.capsule_id,
            capsule.trigger,
            capsule.at_seq
        )?;
    }
    if !last_events.is_empty() {
        writeln!(io.out(), "\nevents:")?;
        for event in &last_events {
            writeln!(io.out(), "{}", pretty_event(event, ctx.tz_offset_minutes()))?;
        }
    }
    Ok(0)
}

/// Creates an interactive session and prints its id.
fn new_session(ctx: &Ctx, io: &mut dyn IoStreams, args: &SessionNewArgs) -> Result<i32, CliError> {
    if let Some(title) = &args.title
        && title.trim().is_empty()
    {
        return Err(CliError::Invalid("session title must not be empty".to_owned()));
    }
    let executor = args.executor.as_deref().map(Executor::parse).transpose()?;
    let cwd = args.cwd.clone().unwrap_or_else(|| ctx.project_root().to_path_buf());
    if let Some(node) = &args.node {
        let name = vowp_protocol::NodeName::parse(node)?;
        let store = ctx.store()?;
        let row = store.require_node(&name)?;
        if !row.is_local {
            return Err(CliError::Remote(format!(
                "session new on node {name} is not available in this build"
            )));
        }
    }
    let title = args.title.clone().unwrap_or_else(|| "interactive".to_owned());
    let service = SessionService::new(ctx.deps().clone());
    let id = service.create_session(NewSession {
        project: Some(ctx.project().clone()),
        job_id: None,
        role: SessionRole::User,
        title,
        cwd,
        parent: None,
        executor,
    })?;
    let store = ctx.store()?;
    let session = store.require_session(&id)?;
    if ctx.json() {
        print_json(io, &session)?;
    } else {
        writeln!(io.out(), "{id}")?;
    }
    Ok(0)
}

/// Runs the interactive chat loop.
fn chat(ctx: &Ctx, io: &mut dyn IoStreams, args: &ChatArgs) -> Result<i32, CliError> {
    let store = ctx.store()?;
    let mut session = resolve_session(&store, &args.id)?;
    if let Some(executor) = &args.executor {
        let target = Executor::parse(executor)?;
        switch_to(ctx, &session.id, target, None, SwitchReason::User, false)?;
        session = store.require_session(&session.id)?;
    }
    let mut lines = String::new();
    loop {
        lines.clear();
        let read = io.input().read_line(&mut lines)?;
        if read == 0 {
            break;
        }
        let line = lines.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('/') {
            let mut parts = rest.split_whitespace();
            match parts.next().unwrap_or_default() {
                "quit" | "q" => break,
                "switch" => {
                    let Some(target) = parts.next() else {
                        writeln!(io.err(), "usage: /switch <harness:model[@effort][#variant]> [--node n]")?;
                        continue;
                    };
                    let target = Executor::parse(target)?;
                    let node = match (parts.next(), parts.next()) {
                        (Some("--node"), Some(node)) => Some(vowp_protocol::NodeName::parse(node)?),
                        (None, None) => None,
                        _ => {
                            writeln!(io.err(), "usage: /switch <executor> [--node n]")?;
                            continue;
                        }
                    };
                    match switch_to(ctx, &session.id, target.clone(), node, SwitchReason::User, true) {
                        Ok(_) => {
                            session = store.require_session(&session.id)?;
                            writeln!(io.out(), "switched to {target}")?;
                        }
                        Err(error) => writeln!(io.err(), "vowp: {error}")?,
                    }
                }
                "capsule" => {
                    let service = SessionService::new(ctx.deps().clone());
                    let capsule = service.write_capsule(&session.id, CapsuleTrigger::Manual)?;
                    writeln!(io.out(), "{}", capsule.capsule_id)?;
                }
                "export" => {
                    let Some(path) = parts.next() else {
                        writeln!(io.err(), "usage: /export <file>")?;
                        continue;
                    };
                    let bundle = bundle::export(ctx.deps(), &session.id, 20)?;
                    let text = bundle::to_json(&bundle)?;
                    std::fs::write(path, text)?;
                    writeln!(io.out(), "{path}")?;
                }
                "events" => {
                    let count = parts.next().and_then(|value| value.parse::<usize>().ok()).unwrap_or(10);
                    let events = SessionLog::open_read(ctx.paths(), &session.id)
                        .map_err(vowp_core::CoreError::from)?
                        .read_all()
                        .map_err(vowp_core::CoreError::from)?;
                    let start = events.len().saturating_sub(count);
                    for event in &events[start..] {
                        writeln!(io.out(), "{}", pretty_event(event, ctx.tz_offset_minutes()))?;
                    }
                }
                other => writeln!(io.err(), "unknown command: /{other}")?,
            }
            continue;
        }

        let executor = session
            .executor
            .clone()
            .or_else(|| {
                ctx.config()
                    .profiles
                    .get("worker")
                    .and_then(|profile| profile.ladder.first().cloned())
            })
            .ok_or_else(|| CliError::Invalid("session has no executor; use /switch".to_owned()))?;
        let before = SessionLog::open_read(ctx.paths(), &session.id)
            .map_err(vowp_core::CoreError::from)?
            .last_seq()
            .map_err(vowp_core::CoreError::from)?;
        let service = SessionService::new(ctx.deps().clone());
        let result = service.run_turn(
            TurnRequest {
                session_id: session.id.clone(),
                executor: ExecutorRef { executor, node: None },
                purpose: RunPurpose::Start,
                role: RunRole::Worker,
                job_id: None,
                prompt: line.to_owned(),
                profile: None,
                rung: 0,
                window: None,
                timeout: None,
            },
            &CancelToken::default(),
        )?;
        let after = SessionLog::open_read(ctx.paths(), &session.id)
            .map_err(vowp_core::CoreError::from)?
            .read_from(before, None)
            .map_err(vowp_core::CoreError::from)?;
        for event in after {
            match event.body() {
                Ok(EventBody::AssistantMessage(payload)) => writeln!(io.out(), "{}", payload.text)?,
                Ok(EventBody::ToolCall(payload)) => writeln!(io.out(), "· tool: {}", payload.name)?,
                _ => {}
            }
        }
        if matches!(
            result.summary.outcome,
            RunOutcome::Limit | RunOutcome::Credits | RunOutcome::Auth
        ) {
            return Err(CliError::Limited(
                "provider limit reached; use /switch <executor>".to_owned(),
            ));
        }
        session = store.require_session(&session.id)?;
        if session.state == SessionState::Blocked {
            return Err(CliError::Limited(
                "session is blocked by a provider limit; use /switch <executor>".to_owned(),
            ));
        }
        io.out().flush()?;
    }
    Ok(0)
}

/// Switches a session, checking the running-turn guard and target limits first.
fn switch(ctx: &Ctx, io: &mut dyn IoStreams, args: &SessionSwitchArgs) -> Result<i32, CliError> {
    let store = ctx.store()?;
    let session = resolve_session(&store, &args.id)?;
    let target = Executor::parse(&args.to)?;
    let node = args.node.as_deref().map(vowp_protocol::NodeName::parse).transpose()?;
    if let Some(node) = &node {
        let row = store.require_node(node)?;
        if !row.is_local {
            return Err(CliError::Remote(format!(
                "cross-node session switch to {node} is not available in this build"
            )));
        }
    }
    let reason = parse_reason(args.reason.as_deref())?;
    let switch = switch_to(ctx, &session.id, target, node, reason, args.force)?;
    if ctx.json() {
        print_json(io, &switch)?;
    } else {
        writeln!(
            io.out(),
            "switched session {} to {} (capsule {})",
            session.id,
            switch.to_executor,
            switch
                .capsule_id
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "none".to_owned())
        )?;
    }
    Ok(0)
}

/// Runs the core switch protocol after the CLI guards.
fn switch_to(
    ctx: &Ctx,
    session: &SessionId,
    target: Executor,
    node: Option<vowp_protocol::NodeName>,
    reason: SwitchReason,
    force: bool,
) -> Result<vowp_protocol::Switch, CliError> {
    let store = ctx.store()?;
    let row = store.require_session(session)?;
    if row.state == SessionState::Running && !force {
        return Err(CliError::Conflict(format!(
            "session {session} is running a turn; pass --force to switch"
        )));
    }
    let provider = target.provider();
    if store
        .active_limits(ctx.clock().now())?
        .iter()
        .any(|limit| limit.provider == provider)
    {
        return Err(CliError::Limited(format!("provider {provider} is limited")));
    }
    let service = SessionService::new(ctx.deps().clone());
    Ok(service.switch(&vowp_protocol::SwitchRequest {
        session_id: session.clone(),
        to: ExecutorRef { executor: target, node },
        reason,
        drain_secs: 5,
    })?)
}

/// Writes a manual capsule and prints its id or Markdown.
fn capsule(ctx: &Ctx, io: &mut dyn IoStreams, args: &SessionCapsuleArgs) -> Result<i32, CliError> {
    let store = ctx.store()?;
    let session = resolve_session(&store, &args.id)?;
    let service = SessionService::new(ctx.deps().clone());
    let capsule = service.write_capsule(&session.id, CapsuleTrigger::Manual)?;
    if ctx.json() {
        print_json(io, &capsule)?;
    } else if args.print {
        write!(io.out(), "{}", capsule.render_markdown())?;
    } else {
        writeln!(io.out(), "{}", capsule.capsule_id)?;
    }
    Ok(0)
}

/// JSON shape for an export to a file.
#[derive(Debug, serde::Serialize)]
struct ExportOutput {
    path: Option<String>,
    session: SessionId,
    format: &'static str,
    events: usize,
}

/// Exports a session bundle.
fn export(ctx: &Ctx, io: &mut dyn IoStreams, args: &SessionExportArgs) -> Result<i32, CliError> {
    let store = ctx.store()?;
    let session = resolve_session(&store, &args.id)?;
    let bundle = bundle::export(ctx.deps(), &session.id, args.tail.unwrap_or(0))?;
    let text = bundle::to_json(&bundle)?;
    if args.out.as_deref() == Some(std::path::Path::new("-")) {
        write!(io.out(), "{text}")?;
        if !text.ends_with('\n') {
            writeln!(io.out())?;
        }
        return Ok(0);
    }
    let path = args
        .out
        .clone()
        .unwrap_or_else(|| PathBuf::from(format!("{}.vowp-bundle.json", session.id)));
    std::fs::write(&path, text)?;
    if ctx.json() {
        print_json(
            io,
            &ExportOutput {
                path: Some(path.display().to_string()),
                session: session.id,
                format: bundle::BUNDLE_FORMAT,
                events: bundle.tail.len(),
            },
        )?;
    } else {
        writeln!(io.out(), "{}", path.display())?;
    }
    Ok(0)
}

/// Imports a bundle as a child session.
fn import(ctx: &Ctx, io: &mut dyn IoStreams, args: &SessionImportArgs) -> Result<i32, CliError> {
    if let Some(node) = &args.node {
        let name = vowp_protocol::NodeName::parse(node)?;
        let store = ctx.store()?;
        let row = store.require_node(&name)?;
        if !row.is_local {
            return Err(CliError::Remote(format!(
                "session import on node {name} is not available in this build"
            )));
        }
    }
    let text = if args.file == "-" {
        crate::read_stdin(io)?
    } else {
        std::fs::read_to_string(&args.file)?
    };
    let bundle = bundle::from_json(&text)?;
    let executor = match &args.executor {
        Some(text) => Some(ExecutorRef {
            executor: Executor::parse(text)?,
            node: None,
        }),
        None => None,
    };
    let id = bundle::import(ctx.deps(), &bundle, executor, args.cwd.as_deref())?;
    let store = ctx.store()?;
    let session = store.require_session(&id)?;
    if ctx.json() {
        print_json(io, &session)?;
    } else {
        writeln!(io.out(), "{id}")?;
    }
    Ok(0)
}

/// Records interactive Claude Code transcripts (the optional v0.1 importer).
fn watch(ctx: &Ctx, io: &mut dyn IoStreams, args: &SessionWatchArgs) -> Result<i32, CliError> {
    let dir = args.claude_dir.clone().unwrap_or_else(default_claude_dir);
    if !dir.is_dir() {
        return Err(CliError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Claude transcript directory {} is unreadable", dir.display()),
        )));
    }
    if !args.once && io.is_tty() {
        return Err(CliError::Invalid(
            "session watch --follow is not available in this build; use --once".to_owned(),
        ));
    }
    let transcripts = transcript_files(&dir)?;
    // The optional importer records new transcript lines once and stores the processed line
    // count next to the node. Its parser is deliberately small: only the event-bearing line
    // types are mapped, and unknown lines are skipped.
    let mut imported = 0;
    for path in &transcripts {
        imported += import_transcript(ctx, path)?;
    }
    if ctx.json() {
        print_json(
            io,
            &serde_json::json!({
                "watching": transcripts.len(),
                "imported": imported,
                "dir": dir.display().to_string(),
            }),
        )?;
    } else {
        writeln!(
            io.out(),
            "watching {} transcripts ({imported} events imported)",
            transcripts.len()
        )?;
    }
    Ok(0)
}

/// Returns the default Claude transcript root without reading it in tests.
fn default_claude_dir() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude")))
        .unwrap_or_else(|| PathBuf::from(".claude"))
}

/// Lists `*.jsonl` transcript files recursively under `dir`.
fn transcript_files(dir: &std::path::Path) -> Result<Vec<PathBuf>, CliError> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(path) = stack.pop() {
        for entry in std::fs::read_dir(&path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|extension| extension == "jsonl") {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Imports one Claude transcript file into a canonical session.
fn import_transcript(ctx: &Ctx, path: &std::path::Path) -> Result<usize, CliError> {
    let text = std::fs::read_to_string(path)?;
    let Some(first) = text
        .lines()
        .find_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
    else {
        return Ok(0);
    };
    let native = first
        .get("session_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if native.is_empty() {
        return Ok(0);
    }
    let store = ctx.store()?;
    let marker_key = format!("watch:{native}");
    let processed: usize = store
        .kv_get(&marker_key)?
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= processed {
        return Ok(0);
    }
    let title = lines
        .iter()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find_map(|value| {
            (value.get("type").and_then(serde_json::Value::as_str) == Some("user"))
                .then(|| {
                    value
                        .get("message")
                        .and_then(|message| message.get("content"))
                        .and_then(|content| content.as_str())
                        .map(|text| text.chars().take(60).collect::<String>())
                })
                .flatten()
        })
        .unwrap_or_else(|| format!("claude {native}"));
    let service = SessionService::new(ctx.deps().clone());
    let id = service.create_session(NewSession {
        project: Some(ctx.project().clone()),
        job_id: None,
        role: SessionRole::User,
        title,
        cwd: ctx.project_root().to_path_buf(),
        parent: None,
        executor: Some(Executor::parse("claude-code:claude-sonnet-5-5@medium")?),
    })?;
    let log = SessionLog::open(ctx.paths(), &id, ctx.deps().clock.clone()).map_err(vowp_core::CoreError::from)?;
    let mut imported = 0;
    for line in &lines[processed..] {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some(body) = transcript_body(&value) {
            let appended = log
                .append(vowp_store::session_log::NewEvent {
                    actor: match body {
                        EventBody::AssistantMessage(_) | EventBody::ToolCall(_) => vowp_protocol::Actor::Assistant,
                        _ => vowp_protocol::Actor::User,
                    },
                    source: vowp_protocol::EventSource {
                        harness: vowp_protocol::HarnessKind::ClaudeCode,
                        model: Some("claude-sonnet-5-5".to_owned()),
                        node: Some(ctx.config().node.name.clone()),
                        native_session_id: Some(native.clone()),
                    },
                    body,
                    raw: Some(value),
                    ts: None,
                })
                .map_err(vowp_core::CoreError::from)?;
            store.index_event(&vowp_store::session_log::index_row(&appended))?;
            imported += 1;
        }
    }
    log.sync().map_err(vowp_core::CoreError::from)?;
    store.kv_set(&marker_key, &lines.len().to_string())?;
    Ok(imported)
}

/// Maps one Claude transcript line to a canonical event body.
fn transcript_body(value: &serde_json::Value) -> Option<EventBody> {
    let kind = value.get("type").and_then(serde_json::Value::as_str)?;
    match kind {
        "user" => {
            let text = value
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(serde_json::Value::as_str)?;
            Some(EventBody::UserMessage(vowp_protocol::UserMessage {
                text: text.to_owned(),
                attachments: Vec::new(),
                note: None,
            }))
        }
        "assistant" => {
            let blocks = value
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(serde_json::Value::as_array)?;
            for block in blocks {
                match block.get("type").and_then(serde_json::Value::as_str) {
                    Some("text") => {
                        if let Some(text) = block.get("text").and_then(serde_json::Value::as_str) {
                            return Some(EventBody::AssistantMessage(vowp_protocol::AssistantMessage {
                                text: text.to_owned(),
                            }));
                        }
                    }
                    Some("tool_use") => {
                        return Some(EventBody::ToolCall(vowp_protocol::ToolCall {
                            call_id: block
                                .get("id")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or_default()
                                .to_owned(),
                            name: block
                                .get("name")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or_default()
                                .to_owned(),
                            input: block.get("input").cloned().unwrap_or(serde_json::Value::Null),
                        }));
                    }
                    _ => {}
                }
            }
            None
        }
        _ => None,
    }
}

/// Prints a session's event log as JSONL or pretty lines.
fn tail(ctx: &Ctx, io: &mut dyn IoStreams, args: &SessionTailArgs) -> Result<i32, CliError> {
    let store = ctx.store()?;
    let session = resolve_session(&store, &args.id)?;
    let reader = SessionLog::open_read(ctx.paths(), &session.id).map_err(vowp_core::CoreError::from)?;
    let after = args.from.unwrap_or(0);
    let mut last = after;
    loop {
        let events = reader.read_from(last, None).map_err(vowp_core::CoreError::from)?;
        for event in &events {
            if args.pretty {
                writeln!(io.out(), "{}", pretty_event(event, ctx.tz_offset_minutes()))?;
            } else {
                writeln!(
                    io.out(),
                    "{}",
                    serde_json::to_string(event).map_err(vowp_protocol::ProtocolError::from)?
                )?;
            }
            last = last.max(event.seq);
        }
        io.out().flush()?;
        if !args.follow {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(0)
}

/// Formats one event as `HH:MM:SS #seq type  summary`.
fn pretty_event(event: &EventEnvelope, offset_minutes: i32) -> String {
    let summary = event
        .body()
        .map(|body| body.summary())
        .unwrap_or_else(|_| event.payload.to_string());
    let summary = summary.lines().next().unwrap_or_default();
    let summary: String = if summary.chars().count() > 160 {
        let mut clipped: String = summary.chars().take(159).collect();
        clipped.push('…');
        clipped
    } else {
        summary.to_owned()
    };
    format!(
        "{} #{} {}  {}",
        clock_time(event.ts, offset_minutes),
        event.seq,
        event.event_type.as_str(),
        summary
    )
}

/// Formats a timestamp as `HH:MM:SS` in a fixed offset.
fn clock_time(ts: Timestamp, offset_minutes: i32) -> String {
    let Ok(base) = OffsetDateTime::from_unix_timestamp(ts.unix_secs()) else {
        return ts.as_rfc3339();
    };
    let Ok(offset) = time::UtcOffset::from_whole_seconds(offset_minutes.saturating_mul(60)) else {
        return ts.as_rfc3339();
    };
    base.to_offset(offset)
        .format(format_description!("[hour]:[minute]:[second]"))
        .unwrap_or_else(|_| ts.as_rfc3339())
}

/// Returns the last indexed event of a session.
fn last_event(store: &Store, session: &Session) -> Option<vowp_protocol::EventIndexRow> {
    if session.last_seq == 0 {
        return None;
    }
    store
        .event_rows(&session.id, session.last_seq.saturating_sub(1), 1)
        .ok()
        .and_then(|mut rows| rows.pop())
}

/// Resolves a session id or unique prefix.
fn resolve_session(store: &Store, input: &str) -> Result<Session, CliError> {
    if let Ok(id) = SessionId::parse(input)
        && let Some(session) = store.session(&id)?
    {
        return Ok(session);
    }
    if input.len() < 6 {
        return Err(CliError::Invalid(
            "session id prefix must be at least 6 characters".to_owned(),
        ));
    }
    let prefix = input.to_ascii_uppercase();
    let matches: Vec<Session> = store
        .list_sessions(&SessionFilter::default())?
        .into_iter()
        .filter(|session| session.id.as_str().to_ascii_uppercase().starts_with(&prefix))
        .collect();
    match matches.as_slice() {
        [session] => Ok(session.clone()),
        [] => Err(CliError::NotFound(format!("session {input}"))),
        many => Err(CliError::Invalid(format!(
            "ambiguous session prefix {input}: {}",
            many.iter()
                .map(|session| session.id.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// Parses a session state name.
fn parse_state(value: &str) -> Result<SessionState, CliError> {
    match value {
        "idle" => Ok(SessionState::Idle),
        "running" => Ok(SessionState::Running),
        "blocked" => Ok(SessionState::Blocked),
        "switching" => Ok(SessionState::Switching),
        "closed" => Ok(SessionState::Closed),
        other => Err(CliError::Invalid(format!("unknown session state: {other}"))),
    }
}

/// Returns the wire name of a session state.
fn state_name(state: SessionState) -> &'static str {
    match state {
        SessionState::Idle => "idle",
        SessionState::Running => "running",
        SessionState::Blocked => "blocked",
        SessionState::Switching => "switching",
        SessionState::Closed => "closed",
    }
}

/// Parses a switch reason.
fn parse_reason(value: Option<&str>) -> Result<SwitchReason, CliError> {
    match value.unwrap_or("user") {
        "user" => Ok(SwitchReason::User),
        "limit" => Ok(SwitchReason::Limit),
        "crash" => Ok(SwitchReason::Crash),
        "context" => Ok(SwitchReason::Context),
        "failover" => Ok(SwitchReason::Failover),
        "probe_back" | "probe-back" => Ok(SwitchReason::ProbeBack),
        other => Err(CliError::Invalid(format!("unknown switch reason: {other}"))),
    }
}
