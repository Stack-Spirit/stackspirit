//! Switch protocol tests: happy path, probe failure, drain timeout, seed rejection and
//! the claude → opencode → claude failover round trip. Hermetic: temp node, fake clock,
//! fake adapters; no harness process, no real HOME.
//! Spec: `docs/design/sessions.md` §5 and `docs/design/crates/vowp-core.md` §3.

mod support;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use support::fake_adapter::FakeAdapter;
use support::fake_git::FakeGit;
use tempfile::{TempDir, tempdir};
use vowp_core::session::service::SessionService;
use vowp_core::session::switch::{run_switch, run_switch_with_input};
use vowp_core::{AdapterRegistry, CoreError, Deps};
use vowp_protocol::{
    Actor, AssistantMessage, AttachAction, CancelToken, Clock, Cost, EventBody, EventEnvelope, EventSource, EventType,
    Executor, ExecutorRef, FakeClock, HarnessKind, MemoryLog, NodeName, ProbeResult, Run, RunId, RunOutcome,
    RunPurpose, RunRole, SessionId, SessionRole, StopReason, SwitchReason, SwitchRequest, SwitchState, Timestamp,
    TurnRequest,
};
use vowp_store::NodePaths;
use vowp_store::session_log::{NewEvent, SessionLog, index_row};
use vowp_tmux::fake::FakeWindows;

fn ts() -> Timestamp {
    Timestamp::parse("2026-11-08T18:00:00Z").expect("test timestamp")
}

fn opencode() -> Executor {
    Executor::parse("opencode:deepseek/deepseek-flash#max").expect("opencode executor")
}

fn claude() -> Executor {
    Executor::parse("claude-code:claude-sonnet-5-5@medium").expect("claude executor")
}

fn node() -> NodeName {
    NodeName::parse("local").expect("node name")
}

fn config_for(tmp: &Path) -> vowp_protocol::VowpConfig {
    let mut config = vowp_protocol::VowpConfig::with_defaults_for_tests();
    let project = tmp.join("project");
    std::fs::create_dir_all(&project).expect("project dir");
    config.project.path = Some(project.to_string_lossy().into_owned());
    config
}

fn deps_with_clock(tmp: &TempDir, registry: AdapterRegistry, clock: FakeClock) -> Deps {
    let paths = NodePaths::new(tmp.path().join("node"));
    paths.ensure().expect("node paths");
    Deps {
        paths,
        config: Arc::new(config_for(tmp.path())),
        node: node(),
        clock: Arc::new(clock),
        log: Arc::new(MemoryLog::new()),
        adapters: Arc::new(registry),
        windows: Arc::new(FakeWindows::new()),
        git: Arc::new(FakeGit::new()),
        cmd: Arc::new(vowp_core::SystemRunner),
        rng: Arc::new(vowp_core::FixedRng(0)),
    }
}

fn deps(tmp: &TempDir, registry: AdapterRegistry) -> Deps {
    deps_with_clock(tmp, registry, FakeClock::at(ts()))
}

fn draft_json(intent: &str) -> String {
    serde_json::json!({
        "intent": intent,
        "constraints": [],
        "decisions": [],
        "state": {"done": ["work"], "in_progress": [], "blocked": [], "next_steps": ["continue"]},
        "files": [],
        "gotchas": [],
        "open_questions": []
    })
    .to_string()
}

fn capsule_adapter(intent: &str) -> FakeAdapter {
    FakeAdapter::from_bodies(
        HarnessKind::Opencode,
        ts(),
        vec![EventBody::AssistantMessage(AssistantMessage {
            text: draft_json(intent),
        })],
    )
}

fn create_session(service: &SessionService, tmp: &TempDir, name: &str) -> SessionId {
    service
        .create_session(vowp_protocol::NewSession {
            project: None,
            job_id: None,
            role: SessionRole::Worker,
            title: name.to_owned(),
            cwd: tmp.path().join("project"),
            parent: None,
            executor: Some(opencode()),
        })
        .expect("create session")
}

fn request(session: &SessionId, to: Executor, reason: SwitchReason) -> SwitchRequest {
    SwitchRequest {
        session_id: session.clone(),
        to: ExecutorRef {
            executor: to,
            node: None,
        },
        reason,
        drain_secs: 5,
    }
}

fn read_events(deps: &Deps, session: &SessionId) -> Vec<EventEnvelope> {
    SessionLog::open_read(&deps.paths, session)
        .expect("open log")
        .read_all()
        .expect("read log")
}

fn turn(session: &SessionId, prompt: &str) -> TurnRequest {
    TurnRequest {
        session_id: session.clone(),
        executor: ExecutorRef {
            executor: opencode(),
            node: None,
        },
        purpose: RunPurpose::Start,
        role: RunRole::Worker,
        job_id: None,
        prompt: prompt.to_owned(),
        profile: Some("worker".to_owned()),
        rung: 0,
        window: None,
        timeout: None,
    }
}

fn open_run(session: &SessionId) -> Run {
    Run {
        id: RunId::new(),
        job_id: None,
        session_id: Some(session.clone()),
        role: RunRole::Worker,
        purpose: RunPurpose::Start,
        profile: None,
        rung: 0,
        executor: Some(opencode()),
        provider: Some("deepseek".to_owned()),
        node: node(),
        native_session_id: None,
        pid: None,
        window: None,
        cwd: None,
        started_at: ts(),
        ended_at: None,
        exit_code: None,
        outcome: None,
        limit_kind: None,
        cost: Cost::unknown(),
        detail: String::new(),
    }
}

#[test]
fn switch_happy_path_events_rows_and_lineage() {
    let tmp = tempdir().expect("tempdir");
    let target = FakeAdapter::new(HarnessKind::ClaudeCode, Vec::new());
    let deps = deps(
        &tmp,
        AdapterRegistry::new()
            .with("opencode", Arc::new(capsule_adapter("Carry the work over")))
            .with("claude-code", Arc::new(target)),
    );
    let service = SessionService::new(deps.clone());
    let session = create_session(&service, &tmp, "switch happy");
    let store = deps.open_store().expect("store");

    let switch = run_switch(
        &deps,
        &request(&session, claude(), SwitchReason::User),
        &CancelToken::default(),
    )
    .expect("switch");

    assert_eq!(switch.state, SwitchState::Completed);
    assert_eq!(switch.from_executor, Some(opencode()));
    assert_eq!(switch.to_executor, claude());
    assert!(switch.capsule_id.is_some(), "the switch carries the capsule");
    assert!(switch.ended_at.is_some());

    let row = store
        .switch(&switch.id)
        .expect("switch row")
        .expect("switch row exists");
    assert_eq!(row.state, SwitchState::Completed);
    assert_eq!(row.reason, SwitchReason::User);
    assert_eq!(row.to_executor, claude());
    assert!(row.ended_at.is_some());

    let current = store.require_session(&session).expect("session");
    assert_eq!(current.executor, Some(claude()));
    assert_eq!(current.state, vowp_protocol::SessionState::Idle);

    let events = read_events(&deps, &session);
    let capsule_at = events
        .iter()
        .position(|event| event.event_type == EventType::Capsule)
        .expect("capsule event");
    let switch_event_at = events
        .iter()
        .position(|event| event.event_type == EventType::Switch)
        .expect("switch event");
    assert!(
        capsule_at < switch_event_at,
        "capsule is recorded before the switch event"
    );

    let detach_at = events
        .iter()
        .position(|event| {
            matches!(
                event.body(),
                Ok(EventBody::SessionExecutor(payload))
                    if payload.action == AttachAction::Detach && payload.executor == opencode()
            )
        })
        .expect("source detach");
    let attach_at = events
        .iter()
        .position(|event| {
            matches!(
                event.body(),
                Ok(EventBody::SessionExecutor(payload))
                    if payload.action == AttachAction::Attach && payload.executor == claude()
            )
        })
        .expect("target attach");
    assert!(detach_at < attach_at);

    let lineage = events
        .iter()
        .find_map(|event| match event.body() {
            Ok(EventBody::Switch(payload)) => Some(payload),
            _ => None,
        })
        .expect("switch payload");
    assert_eq!(lineage.switch_id, switch.id);
    assert_eq!(lineage.reason, SwitchReason::User);
    assert_eq!(lineage.to.executor, claude());
    assert_eq!(lineage.capsule_id, switch.capsule_id);

    assert!(
        events.iter().all(|event| !matches!(
            event.body(),
            Ok(EventBody::UserMessage(payload)) if payload.note.as_deref() == Some("handoff")
        )),
        "a pure switch does not start a turn or deliver a seed"
    );
}

#[test]
fn switch_target_probe_fails_stays_on_source() {
    let tmp = tempdir().expect("tempdir");
    let target = FakeAdapter::new(HarnessKind::ClaudeCode, Vec::new()).with_probe(ProbeResult {
        ok: false,
        version: None,
        path: None,
        detail: "binary missing".to_owned(),
    });
    let deps = deps(
        &tmp,
        AdapterRegistry::new()
            .with("opencode", Arc::new(capsule_adapter("should not be written")))
            .with("claude-code", Arc::new(target)),
    );
    let service = SessionService::new(deps.clone());
    let session = create_session(&service, &tmp, "probe failure");
    let store = deps.open_store().expect("store");
    store
        .set_session_state(&session, vowp_protocol::SessionState::Blocked, Some(&opencode()))
        .expect("blocked");

    let error = run_switch(
        &deps,
        &request(&session, claude(), SwitchReason::Failover),
        &CancelToken::default(),
    )
    .expect_err("probe failure aborts");
    assert!(matches!(error, CoreError::SwitchAborted(_)), "got {error:?}");

    let current = store.require_session(&session).expect("session");
    assert_eq!(current.executor, Some(opencode()), "still on the source");
    assert_eq!(
        current.state,
        vowp_protocol::SessionState::Blocked,
        "blocked stays blocked"
    );

    let row = store
        .list_switches(&session)
        .expect("switches")
        .into_iter()
        .next()
        .expect("row");
    assert_eq!(row.state, SwitchState::Aborted);
    assert!(row.error.as_deref().is_some_and(|error| error.contains("probe")));

    let events = read_events(&deps, &session);
    assert!(
        events.iter().any(|event| matches!(
            event.body(),
            Ok(EventBody::Error(payload)) if payload.kind == vowp_protocol::ErrorKind::AdapterUnavailable
        )),
        "the abort is visible in the log"
    );
    assert!(
        !events.iter().any(|event| event.event_type == EventType::Capsule),
        "no capsule is written when the target cannot run"
    );
    assert!(
        !events.iter().any(|event| matches!(
            event.body(),
            Ok(EventBody::SessionExecutor(payload)) if payload.action == AttachAction::Detach
        )),
        "the source is never detached"
    );
}

#[test]
fn switch_drain_timeout_interrupts_after_5s_fakeclock() {
    let tmp = tempdir().expect("tempdir");
    let clock = FakeClock::at(ts());
    let target = FakeAdapter::new(HarnessKind::ClaudeCode, Vec::new());
    let deps = deps_with_clock(
        &tmp,
        AdapterRegistry::new()
            .with("opencode", Arc::new(capsule_adapter("drained handoff")))
            .with("claude-code", Arc::new(target)),
        clock.clone(),
    );
    let service = SessionService::new(deps.clone());
    let session = create_session(&service, &tmp, "drain timeout");
    let store = deps.open_store().expect("store");

    let run = open_run(&session);
    store.insert_run(&run).expect("run row");
    store
        .set_session_state(&session, vowp_protocol::SessionState::Running, Some(&opencode()))
        .expect("running");
    {
        let log = SessionLog::open(&deps.paths, &session, deps.clock.clone()).expect("log");
        let appended = log
            .append(NewEvent {
                actor: Actor::System,
                source: EventSource {
                    harness: HarnessKind::Opencode,
                    model: None,
                    node: Some(node()),
                    native_session_id: None,
                },
                body: EventBody::TurnStarted(vowp_protocol::TurnStarted { turn: 1 }),
                raw: None,
                ts: None,
            })
            .expect("turn started");
        store.index_event(&index_row(&appended)).expect("index");
        log.sync().expect("sync");
    }

    let advanced = clock.clone();
    let advancer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(60));
        advanced.advance(Duration::from_secs(5));
    });
    let before = clock.now();
    let switch = run_switch(
        &deps,
        &request(&session, claude(), SwitchReason::Failover),
        &CancelToken::default(),
    )
    .expect("drained switch");
    advancer.join().expect("advancer");
    let elapsed = clock.now().since(before);
    assert!(elapsed >= Duration::from_secs(5), "the drain waited for the fake 5 s");
    assert_eq!(switch.state, SwitchState::Completed);

    let finished = store.run(&run.id).expect("run").expect("run row");
    assert_eq!(finished.outcome, Some(RunOutcome::Killed));
    assert!(finished.ended_at.is_some());

    let events = read_events(&deps, &session);
    assert!(
        events.iter().any(|event| matches!(
            event.body(),
            Ok(EventBody::TurnEnded(payload)) if payload.stop_reason == StopReason::Interrupted
        )),
        "the switch appends the missing interrupted turn.ended"
    );
    assert_eq!(
        store.require_session(&session).expect("session").executor,
        Some(claude())
    );
}

#[test]
fn switch_seed_rejected_retries_capsule_only_then_aborts() {
    let tmp = tempdir().expect("tempdir");
    let target = Arc::new(FakeAdapter::new(HarnessKind::ClaudeCode, Vec::new()).with_start_error("seed rejected"));
    let deps = deps(
        &tmp,
        AdapterRegistry::new()
            .with("opencode", Arc::new(capsule_adapter("Seed retry handoff")))
            .with("claude-code", target.clone()),
    );
    let service = SessionService::new(deps.clone());
    let session = create_session(&service, &tmp, "seed rejection");
    service
        .run_turn(turn(&session, "TAIL-UNIQUE-42, keep working"), &CancelToken::default())
        .expect("prior turn");
    let store = deps.open_store().expect("store");

    let error = run_switch_with_input(
        &deps,
        &request(&session, claude(), SwitchReason::Failover),
        "continue the work",
        &CancelToken::default(),
    )
    .expect_err("both seed attempts fail");
    assert!(matches!(error, CoreError::SwitchAborted(_)), "got {error:?}");

    let starts = target.starts();
    assert_eq!(starts.len(), 2, "one seeded attempt then one capsule-only attempt");
    assert!(
        starts[0].prompt.contains("TAIL-UNIQUE-42"),
        "the first seed carries the last turns"
    );
    assert!(
        !starts[1].prompt.contains("TAIL-UNIQUE-42"),
        "the retry is the capsule only"
    );
    assert!(starts[1].prompt.contains("Seed retry handoff"));

    let row = store
        .list_switches(&session)
        .expect("switches")
        .into_iter()
        .next()
        .expect("row");
    assert_eq!(row.state, SwitchState::Aborted);
    assert!(row.error.as_deref().is_some_and(|error| error.contains("twice")));
    assert_eq!(
        store.require_session(&session).expect("session").executor,
        Some(opencode()),
        "the aborted switch re-attaches the source"
    );
    let last_attach = read_events(&deps, &session)
        .into_iter()
        .rev()
        .find_map(|event| match event.body() {
            Ok(EventBody::SessionExecutor(payload)) if payload.action == AttachAction::Attach => Some(payload.executor),
            _ => None,
        })
        .expect("source re-attach");
    assert_eq!(last_attach, opencode());
}

#[test]
fn failover_roundtrip_claude_to_opencode_and_back_preserves_capsule_content() {
    let tmp = tempdir().expect("tempdir");
    let deps = deps(
        &tmp,
        AdapterRegistry::new()
            .with("opencode", Arc::new(capsule_adapter("Preserve this handoff")))
            .with(
                "claude-code",
                Arc::new(FakeAdapter::new(HarnessKind::ClaudeCode, Vec::new())),
            ),
    );
    let service = SessionService::new(deps.clone());
    let session = create_session(&service, &tmp, "failover roundtrip");
    let store = deps.open_store().expect("store");

    let first = run_switch(
        &deps,
        &request(&session, opencode(), SwitchReason::Limit),
        &CancelToken::default(),
    )
    .expect("claude -> opencode");
    let second = run_switch(
        &deps,
        &request(&session, claude(), SwitchReason::ProbeBack),
        &CancelToken::default(),
    )
    .expect("opencode -> claude");

    assert_eq!(first.state, SwitchState::Completed);
    assert_eq!(second.state, SwitchState::Completed);
    assert_eq!(
        store.require_session(&session).expect("session").executor,
        Some(claude())
    );

    let lineage = store.list_switches(&session).expect("lineage");
    assert_eq!(lineage.len(), 2);
    assert_eq!(lineage[0].reason, SwitchReason::Limit);
    assert_eq!(lineage[1].reason, SwitchReason::ProbeBack);

    let capsule = store
        .latest_capsule(&session)
        .expect("capsule")
        .expect("capsule exists");
    assert_eq!(capsule.intent, "Preserve this handoff");
    assert_eq!(
        capsule.from.executor,
        opencode(),
        "the second switch summarises the opencode executor"
    );
    let seed = vowp_core::session::seed::seed_prompt(&capsule, &[], 2, "continue");
    assert!(seed.contains("Preserve this handoff"));
}
