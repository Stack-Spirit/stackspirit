//! On-demand history (`job ls --all`) and the safe history clean-up (`gc`).

use std::sync::Arc;

use spirit_core::engine::{EngineSupervisor, EngineSupervisorConfig, FakeEngineRuntime};
use spirit_core::error::SpiritErrorCode;
use spirit_core::host::{HostDraft, HostKeyPin, InMemoryHostStore, NewHostProfile};
use spirit_core::service::SpiritCore;
use spirit_core::ssh::command::{CommandOutput, CommandSpec};
use spirit_core::ssh::{FakeCommandRunner, SshOptions, SshTarget};
use spirit_core::time::Timestamp;
use spirit_core::vowp::{
    GcClassView, GcReportView, HistoryJobView, MockVowpClient, SshVowpClient, VowpClient,
    parse_gc_report, parse_history_jobs,
};
use spirit_core::wire::{parse_capacity_report, parse_fleet_status};

fn ssh_client(runner: Arc<FakeCommandRunner>) -> SshVowpClient {
    SshVowpClient::new(
        runner,
        SshTarget::new("owner", "192.0.2.10", 22).unwrap(),
        SshOptions::new("/tmp/opencode/known_hosts"),
        None,
    )
}

fn remote_argv(call: &CommandSpec) -> String {
    call.args.last().cloned().unwrap_or_default()
}

fn fresh_mock() -> MockVowpClient {
    let status =
        parse_fleet_status(include_str!("../fixtures/vowp/fleet-status-fresh.json").as_bytes())
            .unwrap()
            .value;
    let capacity = parse_capacity_report(
        include_str!("../fixtures/vowp/capacity-report-full.json").as_bytes(),
    )
    .unwrap()
    .value;
    MockVowpClient::new(status, capacity)
}

fn core() -> SpiritCore {
    let supervisor = EngineSupervisor::new(
        Arc::new(FakeEngineRuntime::new()),
        EngineSupervisorConfig {
            restart_budget: 3,
            health_attempts: 1,
            retry_delay: std::time::Duration::ZERO,
            port: 0,
        },
    );
    SpiritCore::new(Arc::new(InMemoryHostStore::new()), supervisor)
}

fn host_input() -> NewHostProfile {
    NewHostProfile {
        draft: HostDraft {
            label: "primary-vps".to_owned(),
            ssh: SshTarget::new("owner", "192.0.2.10", 22).unwrap(),
            identity_file: None,
            vowp_home: None,
            default_project: Some("vowp".to_owned()),
        },
        host_key: HostKeyPin {
            algorithm: "ssh-ed25519".to_owned(),
            sha256: "SHA256:AbCdEfGhIjKlMnOpQrStUvWxYz0123456789ABCDEFG".to_owned(),
            pinned_at: Timestamp::epoch(),
        },
    }
}

const JOBS: &str = r#"[
 {"id":"ss-1","project":"demo","title":"Old build","kind":"build","role":"worker","state":"done","updated_at":"2026-11-01T10:00:00Z","worktree":"/home/owner/wt/ss-1"},
 {"id":"ss-2","project":"demo","title":"","kind":"fix","role":"worker","state":"cancelled","updated_at":"2026-11-03T10:00:00Z"},
 {"id":"ss-3","project":"demo","title":"Live","kind":"build","role":"worker","state":"running","updated_at":"2026-11-04T10:00:00Z"},
 {"id":"ss-4","project":"demo","title":"Plan","kind":"build","role":"worker","state":"replanned","updated_at":"2026-11-02T10:00:00Z"},
 {"id":"ss-5","project":"demo","title":"Queued","kind":"build","role":"worker","state":"ready","updated_at":"2026-11-05T10:00:00Z"}
]"#;

const GC_JSON: &str = r#"{"generated_at":"2026-11-26T00:00:00Z","dry_run":true,
 "classes":[
  {"class":"production","files":9,"bytes":9000,"eligible_files":0,"eligible_bytes":0,"sample_paths":["/home/owner/x"]},
  {"class":"closed_session","files":4,"bytes":4096,"eligible_files":3,"eligible_bytes":3000,"sample_paths":["/home/owner/s"]},
  {"class":"run_output","files":2,"bytes":2048,"eligible_files":2,"eligible_bytes":2000,"sample_paths":[]},
  {"class":"cache","files":1,"bytes":4,"eligible_files":1,"eligible_bytes":4,"sample_paths":[]},
  {"class":"secrets","files":1,"bytes":10,"eligible_files":0,"eligible_bytes":0,"sample_paths":[]}],
 "refused":[{"class":"closed_session","session":"s1","path":"/home/owner/s1","reason":"lease"}],
 "deleted_files":0,"freed_bytes":0,"compacted_sessions":0,"closed_windows":0,
 "errors":["/home/owner/session.jsonl: permission denied","plain failure"]}"#;

#[test]
fn history_keeps_only_terminal_jobs_newest_first() {
    let jobs = parse_history_jobs("demo", JOBS.as_bytes()).unwrap();
    let ids: Vec<&str> = jobs.iter().map(|job| job.job_id.as_str()).collect();
    assert_eq!(ids, ["ss-2", "ss-4", "ss-1"]);
    assert_eq!(jobs[0].state, "cancelled");
    assert!(parse_history_jobs("demo", b"{}").is_err());
    assert!(parse_history_jobs("demo", b"nope").is_err());
    let json = serde_json::to_string(&jobs).unwrap();
    assert!(!json.contains("/home/owner"), "no home-side paths: {json}");
}

/// Shape captured from a real `stackspirit --project stackspirit --json job ls --all`
/// (paths shortened): every field VOWP prints is present, including active rows the
/// History tab must not list. `id`, `worktree`, `brief` and `report` are the exact
/// keys the live CLI emits.
const REAL_JOB_LS: &str = r#"[
 {"id":"ss-1","project":"stackspirit","title":"docs: update SECURITY.md","kind":"docs","role":"worker","profile":"worker","state":"done","priority":111,"needs":[],"owns":["SECURITY.md"],"reads":[],"brief":".vowp/jobs/ss-1/brief.md","report":".vowp/jobs/ss-1/report.md","gates":[{"report":{"fields":["status"]}}],"loop":{"until":"gates","max_attempts":2,"on_exhausted":"escalate"},"branch":"job/ss-1","worktree":"/home/user/stackspirit-wt/ss-1","attempts":0,"continues":0,"hang_restarts":0,"parked_reason":null,"note":"","created_by":"cli","last_progress_at":null,"state_changed_at":"2026-11-22T17:48:51.396Z","created_at":"2026-11-22T17:42:18.718Z","updated_at":"2026-11-22T17:48:51.396Z"},
 {"id":"ss-2","project":"stackspirit","title":"history: fix the buttons","kind":"build","role":"worker","profile":"worker","state":"failed","priority":96,"needs":["ss-1"],"owns":[],"reads":[],"brief":".vowp/jobs/ss-2/brief.md","report":".vowp/jobs/ss-2/report.md","gates":[],"loop":{"until":"gates","max_attempts":2,"on_exhausted":"escalate"},"branch":null,"worktree":null,"attempts":1,"continues":1,"hang_restarts":0,"parked_reason":null,"note":"retry","created_by":"planner","last_progress_at":null,"state_changed_at":"2026-11-26T14:00:00.000Z","created_at":"2026-11-26T13:00:00.000Z","updated_at":"2026-11-26T14:00:00.000Z"},
 {"id":"ss-3","project":"stackspirit","title":"live worker","kind":"build","role":"worker","profile":"worker","state":"running","priority":50,"needs":[],"owns":[],"reads":[],"brief":".vowp/jobs/ss-3/brief.md","report":".vowp/jobs/ss-3/report.md","gates":[],"loop":{"until":"gates","max_attempts":2,"on_exhausted":"escalate"},"branch":"job/ss-3","worktree":"/home/user/stackspirit-wt/ss-3","attempts":0,"continues":0,"hang_restarts":0,"parked_reason":null,"note":"","created_by":"planner","last_progress_at":null,"state_changed_at":"2026-11-24T10:00:00.000Z","created_at":"2026-11-23T10:00:00.000Z","updated_at":"2026-11-24T10:00:00.000Z"},
 {"id":"ss-4","project":"stackspirit","title":"queued work","kind":"fix","role":"worker","profile":"worker","state":"ready","priority":50,"needs":[],"owns":[],"reads":[],"brief":".vowp/jobs/ss-4/brief.md","report":".vowp/jobs/ss-4/report.md","gates":[],"loop":{"until":"gates","max_attempts":2,"on_exhausted":"escalate"},"branch":null,"worktree":null,"attempts":0,"continues":0,"hang_restarts":0,"parked_reason":null,"note":"","created_by":"planner","last_progress_at":null,"state_changed_at":"2026-11-24T09:00:00.000Z","created_at":"2026-11-24T09:00:00.000Z","updated_at":"2026-11-24T09:00:00.000Z"},
 {"job_id":"ss-5","project":"stackspirit","title":"old export row","kind":"build","role":"worker","state":"cancelled","state_changed_at":"2026-11-25T09:00:00.000Z"},
 {"id":"ss-6","project":"stackspirit","title":"replanned row","kind":"build","role":"worker","state":"replanned","updated_at":"2026-11-21T09:00:00.000Z"}
]"#;

#[test]
fn real_job_ls_output_parses_and_drops_live_rows_and_paths() {
    let jobs = parse_history_jobs("stackspirit", REAL_JOB_LS.as_bytes()).unwrap();
    let ids: Vec<&str> = jobs.iter().map(|job| job.job_id.as_str()).collect();
    // Newest first; `running` and `ready` never enter the History tab.
    assert_eq!(ids, ["ss-2", "ss-5", "ss-1", "ss-6"]);
    let cancelled = jobs.iter().find(|job| job.job_id == "ss-5").unwrap();
    assert_eq!(cancelled.state, "cancelled");
    // The older-export row has no `updated_at`; it falls back to `state_changed_at`.
    assert_eq!(
        cancelled.updated_at.as_deref(),
        Some("2026-11-25T09:00:00.000Z")
    );
    let json = serde_json::to_string(&jobs).unwrap();
    assert!(!json.contains("/home/user"), "no home-side paths: {json}");
    assert!(!json.contains(".vowp/jobs"), "no brief paths: {json}");
    assert!(!json.contains("worktree"), "no worktree fields: {json}");
}

#[test]
fn gc_report_is_path_free_and_counts_every_eligible_class() {
    let report = parse_gc_report(GC_JSON.as_bytes()).unwrap();
    assert!(report.dry_run);
    let classes: Vec<&str> = report.classes.iter().map(|c| c.class.as_str()).collect();
    assert_eq!(classes, ["closed_session", "run_output", "cache"]);
    assert_eq!(report.eligible_files, 6);
    assert_eq!(report.eligible_bytes, 5004);
    assert_eq!(report.refused, 1);
    assert_eq!(
        report.errors,
        ["[remote-path]: permission denied", "plain failure"]
    );
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains("/home/owner"), "no home-side paths: {json}");
}

/// Shape captured from a real `stackspirit --project stackspirit --json gc
/// --older-than 1d`: `sample` has eligible files but zero bytes, and the report
/// carries `closed_windows`, which the view does not need.
const REAL_GC_JSON: &str = r#"{
 "generated_at":"2026-11-26T14:14:56.675Z",
 "dry_run":true,
 "classes":[
  {"class":"truth","files":3109,"bytes":536339055,"eligible_files":0,"eligible_bytes":0,"sample_paths":["/home/user/.stackspirit/archive/projects/demo/project.db"]},
  {"class":"secrets","files":1,"bytes":35,"eligible_files":0,"eligible_bytes":0,"sample_paths":["/home/user/.stackspirit/secrets/EXAMPLE_KEY"]},
  {"class":"open_work","files":1847,"bytes":1007701306,"eligible_files":0,"eligible_bytes":0,"sample_paths":[]},
  {"class":"closed_session","files":502,"bytes":51116199,"eligible_files":0,"eligible_bytes":0,"sample_paths":[]},
  {"class":"run_output","files":1080,"bytes":69679917,"eligible_files":2,"eligible_bytes":4207,"sample_paths":["/home/user/.stackspirit/runs/a.jsonl"]},
  {"class":"worktree","files":14,"bytes":15355494642,"eligible_files":1,"eligible_bytes":187,"sample_paths":["/home/user/stackspirit-wt/ss-108"]},
  {"class":"sample","files":86,"bytes":0,"eligible_files":86,"eligible_bytes":0,"sample_paths":["host_samples/125262"]}],
 "refused":[],
 "deleted_files":0,"freed_bytes":0,"compacted_sessions":0,"closed_windows":0,
 "errors":[]}"#;

#[test]
fn real_gc_output_keeps_zero_byte_classes_and_scrubs_paths() {
    let report = parse_gc_report(REAL_GC_JSON.as_bytes()).unwrap();
    let classes: Vec<&str> = report.classes.iter().map(|c| c.class.as_str()).collect();
    assert_eq!(classes, ["run_output", "worktree", "sample"]);
    // Host samples are zero-byte files but still eligible: counting only bytes would
    // make the preview claim "nothing to clean up" while `--apply` deletes 86 files.
    assert_eq!(report.eligible_files, 89);
    assert_eq!(report.eligible_bytes, 4394);
    assert!(report.dry_run);
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains("/home/user"), "no home-side paths: {json}");
}

#[tokio::test]
async fn history_uses_the_exact_argv_and_runs_only_on_demand() {
    let runner = Arc::new(FakeCommandRunner::new(vec![CommandOutput::ok(
        JOBS.as_bytes().to_vec(),
    )]));
    let client = ssh_client(runner.clone());
    assert!(
        runner.calls().is_empty(),
        "construction never loads history"
    );
    let jobs = client.project_history("demo").await.unwrap();
    assert_eq!(jobs.len(), 3);
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    let argv = remote_argv(&calls[0]);
    assert_eq!(
        argv,
        "'stackspirit' '--project' 'demo' '--json' 'job' 'ls' '--all'"
    );
    assert!(client.project_history("-bad name").await.is_err());
    assert_eq!(runner.calls().len(), 1);
}

#[tokio::test]
async fn gc_preview_is_dry_and_apply_adds_the_flag() {
    let runner = Arc::new(FakeCommandRunner::new(vec![
        CommandOutput::ok(GC_JSON.as_bytes().to_vec()),
        CommandOutput::ok(
            br#"{"dry_run":false,"classes":[],"refused":[],"deleted_files":5,"freed_bytes":5000,"compacted_sessions":3,"errors":[]}"#.to_vec(),
        ),
    ]));
    let client = ssh_client(runner.clone());
    let preview = client.project_gc("demo", false).await.unwrap();
    assert!(preview.dry_run);
    let applied = client.project_gc("demo", true).await.unwrap();
    assert_eq!((applied.deleted_files, applied.freed_bytes), (5, 5000));
    let calls = runner.calls();
    let first = remote_argv(&calls[0]);
    let second = remote_argv(&calls[1]);
    assert_eq!(
        first,
        "'stackspirit' '--project' 'demo' '--json' 'gc' '--older-than' '1d'"
    );
    assert_eq!(
        second,
        "'stackspirit' '--project' 'demo' '--json' 'gc' '--older-than' '1d' '--apply'"
    );
}

fn gc_report(dry_run: bool) -> GcReportView {
    GcReportView {
        dry_run,
        classes: vec![GcClassView {
            class: "run_output".to_owned(),
            files: 2,
            bytes: 2048,
            eligible_files: 2,
            eligible_bytes: 2000,
        }],
        refused: 0,
        deleted_files: if dry_run { 0 } else { 2 },
        freed_bytes: if dry_run { 0 } else { 2000 },
        compacted_sessions: 0,
        eligible_files: 2,
        eligible_bytes: 2000,
        errors: Vec::new(),
    }
}

#[tokio::test]
async fn service_gates_history_and_requires_confirmation_for_apply() {
    let job = HistoryJobView {
        job_id: "ss-1".to_owned(),
        project: "demo".to_owned(),
        title: "Old".to_owned(),
        kind: "build".to_owned(),
        role: "worker".to_owned(),
        state: "done".to_owned(),
        updated_at: None,
    };
    let mock = fresh_mock()
        .with_history_jobs(vec![job])
        .with_gc(gc_report(true), gc_report(false));
    let core = core();
    // Not connected: nothing reaches the client.
    assert!(core.project_history(&mock, "demo").await.is_err());
    assert!(mock.history_calls().is_empty());
    let host = core.add_host(host_input()).unwrap();
    core.connect(host.id, &mock).await.unwrap();

    assert_eq!(core.project_history(&mock, "demo").await.unwrap().len(), 1);
    assert_eq!(mock.history_calls(), ["demo"]);

    let preview = core.project_gc(&mock, "demo", false, false).await.unwrap();
    assert!(preview.dry_run);
    let refused = core
        .project_gc(&mock, "demo", true, false)
        .await
        .unwrap_err();
    assert_eq!(refused.code, SpiritErrorCode::InvalidInput);
    assert_eq!(mock.gc_calls(), [("demo".to_owned(), false)]);
    let applied = core.project_gc(&mock, "demo", true, true).await.unwrap();
    assert_eq!(applied.freed_bytes, 2000);
    assert_eq!(
        mock.gc_calls(),
        [("demo".to_owned(), false), ("demo".to_owned(), true)]
    );
}
