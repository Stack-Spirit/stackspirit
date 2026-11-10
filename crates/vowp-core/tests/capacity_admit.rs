//! E2 capacity admission, brakes, targets and fairness tests.
//! Spec: `docs/design/v0.2/home-and-surfaces.md` §6 and the E2 test list in §15.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tempfile::tempdir;
use vowp_core::capacity::{self, CapacityManager};
use vowp_core::{AdapterRegistry, Deps, FixedRng, SystemGit, SystemRunner};
use vowp_protocol::{
    AdmitDecision, CapacityTarget, Cost, EstimateSource, FakeClock, HarnessKind, HostSample, Job, JobId, JobKind,
    Limit, LimitRowKind, MemoryLog, NewJob, NodeName, Project, ProjectEntry, ProjectName, ProjectState, Role, Run,
    RunId, RunPurpose, RunRole, SampleSource, TargetMode, Timestamp, UsageEstimate, VowpConfig, WaitCode,
};
use vowp_store::{NodePaths, Store};

const NOW_TEXT: &str = "2026-11-10T12:00:00Z";
const GIB: u64 = 1024 * 1024 * 1024;

fn now() -> Timestamp {
    Timestamp::parse(NOW_TEXT).unwrap()
}

fn test_deps(root: &Path, project: &ProjectName) -> Deps {
    let mut config = VowpConfig::with_defaults_for_tests();
    config.project.name = project.clone();
    Deps {
        paths: NodePaths::new(root),
        config: Arc::new(config),
        node: NodeName::parse("local").unwrap(),
        clock: Arc::new(FakeClock::at(now())),
        log: Arc::new(MemoryLog::new()),
        adapters: Arc::new(AdapterRegistry::new()),
        windows: Arc::new(vowp_tmux::fake::FakeWindows::new()),
        git: Arc::new(SystemGit::default()),
        cmd: Arc::new(SystemRunner),
        rng: Arc::new(FixedRng(0)),
    }
}

fn store_at(root: &Path) -> Store {
    Store::open(&NodePaths::new(root))
        .unwrap()
        .with_clock(Arc::new(FakeClock::at(now())))
}

fn register_project(store: &Store, name: &ProjectName) {
    store
        .upsert_project(&Project {
            name: name.clone(),
            path: format!("/tmp/{name}"),
            remote_url: None,
            default_branch: "main".to_owned(),
            config: serde_json::json!({}),
            created_at: now(),
            updated_at: now(),
        })
        .unwrap();
}

fn insert_job(store: &Store, project: &ProjectName, id: &str) -> Job {
    let new = NewJob {
        id: JobId::parse(id).unwrap(),
        title: format!("job {id}"),
        kind: JobKind::Build,
        role: Role::Worker,
        profile: Some("worker".to_owned()),
        needs: Vec::new(),
        owns: vec!["src/**".to_owned()],
        reads: Vec::new(),
        gates: None,
        priority: None,
        loop_: None,
        brief_text: None,
    };
    store.insert_job(project, &new, "test").unwrap()
}

fn insert_open_run(store: &Store, job: &Job, id: &RunId) {
    let run = Run {
        id: id.clone(),
        job_id: Some(job.id.clone()),
        session_id: None,
        role: RunRole::Worker,
        purpose: RunPurpose::Start,
        profile: Some("worker".to_owned()),
        rung: 0,
        executor: Some(vowp_protocol::Executor::parse("opencode:deepseek/deepseek-flash").unwrap()),
        provider: Some("deepseek".to_owned()),
        node: NodeName::parse("local").unwrap(),
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
    };
    store.insert_run(&run).unwrap();
}

fn sample(
    at: Timestamp,
    mem_total: u64,
    mem_available: u64,
    swap_used: u64,
    pressure: Option<f64>,
    disk: u64,
    quota: Option<f64>,
) -> HostSample {
    HostSample {
        at,
        interval_secs: 5,
        mem_total_bytes: mem_total,
        mem_available_bytes: mem_available,
        swap_used_bytes: swap_used,
        load1: 0.1,
        load5: 0.1,
        load15: 0.1,
        cpu_count: 4,
        cpu_quota_cores: quota,
        disk_free_home_bytes: disk,
        disk_free_worktrees_bytes: None,
        pressure_cpu_avg10: None,
        pressure_mem_avg10: pressure,
        pressure_io_avg10: None,
        source: SampleSource::Proc,
    }
}

fn healthy_sample(at: Timestamp) -> HostSample {
    sample(at, 8 * GIB, 6 * GIB, 0, Some(0.0), 50 * GIB, Some(4.0))
}

fn seed_estimate(store: &Store, project: &ProjectName, ram_p90: u64, cpu_p90: f64) {
    store
        .upsert_usage_estimate(&UsageEstimate {
            project: Some(project.clone()),
            job_kind: Some(JobKind::Build),
            harness: Some(HarnessKind::Opencode),
            model: Some("deepseek/deepseek-flash".to_owned()),
            build_mode: None,
            samples: 5,
            ram_p50_bytes: ram_p90,
            ram_p90_bytes: ram_p90,
            cpu_p50_cores: cpu_p90,
            cpu_p90_cores: cpu_p90,
            duration_p50_ms: 1_000,
            duration_p90_ms: 2_000,
            source: EstimateSource::Measured,
        })
        .unwrap();
}

fn fixture() -> (tempfile::TempDir, Deps, Store, ProjectName, Job) {
    let tmp = tempdir().unwrap();
    let root = tmp.path().join("node");
    let project = ProjectName::parse("test").unwrap();
    let store = store_at(&root);
    register_project(&store, &project);
    let job = insert_job(&store, &project, "job-one");
    let deps = test_deps(&root, &project);
    (tmp, deps, store, project, job)
}

fn request(deps: &Deps, store: &Store, job: &Job) -> vowp_protocol::AdmitRequest {
    capacity::request_for(deps, store, job).unwrap()
}

fn wait_code(decision: AdmitDecision) -> WaitCode {
    match decision {
        AdmitDecision::Wait(reason) => reason.code,
        AdmitDecision::Admit => panic!("expected a wait decision, got admit"),
    }
}

#[test]
fn admit_blocks_when_ram_would_drop_below_reserve() {
    let (_tmp, deps, store, _project, job) = fixture();
    store
        .insert_host_sample(&sample(now(), 8 * GIB, 2 * GIB, 0, Some(0.0), 50 * GIB, Some(4.0)))
        .unwrap();
    let manager = CapacityManager::new(deps.clone());
    let decision = manager.admit(&store, &request(&deps, &store, &job), now()).unwrap();
    assert_eq!(wait_code(decision), WaitCode::Ram);
}

#[test]
fn admit_blocks_on_cpu_and_disk() {
    let (_tmp, deps, store, _project, job) = fixture();
    store
        .insert_host_sample(&sample(now(), 8 * GIB, 7 * GIB, 0, Some(0.0), 50 * GIB, Some(0.5)))
        .unwrap();
    let manager = CapacityManager::new(deps.clone());
    let decision = manager.admit(&store, &request(&deps, &store, &job), now()).unwrap();
    assert_eq!(wait_code(decision), WaitCode::Cpu);

    store
        .insert_host_sample(&sample(
            now().plus(Duration::from_secs(5)),
            8 * GIB,
            7 * GIB,
            0,
            Some(0.0),
            GIB,
            Some(4.0),
        ))
        .unwrap();
    let decision = manager.admit(&store, &request(&deps, &store, &job), now()).unwrap();
    assert_eq!(wait_code(decision), WaitCode::Disk);
}

#[test]
fn admit_blocks_on_provider_limit() {
    let (_tmp, deps, store, _project, job) = fixture();
    store.insert_host_sample(&healthy_sample(now())).unwrap();
    store
        .upsert_limit(&Limit {
            provider: "deepseek".to_owned(),
            harness: Some(HarnessKind::Opencode),
            kind: LimitRowKind::Rate,
            resets_at: None,
            detail: "429".to_owned(),
            since: now(),
            last_probe_at: None,
            probe_fail_streak: 0,
            concurrency_cap: None,
            cap_until: None,
        })
        .unwrap();
    let manager = CapacityManager::new(deps.clone());
    let decision = manager.admit(&store, &request(&deps, &store, &job), now()).unwrap();
    assert_eq!(wait_code(decision), WaitCode::Provider);
}

#[test]
fn admit_blocks_at_target() {
    let (_tmp, deps, store, _project, job) = fixture();
    store.insert_host_sample(&healthy_sample(now())).unwrap();
    let manager = CapacityManager::new(deps.clone());
    manager
        .set_target(&store, TargetMode::Fixed, 1, false, "owner", "one worker")
        .unwrap();
    insert_open_run(&store, &job, &RunId::new());
    let decision = manager.admit(&store, &request(&deps, &store, &job), now()).unwrap();
    assert_eq!(wait_code(decision), WaitCode::Target);
}

#[test]
fn brake_trips_on_memory_pressure() {
    let (_tmp, deps, store, _project, _job) = fixture();
    store
        .insert_host_sample(&sample(
            now().minus(Duration::from_secs(5)),
            8 * GIB,
            6 * GIB,
            0,
            Some(14.2),
            50 * GIB,
            Some(4.0),
        ))
        .unwrap();
    store
        .insert_host_sample(&sample(now(), 8 * GIB, 6 * GIB, 0, Some(15.0), 50 * GIB, Some(4.0)))
        .unwrap();
    let brake = capacity::brakes::evaluate(&store, now()).unwrap();
    assert!(brake.active);
    assert_eq!(brake.reasons[0].code, vowp_protocol::BrakeCode::MemoryPressure);
    assert_eq!(brake.since, Some(now()));
    let _ = deps;
}

#[test]
fn brake_clears_after_two_clean_samples() {
    let (_tmp, _deps, store, _project, _job) = fixture();
    store
        .insert_host_sample(&sample(
            now().minus(Duration::from_secs(5)),
            8 * GIB,
            6 * GIB,
            0,
            Some(14.2),
            50 * GIB,
            Some(4.0),
        ))
        .unwrap();
    store
        .insert_host_sample(&sample(now(), 8 * GIB, 6 * GIB, 0, Some(15.0), 50 * GIB, Some(4.0)))
        .unwrap();
    assert!(capacity::brakes::evaluate(&store, now()).unwrap().active);

    store
        .insert_host_sample(&healthy_sample(now().plus(Duration::from_secs(5))))
        .unwrap();
    let one_clean = capacity::brakes::evaluate(&store, now().plus(Duration::from_secs(5))).unwrap();
    assert!(one_clean.active, "one clean sample must not clear the brake");

    store
        .insert_host_sample(&healthy_sample(now().plus(Duration::from_secs(10))))
        .unwrap();
    let two_clean = capacity::brakes::evaluate(&store, now().plus(Duration::from_secs(10))).unwrap();
    assert!(!two_clean.active, "two clean samples clear the brake");
}

#[test]
fn brake_never_signals_running_work() {
    let (_tmp, _deps, store, _project, job) = fixture();
    let run_id = RunId::new();
    insert_open_run(&store, &job, &run_id);
    let initial_state = store.require_job(&job.id).unwrap().state;
    store
        .insert_host_sample(&sample(
            now().minus(Duration::from_secs(5)),
            8 * GIB,
            6 * GIB,
            0,
            Some(25.0),
            50 * GIB,
            Some(4.0),
        ))
        .unwrap();
    store
        .insert_host_sample(&sample(now(), 8 * GIB, 6 * GIB, 0, Some(30.0), 50 * GIB, Some(4.0)))
        .unwrap();
    let brake = capacity::brakes::evaluate(&store, now()).unwrap();
    assert!(brake.active);
    let runs = store.open_runs().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].id, run_id);
    assert!(runs[0].ended_at.is_none());
    assert_eq!(store.require_job(&job.id).unwrap().state, initial_state);
}

#[test]
fn owner_override_above_advice_is_honoured_and_shown() {
    let (_tmp, deps, store, project, _job) = fixture();
    store.insert_host_sample(&healthy_sample(now())).unwrap();
    seed_estimate(&store, &project, GIB, 1.0);
    let manager = CapacityManager::new(deps.clone());

    let advised = capacity::targets::current_for(&deps, &store, now()).unwrap();
    assert_eq!(
        advised.target_workers, 3,
        "6 GiB free minus 1 GiB reserve fits 5, CPU fits 3"
    );

    let target = manager
        .set_target(&store, TargetMode::Fixed, 8, true, "owner", "more workers")
        .unwrap();
    assert_eq!(target.target_workers, 8);
    assert_eq!(target.advised_workers, Some(3));
    assert!(target.is_override);
    let current = capacity::targets::current_for(&deps, &store, now()).unwrap();
    assert_eq!(current.mode, TargetMode::Fixed);
    assert_eq!(current.target_workers, 8);
    assert!(current.is_override);

    let refused = manager.set_target(&store, TargetMode::Fixed, 9, false, "owner", "too high");
    assert!(matches!(refused, Err(vowp_core::CoreError::Refused(_))));
    assert_eq!(
        refused.unwrap_err().kind(),
        vowp_protocol::ErrorKind::Refused,
        "CLI maps this to exit code 5"
    );
}

#[test]
fn capacity_set_history_and_json() {
    let (_tmp, deps, store, _project, _job) = fixture();
    store.insert_host_sample(&healthy_sample(now())).unwrap();
    let manager = CapacityManager::new(deps.clone());
    manager
        .set_target(&store, TargetMode::Fixed, 2, false, "owner", "two")
        .unwrap();
    let target = manager
        .set_target(&store, TargetMode::Fixed, 3, true, "owner", "three")
        .unwrap();
    assert_eq!(target.target_workers, 3);
    let latest = store.latest_capacity_target().unwrap().expect("history row");
    assert_eq!(latest.target_workers, 3);
    assert_eq!(latest.set_by, "owner");
    assert_eq!(latest.reason, "three");

    let report = manager.report(&store, now()).unwrap();
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["target"]["target_workers"], 3);
    assert_eq!(json["target"]["mode"], "fixed");
    assert!(json["generated_at"].is_string());
    assert!(json["estimates"].is_array());
    assert!(json["waiting"].is_array());
}

#[test]
fn fairness_round_robins_by_weight() {
    let entry = |name: &str, weight: u8| ProjectEntry {
        name: ProjectName::parse(name).unwrap(),
        path: format!("/tmp/{name}"),
        remote_url: None,
        default_branch: "main".to_owned(),
        state: ProjectState::Active,
        weight,
        paused_reason: String::new(),
        archived_at: None,
        created_at: now(),
        updated_at: now(),
    };
    let mut projects = vec![
        entry("alpha", 1),
        entry("bravo", 10),
        entry("charlie", 1),
        entry("delta", 5),
    ];
    capacity::fair::order(&mut projects, None);
    let names: Vec<&str> = projects.iter().map(|project| project.name.as_str()).collect();
    assert_eq!(names, ["bravo", "delta", "alpha", "charlie"]);

    let last = ProjectName::parse("alpha").unwrap();
    capacity::fair::order(&mut projects, Some(&last));
    let names: Vec<&str> = projects.iter().map(|project| project.name.as_str()).collect();
    assert_eq!(
        names,
        ["bravo", "delta", "charlie", "alpha"],
        "the last-admitted project moves to the back of its weight group"
    );
}

#[test]
fn waiting_reason_on_board_and_queue() {
    let (_tmp, deps, store, _project, job) = fixture();
    store
        .insert_host_sample(&sample(now(), 8 * GIB, 2 * GIB, 0, Some(0.0), 50 * GIB, Some(4.0)))
        .unwrap();
    let admitted = vowp_core::loops::dispatch::admit(&deps, &store, &job).unwrap();
    assert!(!admitted);
    let waiting_job = store.require_job(&job.id).unwrap();
    assert!(
        waiting_job.note.starts_with("waiting: RAM"),
        "note was {:?}",
        waiting_job.note
    );

    let snapshot = vowp_core::fleet::build_snapshot(&deps, &store, now()).unwrap();
    let rows = vowp_core::fleet::queue_rows(&snapshot);
    let row = rows.iter().find(|row| row.job == job.id).expect("job in the queue");
    assert_eq!(row.waiting_reason.as_deref(), Some(waiting_job.note.as_str()));

    store
        .insert_host_sample(&healthy_sample(now().plus(Duration::from_secs(5))))
        .unwrap();
    let admitted = vowp_core::loops::dispatch::admit(&deps, &store, &job).unwrap();
    assert!(admitted);
    assert_eq!(store.require_job(&job.id).unwrap().note, "");
}

#[test]
fn auto_target_uses_measured_limit() {
    let (_tmp, deps, store, project, _job) = fixture();
    store.insert_host_sample(&healthy_sample(now())).unwrap();
    seed_estimate(&store, &project, GIB, 1.0);
    let target = capacity::targets::current_for(&deps, &store, now()).unwrap();
    assert_eq!(target.mode, TargetMode::Auto);
    assert_eq!(target.target_workers, 3);
    assert_eq!(target.advised_workers, Some(3));

    let report = CapacityManager::new(deps.clone()).report(&store, now()).unwrap();
    assert_eq!(report.target.target_workers, 3);
    assert_eq!(report.reserve_ram_bytes, GIB);
    assert_eq!(report.min_disk_bytes, 5 * GIB);
}

#[test]
fn fallback_host_keeps_auto_at_one() {
    let (_tmp, _deps, store, _project, _job) = fixture();
    let mut fallback = healthy_sample(now());
    fallback.source = SampleSource::Fallback;
    fallback.mem_total_bytes = 0;
    fallback.mem_available_bytes = 0;
    store.insert_host_sample(&fallback).unwrap();
    let target = capacity::targets::current(&store).unwrap();
    assert_eq!(target.mode, TargetMode::Auto);
    assert_eq!(target.target_workers, 1);
    assert_eq!(
        target.set_by, "supervisor",
        "the fallback target is labelled as the supervisor's, not measured"
    );
}

#[test]
fn capacity_target_serializes_as_the_snapshot_expects() {
    let target = CapacityTarget {
        mode: TargetMode::Fixed,
        target_workers: 8,
        advised_workers: Some(5),
        is_override: true,
        set_by: "owner".to_owned(),
        set_at: now(),
        reason: "override".to_owned(),
    };
    let json = serde_json::to_value(&target).unwrap();
    assert_eq!(json["mode"], "fixed");
    assert_eq!(json["target_workers"], 8);
    assert_eq!(json["advised_workers"], 5);
    assert_eq!(json["is_override"], true);
}
