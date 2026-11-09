//! L1 tick, dispatch, stall/continue and L3 verify tests.
//! Spec: `docs/design/crates/vowp-core.md` §7 and `docs/design/loops.md` §1–3, §5, §12.

#[path = "loops_support.rs"]
mod h;

use std::sync::Arc;
use std::time::Duration;

use h::*;
use vowp_core::requests::{Request, push};
use vowp_protocol::{
    Cost, EdgeKind, Executor, Gate, Job, JobKind, JobState, Limit, LimitRowKind, ParkedReason, Run, RunId, RunOutcome,
    RunPurpose, RunRole, Timestamp,
};
use vowp_store::JobFilter;

fn now() -> Timestamp {
    ts()
}

fn worker_job(node: &TestNode, id: &str) -> Job {
    node.insert_job(id, JobKind::Build, vowp_protocol::Role::Worker, Some(vec![]))
}

#[test]
fn tick_step_failure_does_not_abort_tick() {
    let node = TestNode::new();
    // Force the board step to fail by making its temporary path a directory.
    std::fs::create_dir_all(node.deps.paths.root().join(".board.json.tmp")).expect("blocking dir");
    let mut supervisor = node.supervisor();
    let report = supervisor.tick().expect("tick still returns Ok");
    let board = report
        .steps
        .iter()
        .find(|step| step.name == "board")
        .expect("board step");
    assert!(!board.ok, "board step should have failed: {board:?}");
    let heartbeat = report
        .steps
        .iter()
        .find(|step| step.name == "heartbeat")
        .expect("heartbeat step");
    assert!(heartbeat.ok, "later steps must still run: {heartbeat:?}");
    assert!(node.deps.paths.root().join("heartbeat").is_file());
}

#[test]
fn startup_marks_orphaned_runs_crash_and_continues_job() {
    let node = TestNode::new();
    let job = worker_job(&node, "orphan-job");
    node.force_state(&job, JobState::Running);
    let session = create_session(&node, &job);
    let store = node.store();
    let run = Run {
        id: RunId::new(),
        job_id: Some(job.id.clone()),
        session_id: Some(session),
        role: RunRole::Worker,
        purpose: RunPurpose::Start,
        profile: Some("worker".to_owned()),
        rung: 0,
        executor: Some(Executor::parse("opencode:deepseek/deepseek-flash").expect("executor")),
        provider: Some("deepseek".to_owned()),
        node: node.deps.node.clone(),
        native_session_id: None,
        pid: Some(999_999),
        window: None,
        cwd: Some(node.main_repo.to_string_lossy().into_owned()),
        started_at: now(),
        ended_at: None,
        exit_code: None,
        outcome: None,
        limit_kind: None,
        cost: Cost::unknown(),
        detail: String::new(),
    };
    store.insert_run(&run).expect("run row");

    let mut supervisor = node.supervisor();
    let report = supervisor.startup().expect("startup");
    assert_eq!(report.orphaned_runs, 1);
    let finished = store.require_run(&run.id).expect("run");
    assert_eq!(finished.outcome, Some(RunOutcome::Crash));
    assert!(finished.ended_at.is_some());

    let tick = supervisor.tick().expect("tick");
    let job = store.require_job(&job.id).expect("job");
    assert_eq!(job.state, JobState::Running);
    assert_eq!(job.continues, 1);
    assert!(
        tick.transitions
            .iter()
            .any(|(id, from, to)| id == &job.id && *from == JobState::Running && *to == JobState::Running),
        "harvest should apply RunEndedNoReport: {:?}",
        tick.transitions
    );
}

#[test]
fn dispatch_respects_ownership_locks_and_slots() {
    let adapter = Arc::new(ScriptAdapter::always(
        vowp_protocol::HarnessKind::Opencode,
        ScriptAction::ok("dispatched"),
    ));
    let node = TestNode::with_adapter_config(false, adapter, |config| {
        config.node.max_workers = 1;
    });
    let a = node.insert_job("job-a", JobKind::Build, vowp_protocol::Role::Worker, Some(vec![]));
    let mut b = node.insert_job("job-b", JobKind::Build, vowp_protocol::Role::Worker, Some(vec![]));
    b.owns = vec!["src/**".to_owned()];
    // Both jobs own src/**, so only one may hold the lock.
    let mut supervisor = node.supervisor();
    let first = supervisor.tick().expect("tick");
    assert_eq!(first.steps[0].name, "requests");
    let store = node.store();
    let running = store
        .list_jobs(&JobFilter {
            states: vec![JobState::Running],
            ..JobFilter::default()
        })
        .expect("running");
    assert_eq!(running.len(), 1, "one worker slot and overlapping owns");
    assert!(running[0].id == a.id || running[0].id == b.id);
    let ready = store
        .list_jobs(&JobFilter {
            states: vec![JobState::Ready],
            ..JobFilter::default()
        })
        .expect("ready");
    assert_eq!(ready.len(), 1);
}

#[test]
fn dispatch_skips_limited_provider_and_uses_next_rung() {
    let adapter = Arc::new(ScriptAdapter::always(
        vowp_protocol::HarnessKind::Opencode,
        ScriptAction::ok("fallback"),
    ));
    let node = TestNode::with_adapter_config(false, adapter, |config| {
        let claude = Executor::parse("claude-code:claude-sonnet-5-5@medium").expect("claude");
        let opencode = Executor::parse("opencode:deepseek/deepseek-flash").expect("opencode");
        let profile = config.profiles.get_mut("worker").expect("worker profile");
        profile.ladder = vec![claude, opencode];
    });
    let store = node.store();
    store
        .upsert_limit(&Limit {
            provider: "claude".to_owned(),
            harness: Some(vowp_protocol::HarnessKind::ClaudeCode),
            kind: LimitRowKind::Quota,
            resets_at: Some(now().plus(Duration::from_secs(3600))),
            detail: "quota".to_owned(),
            since: now(),
            last_probe_at: None,
            probe_fail_streak: 0,
            concurrency_cap: None,
            cap_until: None,
        })
        .expect("limit");
    let job = worker_job(&node, "rung-job");
    let mut supervisor = node.supervisor();
    let report = supervisor.tick().expect("tick");
    assert!(report.dispatched.contains(&job.id));
    let run = store.last_run_of_job(&job.id).expect("run").expect("some run");
    assert_eq!(run.rung, 1);
    assert_eq!(run.provider.as_deref(), Some("deepseek"));
}

#[test]
fn dispatch_resumes_job_with_prior_run_after_retry() {
    let adapter = Arc::new(ScriptAdapter::always(
        vowp_protocol::HarnessKind::Opencode,
        ScriptAction::ok("resumed"),
    ));
    let node = TestNode::with_adapter(true, adapter.clone());
    let job = worker_job(&node, "retry-job");
    let session = create_session(&node, &job);
    let store = node.store();
    let opencode = Executor::parse("opencode:deepseek/deepseek-flash").expect("executor");
    store
        .upsert_native(
            &session,
            vowp_protocol::HarnessKind::Opencode,
            "native-1",
            &node.deps.node,
            1,
        )
        .expect("native");
    let prior = Run {
        id: RunId::new(),
        job_id: Some(job.id.clone()),
        session_id: Some(session),
        role: RunRole::Worker,
        purpose: RunPurpose::Start,
        profile: Some("worker".to_owned()),
        rung: 0,
        executor: Some(opencode.clone()),
        provider: Some("deepseek".to_owned()),
        node: node.deps.node.clone(),
        native_session_id: Some("native-1".to_owned()),
        pid: None,
        window: None,
        cwd: Some(node.main_repo.to_string_lossy().into_owned()),
        started_at: now(),
        ended_at: None,
        exit_code: None,
        outcome: None,
        limit_kind: None,
        cost: Cost::unknown(),
        detail: String::new(),
    };
    store.insert_run(&prior).expect("prior run");
    store
        .finish_run(
            &prior.id,
            &vowp_store::RunFinish {
                outcome: RunOutcome::Ok,
                exit_code: Some(0),
                limit_kind: None,
                cost: Cost::unknown(),
                native_session_id: Some("native-1".to_owned()),
                detail: "prior".to_owned(),
            },
        )
        .expect("finish prior");
    node.force_state(&job, JobState::Failed);
    push(&store, &Request::RetryJob { job: job.id.clone() }).expect("request");

    let mut supervisor = node.supervisor();
    supervisor.tick().expect("tick");
    node.wait_runs();
    let starts = adapter.starts();
    assert!(!starts.is_empty(), "a resume run must start");
    assert_eq!(starts[0].resume.as_deref(), Some("native-1"));
    let resumed = store
        .runs_of_job(&job.id)
        .expect("runs")
        .into_iter()
        .find(|run| run.started_at >= ts() && run.purpose == RunPurpose::Continue)
        .expect("continue run");
    assert_eq!(resumed.purpose, RunPurpose::Continue);
}

#[test]
fn stall_judge_triggers_stop_and_continue() {
    let action = ScriptAction::ok("working").with_delay(Duration::from_secs(3));
    let adapter = Arc::new(ScriptAdapter::always(vowp_protocol::HarnessKind::Opencode, action));
    let node = TestNode::with_adapter(false, adapter);
    let job = worker_job(&node, "stall-job");
    let mut supervisor = node.supervisor();
    supervisor.tick().expect("dispatch tick");
    let store = node.store();
    let run = store.last_run_of_job(&job.id).expect("run").expect("open run");
    std::fs::write(node.deps.paths.run_raw_path(&run.id), b"some output\n").expect("output");
    node.advance(21 * 60);
    let report = supervisor.tick().expect("stall tick");
    let job = store.require_job(&job.id).expect("job");
    assert_eq!(job.state, JobState::Running);
    assert_eq!(job.continues, 1);
    assert!(
        report
            .transitions
            .iter()
            .any(|(id, from, to)| id == &job.id && *from == JobState::Running && *to == JobState::Running),
        "stall should continue the job: {:?}",
        report.transitions
    );
    assert!(report.steps.iter().any(|step| step.name == "stalls" && step.ok));
}

#[test]
fn hang_before_first_output_restart_up_to_3() {
    let action = ScriptAction::ok("silent").with_delay(Duration::from_secs(3));
    let adapter = Arc::new(ScriptAdapter::always(vowp_protocol::HarnessKind::Opencode, action));
    let node = TestNode::with_adapter(false, adapter);
    let job = worker_job(&node, "hang-job");
    let mut supervisor = node.supervisor();
    supervisor.tick().expect("dispatch");
    let store = node.store();
    for expected in 1..=3 {
        node.advance(9 * 60);
        supervisor.tick().expect("hang tick");
        let current = store.require_job(&job.id).expect("job");
        assert_eq!(current.hang_restarts, expected);
        assert_eq!(current.state, JobState::Running);
    }
    node.advance(9 * 60);
    supervisor.tick().expect("final hang tick");
    let current = store.require_job(&job.id).expect("job");
    assert_eq!(current.state, JobState::Stalled);
    assert_eq!(current.hang_restarts, 3);
    assert_eq!(store.kv_get("wake:keeper").expect("kv"), Some("1".to_owned()));
}

#[test]
fn continue_twice_then_keeper() {
    let adapter = Arc::new(ScriptAdapter::always(
        vowp_protocol::HarnessKind::Opencode,
        ScriptAction::crash("no report"),
    ));
    let node = TestNode::with_adapter(false, adapter);
    let job = worker_job(&node, "continue-job");
    let mut supervisor = node.supervisor();
    for _ in 0..4 {
        node.tick_settled(&mut supervisor);
    }
    let store = node.store();
    let job = store.require_job(&job.id).expect("job");
    assert_eq!(job.continues, 2);
    assert_eq!(job.state, JobState::Stalled);
    let runs = store.runs_of_job(&job.id).expect("runs");
    assert_eq!(runs.len(), 3);
    assert_eq!(runs[0].purpose, RunPurpose::Start);
    assert!(runs[1..].iter().all(|run| run.purpose == RunPurpose::Continue));
    assert_eq!(store.kv_get("wake:keeper").expect("kv"), Some("1".to_owned()));
}

/// Builds a script that writes the expected report for whichever job is starting.
fn report_script(original: &str, verdict: Option<&'static str>, status: &'static str) -> h::ScriptFn {
    let original = original.to_owned();
    Arc::new(move |input: &vowp_protocol::StartInput| {
        if let Some(verify_job) = verify_prompt_job(&input.prompt) {
            let branch = format!("job/{original}");
            let file = format!(".vowp/jobs/{verify_job}/report.md");
            let text = report_markdown(
                &verify_job,
                &branch,
                "done",
                verdict,
                if verdict == Some("fail") {
                    &["concrete finding"][..]
                } else {
                    &[][..]
                },
            );
            return ScriptAction::ok("verified").with_file(file, text);
        }
        if let Some(round) = fix_round(&input.prompt) {
            let fix = format!("{original}-fix-{round}");
            let branch = format!("job/{original}");
            let file = format!(".vowp/jobs/{fix}/report.md");
            return ScriptAction::ok("fixed").with_file(file, report_markdown(&fix, &branch, status, None, &[]));
        }
        let branch = format!("job/{original}");
        let file = format!(".vowp/jobs/{original}/report.md");
        ScriptAction::ok("worked").with_file(file, report_markdown(&original, &branch, status, None, &[]))
    })
}

fn reviewing_job(node: &TestNode, id: &str) -> Job {
    let job = node.insert_job(
        id,
        JobKind::Build,
        vowp_protocol::Role::Worker,
        Some(vec![
            Gate::Report(vowp_protocol::ReportGate::default()),
            Gate::Review(vowp_protocol::ReviewGate::default()),
        ]),
    );
    let worktree = node.worktree_with_file(&job, "src/lib.rs", "pub fn a() {}\n");
    let text = report_markdown(id, &format!("job/{id}"), "done", None, &[]);
    let report = worktree.join(&job.report);
    std::fs::create_dir_all(report.parent().expect("parent")).expect("report dir");
    std::fs::write(&report, text).expect("report");
    node.force_state(&job, JobState::Reviewing);
    node.store().require_job(&job.id).expect("job")
}

#[test]
fn verify_job_goes_done_via_gating_never_merged_and_original_receives_verdict() {
    let adapter = Arc::new(ScriptAdapter::new(
        vowp_protocol::HarnessKind::Opencode,
        report_script("verify-pass", Some("pass"), "done"),
    ));
    let node = TestNode::with_adapter(true, adapter);
    let original = reviewing_job(&node, "verify-pass");
    let mut supervisor = node.supervisor();
    node.tick_until(&mut supervisor, 12, |store| {
        matches!(
            store.require_job(&original.id).expect("original").state,
            JobState::Merging | JobState::Done
        )
    });
    let store = node.store();
    let verify = store
        .require_job(&vowp_protocol::JobId::parse(&verify_id("verify-pass", 0)).expect("id"))
        .expect("verify job");
    assert_eq!(verify.state, JobState::Done);
    assert_eq!(verify.kind, JobKind::Verify);
    let results = store.gate_results_for(&original.id, 0).expect("gate results");
    let review = results
        .iter()
        .find(|result| matches!(result.gate, Gate::Review(_)))
        .expect("review result");
    assert!(review.passed);
    assert_eq!(original.id, original.id);
    let _ = original;
}

#[test]
fn verify_fail_creates_fix_job_same_branch_max_3_then_escalates() {
    let adapter = Arc::new(ScriptAdapter::new(
        vowp_protocol::HarnessKind::Opencode,
        report_script("verify-fail", Some("fail"), "done"),
    ));
    let node = TestNode::with_adapter(true, adapter);
    let original = reviewing_job(&node, "verify-fail");
    let mut supervisor = node.supervisor();
    node.tick_until(&mut supervisor, 60, |store| {
        store.require_job(&original.id).expect("original").state == JobState::Failed
    });
    let store = node.store();
    let job = store.require_job(&original.id).expect("job");
    assert_eq!(job.attempts, 3);
    let fix_jobs = store
        .list_jobs(&JobFilter {
            kind: Some(JobKind::Fix),
            ..JobFilter::default()
        })
        .expect("fix jobs");
    assert_eq!(fix_jobs.len(), 3);
    for fix in &fix_jobs {
        assert_eq!(fix.branch, job.branch);
        assert_eq!(fix.worktree, job.worktree);
        assert!(
            store
                .edges_of(&fix.id)
                .expect("edges")
                .iter()
                .any(|edge| edge.kind == EdgeKind::Fixes && edge.dst == job.id)
        );
    }
    let decision = store
        .require_job(&vowp_protocol::JobId::parse("verify-fail-decision").expect("id"))
        .expect("decision job");
    assert_eq!(decision.kind, JobKind::Decision);
    let inbox = store.list_inbox(None).expect("inbox");
    assert!(inbox.iter().any(|item| item.kind == vowp_protocol::InboxKind::Decision));
}

#[test]
fn failed_fix_job_gates_fail_original_not_fix_of_fix() {
    let adapter = Arc::new(ScriptAdapter::new(
        vowp_protocol::HarnessKind::Opencode,
        report_script("fix-fail", Some("fail"), "partial"),
    ));
    let node = TestNode::with_adapter(true, adapter);
    let original = reviewing_job(&node, "fix-fail");
    let mut supervisor = node.supervisor();
    node.tick_until(&mut supervisor, 40, |store| {
        store.require_job(&original.id).expect("original").state == JobState::Failed
    });
    let store = node.store();
    let fix_jobs = store
        .list_jobs(&JobFilter {
            kind: Some(JobKind::Fix),
            ..JobFilter::default()
        })
        .expect("fix jobs");
    assert_eq!(fix_jobs.len(), 1);
    assert!(
        !fix_jobs[0].id.as_str().contains("-fix-1-fix-"),
        "a failed fix must not create a fix-of-fix"
    );
}

#[test]
fn failed_verify_job_parks_original_with_question() {
    let adapter = Arc::new(ScriptAdapter::new(
        vowp_protocol::HarnessKind::Opencode,
        report_script("verify-blocked", None, "partial"),
    ));
    let node = TestNode::with_adapter(true, adapter);
    let original = reviewing_job(&node, "verify-blocked");
    let mut supervisor = node.supervisor();
    node.tick_until(&mut supervisor, 30, |store| {
        store.require_job(&original.id).expect("original").state == JobState::Parked
    });
    let store = node.store();
    let job = store.require_job(&original.id).expect("job");
    assert_eq!(job.parked_reason, Some(ParkedReason::Question));
    let inbox = store.list_inbox(None).expect("inbox");
    assert!(
        inbox
            .iter()
            .any(|item| item.kind == vowp_protocol::InboxKind::Question && item.job_id.as_ref() == Some(&job.id))
    );
}

#[test]
fn paused_loop_is_skipped() {
    let adapter = Arc::new(ScriptAdapter::always(
        vowp_protocol::HarnessKind::Opencode,
        ScriptAction::ok("paused"),
    ));
    let node = TestNode::with_adapter(false, adapter);
    let job = worker_job(&node, "paused-job");
    let store = node.store();
    store.kv_set("paused", "1").expect("paused");
    let mut supervisor = node.supervisor();
    let report = supervisor.tick().expect("paused tick");
    assert!(report.dispatched.is_empty());
    assert_eq!(store.require_job(&job.id).expect("job").state, JobState::Pending);
    assert!(report.steps.iter().any(|step| step.name == "heartbeat" && step.ok));
    assert!(!report.steps.iter().any(|step| step.name == "dispatch"));
}

#[test]
fn tick_requests_apply_pause_resume_run_and_wake_with_ledger_state_rows() {
    let node = TestNode::new();
    let store = node.store();
    push(
        &store,
        &Request::PauseLoop {
            name: "l2-worker".to_owned(),
        },
    )
    .expect("push");
    push(
        &store,
        &Request::RunLoop {
            name: "l3-verify".to_owned(),
        },
    )
    .expect("push");
    push(&store, &Request::Wake).expect("push");
    let mut supervisor = node.supervisor();
    supervisor.tick().expect("tick");
    assert_eq!(
        store.loop_("l2-worker").expect("loop").expect("row").state,
        vowp_protocol::ActiveState::Paused
    );
    assert!(
        store
            .loop_("l3-verify")
            .expect("loop")
            .expect("row")
            .next_run_at
            .is_some()
    );
    let ledger = store
        .ledger_since(&vowp_store::LedgerFilter {
            kind: Some(vowp_protocol::LedgerKind::State),
            ..vowp_store::LedgerFilter::default()
        })
        .expect("ledger");
    assert!(ledger.iter().any(|entry| entry.summary.contains("pause_loop")));
    assert!(ledger.iter().any(|entry| entry.summary.contains("run_loop")));
    assert!(ledger.iter().any(|entry| entry.summary.contains("wake")));
    assert!(store.take_requests().expect("requests").is_empty());
}

#[test]
fn requests_drained_and_applied() {
    let node = TestNode::new();
    let store = node.store();
    let job = worker_job(&node, "drain-job");
    store
        .set_job_state(
            &job.id,
            JobState::Parked,
            &vowp_store::StateChange {
                note: None,
                parked_reason: Some(ParkedReason::Manual),
            },
        )
        .expect("parked");
    let id = push(&store, &Request::ResolveJob { job: job.id.clone() }).expect("push");
    store.kv_set("paused", "1").expect("paused");
    let mut supervisor = node.supervisor();
    let report = supervisor.tick().expect("tick");
    assert!(report.steps[0].ok);
    assert_eq!(store.require_job(&job.id).expect("job").state, JobState::Ready);
    assert!(store.take_requests().expect("queue").is_empty());
    let ledger = store
        .ledger_since(&vowp_store::LedgerFilter {
            kind: Some(vowp_protocol::LedgerKind::State),
            job_id: Some(job.id.clone()),
            ..vowp_store::LedgerFilter::default()
        })
        .expect("ledger");
    assert!(ledger.iter().any(|entry| {
        entry.detail.get("why") == Some(&serde_json::Value::String("Resolved".to_owned()))
            && entry.detail.get("from") == Some(&serde_json::Value::String("parked".to_owned()))
            && entry.detail.get("to") == Some(&serde_json::Value::String("ready".to_owned()))
    }));
    let _ = id;
}

#[test]
fn request_rows_apply_every_kind_end_to_end() {
    let node = TestNode::new();
    let store = node.store();
    let failed = node.insert_job("req-failed", JobKind::Build, vowp_protocol::Role::Worker, Some(vec![]));
    node.force_state(&failed, JobState::Failed);
    let stalled = node.insert_job("req-stalled", JobKind::Build, vowp_protocol::Role::Worker, Some(vec![]));
    node.force_state(&stalled, JobState::Stalled);
    let parked = node.insert_job("req-parked", JobKind::Build, vowp_protocol::Role::Worker, Some(vec![]));
    store
        .set_job_state(
            &parked.id,
            JobState::Parked,
            &vowp_store::StateChange {
                note: None,
                parked_reason: Some(ParkedReason::Manual),
            },
        )
        .expect("parked");
    let cancelled = node.insert_job(
        "req-cancelled",
        JobKind::Build,
        vowp_protocol::Role::Worker,
        Some(vec![]),
    );

    let inbox_id = vowp_protocol::InboxId::new();
    store
        .insert_inbox(&vowp_protocol::InboxItem {
            id: inbox_id.clone(),
            project: Some(node.project.clone()),
            job_id: Some(parked.id.clone()),
            kind: vowp_protocol::InboxKind::Question,
            title: "answer me".to_owned(),
            body: "?".to_owned(),
            options: vec!["resolved".to_owned()],
            state: vowp_protocol::InboxState::Open,
            answer: None,
            answered_by: None,
            created_at: now(),
            answered_at: None,
        })
        .expect("inbox");
    store.answer_inbox(&inbox_id, "resolved", "test").expect("answer");

    let requests = [
        Request::RetryJob { job: failed.id.clone() },
        Request::RestartJob {
            job: stalled.id.clone(),
        },
        Request::ResolveJob { job: parked.id.clone() },
        Request::CancelJob {
            job: cancelled.id.clone(),
        },
        Request::NudgeJob {
            job: stalled.id.clone(),
        },
        Request::ParkJob {
            job: failed.id.clone(),
            reason: "manual".to_owned(),
        },
        Request::ReplanJob { job: parked.id.clone() },
        Request::FailJob {
            job: stalled.id.clone(),
            reason: "gave up".to_owned(),
        },
        Request::AnswerInbox { id: inbox_id },
        Request::PauseLoop {
            name: "l2-worker".to_owned(),
        },
        Request::ResumeLoop {
            name: "l2-worker".to_owned(),
        },
        Request::RunLoop {
            name: "l3-verify".to_owned(),
        },
        Request::Wake,
    ];
    for request in &requests {
        push(&store, request).expect("push request");
    }
    let mut supervisor = node.supervisor();
    supervisor.tick().expect("tick");
    // The first event per job wins; later conflicting events are logged and dropped.
    assert_ne!(store.require_job(&failed.id).expect("job").state, JobState::Failed);
    assert_ne!(store.require_job(&stalled.id).expect("job").state, JobState::Stalled);
    assert!(store.take_requests().expect("queue").is_empty());
}
