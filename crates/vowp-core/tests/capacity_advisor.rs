//! E3 capacity advisor tests.
//!
//! Spec: `docs/design/v0.2/home-and-surfaces.md` §6.4 and the E3 test list in §15. Every
//! test uses a temporary node home, a fake clock and fake adapters; nothing reads the real
//! `~/.vowp`, the network or a real harness.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tempfile::tempdir;
use vowp_core::capacity::advisor::ADVICE_INTERVAL;
use vowp_core::capacity::{self, CapacityManager};
use vowp_core::{AdapterRegistry, Deps, FixedRng, SystemGit, SystemRunner};
use vowp_protocol::{
    AdviceState, CapacityTarget, EstimateSource, FakeClock, HarnessKind, HostSample, InboxKind, InboxState, Job, JobId,
    JobKind, MemoryLog, NewJob, NodeName, Project, ProjectName, Role, SampleSource, TargetMode, Timestamp,
    UsageEstimate, VowpConfig,
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

fn sample(at: Timestamp, mem_total: u64, mem_available: u64, quota: Option<f64>) -> HostSample {
    HostSample {
        at,
        interval_secs: 5,
        mem_total_bytes: mem_total,
        mem_available_bytes: mem_available,
        swap_used_bytes: 0,
        load1: 0.1,
        load5: 0.1,
        load15: 0.1,
        cpu_count: 4,
        cpu_quota_cores: quota,
        disk_free_home_bytes: 50 * GIB,
        disk_free_worktrees_bytes: None,
        pressure_cpu_avg10: Some(0.0),
        pressure_mem_avg10: Some(0.0),
        pressure_io_avg10: None,
        source: SampleSource::Proc,
    }
}

fn healthy_sample(at: Timestamp) -> HostSample {
    sample(at, 8 * GIB, 6 * GIB, Some(4.0))
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

#[test]
fn advisor_writes_capacity_advice_on_timer_and_events() {
    let (_tmp, deps, store, project, _job) = fixture();
    store.insert_host_sample(&healthy_sample(now())).unwrap();
    seed_estimate(&store, &project, GIB, 1.0);

    let first = capacity::maybe_advise(&deps, &store, now(), capacity::AdviceTrigger::Timer)
        .unwrap()
        .expect("the first timer turn writes advice");
    assert_eq!(first.state, AdviceState::Open);
    assert_eq!(first.current_target, 3);
    assert_eq!(first.proposed_target, 3);

    let mut ids = HashSet::new();
    ids.insert(first.id.clone());
    for (offset, trigger) in [
        (1, capacity::AdviceTrigger::NewProject),
        (2, capacity::AdviceTrigger::Brake),
        (3, capacity::AdviceTrigger::Oom),
        (4, capacity::AdviceTrigger::Idle),
    ] {
        let at = now().plus(Duration::from_secs(offset));
        let advice = capacity::maybe_advise(&deps, &store, at, trigger)
            .unwrap()
            .unwrap_or_else(|| panic!("{trigger:?} must write advice"));
        assert_eq!(advice.created_at, at);
        assert!(ids.insert(advice.id.clone()), "every advice row has a unique id");
        let stored = store.capacity_advice(&advice.id).unwrap();
        assert_eq!(stored, Some(advice));
    }
}

#[test]
fn advisor_creates_decision_inbox_item_with_apply_keep() {
    let (_tmp, deps, store, project, _job) = fixture();
    store.insert_host_sample(&healthy_sample(now())).unwrap();
    seed_estimate(&store, &project, GIB, 1.0);

    let advice = capacity::maybe_advise(&deps, &store, now(), capacity::AdviceTrigger::Timer)
        .unwrap()
        .expect("advice");
    let inbox_id = advice.inbox_id.clone().expect("advice links an inbox item");
    let item = store.inbox_item(&inbox_id).unwrap().expect("inbox item exists");
    assert_eq!(item.kind, InboxKind::Decision);
    assert_eq!(item.project, None, "capacity advice is node-wide");
    assert_eq!(item.options, vec!["apply".to_owned(), "keep".to_owned()]);
    assert_eq!(
        item.title,
        format!(
            "Capacity: {} workers (advised {})",
            advice.proposed_target, advice.proposed_target
        )
    );
    assert!(
        item.body.contains("vowp capacity set --target"),
        "body explains the owner action: {}",
        item.body
    );
    assert!(
        store
            .list_inbox(Some(InboxState::Open))
            .unwrap()
            .iter()
            .any(|open| open.id == inbox_id),
        "the decision item is open"
    );
}

#[test]
fn advisor_never_changes_target_itself() {
    let (_tmp, deps, store, project, _job) = fixture();
    store.insert_host_sample(&healthy_sample(now())).unwrap();
    seed_estimate(&store, &project, GIB, 1.0);
    let owner_target = CapacityTarget {
        mode: TargetMode::Fixed,
        target_workers: 8,
        advised_workers: Some(3),
        is_override: true,
        set_by: "owner".to_owned(),
        set_at: now(),
        reason: "burst".to_owned(),
    };
    store.insert_capacity_target(&owner_target).unwrap();
    let before = store.latest_capacity_target().unwrap().expect("owner target");

    let advice = capacity::maybe_advise(
        &deps,
        &store,
        now().plus(Duration::from_secs(1)),
        capacity::AdviceTrigger::Brake,
    )
    .unwrap()
    .expect("advice");
    assert_eq!(advice.current_target, 8);
    assert_eq!(advice.proposed_target, 3);
    assert_eq!(advice.state, AdviceState::Open);
    assert_eq!(
        store.latest_capacity_target().unwrap().expect("target row remains"),
        before,
        "the advisor must not write a target row or change the owner's number"
    );
}

#[test]
fn advisor_applies_only_after_owner_yes() {
    let (_tmp, deps, store, project, _job) = fixture();
    store.insert_host_sample(&healthy_sample(now())).unwrap();
    seed_estimate(&store, &project, GIB, 1.0);

    let advice = capacity::maybe_advise(&deps, &store, now(), capacity::AdviceTrigger::Timer)
        .unwrap()
        .expect("advice");
    assert_eq!(store.latest_capacity_target().unwrap(), None);
    let inbox_id = advice.inbox_id.clone().expect("linked item");
    let answered = store.answer_inbox(&inbox_id, "apply", "owner").unwrap();
    assert_eq!(answered.state, InboxState::Answered);
    assert_eq!(
        store.latest_capacity_target().unwrap(),
        None,
        "answering apply is not a target change"
    );

    let manager = CapacityManager::new(deps.clone());
    let target = manager
        .set_target(
            &store,
            TargetMode::Fixed,
            advice.proposed_target,
            false,
            "owner",
            "apply capacity advice",
        )
        .unwrap();
    assert_eq!(target.target_workers, advice.proposed_target);
    let applied = store.capacity_advice(&advice.id).unwrap().expect("advice row");
    assert_eq!(applied.state, AdviceState::Applied);
    assert_eq!(applied.applied_by.as_deref(), Some("owner"));
    assert!(applied.applied_at.is_some());
}

#[test]
fn advisor_report_json_includes_estimates_and_waiting() {
    let (_tmp, deps, store, project, job) = fixture();
    store.insert_host_sample(&healthy_sample(now())).unwrap();
    seed_estimate(&store, &project, GIB, 1.0);
    store.set_job_note(&job.id, "waiting: RAM 1.2 GB short").unwrap();

    let advice = capacity::maybe_advise(&deps, &store, now(), capacity::AdviceTrigger::Timer)
        .unwrap()
        .expect("advice");
    assert!(
        advice
            .reasons
            .iter()
            .any(|reason| reason.starts_with("estimates: 1 group(s), 5 sample(s)")),
        "reasons: {:?}",
        advice.reasons
    );
    assert!(
        advice
            .reasons
            .iter()
            .any(|reason| reason.starts_with("waiting: 1 job(s); first job-one")),
        "reasons: {:?}",
        advice.reasons
    );

    let report = CapacityManager::new(deps.clone()).report(&store, now()).unwrap();
    let json = serde_json::to_value(&report).unwrap();
    assert!(
        json["estimates"].as_array().is_some_and(|values| !values.is_empty()),
        "the advisor's report carries estimates"
    );
    assert!(
        json["waiting"].as_array().is_some_and(|values| !values.is_empty()),
        "the advisor's report carries waiting jobs"
    );
    assert!(json["target"]["advised_workers"].is_number());
}

#[test]
fn advisor_respects_the_thirty_minute_gap() {
    let (_tmp, deps, store, project, _job) = fixture();
    store.insert_host_sample(&healthy_sample(now())).unwrap();
    seed_estimate(&store, &project, GIB, 1.0);

    let first = capacity::maybe_advise(&deps, &store, now(), capacity::AdviceTrigger::Timer)
        .unwrap()
        .expect("first advice");
    assert!(
        capacity::maybe_advise(
            &deps,
            &store,
            now().plus(ADVICE_INTERVAL - Duration::from_secs(1)),
            capacity::AdviceTrigger::Timer,
        )
        .unwrap()
        .is_none(),
        "the timer must not advise again inside the gap"
    );
    let second = capacity::maybe_advise(
        &deps,
        &store,
        now().plus(ADVICE_INTERVAL),
        capacity::AdviceTrigger::Timer,
    )
    .unwrap()
    .expect("the timer advises again at thirty minutes");
    assert_ne!(first.id, second.id);
    assert_eq!(second.created_at, now().plus(ADVICE_INTERVAL));
    assert!(
        capacity::maybe_advise(
            &deps,
            &store,
            now().plus(ADVICE_INTERVAL + Duration::from_secs(1)),
            capacity::AdviceTrigger::Timer,
        )
        .unwrap()
        .is_none(),
        "the second advice starts a new gap"
    );
}

#[test]
fn advisor_dismisses_a_kept_advice() {
    let (_tmp, deps, store, project, _job) = fixture();
    store.insert_host_sample(&healthy_sample(now())).unwrap();
    seed_estimate(&store, &project, GIB, 1.0);

    let advice = capacity::maybe_advise(&deps, &store, now(), capacity::AdviceTrigger::Timer)
        .unwrap()
        .expect("advice");
    let inbox_id = advice.inbox_id.clone().expect("linked item");
    store.answer_inbox(&inbox_id, "keep", "owner").unwrap();
    assert!(
        capacity::maybe_advise(
            &deps,
            &store,
            now().plus(ADVICE_INTERVAL),
            capacity::AdviceTrigger::Timer,
        )
        .unwrap()
        .is_none(),
        "keep closes the proposal and starts a fresh gap instead of re-advising"
    );
    let dismissed = store.capacity_advice(&advice.id).unwrap().expect("advice row");
    assert_eq!(dismissed.state, AdviceState::Dismissed);
    assert_eq!(store.latest_capacity_target().unwrap(), None);
}

#[test]
fn advisor_keeps_the_owner_target_without_a_measured_host() {
    let (_tmp, deps, store, _project, _job) = fixture();
    let mut fallback = healthy_sample(now());
    fallback.source = SampleSource::Fallback;
    fallback.mem_total_bytes = 0;
    fallback.mem_available_bytes = 0;
    store.insert_host_sample(&fallback).unwrap();
    store
        .insert_capacity_target(&CapacityTarget {
            mode: TargetMode::Fixed,
            target_workers: 8,
            advised_workers: Some(1),
            is_override: true,
            set_by: "owner".to_owned(),
            set_at: now(),
            reason: "macOS target".to_owned(),
        })
        .unwrap();

    let advice = capacity::maybe_advise(&deps, &store, now(), capacity::AdviceTrigger::Timer)
        .unwrap()
        .expect("advice");
    assert_eq!(advice.current_target, 8);
    assert_eq!(
        advice.proposed_target, 8,
        "a fallback host must not shrink the owner's fixed target"
    );
    assert!(
        advice.reasons.iter().any(|reason| reason.contains("fallback sample")),
        "reasons: {:?}",
        advice.reasons
    );
}
