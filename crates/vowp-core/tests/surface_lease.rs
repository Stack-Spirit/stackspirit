//! Surface lease and takeover tests. Hermetic: a temporary node home, a fake clock, fake
//! adapters and no harness process. Spec: `docs/design/v0.2/home-and-surfaces.md` §7.3
//! and the F2 named tests in §15.

mod support;

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use support::fake_adapter::FakeAdapter;
use support::fake_git::FakeGit;
use tempfile::{TempDir, tempdir};
use vowp_core::session::service::SessionService;
use vowp_core::surface::lease::{
    AcquireLease, HEARTBEAT_INTERVAL, LeaseGuard, STALE_AFTER, check_lease_with, clear_revocation, revocation_ack,
    wait_for_revocation,
};
use vowp_core::{AdapterRegistry, CoreError, Deps};
use vowp_protocol::{
    Actor, AssistantMessage, Clock, EventBody, EventSource, EventType, Executor, FakeClock, HarnessKind, IngestAck,
    MemoryLog, NewSession, NodeName, SessionId, SessionRole, SessionState, Surface, SurfaceId, SurfaceKind,
    SwitchReason, Timestamp,
};
use vowp_store::session_log::{NewEvent, SessionLog, index_row};
use vowp_store::{NodePaths, Store, StoreError};
use vowp_tmux::fake::FakeWindows;

fn ts() -> Timestamp {
    Timestamp::parse("2026-11-08T18:00:00Z").expect("test timestamp")
}

fn opencode() -> Executor {
    Executor::parse("opencode:deepseek/deepseek-flash#max").expect("opencode executor")
}

fn local() -> NodeName {
    NodeName::parse("local").expect("node name")
}

fn config_for(tmp: &Path) -> vowp_protocol::VowpConfig {
    let mut config = vowp_protocol::VowpConfig::with_defaults_for_tests();
    let project = tmp.join("project");
    std::fs::create_dir_all(&project).expect("project dir");
    config.project.path = Some(project.to_string_lossy().into_owned());
    config.capsule.executor = Some(opencode());
    config
}

fn deps_with_clock(tmp: &TempDir, registry: AdapterRegistry, clock: FakeClock) -> Deps {
    let paths = NodePaths::new(tmp.path().join("node"));
    paths.ensure().expect("node paths");
    Deps {
        paths,
        config: Arc::new(config_for(tmp.path())),
        node: local(),
        clock: Arc::new(clock),
        log: Arc::new(MemoryLog::new()),
        adapters: Arc::new(registry),
        windows: Arc::new(FakeWindows::new()),
        git: Arc::new(FakeGit::new()),
        cmd: Arc::new(vowp_core::SystemRunner),
        rng: Arc::new(vowp_core::FixedRng(0)),
    }
}

fn deps(tmp: &TempDir, clock: &FakeClock) -> Deps {
    let adapter = FakeAdapter::from_bodies(
        HarnessKind::Opencode,
        ts(),
        vec![EventBody::AssistantMessage(AssistantMessage {
            text: serde_json::json!({
                "intent": "continue the session",
                "constraints": [],
                "decisions": [],
                "state": {"done": [], "in_progress": [], "blocked": [], "next_steps": ["keep going"]},
                "files": [],
                "gotchas": [],
                "open_questions": []
            })
            .to_string(),
        })],
    );
    deps_with_clock(
        tmp,
        AdapterRegistry::new().with("opencode", Arc::new(adapter)),
        clock.clone(),
    )
}

fn create_session(service: &SessionService, tmp: &TempDir, executor: Option<Executor>) -> SessionId {
    service
        .create_session(NewSession {
            project: None,
            job_id: None,
            role: SessionRole::Worker,
            title: "surface lease".to_owned(),
            cwd: tmp.path().join("project"),
            parent: None,
            executor,
        })
        .expect("create session")
}

fn surface(kind: SurfaceKind, holder: &str, now: Timestamp) -> Surface {
    Surface {
        id: SurfaceId::new(),
        kind,
        node: local(),
        holder: holder.to_owned(),
        app: None,
        cwd: None,
        metadata: serde_json::json!({}),
        created_at: now,
        last_seen_at: now,
    }
}

fn acquire(deps: &Deps, store: &Store, session: &SessionId, surface: &Surface) -> Result<LeaseGuard, CoreError> {
    LeaseGuard::acquire(
        deps,
        store,
        &AcquireLease {
            session: session.clone(),
            surface: surface.clone(),
            takeover: false,
        },
    )
}

fn read_events(deps: &Deps, session: &SessionId) -> Vec<vowp_protocol::EventEnvelope> {
    SessionLog::open_read(&deps.paths, session)
        .expect("open log")
        .read_all()
        .expect("read log")
}

fn append_note(deps: &Deps, session: &SessionId, text: &str) {
    let store = deps.open_store().expect("store");
    let log = SessionLog::open(&deps.paths, session, deps.clock.clone()).expect("log");
    let appended = log
        .append(NewEvent {
            actor: Actor::System,
            source: EventSource {
                harness: HarnessKind::Vowp,
                model: None,
                node: Some(local()),
                native_session_id: None,
            },
            body: EventBody::Note(vowp_protocol::Note { text: text.to_owned() }),
            raw: None,
            ts: None,
        })
        .expect("append note");
    store.index_event(&index_row(&appended)).expect("index note");
    log.sync().expect("sync note");
}

fn token_of(store: &Store, session: &SessionId) -> String {
    store.lease(session).expect("lease").expect("held lease").token
}

#[test]
fn lease_acquire_is_one_holder() {
    let tmp = tempdir().expect("tempdir");
    let clock = FakeClock::at(ts());
    let deps = deps(&tmp, &clock);
    let service = SessionService::new(deps.clone());
    let session = create_session(&service, &tmp, Some(opencode()));
    let store = deps.open_store().expect("store");

    let first = surface(SurfaceKind::Chat, "laptop", ts());
    let guard = acquire(&deps, &store, &session, &first).expect("first lease");
    let lease = store.lease(&session).expect("lease").expect("held");
    assert_eq!(lease.holder, "laptop");
    assert_eq!(lease.surface_kind, SurfaceKind::Chat);
    assert_eq!(lease.surface_id.as_ref(), Some(&first.id));
    assert_eq!(lease.token.len(), 52, "two ULIDs, 160 random bits");
    assert_eq!(guard.token(), lease.token);

    let second = surface(SurfaceKind::Opencode, "phone", ts());
    let error = match acquire(&deps, &store, &session, &second) {
        Ok(_) => panic!("second surface must be refused"),
        Err(error) => error,
    };
    match error {
        CoreError::Store(StoreError::Conflict(text)) => {
            assert!(text.contains("laptop"), "conflict names the holder: {text}");
        }
        other => panic!("expected a conflict, got {other:?}"),
    }
    let leases = store.list_leases().expect("leases");
    assert_eq!(leases.len(), 1, "one row per session");
    assert_eq!(
        leases[0].token, lease.token,
        "a refused acquire must not replace the token"
    );

    guard.release().expect("release");
    assert!(store.lease(&session).expect("lease").is_none());
}

#[test]
fn heartbeat_keeps_lease_live() {
    let tmp = tempdir().expect("tempdir");
    let clock = FakeClock::at(ts());
    let deps = deps(&tmp, &clock);
    let service = SessionService::new(deps.clone());
    let session = create_session(&service, &tmp, Some(opencode()));
    let store = deps.open_store().expect("store");
    let target = surface(SurfaceKind::Chat, "laptop", ts());
    let guard = acquire(&deps, &store, &session, &target).expect("lease");
    let acquired = store.lease(&session).expect("lease").expect("held").heartbeat_at;

    clock.advance(HEARTBEAT_INTERVAL);
    std::thread::sleep(HEARTBEAT_INTERVAL + Duration::from_secs(2));

    let lease = store.lease(&session).expect("lease").expect("heartbeat kept it");
    assert!(
        lease.heartbeat_at > acquired,
        "the guard thread must advance heartbeat_at beyond the acquisition time"
    );
    assert!(
        ts().plus(STALE_AFTER) > lease.heartbeat_at,
        "the lease is still inside the stale window"
    );
    guard.release().expect("release");
}

#[test]
fn stale_lease_free_after_sixty_seconds() {
    let tmp = tempdir().expect("tempdir");
    let clock = FakeClock::at(ts());
    let deps = deps(&tmp, &clock);
    let service = SessionService::new(deps.clone());
    let session = create_session(&service, &tmp, Some(opencode()));
    let store = deps.open_store().expect("store");
    let old = surface(SurfaceKind::Chat, "laptop", ts());
    let old_guard = acquire(&deps, &store, &session, &old).expect("old lease");

    clock.advance(STALE_AFTER + Duration::from_secs(1));
    let new = surface(SurfaceKind::Opencode, "phone", clock.now());
    let new_guard = acquire(&deps, &store, &session, &new).expect("stale lease is free");

    let lease = store.lease(&session).expect("lease").expect("held");
    assert_eq!(lease.holder, "phone");
    assert_eq!(new_guard.token(), lease.token);
    let notes: Vec<String> = read_events(&deps, &session)
        .into_iter()
        .filter_map(|event| match event.body() {
            Ok(EventBody::Note(note)) => Some(note.text),
            _ => None,
        })
        .collect();
    assert!(
        notes.iter().any(|note| note == "stale surface laptop replaced"),
        "the replacement is recorded: {notes:?}"
    );

    old_guard.release().expect("release old");
    new_guard.release().expect("release new");
}

#[test]
fn takeover_cuts_old_surface_within_two_seconds() {
    let tmp = tempdir().expect("tempdir");
    let clock = FakeClock::at(ts());
    let deps = deps(&tmp, &clock);
    let service = SessionService::new(deps.clone());
    // Pre-attach session: no executor, so the takeover is the note-only path.
    let session = create_session(&service, &tmp, None);
    let store = deps.open_store().expect("store");
    let old = surface(SurfaceKind::Chat, "laptop", ts());
    let guard = acquire(&deps, &store, &session, &old).expect("old lease");
    let old_token = guard.token().to_owned();

    let phone = surface(SurfaceKind::Chat, "phone", clock.now());
    let started = Instant::now();
    let report = vowp_core::takeover(&deps, &store, &session, &phone, clock.now()).expect("takeover");
    let elapsed = started.elapsed();

    assert!(elapsed < Duration::from_secs(2), "takeover took {elapsed:?}");
    assert_eq!(report.previous.as_deref(), Some("laptop"));
    assert!(!report.drained);
    assert_eq!(report.capsule_id, None);
    assert!(
        check_lease_with(&store, &session, Some(&old_token)).is_err(),
        "the old surface is cut off immediately"
    );
    assert!(
        check_lease_with(&store, &session, Some(&token_of(&store, &session))).is_ok(),
        "the new surface holds the live lease"
    );
    clear_revocation(&session);
}

#[test]
fn lease_revoked_is_pushed_within_2s() {
    let tmp = tempdir().expect("tempdir");
    let clock = FakeClock::at(ts());
    let deps = deps(&tmp, &clock);
    let service = SessionService::new(deps.clone());
    let session = create_session(&service, &tmp, None);
    let store = deps.open_store().expect("store");
    let old = surface(SurfaceKind::Chat, "laptop", ts());
    let _guard = acquire(&deps, &store, &session, &old).expect("old lease");

    let wait_session = session.clone();
    let waiter = std::thread::spawn(move || {
        let started = Instant::now();
        let notice = wait_for_revocation(&wait_session, Duration::from_secs(2));
        (notice, started.elapsed())
    });
    std::thread::sleep(Duration::from_millis(100));
    let phone = surface(SurfaceKind::Opencode, "phone", clock.now());
    let _report = vowp_core::takeover(&deps, &store, &session, &phone, clock.now()).expect("takeover");

    let (notice, elapsed) = waiter.join().expect("waiter");
    let notice = notice.expect("revocation notice within the timeout");
    assert_eq!(notice.by, "phone");
    assert_eq!(notice.session, session);
    assert!(elapsed < Duration::from_secs(2), "notice took {elapsed:?}");
    match revocation_ack(&notice) {
        IngestAck::LeaseRevoked {
            by,
            reason,
            at,
            last_seq,
        } => {
            assert_eq!(by, "phone");
            assert_eq!(reason, "session taken over");
            assert_eq!(at, notice.at);
            assert_eq!(last_seq, notice.last_seq);
        }
        other => panic!("expected LeaseRevoked, got {other:?}"),
    }
    clear_revocation(&session);
}

#[test]
fn takeover_drains_and_writes_capsule() {
    let tmp = tempdir().expect("tempdir");
    let clock = FakeClock::at(ts());
    let deps = deps(&tmp, &clock);
    let service = SessionService::new(deps.clone());
    let session = create_session(&service, &tmp, Some(opencode()));
    let store = deps.open_store().expect("store");
    let old = surface(SurfaceKind::Chat, "laptop", ts());
    let guard = acquire(&deps, &store, &session, &old).expect("old lease");
    let old_token = guard.token().to_owned();
    store
        .set_session_state(&session, SessionState::Running, Some(&opencode()))
        .expect("mark running");

    let phone = surface(SurfaceKind::Chat, "phone", clock.now());
    let report = vowp_core::takeover(&deps, &store, &session, &phone, clock.now()).expect("takeover");

    assert!(report.drained, "a running turn is drained before the handoff");
    let capsule = report.capsule_id.expect("switch writes a capsule");
    assert_eq!(
        store
            .latest_capsule(&session)
            .expect("latest capsule")
            .expect("capsule")
            .capsule_id,
        capsule
    );
    let switches = store.list_switches(&session).expect("switches");
    assert_eq!(switches.len(), 1);
    assert_eq!(switches[0].reason, SwitchReason::Takeover);
    let events = read_events(&deps, &session);
    assert!(
        events.iter().any(|event| matches!(
            event.body(),
            Ok(EventBody::TurnEnded(payload))
                if payload.stop_reason == vowp_protocol::StopReason::Interrupted
        )),
        "the interrupted turn is closed in the log"
    );
    assert!(
        events.iter().any(|event| event.event_type == EventType::Capsule),
        "the handoff capsule is in the log"
    );
    assert!(
        check_lease_with(&store, &session, Some(&old_token)).is_err(),
        "the old surface is read-only after the drain"
    );
    clear_revocation(&session);
}

#[test]
fn takeover_before_first_executor_writes_note_only() {
    let tmp = tempdir().expect("tempdir");
    let clock = FakeClock::at(ts());
    let deps = deps(&tmp, &clock);
    let service = SessionService::new(deps.clone());
    let session = create_session(&service, &tmp, None);
    let store = deps.open_store().expect("store");
    let old = surface(SurfaceKind::Chat, "laptop", ts());
    let _guard = acquire(&deps, &store, &session, &old).expect("old lease");

    let phone = surface(SurfaceKind::Chat, "phone", clock.now());
    let report = vowp_core::takeover(&deps, &store, &session, &phone, clock.now()).expect("takeover");

    assert_eq!(report.previous.as_deref(), Some("laptop"));
    assert!(!report.drained);
    assert_eq!(report.capsule_id, None);
    assert!(
        store.list_switches(&session).expect("switches").is_empty(),
        "a pre-attach session has no valid switch target"
    );
    assert!(store.latest_capsule(&session).expect("capsule").is_none());
    let notes: Vec<String> = read_events(&deps, &session)
        .into_iter()
        .filter_map(|event| match event.body() {
            Ok(EventBody::Note(note)) => Some(note.text),
            _ => None,
        })
        .collect();
    assert!(
        notes.iter().any(|note| note.starts_with("taken over by phone at ")),
        "the note records the takeover: {notes:?}"
    );
    clear_revocation(&session);
}

#[test]
fn follow_mode_prints_new_events_and_refuses_writes() {
    let tmp = tempdir().expect("tempdir");
    let clock = FakeClock::at(ts());
    let deps = deps(&tmp, &clock);
    let service = SessionService::new(deps.clone());
    let session = create_session(&service, &tmp, None);
    let store = deps.open_store().expect("store");
    let old = surface(SurfaceKind::Chat, "laptop", ts());
    let guard = acquire(&deps, &store, &session, &old).expect("old lease");
    let old_token = guard.token().to_owned();

    let phone = surface(SurfaceKind::Chat, "phone", clock.now());
    vowp_core::takeover(&deps, &store, &session, &phone, clock.now()).expect("takeover");
    let after = SessionLog::open_read(&deps.paths, &session)
        .expect("log")
        .last_seq()
        .expect("last seq");
    append_note(&deps, &session, "new event for the follower");

    let fresh = SessionLog::open_read(&deps.paths, &session)
        .expect("log")
        .read_from(after, None)
        .expect("read new events");
    assert_eq!(fresh.len(), 1);
    assert!(matches!(
        fresh[0].body(),
        Ok(EventBody::Note(note)) if note.text == "new event for the follower"
    ));
    assert!(
        check_lease_with(&store, &session, Some(&old_token)).is_err(),
        "follow mode cannot write"
    );
    assert!(check_lease_with(&store, &session, Some(&token_of(&store, &session))).is_ok());
    clear_revocation(&session);
}

#[test]
fn lease_check_rejects_wrong_token() {
    let tmp = tempdir().expect("tempdir");
    let clock = FakeClock::at(ts());
    let deps = deps(&tmp, &clock);
    let service = SessionService::new(deps.clone());
    let session = create_session(&service, &tmp, None);
    let store = deps.open_store().expect("store");

    assert!(
        check_lease_with(&store, &session, Some("no-lease-yet")).is_ok(),
        "a job session without a lease passes"
    );
    let target = surface(SurfaceKind::Chat, "laptop", ts());
    let guard = acquire(&deps, &store, &session, &target).expect("lease");
    assert!(check_lease_with(&store, &session, Some(guard.token())).is_ok());
    assert!(
        check_lease_with(&store, &session, None).is_ok(),
        "None means no lease required"
    );

    let error = check_lease_with(&store, &session, Some("wrong-token")).expect_err("wrong token");
    match error {
        CoreError::Refused(text) => {
            assert!(text.contains("read-only"), "refusal explains the state: {text}");
            assert!(text.contains("laptop"), "refusal names the holder: {text}");
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    guard.release().expect("release");
    assert!(
        check_lease_with(&store, &session, Some("wrong-token")).is_ok(),
        "without a lease the session is a job session again"
    );
}

#[test]
fn lease_sweep_drops_dead_surface() {
    let tmp = tempdir().expect("tempdir");
    let clock = FakeClock::at(ts());
    let deps = deps(&tmp, &clock);
    let service = SessionService::new(deps.clone());
    let session = create_session(&service, &tmp, None);
    let store = deps.open_store().expect("store");
    let target = surface(SurfaceKind::Chat, "laptop", ts());
    let _guard = acquire(&deps, &store, &session, &target).expect("lease");

    clock.advance(STALE_AFTER + Duration::from_secs(1));
    let dropped = vowp_core::surface::sweep_leases(&deps, &store, clock.now()).expect("sweep");
    assert_eq!(dropped, 1);
    assert!(store.lease(&session).expect("lease").is_none());
    assert_eq!(
        vowp_core::surface::sweep_leases(&deps, &store, clock.now()).expect("second sweep"),
        0
    );
}
