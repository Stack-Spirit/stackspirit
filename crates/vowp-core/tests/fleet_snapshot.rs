//! `snapshot.json` and the fleet snapshot cards.
//! Spec: `docs/design/v0.2/home-and-surfaces.md` §5 and §15 (D1 named tests).

mod support;

use std::sync::Arc;
use std::time::Duration;

use support::fake_git::FakeGit;
use tempfile::{TempDir, tempdir};
use vowp_core::fleet;
use vowp_core::{AdapterRegistry, Deps, FixedRng, SystemRunner};
use vowp_protocol::{
    ActiveState, Clock, Cost, Evidence, Executor, FakeClock, Gate, GateResult, HarnessKind, InboxItem, InboxKind,
    InboxState, Job, JobId, JobKind, JobState, LedgerEntry, LedgerKind, Loop, LoopKind, NewJob, Node, NodeName,
    ParkedReason, Project, ProjectName, ReportGate, Role, Run, RunId, RunPurpose, RunRole, SNAPSHOT_FORMAT,
    SNAPSHOT_VERSION, Session, SessionId, SessionRole, SessionState, Snapshot, Timestamp, Tokens,
};
use vowp_store::{NodePaths, StateChange, Store};
use vowp_tmux::fake::FakeWindows;

/// A hermetic, file-backed home with two registered projects.
struct Fixture {
    _home: TempDir,
    _alpha_dir: TempDir,
    _beta_dir: TempDir,
    paths: NodePaths,
    deps: Deps,
    store: Store,
    clock: FakeClock,
    alpha: ProjectName,
    beta: ProjectName,
}

impl Fixture {
    fn new() -> Self {
        let home = tempdir().expect("home");
        let alpha_dir = tempdir().expect("alpha dir");
        let beta_dir = tempdir().expect("beta dir");
        let paths = NodePaths::new(home.path());
        let clock = FakeClock::at(timestamp("2026-11-10T12:00:00Z"));
        let store = Store::open(&paths).expect("store").with_clock(Arc::new(clock.clone()));
        let alpha = ProjectName::parse("alpha").expect("alpha");
        let beta = ProjectName::parse("beta").expect("beta");
        let mut config = vowp_protocol::VowpConfig::with_defaults_for_tests();
        config.project.name = alpha.clone();
        config.project.path = Some(alpha_dir.path().display().to_string());
        let now = clock.now();
        store
            .upsert_node(&Node {
                name: NodeName::parse("local").expect("node"),
                ssh_target: None,
                is_local: true,
                capabilities: serde_json::json!({}),
                last_seen_at: Some(now),
                created_at: now,
            })
            .expect("node");
        for (name, dir, project_config) in [(&alpha, alpha_dir.path(), &config), (&beta, beta_dir.path(), &config)] {
            let mut project_config = (*project_config).clone();
            project_config.project.name = name.clone();
            project_config.project.path = Some(dir.display().to_string());
            store
                .upsert_project(&Project {
                    name: name.clone(),
                    path: dir.display().to_string(),
                    remote_url: None,
                    default_branch: "main".to_owned(),
                    config: serde_json::to_value(&project_config).expect("config json"),
                    created_at: now,
                    updated_at: now,
                })
                .expect("project");
        }
        let deps = Deps {
            paths: paths.clone(),
            config: Arc::new(config),
            node: NodeName::parse("local").expect("node"),
            clock: Arc::new(clock.clone()),
            log: Arc::new(vowp_protocol::MemoryLog::new()),
            adapters: Arc::new(AdapterRegistry::new()),
            windows: Arc::new(FakeWindows::new()),
            git: Arc::new(FakeGit::new()),
            cmd: Arc::new(SystemRunner),
            rng: Arc::new(FixedRng(0)),
        };
        Self {
            _home: home,
            _alpha_dir: alpha_dir,
            _beta_dir: beta_dir,
            paths,
            deps,
            store,
            clock,
            alpha,
            beta,
        }
    }

    fn store_for(&self, project: &ProjectName) -> Store {
        Store::open_with_project(&self.paths, project)
            .expect("project store")
            .with_clock(Arc::new(self.clock.clone()))
    }

    fn job(&self, project: &ProjectName, id: &str, priority: i32) -> Job {
        let store = self.store_for(project);
        store
            .insert_job(
                project,
                &NewJob {
                    id: JobId::parse(id).expect("job id"),
                    title: format!("job {id}"),
                    kind: JobKind::Build,
                    role: Role::Worker,
                    profile: None,
                    needs: Vec::new(),
                    owns: Vec::new(),
                    reads: Vec::new(),
                    gates: None,
                    priority: Some(priority),
                    loop_: None,
                    brief_text: None,
                },
                "test",
            )
            .expect("insert job")
    }

    fn set_state(&self, project: &ProjectName, job: &Job, state: JobState, change: StateChange) {
        self.store_for(project)
            .set_job_state(&job.id, state, &change)
            .expect("state");
    }

    fn snapshot(&self) -> Snapshot {
        fleet::build_snapshot(&self.deps, &self.store, self.clock.now()).expect("snapshot")
    }
}

/// Parses a timestamp, failing the test on bad input.
fn timestamp(text: &str) -> Timestamp {
    Timestamp::parse(text).expect("timestamp")
}

/// Builds a small token count.
fn tokens(input: u64, output: u64) -> Tokens {
    Tokens {
        input,
        output,
        ..Tokens::default()
    }
}

#[test]
fn snapshot_json_validates_against_exported_schema() {
    let fixture = Fixture::new();
    fixture.job(&fixture.alpha, "a01", 50);
    let snapshot = fixture.snapshot();
    let value = serde_json::to_value(&snapshot).expect("serialise snapshot");
    assert_eq!(value["schema"], SNAPSHOT_FORMAT);
    assert_eq!(value["version"], SNAPSHOT_VERSION);

    let schema = vowp_protocol::schema::schema_for_name("Snapshot").expect("Snapshot schema");
    let properties = schema["properties"].as_object().expect("properties");
    let object = value.as_object().expect("snapshot object");
    for key in object.keys() {
        assert!(
            properties.contains_key(key),
            "snapshot key {key:?} is absent from the exported schema"
        );
    }
    for required in schema["required"].as_array().expect("required list") {
        let name = required.as_str().expect("required name");
        assert!(object.contains_key(name), "snapshot is missing required {name:?}");
    }
    assert!(
        schema["$defs"].get("ProjectSnapshot").is_some(),
        "exported Snapshot schema must define ProjectSnapshot: {schema}"
    );
    let decoded: Snapshot = serde_json::from_value(value).expect("schema-valid document decodes");
    assert_eq!(decoded, snapshot);
}

#[test]
fn snapshot_cards_cover_every_project_and_state() {
    let fixture = Fixture::new();
    let now = fixture.clock.now();

    let done = fixture.job(&fixture.alpha, "a01-done", 50);
    fixture.set_state(
        &fixture.alpha,
        &done,
        JobState::Done,
        StateChange {
            note: None,
            parked_reason: None,
        },
    );
    let running = fixture.job(&fixture.alpha, "a02-run", 70);
    fixture.set_state(
        &fixture.alpha,
        &running,
        JobState::Running,
        StateChange {
            note: None,
            parked_reason: None,
        },
    );
    let ready = fixture.job(&fixture.alpha, "a03-ready", 90);
    fixture.set_state(
        &fixture.alpha,
        &ready,
        JobState::Ready,
        StateChange {
            note: None,
            parked_reason: None,
        },
    );
    fixture
        .store_for(&fixture.alpha)
        .set_job_note(&ready.id, "waiting: RAM 1.2 GB short")
        .expect("waiting note");
    let parked = fixture.job(&fixture.alpha, "a04-parked", 10);
    fixture.set_state(
        &fixture.alpha,
        &parked,
        JobState::Parked,
        StateChange {
            note: Some("out of credits".to_owned()),
            parked_reason: Some(ParkedReason::Credits),
        },
    );
    let failed = fixture.job(&fixture.alpha, "a05-failed", 10);
    fixture.set_state(
        &fixture.alpha,
        &failed,
        JobState::Failed,
        StateChange {
            note: Some("compile failed".to_owned()),
            parked_reason: None,
        },
    );
    fixture.job(&fixture.beta, "b01-pending", 50);

    let alpha_store = fixture.store_for(&fixture.alpha);
    let run = Run {
        id: RunId::new(),
        job_id: Some(running.id.clone()),
        session_id: None,
        role: RunRole::Worker,
        purpose: RunPurpose::Start,
        profile: None,
        rung: 0,
        executor: Some(Executor::parse("opencode:deepseek/deepseek-flash").expect("executor")),
        provider: Some("deepseek".to_owned()),
        node: NodeName::parse("local").expect("node"),
        native_session_id: None,
        pid: None,
        window: None,
        cwd: None,
        started_at: now.minus(Duration::from_secs(120)),
        ended_at: None,
        exit_code: None,
        outcome: None,
        limit_kind: None,
        cost: Cost::unknown(),
        detail: String::new(),
    };
    alpha_store.insert_run(&run).expect("run");

    alpha_store
        .insert_gate_result(&GateResult {
            id: None,
            job_id: ready.id.clone(),
            run_id: None,
            attempt: 0,
            gate_index: 0,
            gate: Gate::Report(ReportGate::default()),
            passed: false,
            evidence: Evidence::Report {
                front_matter: None,
                missing_fields: vec!["status".to_owned()],
                problems: Vec::new(),
            },
            output_blob: None,
            duration_ms: 1,
            ts: now.minus(Duration::from_secs(30)),
        })
        .expect("gate result");

    alpha_store
        .append_ledger(&LedgerEntry {
            id: None,
            ts: now.minus(Duration::from_secs(3600)),
            kind: LedgerKind::Merge,
            project: Some(fixture.alpha.clone()),
            job_id: Some(done.id.clone()),
            run_id: None,
            session_id: None,
            role: None,
            provider: None,
            harness: None,
            model: None,
            cost: None,
            summary: "merged a01-done".to_owned(),
            detail: serde_json::json!({
                "main_after": "abc1234",
                "branch": "job/a01-done",
            }),
        })
        .expect("merge row");
    alpha_store
        .append_ledger(&LedgerEntry {
            id: None,
            ts: now.minus(Duration::from_secs(60)),
            kind: LedgerKind::Cost,
            project: Some(fixture.alpha.clone()),
            job_id: Some(running.id.clone()),
            run_id: Some(run.id.clone()),
            session_id: None,
            role: Some(RunRole::Worker),
            provider: Some("deepseek".to_owned()),
            harness: Some(HarnessKind::Opencode),
            model: Some("deepseek-flash".to_owned()),
            cost: Some(Cost::receipt(1.25, tokens(1_000, 100))),
            summary: "cost".to_owned(),
            detail: serde_json::json!({}),
        })
        .expect("cost row");
    alpha_store
        .insert_inbox(&InboxItem {
            id: vowp_protocol::InboxId::new(),
            project: Some(fixture.alpha.clone()),
            job_id: None,
            kind: InboxKind::Question,
            title: "Which branch?".to_owned(),
            body: String::new(),
            options: Vec::new(),
            state: InboxState::Open,
            answer: None,
            answered_by: None,
            created_at: now.minus(Duration::from_secs(10)),
            answered_at: None,
        })
        .expect("project inbox");
    fixture
        .store
        .insert_node_inbox(&InboxItem {
            id: vowp_protocol::InboxId::new(),
            project: None,
            job_id: None,
            kind: InboxKind::Notice,
            title: "Node notice".to_owned(),
            body: String::new(),
            options: Vec::new(),
            state: InboxState::Open,
            answer: None,
            answered_by: None,
            created_at: now.minus(Duration::from_secs(20)),
            answered_at: None,
        })
        .expect("node inbox");
    fixture
        .store
        .upsert_session(&Session {
            id: SessionId::new(),
            project: Some(fixture.alpha.clone()),
            job_id: Some(running.id.clone()),
            role: Some(SessionRole::Worker),
            title: "worker session".to_owned(),
            parent_session_id: None,
            node: NodeName::parse("local").expect("node"),
            cwd: "/srv/vowp".to_owned(),
            state: SessionState::Running,
            executor: Some(Executor::parse("opencode:deepseek/deepseek-flash").expect("executor")),
            last_seq: 0,
            last_capsule_id: None,
            log_bytes: 0,
            created_at: now.minus(Duration::from_secs(300)),
            updated_at: now.minus(Duration::from_secs(5)),
        })
        .expect("session");

    let snapshot = fixture.snapshot();
    let names: Vec<&str> = snapshot.projects.iter().map(|project| project.name.as_str()).collect();
    assert_eq!(names, vec!["alpha", "beta"], "every registered project has a card");

    let alpha = snapshot
        .projects
        .iter()
        .find(|project| project.name == fixture.alpha)
        .expect("alpha card");
    assert_eq!(alpha.jobs_by_state.get("running"), Some(&1));
    assert_eq!(alpha.jobs_by_state.get("ready"), Some(&1));
    assert_eq!(alpha.jobs_by_state.get("parked"), Some(&1));
    assert_eq!(alpha.jobs_by_state.get("failed"), Some(&1));

    assert_eq!(alpha.running.len(), 1);
    let card = &alpha.running[0];
    assert_eq!(card.job, running.id);
    assert_eq!(card.executor.as_deref(), Some("opencode:deepseek/deepseek-flash"));
    assert_eq!(card.node.as_str(), "local");
    assert_eq!(card.age_secs, 120);

    let waiting = alpha
        .queue
        .iter()
        .find(|job| job.job == ready.id)
        .expect("ready job in queue");
    assert_eq!(
        waiting.waiting_reason.as_deref(),
        Some("waiting: RAM 1.2 GB short"),
        "the admission reason comes from jobs.note"
    );
    assert!(
        alpha.queue.iter().any(|job| job.job == parked.id),
        "parked jobs also appear in the queue"
    );

    let blocked: Vec<&str> = alpha.blocked.iter().map(|job| job.job.as_str()).collect();
    assert_eq!(blocked, vec!["a04-parked", "a05-failed"]);
    let parked_card = alpha
        .blocked
        .iter()
        .find(|job| job.job == parked.id)
        .expect("parked card");
    assert_eq!(parked_card.reason, "credits");
    assert_eq!(parked_card.detail, "out of credits");

    assert_eq!(alpha.gate_failures.len(), 1);
    assert_eq!(alpha.gate_failures[0].gate, "report");
    assert!(alpha.gate_failures[0].evidence.contains("missing fields: status"));
    assert_eq!(
        alpha.last_merge.as_ref().map(|merge| merge.commit.as_str()),
        Some("abc1234")
    );
    assert_eq!(
        alpha.last_merge.as_ref().map(|merge| merge.branch.as_str()),
        Some("job/a01-done")
    );
    assert_eq!(alpha.cost_today_usd, Some(1.25));

    let beta = snapshot
        .projects
        .iter()
        .find(|project| project.name == fixture.beta)
        .expect("beta card");
    assert_eq!(beta.jobs_by_state.get("pending"), Some(&1));

    assert_eq!(snapshot.inbox.len(), 2, "node and project inbox rows merge");
    assert_eq!(snapshot.inbox[0].title, "Node notice", "oldest first");
    assert_eq!(snapshot.sessions.len(), 1);
    assert_eq!(
        snapshot.sessions[0].active_surface, None,
        "no lease is live in this fixture"
    );
}

#[test]
fn snapshot_written_atomically_every_tick() {
    let fixture = Fixture::new();
    fixture.job(&fixture.alpha, "a01", 50);
    let now = fixture.clock.now();

    fleet::write_snapshot(&fixture.deps, &fixture.store, now).expect("first write");
    let first = fleet::read_snapshot(&fixture.paths).expect("first read");
    assert_eq!(first.sequence, 1);
    assert_eq!(first.schema, SNAPSHOT_FORMAT);
    assert_eq!(first.version, SNAPSHOT_VERSION);
    assert!(
        !fixture.paths.tmp_dir().join("snapshot.json.tmp").exists(),
        "the temporary file is renamed away"
    );

    fleet::write_snapshot(&fixture.deps, &fixture.store, now).expect("second write");
    let second = fleet::read_snapshot(&fixture.paths).expect("second read");
    assert_eq!(second.sequence, 2, "the sequence increments every write");
    assert_eq!(second.projects.len(), 2);
}

#[test]
fn snapshot_reader_never_sees_partial_json() {
    let fixture = Fixture::new();
    fixture.job(&fixture.alpha, "a01", 50);
    fleet::write_snapshot(&fixture.deps, &fixture.store, fixture.clock.now()).expect("write");
    let valid = fleet::read_snapshot(&fixture.paths).expect("valid snapshot");

    // A half-written temporary file is invisible to readers: only the renamed file is read.
    std::fs::write(
        fixture.paths.tmp_dir().join("snapshot.json.tmp"),
        b"{\"schema\": \"vowp-snapshot/1\", \"version\":",
    )
    .expect("partial tmp");
    assert_eq!(
        fleet::read_snapshot(&fixture.paths).expect("main file still valid"),
        valid
    );

    // A truncated main file is a clean error, never a silent partial document.
    let path = fixture.paths.root().join("snapshot.json");
    let text = std::fs::read_to_string(&path).expect("snapshot text");
    std::fs::write(&path, &text[..text.len() / 2]).expect("truncated snapshot");
    let error = fleet::read_snapshot(&fixture.paths).expect_err("partial JSON must fail");
    assert!(
        error.to_string().contains("snapshot.json"),
        "the reader names the file: {error}"
    );
}

#[test]
fn snapshot_reader_rejects_newer_or_wrong_schema() {
    let fixture = Fixture::new();
    fleet::write_snapshot(&fixture.deps, &fixture.store, fixture.clock.now()).expect("write");
    let path = fixture.paths.root().join("snapshot.json");
    let text = std::fs::read_to_string(&path).expect("snapshot text");
    let mut value: serde_json::Value = serde_json::from_str(&text).expect("snapshot json");

    value["future_field"] = serde_json::json!(true);
    std::fs::write(&path, serde_json::to_string_pretty(&value).expect("render")).expect("unknown field");
    assert!(
        fleet::read_snapshot(&fixture.paths).is_ok(),
        "unknown fields in a known version are ignored"
    );

    value["version"] = serde_json::json!(SNAPSHOT_VERSION + 1);
    std::fs::write(&path, serde_json::to_string_pretty(&value).expect("render")).expect("newer version");
    let error = fleet::read_snapshot(&fixture.paths).expect_err("newer version must be rejected");
    assert!(error.to_string().contains("newer"), "{error}");

    value["version"] = serde_json::json!(SNAPSHOT_VERSION);
    value["schema"] = serde_json::json!("vowp-snapshot/99");
    std::fs::write(&path, serde_json::to_string_pretty(&value).expect("render")).expect("wrong schema");
    let error = fleet::read_snapshot(&fixture.paths).expect_err("wrong schema must be rejected");
    assert!(error.to_string().contains("schema"), "{error}");
}

#[test]
fn snapshot_degrades_one_broken_project_and_keeps_the_rest() {
    let fixture = Fixture::new();
    fixture.job(&fixture.alpha, "a01", 50);
    let beta_db = fixture.paths.project_db_path(&fixture.beta);
    std::fs::remove_file(&beta_db).expect("remove beta db");

    let snapshot = fixture.snapshot();
    let alpha = snapshot
        .projects
        .iter()
        .find(|project| project.name == fixture.alpha)
        .expect("alpha card");
    assert_eq!(alpha.jobs_by_state.get("pending"), Some(&1));

    let beta = snapshot
        .projects
        .iter()
        .find(|project| project.name == fixture.beta)
        .expect("beta card still exists");
    assert_eq!(beta.state, vowp_protocol::ProjectState::Paused);
    assert!(
        beta.paused_reason.starts_with("broken:"),
        "the card explains why: {}",
        beta.paused_reason
    );
    assert!(beta.running.is_empty() && beta.queue.is_empty());
}

#[test]
fn gate_failures_include_older_attempts() {
    let fixture = Fixture::new();
    let job = fixture.job(&fixture.alpha, "g01-two-attempts", 50);
    let store = fixture.store_for(&fixture.alpha);
    let now = fixture.clock.now();
    store
        .set_job_counters(&job.id, Some(1), None, None)
        .expect("attempt counter");

    store
        .insert_gate_result(&GateResult {
            id: None,
            job_id: job.id.clone(),
            run_id: None,
            attempt: 0,
            gate_index: 0,
            gate: Gate::Report(ReportGate::default()),
            passed: false,
            evidence: Evidence::Report {
                front_matter: None,
                missing_fields: vec!["status".to_owned()],
                problems: Vec::new(),
            },
            output_blob: None,
            duration_ms: 1,
            ts: now.minus(Duration::from_secs(60)),
        })
        .expect("failed attempt 0");
    store
        .insert_gate_result(&GateResult {
            id: None,
            job_id: job.id.clone(),
            run_id: None,
            attempt: 1,
            gate_index: 0,
            gate: Gate::Report(ReportGate::default()),
            passed: true,
            evidence: Evidence::Report {
                front_matter: Some(serde_json::json!({"status": "done"})),
                missing_fields: Vec::new(),
                problems: Vec::new(),
            },
            output_blob: None,
            duration_ms: 1,
            ts: now.minus(Duration::from_secs(10)),
        })
        .expect("passed attempt 1");

    let snapshot = fixture.snapshot();
    let alpha = snapshot
        .projects
        .iter()
        .find(|project| project.name == fixture.alpha)
        .expect("alpha card");
    assert_eq!(
        alpha.gate_failures.len(),
        1,
        "a failed older attempt is still a project failure: {:?}",
        alpha.gate_failures
    );
    assert_eq!(alpha.gate_failures[0].job, job.id);
    assert_eq!(alpha.gate_failures[0].gate, "report");
    assert!(alpha.gate_failures[0].evidence.contains("status"));
}

#[test]
fn gate_failures_keep_only_the_newest_ten() {
    let fixture = Fixture::new();
    let job = fixture.job(&fixture.alpha, "g02-many-failures", 50);
    let store = fixture.store_for(&fixture.alpha);
    let now = fixture.clock.now();
    store
        .set_job_counters(&job.id, Some(1), None, None)
        .expect("attempt counter");

    for index in 0..12_u32 {
        store
            .insert_gate_result(&GateResult {
                id: None,
                job_id: job.id.clone(),
                run_id: None,
                attempt: 0,
                gate_index: i32::try_from(index).expect("gate index"),
                gate: Gate::Report(ReportGate::default()),
                passed: false,
                evidence: Evidence::Report {
                    front_matter: None,
                    missing_fields: vec![format!("field{index}")],
                    problems: Vec::new(),
                },
                output_blob: None,
                duration_ms: 1,
                ts: now.minus(Duration::from_secs(u64::from(index) + 1)),
            })
            .expect("failed result");
    }
    store
        .insert_gate_result(&GateResult {
            id: None,
            job_id: job.id.clone(),
            run_id: None,
            attempt: 1,
            gate_index: 0,
            gate: Gate::Report(ReportGate::default()),
            passed: true,
            evidence: Evidence::Report {
                front_matter: Some(serde_json::json!({"status": "done"})),
                missing_fields: Vec::new(),
                problems: Vec::new(),
            },
            output_blob: None,
            duration_ms: 1,
            ts: now,
        })
        .expect("passed newest attempt");

    let snapshot = fixture.snapshot();
    let alpha = snapshot
        .projects
        .iter()
        .find(|project| project.name == fixture.alpha)
        .expect("alpha card");
    assert_eq!(alpha.gate_failures.len(), 10, "the card is capped at ten");
    assert!(
        alpha.gate_failures[0].evidence.contains("field0"),
        "newest failure first: {:?}",
        alpha.gate_failures
    );
    assert!(
        alpha.gate_failures[9].evidence.contains("field9"),
        "the ten newest survive: {:?}",
        alpha.gate_failures
    );
    assert!(
        alpha
            .gate_failures
            .iter()
            .all(|failure| !failure.evidence.contains("field10") && !failure.evidence.contains("field11")),
        "older failures are dropped: {:?}",
        alpha.gate_failures
    );
}

#[test]
fn supervisor_card_reports_l1_fail_streak() {
    let fixture = Fixture::new();
    let now = fixture.clock.now();
    fixture
        .store_for(&fixture.alpha)
        .upsert_loop(&Loop {
            name: "l1-supervisor".to_owned(),
            kind: LoopKind::L1,
            project: Some(fixture.alpha.clone()),
            interval_secs: 60,
            state: ActiveState::Active,
            bounds: Default::default(),
            last_run_at: Some(now),
            next_run_at: Some(now.plus(Duration::from_secs(60))),
            last_status: "degraded: merge".to_owned(),
            fail_streak: 3,
            updated_at: now,
        })
        .expect("supervisor loop row");

    let snapshot = fixture.snapshot();
    assert_eq!(
        snapshot.supervisor.fail_streak, 3,
        "the card mirrors loops['l1-supervisor'].fail_streak"
    );
}
