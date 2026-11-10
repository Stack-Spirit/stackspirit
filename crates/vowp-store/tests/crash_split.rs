//! Real child-process crash injection for the global/project write order.
//! Spec: `docs/design/v0.2/home-and-surfaces.md` §3.7 and §15 (C1).

use std::path::Path;
use std::sync::Arc;

use tempfile::TempDir;
use vowp_protocol::{
    Actor, Cost, EventId, EventIndexRow, EventType, FakeClock, HarnessKind, NodeName, Project, ProjectName, Run, RunId,
    RunPurpose, RunRole, Session, SessionId, SessionRole, SessionState, Timestamp,
};
use vowp_store::{NodePaths, RunFinish, Store};

const EPOCH: &str = "2026-01-01T00:00:00.000Z";

fn now() -> Timestamp {
    Timestamp::parse(EPOCH).expect("epoch")
}

fn project() -> ProjectName {
    ProjectName::parse("proj").expect("project")
}

fn open_bound(root: &Path) -> Store {
    let paths = NodePaths::new(root);
    let clock = FakeClock::at(now());
    let store = Store::open(&paths).expect("store").with_clock(Arc::new(clock));
    if store.bound_project().is_none() {
        store
            .upsert_project(&Project {
                name: project(),
                path: root.display().to_string(),
                remote_url: None,
                default_branch: "main".to_owned(),
                config: serde_json::json!({}),
                created_at: now(),
                updated_at: now(),
            })
            .expect("project");
    }
    Store::open_with_project(&paths, &project())
        .expect("bound")
        .with_clock(Arc::new(FakeClock::at(now())))
}

fn session(store: &Store, state: SessionState) -> SessionId {
    let id = SessionId::new();
    store
        .upsert_session(&Session {
            id: id.clone(),
            project: Some(project()),
            job_id: None,
            role: Some(SessionRole::Worker),
            title: "crash".to_owned(),
            parent_session_id: None,
            node: NodeName::parse("local").expect("node"),
            cwd: "/tmp".to_owned(),
            state,
            executor: None,
            last_seq: 0,
            last_capsule_id: None,
            log_bytes: 0,
            created_at: now(),
            updated_at: now(),
        })
        .expect("session");
    id
}

fn open_run(store: &Store, session: &SessionId) -> RunId {
    let id = RunId::new();
    store
        .insert_run(&Run {
            id: id.clone(),
            job_id: None,
            session_id: Some(session.clone()),
            role: RunRole::Worker,
            purpose: RunPurpose::Start,
            profile: None,
            rung: 0,
            executor: None,
            provider: None,
            node: NodeName::parse("local").expect("node"),
            native_session_id: None,
            pid: None,
            window: None,
            cwd: None,
            started_at: now(),
            ended_at: None,
            exit_code: None,
            outcome: None,
            limit_kind: None,
            cost: Cost::unknown(),
            detail: String::new(),
        })
        .expect("run");
    id
}

fn child_case() -> Option<&'static str> {
    std::env::var("VOWP_CRASH_CASE").ok().map(|value| match value.as_str() {
        "global" => "global",
        "project" => "project",
        _ => "unknown",
    })
}

fn child_root() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("VOWP_CRASH_ROOT").expect("VOWP_CRASH_ROOT"))
}

fn run_child(test: &str, case: &str, root: &Path) {
    let mut command = std::process::Command::new(std::env::current_exe().expect("test binary"));
    command
        .arg("--exact")
        .arg(test)
        .arg("--nocapture")
        .env("VOWP_CRASH_CASE", case)
        .env("VOWP_CRASH_ROOT", root);
    let status = command.status().expect("spawn child");
    assert!(status.success(), "crash child exited with {status}");
}

#[test]
fn crash_between_global_framing_and_project_run() {
    if child_case() == Some("global") {
        let root = child_root();
        let store = open_bound(&root);
        let session = session(&store, SessionState::Running);
        store
            .index_event(&EventIndexRow {
                session_id: session,
                seq: 1,
                id: EventId::new(),
                ts: now(),
                event_type: EventType::TurnStarted,
                actor: Actor::System,
                harness: HarnessKind::Vowp,
                model: None,
                byte_offset: 0,
                byte_len: 0,
                summary: "turn started".to_owned(),
            })
            .expect("global event");
        std::process::exit(0);
    }

    let tmp = TempDir::new().expect("tempdir");
    run_child("crash_between_global_framing_and_project_run", "global", tmp.path());
    let store = open_bound(tmp.path());
    let report = store.reconcile_cross_db().expect("reconcile");
    assert_eq!(report.sessions_repaired, 1);
    assert_eq!(
        store.list_sessions(&Default::default()).expect("sessions")[0].state,
        SessionState::Idle
    );
    let second = store.reconcile_cross_db().expect("idempotent");
    assert_eq!(second, Default::default());
}

#[test]
fn crash_between_project_finish_and_terminal_events() {
    if child_case() == Some("project") {
        let root = child_root();
        let store = open_bound(&root);
        let session = session(&store, SessionState::Idle);
        let run = open_run(&store, &session);
        store
            .finish_run(
                &run,
                &RunFinish {
                    outcome: vowp_protocol::RunOutcome::Ok,
                    exit_code: Some(0),
                    limit_kind: None,
                    cost: Cost::unknown(),
                    native_session_id: None,
                    detail: String::new(),
                },
            )
            .expect("finish project row");
        std::process::exit(0);
    }

    let tmp = TempDir::new().expect("tempdir");
    run_child(
        "crash_between_project_finish_and_terminal_events",
        "project",
        tmp.path(),
    );
    let store = open_bound(tmp.path());
    let report = store.reconcile_cross_db().expect("reconcile");
    assert_eq!(report.terminal_events_repaired, 1);
    assert_eq!(report.ledgers_repaired, 1);
    let second = store.reconcile_cross_db().expect("idempotent");
    assert_eq!(second, Default::default());
}
