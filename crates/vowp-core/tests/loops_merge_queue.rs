//! Merge-queue tests against a real bare origin and clone.
//! Spec: `docs/design/crates/vowp-core.md` §7 and `docs/design/loops.md` §6.

#[path = "loops_support.rs"]
mod h;

use h::*;
use vowp_core::loops::merge_queue::{MergeQueue, MergeStep};
use vowp_protocol::{JobKind, JobState, LedgerKind, Role};
use vowp_store::LedgerFilter;

fn merging_job(node: &TestNode, id: &str) -> vowp_protocol::Job {
    let job = node.insert_job(id, JobKind::Build, Role::Worker, Some(vec![]));
    let worktree = node.worktree_with_file(&job, "src/lib.rs", &format!("pub fn {id}() {{}}\n"));
    let text = report_markdown(id, &format!("job/{id}"), "done", None, &[]);
    let report = worktree.join(&job.report);
    std::fs::create_dir_all(report.parent().expect("parent")).expect("report dir");
    std::fs::write(&report, text).expect("report");
    node.force_state(&job, JobState::Merging);
    node.store().require_job(&job.id).expect("job")
}

#[test]
fn merge_queue_happy_path_ff_push_to_local_bare_origin() {
    let node = TestNode::new_real_git();
    let job = merging_job(&node, "happy");
    let queue = MergeQueue::new(node.deps.clone());
    let step = queue.step(&node.store()).expect("merge step");
    assert_eq!(step, MergeStep::Merged(job.id.clone()));

    let origin = node.tmp.path().join("origin.git");
    let shown = node.git(&origin, &["show", "main:src/lib.rs"]);
    assert!(shown.contains("pub fn happy"));
    let ledger = node
        .store()
        .ledger_since(&LedgerFilter {
            kind: Some(LedgerKind::Merge),
            ..LedgerFilter::default()
        })
        .expect("ledger");
    assert_eq!(ledger.len(), 1);
    assert_eq!(ledger[0].job_id.as_ref(), Some(&job.id));
    assert_eq!(
        ledger[0].detail.get("lockfile_refreshed"),
        Some(&serde_json::Value::Bool(false))
    );
}

#[test]
fn merge_queue_conflict_creates_merge_job() {
    let node = TestNode::new_real_git();
    let first = node.insert_job("first", JobKind::Build, Role::Worker, Some(vec![]));
    node.worktree_with_file(&first, "src/shared.rs", "pub fn value() -> u8 { 1 }\n");
    node.force_state(&first, JobState::Merging);
    let second = node.insert_job("second", JobKind::Build, Role::Worker, Some(vec![]));
    let second_tree = node.worktree_with_file(&second, "src/shared.rs", "pub fn value() -> u8 { 2 }\n");
    node.force_state(&second, JobState::Merging);

    let mut supervisor = node.supervisor();
    // The oldest merging job is merged first.
    node.tick_settled(&mut supervisor);
    let store = node.store();
    assert_eq!(store.require_job(&first.id).expect("first").state, JobState::Done);
    // Now the second job hits a real rebase conflict.
    node.tick_settled(&mut supervisor);
    let job = store.require_job(&second.id).expect("second");
    assert_eq!(job.state, JobState::Fixing);
    let merge_job = store
        .require_job(&vowp_protocol::JobId::parse("second-merge-1").expect("id"))
        .expect("merge job");
    assert_eq!(merge_job.kind, JobKind::Merge);
    assert_eq!(merge_job.worktree, Some(second_tree.to_string_lossy().into_owned()));
    assert_eq!(merge_job.branch, job.branch);
    assert!(
        store
            .edges_of(&merge_job.id)
            .expect("edges")
            .iter()
            .any(|edge| edge.kind == vowp_protocol::EdgeKind::Fixes && edge.dst == second.id)
    );
}

#[test]
fn merge_queue_cargo_lock_regenerated_not_merged() {
    let node = TestNode::new_real_git();
    let first = node.insert_job("lock-a", JobKind::Build, Role::Worker, Some(vec![]));
    node.worktree_with_file(&first, "Cargo.lock", "version = 3\n\"a\" = 1\n");
    node.force_state(&first, JobState::Merging);
    let second = node.insert_job("lock-b", JobKind::Build, Role::Worker, Some(vec![]));
    node.worktree_with_file(&second, "Cargo.lock", "version = 3\n\"b\" = 2\n");
    node.force_state(&second, JobState::Merging);

    let mut supervisor = node.supervisor();
    node.tick_settled(&mut supervisor);
    node.tick_settled(&mut supervisor);
    let store = node.store();
    let job = store.require_job(&second.id).expect("second");
    assert_eq!(job.state, JobState::Done);
    let merge_jobs = store
        .list_jobs(&vowp_store::JobFilter {
            kind: Some(JobKind::Merge),
            ..vowp_store::JobFilter::default()
        })
        .expect("merge jobs");
    assert!(merge_jobs.is_empty(), "lockfile-only must not create a merge job");
    let ledger = store
        .ledger_since(&LedgerFilter {
            kind: Some(LedgerKind::Merge),
            ..LedgerFilter::default()
        })
        .expect("ledger");
    let second_row = ledger
        .iter()
        .find(|entry| entry.job_id.as_ref() == Some(&second.id))
        .expect("second merge row");
    assert_eq!(
        second_row.detail.get("lockfile_refreshed"),
        Some(&serde_json::Value::Bool(true))
    );
    let origin = node.tmp.path().join("origin.git");
    // The queue takes the main version of a lockfile conflict (a real cargo update
    // would then re-add the branch's dependency set).
    let shown = node.git(&origin, &["show", "main:Cargo.lock"]);
    assert!(shown.contains("\"a\" = 1"));
}

#[test]
fn merge_queue_check_failure_returns_job_to_fixing() {
    let adapter = std::sync::Arc::new(ScriptAdapter::always(
        vowp_protocol::HarnessKind::Opencode,
        ScriptAction::ok("fixing checks"),
    ));
    let node = TestNode::with_adapter_config(true, adapter, |config| {
        config.merge.check_cmds = vec!["false".to_owned()];
    });
    let job = merging_job(&node, "checks-fail");
    let mut supervisor = node.supervisor();
    let report = node.tick_settled(&mut supervisor);
    assert!(report.steps.iter().any(|step| step.name == "merge" && step.ok));
    let store = node.store();
    let job = store.require_job(&job.id).expect("job");
    assert_eq!(job.state, JobState::Fixing);
    assert_eq!(job.attempts, 1);
    let fix = store
        .require_job(&vowp_protocol::JobId::parse("checks-fail-fix-1").expect("id"))
        .expect("fix job");
    assert_eq!(fix.kind, JobKind::Fix);
}
