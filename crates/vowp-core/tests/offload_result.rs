//! Result return for offloaded runs: fetch, verify, gate and hand off to the merge path.
//!
//! Every test uses a temporary home, a temporary bare remote, a temporary project clone, a
//! temporary agent clone and a fake agent summary fed through the home ingest state machine.
//! No test opens `~/.vowp`, touches the network, starts tmux or runs a real harness.
//! Spec: `docs/design/v0.4/local-orchestrator.md` §6.3 step 7, §6.4 and §14.3.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;
use vowp_core::agent::{dispatch, result};
use vowp_core::session::remote::{BeginTurn, IngestGrant, IngestSession, begin_turn};
use vowp_core::{AdapterRegistry, CommandRunner, CommandSpec, Deps, FixedRng, SystemGit, SystemRunner};
use vowp_protocol::{
    AssignmentPayload, Cost, Executor, ExecutorRef, FakeClock, Gate, GateKind, HarnessKind, Job, JobId, JobKind,
    JobState, Lease, MemoryLog, NewJob, NodeName, PathRef, Project, ProjectName, RemoteLine, Role, RunId, RunOutcome,
    RunPurpose, Session, SessionId, SessionRole, SessionState, StopReason, SurfaceKind, Timestamp, VowpConfig,
};
use vowp_store::repo_offload::{AgentAssignment, AgentAssignmentState};
use vowp_store::session_log::SessionLog;
use vowp_store::{LedgerFilter as StoreLedgerFilter, NodePaths, StateChange, Store};

const LEASE: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

fn ts() -> Timestamp {
    Timestamp::parse("2026-11-14T10:00:00Z").expect("timestamp")
}

fn project() -> ProjectName {
    ProjectName::parse("alpha").expect("project")
}

fn node() -> NodeName {
    NodeName::parse("mac-studio").expect("node")
}

fn executor() -> Executor {
    Executor {
        harness: HarnessKind::Opencode,
        model: "deepseek/deepseek-flash".to_owned(),
        effort: None,
        variant: None,
    }
}

/// One hermetic result-return fixture.
struct Fixture {
    _tmp: TempDir,
    deps: Deps,
    store: Store,
    node: NodeName,
    job: Job,
    run: RunId,
    session: SessionId,
    home: PathBuf,
    origin: PathBuf,
    root: PathBuf,
    base: String,
}

impl Fixture {
    fn new(owns: &[&str]) -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().to_path_buf();
        let origin = root.join("origin.git");
        let home = root.join("home");
        init_bare(&origin);
        init_repo(&home);
        git_ok(&home, &["remote", "add", "origin", &origin.display().to_string()]);
        git_ok(&home, &["push", "-u", "origin", "main"]);
        let base = git_output(&home, &["rev-parse", "HEAD"]).trim().to_owned();

        let paths = NodePaths::new(root.join("node"));
        paths.ensure().expect("node paths");
        let clock = Arc::new(FakeClock::at(ts()));
        let project = project();
        let node = node();
        let mut config = VowpConfig::with_defaults_for_tests();
        config.project.name = project.clone();
        config.project.path = Some(home.display().to_string());
        config.project.remote = "origin".to_owned();
        config.project.default_branch = "main".to_owned();
        config.offload.enabled = true;
        let deps = Deps {
            paths,
            config: Arc::new(config),
            node: NodeName::parse("home-1").expect("home node"),
            clock: clock.clone(),
            log: Arc::new(MemoryLog::new()),
            adapters: Arc::new(AdapterRegistry::new()),
            windows: Arc::new(vowp_tmux::fake::FakeWindows::new()),
            git: Arc::new(SystemGit::default()),
            cmd: Arc::new(SystemRunner),
            rng: Arc::new(FixedRng(3)),
        };
        let store = Store::open(&deps.paths).expect("store").with_clock(clock.clone());
        store
            .upsert_project(&Project {
                name: project.clone(),
                path: home.display().to_string(),
                remote_url: Some(origin.display().to_string()),
                default_branch: "main".to_owned(),
                config: serde_json::json!({}),
                created_at: ts(),
                updated_at: ts(),
            })
            .expect("project row");

        let id = JobId::parse("result-job").expect("job id");
        let new = NewJob {
            id: id.clone(),
            title: "result return".to_owned(),
            kind: JobKind::Build,
            role: Role::Worker,
            profile: Some("worker".to_owned()),
            needs: Vec::new(),
            owns: owns.iter().map(|pattern| (*pattern).to_owned()).collect(),
            reads: Vec::new(),
            gates: Some(vec![Gate::OwnsRespected(true), Gate::SecretScan(true)]),
            priority: None,
            loop_: None,
            brief_text: None,
        };
        let job = store.insert_job(&project, &new, "test").expect("job");
        store
            .set_job_state(
                &job.id,
                JobState::Ready,
                &StateChange {
                    note: None,
                    parked_reason: None,
                },
            )
            .expect("ready job");

        let session = Session {
            id: SessionId::new(),
            project: Some(project.clone()),
            job_id: Some(job.id.clone()),
            role: Some(SessionRole::Worker),
            title: job.id.as_str().to_owned(),
            parent_session_id: None,
            node: node.clone(),
            cwd: ".".to_owned(),
            state: SessionState::Idle,
            executor: Some(executor()),
            last_seq: 0,
            last_capsule_id: None,
            log_bytes: 0,
            created_at: ts(),
            updated_at: ts(),
        };
        store.upsert_session(&session).expect("session row");
        SessionLog::create(&deps.paths, &session.id, clock.clone())
            .expect("session log")
            .sync()
            .expect("sync log");

        let grant = begin_turn(
            &deps,
            &store,
            &BeginTurn {
                session: session.id.clone(),
                lease: String::new(),
                executor: ExecutorRef {
                    executor: executor(),
                    node: Some(node.clone()),
                },
                purpose: RunPurpose::Start,
                node: node.clone(),
                cwd: None,
                prompt: "do the job".to_owned(),
            },
        )
        .expect("begin turn");

        let payload = AssignmentPayload {
            assignment_id: format!("assignment-{}", grant.run_id),
            run_id: grant.run_id.clone(),
            session_id: session.id.clone(),
            project: project.clone(),
            job: job.id.clone(),
            executor: executor(),
            purpose: RunPurpose::Start,
            prompt: "do the job".to_owned(),
            cwd_ref: PathRef::Worktree {
                project: project.clone(),
                job: job.id.clone(),
            },
            base_commit: base.clone(),
            remote: "origin".to_owned(),
            remote_url: origin.display().to_string(),
            env_names: Vec::new(),
            secret_names: Vec::new(),
            epoch: 1,
            deadline: None,
            spool_cap_bytes: 512 * 1024 * 1024,
        };
        store
            .insert_assignment(&AgentAssignment {
                id: payload.assignment_id.clone(),
                run_id: grant.run_id.clone(),
                session_id: session.id.clone(),
                project: project.clone(),
                node: node.clone(),
                epoch: 1,
                state: AgentAssignmentState::Sent,
                payload,
                created_at: ts(),
                claimed_at: Some(ts()),
                expires_at: None,
                detail: String::new(),
            })
            .expect("assignment");
        dispatch::accept(
            &deps,
            &store,
            &node,
            &format!("assignment-{}", grant.run_id),
            &grant.run_id,
            1,
            ts(),
        )
        .expect("accept assignment");
        store
            .take_lease(&Lease {
                session_id: session.id.clone(),
                surface_id: None,
                surface_kind: SurfaceKind::Agent,
                node: node.clone(),
                holder: "agent".to_owned(),
                token: LEASE.to_owned(),
                acquired_at: ts(),
                heartbeat_at: ts(),
            })
            .expect("run lease");

        Self {
            _tmp: tmp,
            deps,
            store,
            node,
            job,
            run: grant.run_id,
            session: session.id,
            home,
            origin,
            root,
            base,
        }
    }

    /// Pushes one job branch from a fresh agent clone and returns its head.
    fn push(&self, files: &[(&str, &str)]) -> String {
        let agent = self.root.join("agent");
        if !agent.join(".git").is_dir() {
            git_ok(
                &self.root,
                &[
                    "clone",
                    &self.origin.display().to_string(),
                    &agent.display().to_string(),
                ],
            );
        }
        git_ok(
            &agent,
            &["checkout", "-B", &format!("job/{}", self.job.id), "origin/main"],
        );
        for (path, text) in files {
            write(&agent.join(path), text);
        }
        git_ok(&agent, &["add", "-A"]);
        git_ok(&agent, &["commit", "-m", "job work"]);
        git_ok(&agent, &["push", "-f", "origin", &format!("job/{}", self.job.id)]);
        git_output(&agent, &["rev-parse", "HEAD"]).trim().to_owned()
    }

    /// Pushes an unrelated root commit as the job branch and returns its head.
    fn push_unrelated(&self) -> String {
        let other = self.root.join("unrelated");
        fs::create_dir_all(&other).expect("unrelated dir");
        git_ok(&other, &["init", "-b", "main"]);
        write(&other.join("README.md"), "unrelated\n");
        git_ok(&other, &["add", "-A"]);
        git_ok(&other, &["commit", "-m", "unrelated root"]);
        let head = git_output(&other, &["rev-parse", "HEAD"]).trim().to_owned();
        git_ok(
            &other,
            &[
                "push",
                "-f",
                &self.origin.display().to_string(),
                &format!("HEAD:refs/heads/job/{}", self.job.id),
            ],
        );
        head
    }

    /// Feeds one hello and one summary through the real ingest state machine.
    fn ingest(&self, reported_head: Option<&str>, summary: RemoteLine) -> result::Settled {
        let mut ingest = IngestSession::begin(
            &self.deps,
            &self.store,
            &IngestGrant {
                session: self.session.clone(),
                lease: LEASE.to_owned(),
                run: self.run.clone(),
            },
        )
        .expect("ingest session");
        ingest.set_reported_head(reported_head);
        let mut ack = Vec::new();
        let hello = RemoteLine::Hello {
            session_id: self.session.clone(),
            run_id: self.run.clone(),
            executor: executor().to_string(),
            node: self.node.clone(),
            spool_id: "test-spool".to_owned(),
            lease: LEASE.to_owned(),
        };
        let mut text = serde_json::to_string(&hello).expect("hello json");
        text.push('\n');
        assert!(!ingest.line(&text, &mut ack).expect("hello"));
        let mut text = serde_json::to_string(&summary).expect("summary json");
        text.push('\n');
        assert!(ingest.line(&text, &mut ack).expect("summary"));
        ingest.finish().expect("finish");
        let run = self.store.require_run(&self.run).expect("run row");
        result::Settled {
            outcome: run.outcome.expect("terminal outcome"),
            detail: run.detail,
        }
    }

    fn summary_ok() -> RemoteLine {
        RemoteLine::Summary {
            exit_code: Some(0),
            outcome: RunOutcome::Ok,
            native_session_id: None,
            cost: Cost::unknown(),
            limit: None,
            result_text: Some("done".to_owned()),
            stop_reason: StopReason::EndTurn,
            usage: None,
        }
    }

    fn summary_push_failed(detail: &str) -> RemoteLine {
        RemoteLine::Summary {
            exit_code: Some(1),
            outcome: RunOutcome::Error,
            native_session_id: None,
            cost: Cost::unknown(),
            limit: None,
            result_text: Some(format!("git_push_failed: {detail}")),
            stop_reason: StopReason::Error,
            usage: None,
        }
    }

    fn job(&self) -> Job {
        self.store.require_job(&self.job.id).expect("job row")
    }

    fn assignment(&self) -> AgentAssignment {
        self.store
            .assignments_for_run(&self.run)
            .expect("assignments")
            .pop()
            .expect("assignment")
    }

    fn gate_results(&self) -> Vec<vowp_protocol::GateResult> {
        self.store
            .gate_results_for(&self.job.id, self.job().attempts)
            .expect("gate results")
    }
}

#[test]
fn offloaded_run_pushes_the_job_branch_and_the_home_verifies_the_head() {
    let fixture = Fixture::new(&["src/**"]);
    let head = fixture.push(&[("src/lib.rs", "pub fn returned() {}\n")]);

    let settled = fixture.ingest(Some(&head), Fixture::summary_ok());

    assert_eq!(settled.outcome, RunOutcome::Ok, "detail: {}", settled.detail);
    assert!(settled.detail.contains("transport=push"), "detail: {}", settled.detail);
    assert!(
        settled.detail.contains(&fixture.base),
        "base missing: {}",
        settled.detail
    );
    assert!(settled.detail.contains(&head), "head missing: {}", settled.detail);

    let run = fixture.store.require_run(&fixture.run).expect("run row");
    assert_eq!(run.outcome, Some(RunOutcome::Ok));
    assert!(run.detail.contains(&fixture.base), "run detail: {}", run.detail);
    assert!(run.detail.contains(&head), "run detail: {}", run.detail);

    let job = fixture.job();
    assert_eq!(job.state, JobState::Gating, "job moved to the normal gate path");
    assert_eq!(job.branch.as_deref(), Some(format!("job/{}", fixture.job.id).as_str()));
    let worktree = PathBuf::from(job.worktree.expect("staged worktree"));
    assert!(worktree.is_dir(), "staging worktree exists");
    assert_eq!(
        git_output(&worktree, &["rev-parse", "HEAD"]).trim(),
        head,
        "staging worktree is the fetched head"
    );

    let record = result::load(&fixture.store, &fixture.run)
        .expect("result record")
        .expect("returned record");
    assert_eq!(record.status, "returned");
    assert_eq!(record.transport, "push");
    assert_eq!(record.base_commit, fixture.base);
    assert_eq!(record.head_commit.as_deref(), Some(head.as_str()));

    let assignment = fixture.assignment();
    assert_eq!(assignment.state, AgentAssignmentState::Finished);
    assert!(fixture.store.lease(&fixture.session).expect("lease").is_none());
}

#[test]
fn offload_fails_cleanly_when_the_push_fails() {
    let fixture = Fixture::new(&["src/**"]);

    let settled = fixture.ingest(
        None,
        Fixture::summary_push_failed("the remote rejected the branch with a credential URL"),
    );

    assert_eq!(settled.outcome, RunOutcome::Error);
    assert!(
        settled.detail.starts_with("git_push_failed:"),
        "detail: {}",
        settled.detail
    );
    assert!(!settled.detail.contains("credential"), "agent stderr is not kept");
    let job = fixture.job();
    assert_eq!(job.state, JobState::Ready, "a failed push keeps the job retryable");
    assert_eq!(job.attempts, 0);
    assert_eq!(job.continues, 0);
    assert_eq!(fixture.assignment().state, AgentAssignmentState::Finished);
    assert!(fixture.store.lease(&fixture.session).expect("lease").is_none());
}

#[test]
fn offload_fails_when_no_pushed_head_is_reported() {
    let fixture = Fixture::new(&["src/**"]);

    let settled = fixture.ingest(None, Fixture::summary_ok());

    assert_eq!(settled.outcome, RunOutcome::Error);
    assert!(settled.detail.contains("no head commit"), "detail: {}", settled.detail);
    assert_eq!(fixture.job().state, JobState::Ready);
    assert_eq!(fixture.assignment().state, AgentAssignmentState::Finished);
    assert!(fixture.store.lease(&fixture.session).expect("lease").is_none());
}

#[test]
fn home_rejects_a_reported_head_that_is_not_the_fetched_head() {
    let fixture = Fixture::new(&["src/**"]);
    let fetched = fixture.push(&[("src/lib.rs", "pub fn work() {}\n")]);
    assert_ne!(fetched, fixture.base);

    let settled = fixture.ingest(Some(&fixture.base), Fixture::summary_ok());

    assert_eq!(settled.outcome, RunOutcome::Error);
    assert!(settled.detail.contains("reported head"), "detail: {}", settled.detail);
    assert!(settled.detail.contains("fetched head"), "detail: {}", settled.detail);
    let job = fixture.job();
    assert_eq!(job.state, JobState::Ready);
    assert!(job.worktree.is_none(), "no staging worktree is published on rejection");
}

#[test]
fn home_rejects_a_head_outside_the_base_history() {
    let fixture = Fixture::new(&["src/**"]);
    let head = fixture.push_unrelated();

    let settled = fixture.ingest(Some(&head), Fixture::summary_ok());

    assert_eq!(settled.outcome, RunOutcome::Error);
    assert!(
        settled.detail.contains("does not descend"),
        "detail: {}",
        settled.detail
    );
    let job = fixture.job();
    assert_eq!(job.state, JobState::Ready);
    assert!(job.worktree.is_none());
}

#[test]
fn home_runs_owns_and_secret_scan_on_the_fetched_diff() {
    let fixture = Fixture::new(&["src/**"]);
    // Built at runtime so this test source itself never contains a scannable literal.
    let token = ["ghp_", "abcdefghijklmnopqrstuvwxyz0123"].concat();
    let leak = format!("pub const TOKEN: &str = \"{token}\";\n");
    let head = fixture.push(&[("src/leak.rs", leak.as_str())]);

    let settled = fixture.ingest(Some(&head), Fixture::summary_ok());

    assert_eq!(settled.outcome, RunOutcome::Error);
    assert!(settled.detail.contains("secret_scan"), "detail: {}", settled.detail);
    let results = fixture.gate_results();
    assert_eq!(results.len(), 2, "the authoritative owns and secret scans ran");
    let owns = results
        .iter()
        .find(|result| result.gate.kind() == GateKind::OwnsRespected)
        .expect("owns result");
    assert!(owns.passed, "the leak is inside src/: {:?}", owns.evidence);
    let secret = results
        .iter()
        .find(|result| result.gate.kind() == GateKind::SecretScan)
        .expect("secret result");
    assert!(!secret.passed);
    match &secret.evidence {
        vowp_protocol::Evidence::SecretScan { hits, .. } => {
            assert!(hits.iter().any(|hit| hit.rule == "github_token"));
        }
        other => panic!("expected secret-scan evidence, got {other:?}"),
    }
    assert_eq!(fixture.job().state, JobState::Ready);
}

#[test]
fn home_does_not_merge_a_head_outside_the_job_owns() {
    let fixture = Fixture::new(&["src/**"]);
    let head = fixture.push(&[
        ("src/lib.rs", "pub fn owned() {}\n"),
        ("notes/outside.txt", "not in the brief\n"),
    ]);

    let settled = fixture.ingest(Some(&head), Fixture::summary_ok());

    assert_eq!(settled.outcome, RunOutcome::Error);
    assert!(settled.detail.contains("owns_respected"), "detail: {}", settled.detail);
    assert!(
        settled.detail.contains("notes/outside.txt"),
        "detail: {}",
        settled.detail
    );
    let job = fixture.job();
    assert_eq!(job.state, JobState::Ready);
    assert_ne!(job.state, JobState::Merging);
    let merges = fixture
        .store
        .ledger_since(&StoreLedgerFilter {
            kind: Some(vowp_protocol::LedgerKind::Merge),
            job_id: Some(fixture.job.id.clone()),
            ..StoreLedgerFilter::default()
        })
        .expect("merge ledger");
    assert!(merges.is_empty(), "result return never merges");
    let results = fixture.gate_results();
    let owns = results
        .iter()
        .find(|result| result.gate.kind() == GateKind::OwnsRespected)
        .expect("owns result");
    assert!(!owns.passed);
    git_ok(
        &fixture.home,
        &["rev-parse", "--verify", &format!("refs/heads/job/{}", fixture.job.id)],
    );
}

#[test]
fn result_return_leaves_the_main_checkout_clean() {
    let fixture = Fixture::new(&["src/**"]);
    let head = fixture.push(&[("src/lib.rs", "pub fn clean() {}\n")]);
    let before = git_output(&fixture.home, &["rev-parse", "HEAD"]).trim().to_owned();

    fixture.ingest(Some(&head), Fixture::summary_ok());

    assert_eq!(
        git_output(&fixture.home, &["rev-parse", "HEAD"]).trim(),
        before,
        "the main checkout does not move"
    );
    assert_eq!(
        git_output(&fixture.home, &["status", "--porcelain"]),
        "",
        "the main checkout stays clean"
    );
    assert!(fixture.home.join(".git").is_dir());
}

#[test]
fn terminal_result_releases_the_lease_and_clears_the_waiting_note() {
    let fixture = Fixture::new(&["src/**"]);
    fixture
        .store
        .set_job_note(&fixture.job.id, "waiting: offload node_offline: no node")
        .expect("waiting note");
    let head = fixture.push(&[("src/lib.rs", "pub fn terminal() {}\n")]);

    let settled = fixture.ingest(Some(&head), Fixture::summary_ok());

    assert_eq!(settled.outcome, RunOutcome::Ok);
    assert!(
        fixture.store.lease(&fixture.session).expect("lease").is_none(),
        "the run lease is released"
    );
    let job = fixture.job();
    assert!(
        !job.note.starts_with("waiting: offload "),
        "the dispatcher waiting note is cleared: {:?}",
        job.note
    );
    assert_eq!(fixture.assignment().state, AgentAssignmentState::Finished);
    assert_eq!(job.state, JobState::Gating);
}

#[test]
fn a_failed_push_keeps_the_job_retryable() {
    let fixture = Fixture::new(&["src/**"]);

    fixture.ingest(None, Fixture::summary_push_failed("no space left on the remote"));

    let job = fixture.job();
    assert_eq!(job.state, JobState::Ready, "the dispatcher may pick the job again");
    assert_eq!(job.attempts, 0);
    assert_eq!(job.continues, 0);
    assert!(
        !job.note.starts_with("waiting: offload "),
        "the old waiting note is cleared: {:?}",
        job.note
    );
    let assignment = fixture.assignment();
    assert_eq!(assignment.state, AgentAssignmentState::Finished);
    assert!(fixture.store.lease(&fixture.session).expect("lease").is_none());
}

/// Runs git in `cwd`, asserting success.
fn git_ok(cwd: &Path, args: &[&str]) -> String {
    let output = run_git(cwd, args);
    assert_eq!(output.exit_code, Some(0), "git {args:?} failed: {}", output.text());
    output.text()
}

/// Runs git in `cwd` and returns its trimmed output.
fn git_output(cwd: &Path, args: &[&str]) -> String {
    git_ok(cwd, args)
}

/// Runs git in `cwd` without asserting success.
fn run_git(cwd: &Path, args: &[&str]) -> vowp_core::CommandOutput {
    let mut argv = vec!["git".to_owned()];
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    SystemRunner
        .run(&CommandSpec {
            shell: None,
            argv,
            cwd: Some(cwd.to_path_buf()),
            env: git_env(),
            timeout: Duration::from_secs(60),
        })
        .expect("git")
}

/// Environment that keeps git hermetic: no user or system config, fixed identity.
fn git_env() -> Vec<(String, String)> {
    vec![
        ("GIT_CONFIG_GLOBAL".to_owned(), "/dev/null".to_owned()),
        ("GIT_CONFIG_NOSYSTEM".to_owned(), "1".to_owned()),
        ("GIT_AUTHOR_NAME".to_owned(), "VOWP Test".to_owned()),
        ("GIT_AUTHOR_EMAIL".to_owned(), "test@example.invalid".to_owned()),
        ("GIT_COMMITTER_NAME".to_owned(), "VOWP Test".to_owned()),
        ("GIT_COMMITTER_EMAIL".to_owned(), "test@example.invalid".to_owned()),
    ]
}

/// Creates a real repository on `main` with one initial commit.
fn init_repo(repo: &Path) {
    fs::create_dir_all(repo).expect("repo dir");
    git_ok(repo, &["init", "-b", "main"]);
    write(&repo.join("README.md"), "base\n");
    git_ok(repo, &["add", "-A"]);
    git_ok(repo, &["commit", "-m", "initial"]);
}

/// Creates a bare repository to act as `origin`.
fn init_bare(path: &Path) {
    fs::create_dir_all(path).expect("bare dir");
    git_ok(path, &["init", "--bare", "-b", "main"]);
}

/// Writes `text` to `path`, creating parent directories.
fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent dir");
    }
    fs::write(path, text).expect("write file");
}
