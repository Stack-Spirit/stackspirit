//! Transcript mirror and resume-context tests. Hermetic: a temporary node home, a fake
//! clock, a file-backed fake host and a loopback HTTP fake for the opencode API.
//!
//! Spec: `docs/design/v0.2/home-and-surfaces.md` §7.5 and the F5 named tests in §15.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{Value, json};
mod support;

use support::fake_git::FakeGit;
use tempfile::{TempDir, tempdir};
use vowp_core::session::importer::{import_file, scan_file};
use vowp_core::session::service::SessionService;
use vowp_core::surface::mirror::{MirrorCursor, mirror_from_host, resume_context};
use vowp_core::surface::native::{
    AppHandle, AppHost, HostHealth, HostRequest, TranscriptBatch, TranscriptEntry, TranscriptKind,
};
use vowp_core::surface::new_lease_token;
use vowp_core::surface::opencode_host::{OpencodeHost, entries_from_api_event, parse_message_list};
use vowp_core::{AdapterRegistry, Deps};
use vowp_protocol::{
    Actor, AssistantMessage, CAPSULE_VERSION, Capsule, CapsuleId, CapsuleState, CapsuleTelemetry, CapsuleTrigger,
    EventBody, EventSource, EventType, Executor, ExecutorRef, FakeClock, HarnessKind, Lease, MemoryLog, NewSession,
    NodeName, OrchestratorApp, SessionId, SessionRole, SurfaceKind, Timestamp, TurnEnded, TurnStarted, UserMessage,
};
use vowp_store::session_log::{NewEvent, SessionLog, index_row};
use vowp_store::{NodePaths, Store};
use vowp_tmux::fake::FakeWindows;

fn ts() -> Timestamp {
    Timestamp::parse("2026-11-10T12:00:00Z").expect("test timestamp")
}

fn local() -> NodeName {
    NodeName::parse("local").expect("node name")
}

fn config_for(tmp: &Path) -> vowp_protocol::VowpConfig {
    let mut config = vowp_protocol::VowpConfig::with_defaults_for_tests();
    let project = tmp.join("project");
    std::fs::create_dir_all(&project).expect("project dir");
    config.project.path = Some(project.to_string_lossy().into_owned());
    config
}

fn deps(tmp: &TempDir, clock: &FakeClock) -> Deps {
    let paths = NodePaths::new(tmp.path().join("node"));
    paths.ensure().expect("node paths");
    Deps {
        paths,
        config: Arc::new(config_for(tmp.path())),
        node: local(),
        clock: Arc::new(clock.clone()),
        log: Arc::new(MemoryLog::new()),
        adapters: Arc::new(AdapterRegistry::new()),
        windows: Arc::new(FakeWindows::new()),
        git: Arc::new(FakeGit::new()),
        cmd: Arc::new(vowp_core::SystemRunner),
        rng: Arc::new(vowp_core::FixedRng(0)),
    }
}

fn create_session(deps: &Deps, tmp: &TempDir) -> SessionId {
    SessionService::new(deps.clone())
        .create_session(NewSession {
            project: None,
            job_id: None,
            role: SessionRole::Orchestrator,
            title: "mirror".to_owned(),
            cwd: tmp.path().join("project"),
            parent: None,
            executor: None,
        })
        .expect("create session")
}

fn append(deps: &Deps, store: &Store, session: &SessionId, body: EventBody) {
    let log = SessionLog::open(&deps.paths, session, deps.clock.clone()).expect("open log");
    let appended = log
        .append(NewEvent {
            actor: Actor::System,
            source: EventSource {
                harness: HarnessKind::Vowp,
                model: None,
                node: Some(local()),
                native_session_id: None,
            },
            body,
            raw: None,
            ts: None,
        })
        .expect("append");
    store.index_event(&index_row(&appended)).expect("index");
    log.sync().expect("sync");
}

fn event_count(deps: &Deps, session: &SessionId) -> usize {
    SessionLog::open_read(&deps.paths, session)
        .expect("read log")
        .read_all()
        .expect("read all")
        .len()
}

fn empty_cursor() -> MirrorCursor {
    MirrorCursor {
        last_native_id: None,
        updated_at: ts(),
    }
}

fn entry(native_id: &str, kind: TranscriptKind, text: Option<&str>) -> TranscriptEntry {
    TranscriptEntry {
        native_id: native_id.to_owned(),
        kind,
        ts: ts(),
        text: text.map(str::to_owned),
        tool: None,
        input: None,
        output: None,
        path: None,
        call_id: None,
        model: Some("fake-model".to_owned()),
        native_session: Some("ses_app".to_owned()),
        ok: None,
        tokens: None,
        usd: None,
    }
}

/// A file-free fake app host: tests hand it transcript entries.
struct FakeHost {
    kind: OrchestratorApp,
    entries: Mutex<Vec<TranscriptEntry>>,
}

impl FakeHost {
    fn new(kind: OrchestratorApp, entries: Vec<TranscriptEntry>) -> Self {
        Self {
            kind,
            entries: Mutex::new(entries),
        }
    }

    fn push(&self, entry: TranscriptEntry) {
        self.entries.lock().expect("entries").push(entry);
    }
}

impl AppHost for FakeHost {
    fn kind(&self) -> OrchestratorApp {
        self.kind
    }

    fn start(&self, req: &HostRequest) -> Result<AppHandle, vowp_core::CoreError> {
        Ok(AppHandle {
            kind: self.kind,
            pid: 1,
            port: None,
            started_at: ts(),
            workdir: req.workdir.clone(),
        })
    }

    fn stop(&self, _handle: &AppHandle) -> Result<(), vowp_core::CoreError> {
        Ok(())
    }

    fn health(&self, _handle: &AppHandle) -> Result<HostHealth, vowp_core::CoreError> {
        Ok(HostHealth {
            ok: true,
            detail: "fake".to_owned(),
        })
    }

    fn join_instructions(&self, _handle: &AppHandle) -> Result<vowp_protocol::JoinInstructions, vowp_core::CoreError> {
        Ok(vowp_protocol::JoinInstructions {
            desktop: "desktop".to_owned(),
            phone: "phone".to_owned(),
            chat: "chat".to_owned(),
        })
    }

    fn transcript_since(
        &self,
        _handle: &AppHandle,
        cursor: &MirrorCursor,
    ) -> Result<TranscriptBatch, vowp_core::CoreError> {
        let entries = self.entries.lock().expect("entries").clone();
        let entries = match cursor.last_native_id.as_deref() {
            None => entries,
            Some(last) => match entries.iter().position(|entry| entry.native_id == last) {
                Some(position) => entries.into_iter().skip(position + 1).collect(),
                None => entries,
            },
        };
        let last = entries
            .last()
            .map(|entry| entry.native_id.clone())
            .or_else(|| cursor.last_native_id.clone());
        Ok(TranscriptBatch {
            entries,
            cursor: MirrorCursor {
                last_native_id: last,
                updated_at: ts(),
            },
        })
    }
}

fn handle(kind: OrchestratorApp, workdir: &Path) -> AppHandle {
    AppHandle {
        kind,
        pid: 1,
        port: None,
        started_at: ts(),
        workdir: workdir.to_path_buf(),
    }
}

#[test]
fn opencode_mirror_maps_messages_to_events() {
    let tmp = tempdir().expect("temp dir");
    let clock = FakeClock::at(ts());
    let deps = deps(&tmp, &clock);
    let store = deps.open_store().expect("store");
    let session = create_session(&deps, &tmp);
    let export = json!({
        "info": {"id": "ses_app"},
        "messages": [
            {
                "id": "m1", "type": "user", "sessionID": "ses_app", "content": "hello",
                "time": {"created": 1_700_000_000_000_i64}
            },
            {
                "id": "m2", "type": "assistant", "sessionID": "ses_app",
                "content": [
                    {"type": "text", "text": "hi"},
                    {"type": "tool_use", "id": "call-1", "name": "read", "input": {"path": "a.rs"}}
                ],
                "model": {"modelID": "fake-model"},
                "usage": {"input": 3, "output": 4, "cost": 0.25},
                "time": {"created": 1_700_000_001_000_i64}
            },
            {
                "id": "m3", "type": "user", "sessionID": "ses_app",
                "content": [{"type": "tool_result", "tool_use_id": "call-1", "content": "done"}]
            }
        ]
    });
    let entries = parse_message_list(&export);
    assert!(entries.len() >= 4, "{entries:?}");
    let host = FakeHost::new(OrchestratorApp::Opencode, entries);
    let handle = handle(OrchestratorApp::Opencode, tmp.path());
    let mut cursor = empty_cursor();
    let report = mirror_from_host(&deps, &store, &session, &host, &handle, &mut cursor).expect("mirror");
    assert_eq!(report.events, 5, "user, assistant text, tool call, tool result, cost");

    let events = SessionLog::open_read(&deps.paths, &session)
        .expect("read log")
        .read_all()
        .expect("read all");
    let bodies: Vec<EventType> = events.iter().map(|event| event.event_type).collect();
    assert!(bodies.contains(&EventType::UserMessage));
    assert!(bodies.contains(&EventType::AssistantMessage));
    assert!(bodies.contains(&EventType::ToolCall));
    assert!(bodies.contains(&EventType::ToolResult));
    assert!(bodies.contains(&EventType::Cost));
    let tool_call = events
        .iter()
        .find(|event| event.event_type == EventType::ToolCall)
        .expect("tool call event");
    let EventBody::ToolCall(call) = tool_call.body().expect("tool call body") else {
        panic!("wrong body");
    };
    assert_eq!(call.call_id, "call-1");
    assert_eq!(call.name, "read");
    assert_eq!(call.input, json!({"path": "a.rs"}));
    let tool_result = events
        .iter()
        .find(|event| event.event_type == EventType::ToolResult)
        .expect("tool result event");
    let EventBody::ToolResult(result) = tool_result.body().expect("tool result body") else {
        panic!("wrong body");
    };
    assert_eq!(result.call_id, "call-1");
    assert!(result.ok);
    assert_eq!(result.output.as_deref(), Some("done"));
    let cost = events
        .iter()
        .find(|event| event.event_type == EventType::Cost)
        .expect("cost event");
    let EventBody::Cost(cost) = cost.body().expect("cost body") else {
        panic!("wrong body");
    };
    assert_eq!(cost.cost.usd, Some(0.25));
    assert_eq!(cost.cost.tokens.input, 3);
    assert_eq!(cost.cost.tokens.output, 4);
    assert!(
        events
            .iter()
            .skip(1)
            .all(|event| event.source.harness == HarnessKind::Opencode)
    );
}

#[test]
fn mirror_reads_api_event_and_backfills_export() {
    let tmp = tempdir().expect("temp dir");
    let server = MiniOpencode::start();
    let workdir = tmp.path().join("orchestrator");
    std::fs::create_dir_all(&workdir).expect("orchestrator dir");
    write_state(&workdir, &server.url());
    server.push_event(json!({
        "type": "session.execution.started",
        "created": 1_700_000_003_000_i64,
        "data": {"sessionID": "ses_fake", "seq": 1}
    }));
    server.push_event(json!({
        "type": "session.text.ended",
        "created": 1_700_000_004_000_i64,
        "data": {"sessionID": "ses_fake", "assistantMessageID": "m_live", "text": "live"}
    }));
    server.push_event(json!({
        "type": "session.execution.succeeded",
        "created": 1_700_000_005_000_i64,
        "data": {"sessionID": "ses_fake", "seq": 2}
    }));

    let clock = FakeClock::at(ts());
    let deps = deps(&tmp, &clock);
    let store = deps.open_store().expect("store");
    let session = create_session(&deps, &tmp);
    let host = OpencodeHost::default();
    let handle = AppHandle {
        kind: OrchestratorApp::Opencode,
        pid: 4242,
        port: Some(server.port()),
        started_at: ts(),
        workdir: workdir.clone(),
    };
    let mut cursor = empty_cursor();
    let first = mirror_from_host(&deps, &store, &session, &host, &handle, &mut cursor).expect("first mirror");
    // Two backfilled messages plus the three live events.
    assert_eq!(first.events, 5, "{:?}", first);
    let events = SessionLog::open_read(&deps.paths, &session)
        .expect("read log")
        .read_all()
        .expect("read all");
    let kinds: Vec<EventType> = events.iter().map(|event| event.event_type).collect();
    assert!(kinds.contains(&EventType::UserMessage));
    assert!(kinds.contains(&EventType::AssistantMessage));
    assert!(kinds.contains(&EventType::TurnStarted));
    assert!(kinds.contains(&EventType::TurnEnded));
    assert!(
        server
            .seen()
            .iter()
            .any(|path| path == "/api/experimental/session/ses_fake/export"),
        "backfill did not read the export: {:?}",
        server.seen()
    );
    assert!(
        server.seen().iter().any(|path| path == "/api/event"),
        "live pass did not read /api/event: {:?}",
        server.seen()
    );

    server.push_event(json!({
        "type": "session.text.ended",
        "created": 1_700_000_006_000_i64,
        "data": {"sessionID": "ses_fake", "assistantMessageID": "m_live_2", "text": "later"}
    }));
    let second = mirror_from_host(&deps, &store, &session, &host, &handle, &mut cursor).expect("second mirror");
    assert_eq!(second.events, 1, "{:?}", second);
    let events = SessionLog::open_read(&deps.paths, &session)
        .expect("read log")
        .read_all()
        .expect("read all");
    assert!(
        events
            .iter()
            .any(|event| matches!(event.body(), Ok(EventBody::AssistantMessage(message)) if message.text == "later")),
        "the later live message was not appended"
    );
}

#[test]
fn opencode_api_event_mapping_covers_tools_cost_and_errors() {
    let user = json!({
        "type": "session.inbox.enqueued",
        "created": 1_700_000_000_000_i64,
        "data": {
            "sessionID": "ses_app", "seq": 1,
            "item": {"id": "item_1", "type": "user", "payload": {"text": "go"}}
        }
    });
    let entries = entries_from_api_event(&user);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind, TranscriptKind::UserMessage);
    assert_eq!(entries[0].text.as_deref(), Some("go"));

    let tool = json!({
        "type": "session.tool.called",
        "created": 1_700_000_001_000_i64,
        "data": {"sessionID": "ses_app", "seq": 2, "id": "call_1", "name": "read", "input": {"path": "a"}}
    });
    let entries = entries_from_api_event(&tool);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind, TranscriptKind::ToolCall);
    assert_eq!(entries[0].call_id.as_deref(), Some("call_1"));

    let failed = json!({
        "type": "session.tool.failed",
        "created": 1_700_000_002_000_i64,
        "data": {"sessionID": "ses_app", "seq": 3, "id": "call_1", "error": {"message": "boom"}}
    });
    let entries = entries_from_api_event(&failed);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind, TranscriptKind::ToolResult);
    assert_eq!(entries[0].ok, Some(false));
    assert_eq!(entries[0].output.as_deref(), Some("boom"));

    let usage = json!({
        "type": "session.usage.updated",
        "created": 1_700_000_003_000_i64,
        "data": {"sessionID": "ses_app", "tokens": {"input": 7, "output": 2}, "cost": 0.5}
    });
    let entries = entries_from_api_event(&usage);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind, TranscriptKind::Cost);
    assert_eq!(entries[0].tokens.expect("tokens").input, 7);
    assert_eq!(entries[0].usd, Some(0.5));

    let execution = json!({
        "type": "session.execution.failed",
        "created": 1_700_000_004_000_i64,
        "data": {"sessionID": "ses_app", "seq": 4, "error": {"message": "model failed"}}
    });
    let entries = entries_from_api_event(&execution);
    assert_eq!(entries.len(), 2, "{entries:?}");
    assert_eq!(entries[0].kind, TranscriptKind::Error);
    assert_eq!(entries[1].kind, TranscriptKind::TurnEnded);
    assert_eq!(entries[1].text.as_deref(), Some("failed"));

    let streaming = json!({
        "type": "session.text.delta",
        "created": 1_700_000_005_000_i64,
        "data": {"sessionID": "ses_app", "text": "partial"}
    });
    assert!(entries_from_api_event(&streaming).is_empty());

    let selected = json!({
        "type": "session.model.selected",
        "created": 1_700_000_006_000_i64,
        "data": {"sessionID": "ses_app", "seq": 5, "model": {"modelID": "other-model"}}
    });
    let entries = entries_from_api_event(&selected);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind, TranscriptKind::Note);
    assert!(entries[0].text.as_deref().unwrap_or_default().contains("other-model"));
}

#[test]
fn mirror_is_incremental_and_idempotent() {
    let tmp = tempdir().expect("temp dir");
    let clock = FakeClock::at(ts());
    let deps = deps(&tmp, &clock);
    let store = deps.open_store().expect("store");
    let session = create_session(&deps, &tmp);
    let host = FakeHost::new(
        OrchestratorApp::Opencode,
        vec![
            entry("u1", TranscriptKind::UserMessage, Some("hello")),
            entry("a1", TranscriptKind::AssistantMessage, Some("hi")),
        ],
    );
    let handle = handle(OrchestratorApp::Opencode, tmp.path());
    let mut cursor = empty_cursor();
    let first = mirror_from_host(&deps, &store, &session, &host, &handle, &mut cursor).expect("first");
    assert_eq!(first.events, 2);
    assert_eq!(first.duplicates, 0);
    let after_first = event_count(&deps, &session);
    assert_eq!(after_first, 3, "session.created plus two entries");

    // A caller that lost its cursor re-reads the same entries; the native map skips them.
    let mut fresh = empty_cursor();
    let second = mirror_from_host(&deps, &store, &session, &host, &handle, &mut fresh).expect("second");
    assert_eq!(second.events, 0);
    assert_eq!(second.duplicates, 2);
    assert_eq!(event_count(&deps, &session), after_first);
    assert_eq!(
        store
            .native_event(&session, HarnessKind::Opencode, "a1")
            .expect("native event"),
        Some(
            SessionLog::open_read(&deps.paths, &session)
                .expect("read")
                .read_all()
                .expect("read all")[2]
                .id
                .clone()
        )
    );

    host.push(entry("a2", TranscriptKind::AssistantMessage, Some("again")));
    let mut third_cursor = first.cursor.clone();
    let third = mirror_from_host(&deps, &store, &session, &host, &handle, &mut third_cursor).expect("third");
    assert_eq!(third.events, 1);
    assert_eq!(event_count(&deps, &session), after_first + 1);
    assert_eq!(third.cursor.last_native_id.as_deref(), Some("a2"));
}

#[test]
fn claude_importer_maps_transcript() {
    let tmp = tempdir().expect("temp dir");
    let transcript = tmp.path().join("session.jsonl");
    let lines = [
        json!({
            "type": "user", "uuid": "u1", "sessionId": "claude-session",
            "timestamp": "2026-11-10T10:00:00Z",
            "message": {"role": "user", "content": "hello"}
        }),
        json!({
            "type": "assistant", "uuid": "a1", "sessionId": "claude-session",
            "timestamp": "2026-11-10T10:00:01Z",
            "message": {
                "role": "assistant", "model": "claude-sonnet-5",
                "content": [
                    {"type": "thinking", "thinking": "considering"},
                    {"type": "text", "text": "hi"},
                    {"type": "tool_use", "id": "toolu_1", "name": "Bash", "input": {"command": "ls"}}
                ],
                "usage": {"input_tokens": 4, "output_tokens": 2}
            }
        }),
        json!({
            "type": "user", "uuid": "u2", "sessionId": "claude-session",
            "timestamp": "2026-11-10T10:00:02Z",
            "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_1", "content": "ok"}
            ]}
        }),
        json!({"type": "summary", "summary": "not session content"}),
        json!("not an object"),
    ];
    let body: String = lines.iter().map(|line| format!("{line}\n")).collect();
    std::fs::write(&transcript, &body).expect("write transcript");

    let entries = scan_file(&transcript).expect("scan");
    assert_eq!(entries.len(), 6, "{entries:?}");
    assert_eq!(entries[0].kind, TranscriptKind::UserMessage);
    assert_eq!(entries[0].text.as_deref(), Some("hello"));
    assert_eq!(entries[1].kind, TranscriptKind::Reasoning);
    assert_eq!(entries[2].kind, TranscriptKind::AssistantMessage);
    assert_eq!(entries[3].kind, TranscriptKind::ToolCall);
    assert_eq!(entries[3].call_id.as_deref(), Some("toolu_1"));
    assert_eq!(entries[3].tool.as_deref(), Some("Bash"));
    assert_eq!(entries[4].kind, TranscriptKind::Cost);
    assert_eq!(entries[4].tokens.expect("tokens").input, 4);
    assert_eq!(entries[5].kind, TranscriptKind::ToolResult);
    assert_eq!(entries[5].call_id.as_deref(), Some("toolu_1"));
    assert_eq!(entries[5].output.as_deref(), Some("ok"));
    assert!(
        entries
            .iter()
            .all(|entry| entry.native_session.as_deref() == Some("claude-session"))
    );

    // The offset reader sees only complete new lines and is read-only.
    let mut cursor = vowp_core::session::importer::ImportCursor::default();
    let first = import_file(&transcript, &mut cursor).expect("first import");
    assert_eq!(first.entries.len(), 6);
    let second = import_file(&transcript, &mut cursor).expect("second import");
    assert!(second.entries.is_empty());
    std::fs::OpenOptions::new()
        .append(true)
        .open(&transcript)
        .expect("open transcript")
        .write_all(format!("{}\n", lines[0]).as_bytes())
        .expect("append transcript");
    let third = import_file(&transcript, &mut cursor).expect("third import");
    assert_eq!(third.entries.len(), 1);
    assert_eq!(third.entries[0].text.as_deref(), Some("hello"));
}

#[test]
fn resume_context_builds_capsule_and_tail() {
    let tmp = tempdir().expect("temp dir");
    let clock = FakeClock::at(ts());
    let deps = deps(&tmp, &clock);
    let store = deps.open_store().expect("store");
    let session = create_session(&deps, &tmp);
    append(&deps, &store, &session, EventBody::TurnStarted(TurnStarted { turn: 1 }));
    append(
        &deps,
        &store,
        &session,
        EventBody::UserMessage(UserMessage {
            text: "one".to_owned(),
            attachments: Vec::new(),
            note: None,
        }),
    );
    append(
        &deps,
        &store,
        &session,
        EventBody::AssistantMessage(AssistantMessage {
            text: "one answer".to_owned(),
        }),
    );
    append(
        &deps,
        &store,
        &session,
        EventBody::TurnEnded(TurnEnded {
            turn: 1,
            stop_reason: vowp_protocol::StopReason::EndTurn,
            duration_ms: None,
        }),
    );
    append(&deps, &store, &session, EventBody::TurnStarted(TurnStarted { turn: 2 }));
    append(
        &deps,
        &store,
        &session,
        EventBody::UserMessage(UserMessage {
            text: "two".to_owned(),
            attachments: Vec::new(),
            note: None,
        }),
    );
    append(
        &deps,
        &store,
        &session,
        EventBody::AssistantMessage(AssistantMessage {
            text: "two answer".to_owned(),
        }),
    );
    append(
        &deps,
        &store,
        &session,
        EventBody::TurnEnded(TurnEnded {
            turn: 2,
            stop_reason: vowp_protocol::StopReason::EndTurn,
            duration_ms: None,
        }),
    );

    let capsule = Capsule {
        version: CAPSULE_VERSION,
        capsule_id: CapsuleId::new(),
        session_id: session.clone(),
        created_at: ts(),
        at_seq: 5,
        trigger: CapsuleTrigger::Manual,
        from: ExecutorRef {
            executor: Executor::parse("opencode:deepseek/deepseek-flash#max").expect("executor"),
            node: None,
        },
        intent: "continue the orchestrator work".to_owned(),
        constraints: Vec::new(),
        decisions: Vec::new(),
        state: CapsuleState::default(),
        files: Vec::new(),
        gotchas: Vec::new(),
        open_questions: Vec::new(),
        telemetry: CapsuleTelemetry::default(),
    };
    store.insert_capsule(&capsule, "capsules/test.json").expect("capsule");
    let token = new_lease_token();
    store
        .take_lease(&Lease {
            session_id: session.clone(),
            surface_id: None,
            surface_kind: SurfaceKind::Mirror,
            node: local(),
            holder: "test".to_owned(),
            token: token.clone(),
            acquired_at: ts(),
            heartbeat_at: ts(),
        })
        .expect("lease");

    let context = resume_context(&deps, &store, &session, 1).expect("resume context");
    assert_eq!(context.session.id, session);
    assert_eq!(
        context.capsule.as_ref().map(|capsule| capsule.capsule_id.clone()),
        Some(capsule.capsule_id)
    );
    assert_eq!(context.lease.as_ref().map(|lease| lease.token.clone()), Some(token));
    assert_eq!(context.tail.len(), 4, "{:?}", context.tail);
    assert_eq!(context.tail[0].event_type, EventType::TurnStarted);
    let EventBody::TurnStarted(start) = context.tail[0].body().expect("turn body") else {
        panic!("wrong first event");
    };
    assert_eq!(start.turn, 2);
    assert!(
        context.tail.iter().any(
            |event| matches!(event.body(), Ok(EventBody::AssistantMessage(message)) if message.text == "two answer")
        )
    );
    assert!(
        !context.tail.iter().any(
            |event| matches!(event.body(), Ok(EventBody::AssistantMessage(message)) if message.text == "one answer")
        )
    );

    let none = resume_context(&deps, &store, &session, 0).expect("zero turns");
    assert!(none.tail.is_empty());
    let all = resume_context(&deps, &store, &session, 10).expect("all turns");
    assert!(all.tail.len() >= 8, "{:?}", all.tail);
}

/// A loopback opencode fake for the mirror tests.
struct MiniOpencode {
    addr: SocketAddr,
    seen: Arc<Mutex<Vec<String>>>,
    events: Arc<Mutex<Vec<Value>>>,
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl MiniOpencode {
    fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mini opencode");
        listener.set_nonblocking(true).expect("nonblocking mini opencode");
        let addr = listener.local_addr().expect("mini address");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let events = Arc::new(Mutex::new(Vec::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let thread = {
            let seen = Arc::clone(&seen);
            let events = Arc::clone(&events);
            let shutdown = Arc::clone(&shutdown);
            thread::spawn(move || {
                while !shutdown.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => mini_handle(stream, &seen, &events),
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(_) => break,
                    }
                }
            })
        };
        Self {
            addr,
            seen,
            events,
            shutdown,
            thread: Some(thread),
        }
    }

    fn port(&self) -> u16 {
        self.addr.port()
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port())
    }

    fn push_event(&self, event: Value) {
        self.events.lock().expect("events").push(event);
    }

    fn seen(&self) -> Vec<String> {
        self.seen.lock().expect("seen").clone()
    }
}

impl Drop for MiniOpencode {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.addr);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn mini_handle(mut stream: TcpStream, seen: &Arc<Mutex<Vec<String>>>, events: &Arc<Mutex<Vec<Value>>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut raw = Vec::new();
    let mut chunk = [0u8; 2048];
    while let Ok(read) = stream.read(&mut chunk) {
        if read == 0 {
            break;
        }
        raw.extend_from_slice(&chunk[..read]);
        if raw.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let text = String::from_utf8_lossy(&raw);
    let path = text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or_default()
        .to_owned();
    seen.lock().expect("seen").push(path.clone());
    let route = path.split('?').next().unwrap_or(&path);
    let (content_type, body) = match route {
        "/api/session" => ("application/json", json!({"data": [{"id": "ses_fake"}]}).to_string()),
        "/api/experimental/session/ses_fake/export" => (
            "application/json",
            json!({
                "info": {"id": "ses_fake"},
                "messages": [
                    {
                        "id": "m_user", "type": "user", "sessionID": "ses_fake", "content": "question",
                        "time": {"created": 1_700_000_000_000_i64}
                    },
                    {
                        "id": "m_answer", "type": "assistant", "sessionID": "ses_fake",
                        "content": [{"type": "text", "text": "answer"}],
                        "time": {"created": 1_700_000_001_000_i64}
                    }
                ]
            })
            .to_string(),
        ),
        "/api/event" => {
            let mut body = String::new();
            for event in events.lock().expect("events").iter() {
                body.push_str("data: ");
                body.push_str(&event.to_string());
                body.push_str("\n\n");
            }
            ("text/event-stream", body)
        }
        _ => ("application/json", "{\"error\":\"not found\"}".to_owned()),
    };
    let status = if route.starts_with("/api/") { 200 } else { 404 };
    let response = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn write_state(workdir: &Path, url: &str) {
    let state = workdir.join(".opencode").join("state").join("opencode");
    std::fs::create_dir_all(&state).expect("state dir");
    std::fs::write(
        state.join("service.json"),
        json!({"id": "fake", "url": url, "pid": 4242, "password": "password"}).to_string(),
    )
    .expect("service.json");
    std::fs::write(
        workdir.join("host.json"),
        json!({
            "kind": "opencode",
            "name": "orch",
            "pid": 4242,
            "port": 1,
            "url": url,
            "started_at": "2026-11-10T12:00:00Z",
            "workdir": workdir.display().to_string(),
            "service_json": state.join("service.json").display().to_string(),
            "native_session_id": "ses_fake",
            "vowp_session": "ses_vowp"
        })
        .to_string(),
    )
    .expect("host.json");
}
