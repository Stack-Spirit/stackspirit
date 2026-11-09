//! `vowp up`, `vowp down`, `plan` and tmux-layout tests.
//!
//! Everything is hermetic: temporary node homes, temporary git repositories, fake adapters
//! and a private tmux socket (`-L vowp-test-…`) killed by a guard on drop.

mod common;

use std::path::Path;
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use vowp_core::deps::SystemRng;
use vowp_core::{AdapterRegistry, CommandRunner, Deps, SystemGit, SystemRunner};
use vowp_protocol::{
    Adapter, AdapterCapabilities, AdapterError, AdapterRun, Cost, Executor, HarnessConfig, HarnessEvent, HarnessKind,
    JobKind, JobState, NewJob, Node, NullLog, ProbeResult, Project, Role, RunOutcome, RunSummary, StartInput,
    StopReason, SystemClock, Timestamp,
};
use vowp_store::{NodePaths, Store};
use vowp_tmux::fake::FakeWindows;
use vowp_tmux::{Tmux, WindowManager};

/// Runs one git command in `dir` and asserts success.
fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Creates a real git repository with one commit on `main`.
fn git_repo() -> tempfile::TempDir {
    let repo = tempfile::tempdir().expect("temp repo");
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test"]);
    std::fs::write(repo.path().join("README.md"), "hello\n").expect("README");
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-m", "initial"]);
    repo
}

/// Registers the local node and one project row in `paths`.
fn register_project(paths: &NodePaths, config: &vowp_protocol::VowpConfig, project_path: &Path) {
    let store = Store::open(paths).expect("store");
    let now = store.clock().now();
    store
        .upsert_node(&Node {
            name: config.node.name.clone(),
            ssh_target: None,
            is_local: true,
            capabilities: serde_json::json!({}),
            last_seen_at: Some(now),
            created_at: now,
        })
        .expect("node");
    store
        .upsert_project(&Project {
            name: config.project.name.clone(),
            path: project_path.display().to_string(),
            remote_url: None,
            default_branch: "main".to_owned(),
            config: serde_json::to_value(config).expect("config json"),
            created_at: now,
            updated_at: now,
        })
        .expect("project");
}

/// Polls `predicate` for up to five seconds.
fn eventually(mut predicate: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if predicate() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

/// A fake adapter that records `StartInput`s and finishes immediately with an `Ok` summary.
#[derive(Clone, Default)]
struct FakeAdapter {
    starts: Arc<Mutex<Vec<StartInput>>>,
}

impl FakeAdapter {
    /// Returns how many runs were started.
    fn starts(&self) -> usize {
        self.starts.lock().expect("starts lock").len()
    }
}

impl Adapter for FakeAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Opencode
    }

    fn probe(&self) -> ProbeResult {
        ProbeResult {
            ok: true,
            version: Some("fake-1.0".to_owned()),
            path: Some("fake-harness".to_owned()),
            detail: "fake adapter".to_owned(),
        }
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities::default()
    }

    fn start(&self, input: StartInput) -> Result<Box<dyn AdapterRun>, AdapterError> {
        self.starts.lock().expect("starts lock").push(input);
        let (sender, receiver) = mpsc::channel();
        drop(sender);
        Ok(Box::new(FakeRun { receiver }))
    }
}

/// The run returned by [`FakeAdapter`]; its event channel is already closed.
struct FakeRun {
    receiver: Receiver<HarnessEvent>,
}

impl AdapterRun for FakeRun {
    fn pid(&self) -> Option<u32> {
        None
    }

    fn events(&self) -> &Receiver<HarnessEvent> {
        &self.receiver
    }

    fn send(&mut self, _text: &str) -> Result<(), AdapterError> {
        Err(AdapterError::Unsupported("live_input"))
    }

    fn interrupt(&mut self) -> Result<(), AdapterError> {
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AdapterError> {
        Ok(())
    }

    fn wait(&mut self) -> Result<RunSummary, AdapterError> {
        Ok(RunSummary {
            exit_code: Some(0),
            outcome: RunOutcome::Ok,
            native_session_id: None,
            cost: Cost::unknown(),
            limit: None,
            result_text: Some("fake run finished".to_owned()),
            stop_reason: StopReason::EndTurn,
        })
    }
}

#[test]
fn up_once_runs_one_tick_with_fake_adapters() {
    let home = tempfile::tempdir().expect("temp home");
    let repo = git_repo();
    let worktrees = tempfile::tempdir().expect("temp worktrees");
    let paths = NodePaths::new(home.path());

    let mut config = vowp_protocol::VowpConfig::with_defaults_for_tests();
    config.project.path = Some(repo.path().display().to_string());
    config.project.worktree_root = Some(worktrees.path().display().to_string());
    register_project(&paths, &config, repo.path());
    let project = config.project.name.clone();

    let store = Store::open(&paths).expect("store");
    let job = store
        .insert_job(
            &project,
            &NewJob {
                id: vowp_protocol::JobId::parse("n09b-job").expect("slug"),
                title: "Fake adapter job".to_owned(),
                kind: JobKind::Build,
                role: Role::Worker,
                profile: None,
                needs: Vec::new(),
                owns: Vec::new(),
                reads: Vec::new(),
                gates: None,
                priority: None,
                loop_: None,
                brief_text: Some("Do the work.\n".to_owned()),
            },
            "test",
        )
        .expect("insert job");

    let fake = FakeAdapter::default();
    let adapter: Arc<dyn Adapter> = Arc::new(fake.clone());
    let command: Arc<dyn CommandRunner> = Arc::new(SystemRunner);
    let deps = Deps {
        paths: paths.clone(),
        config: Arc::new(config.clone()),
        node: config.node.name.clone(),
        clock: Arc::new(SystemClock),
        log: Arc::new(NullLog),
        adapters: Arc::new(AdapterRegistry::new().with("opencode", adapter)),
        windows: Arc::new(FakeWindows::new()),
        git: Arc::new(SystemGit::new(Arc::clone(&command))),
        cmd: command,
        rng: Arc::new(SystemRng::new()),
    };

    let report = vowp_cli::cmd::up::tick_once(&deps, &project).expect("one tick");
    for step in &report.steps {
        assert!(step.ok, "step {} failed: {}", step.name, step.detail);
    }
    assert_eq!(report.dispatched, vec![job.id.clone()]);
    assert!(
        paths.root().join("heartbeat").is_file(),
        "tick step 13 writes the heartbeat"
    );
    let board: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(paths.root().join("board.json")).expect("board.json"))
            .expect("board json");
    assert!(board.get("jobs_by_state").is_some(), "{board}");

    assert!(
        eventually(|| fake.starts() == 1),
        "the fake adapter must be started exactly once"
    );
    let start = fake.starts.lock().expect("starts lock")[0].clone();
    assert_eq!(start.executor.harness, HarnessKind::Opencode);
    assert!(start.cwd.starts_with(worktrees.path()), "{:?}", start.cwd);

    let store = Store::open(&paths).expect("store");
    assert!(
        eventually(|| {
            store
                .last_run_of_job(&job.id)
                .ok()
                .flatten()
                .is_some_and(|run| run.ended_at.is_some())
        }),
        "the fake run must finish"
    );
    let run = store.last_run_of_job(&job.id).expect("runs").expect("one run");
    assert_eq!(run.outcome, Some(RunOutcome::Ok));
    let sessions = store
        .list_sessions(&vowp_store::SessionFilter {
            job_id: Some(job.id.clone()),
            ..vowp_store::SessionFilter::default()
        })
        .expect("sessions");
    assert_eq!(sessions.len(), 1, "the run owns one session");
}

#[test]
fn up_once_command_writes_heartbeat_board_and_loop_row() {
    let env = common::TestEnv::new();
    let (code, stdout, stderr) = env.run(&["up", "--once", "--no-tmux"]);
    assert_eq!(code, 0, "up --once failed: {stderr}");
    assert!(stdout.starts_with("tick "), "{stdout}");
    assert!(stdout.contains("steps ok"), "{stdout}");

    assert!(env.home_path().join("heartbeat").is_file());
    let board: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(env.home_path().join("board.json")).expect("board.json"))
            .expect("board json");
    assert_eq!(board["project"], "test", "{board}");

    let store = env.store();
    let loop_row = store.loop_("l1-supervisor").expect("loop lookup").expect("loop row");
    assert_eq!(loop_row.last_status, "ok");
    assert!(loop_row.last_run_at.is_some());

    let (code, value, stderr) = env.run_json(&["up", "--once", "--no-tmux"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(value["ok"], true, "{value}");
    assert!(value["steps"].as_array().is_some_and(|steps| !steps.is_empty()));
    assert_eq!(value["dispatched"], serde_json::json!([]));
}

#[test]
fn down_writes_stop_file_and_clears_stale_lock() {
    let env = common::TestEnv::new();
    let lock = env.home_path().join("supervisor.lock");
    std::fs::write(&lock, format!("{}\n{}\n", u32::MAX, Timestamp::now().as_rfc3339())).expect("lock");

    let (code, stdout, stderr) = env.run(&["down"]);
    assert_eq!(code, 0, "down with no live supervisor must succeed: {stderr}");
    assert!(stdout.contains("not running"), "{stdout}");
    let stop = env.home_path().join("stop");
    assert!(stop.is_file(), "down must write the stop file");
    let value = std::fs::read_to_string(&stop).expect("stop contents");
    assert!(
        value.trim().parse::<i64>().is_ok(),
        "stop file holds unix seconds: {value:?}"
    );
    assert!(!lock.exists(), "a stale lock is removed");

    // A later `up` clears the stop file so the supervisor does not stop immediately.
    let (code, _, stderr) = env.run(&["up", "--once", "--no-tmux"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(!stop.exists(), "up must clear a stale stop file");
}

#[test]
fn tmux_layout_created_with_fake_windows() {
    let windows = FakeWindows::new();
    let cwd = tempfile::tempdir().expect("temp cwd");
    vowp_cli::cmd::up::ensure_project_windows(&windows, "demo", Some(cwd.path()), &["vowp".to_owned()])
        .expect("layout");

    let specs = windows.specs("vowp-demo");
    let names: Vec<&str> = specs.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["board", "supervisor", "planner", "keeper", "inbox"]);
    for (name, spec) in &specs {
        assert_eq!(
            spec.argv,
            vec!["vowp".to_owned(), "__window".to_owned(), name.clone()],
            "window {name} runs the read-only follower"
        );
        assert_eq!(spec.cwd.as_deref(), Some(cwd.path()));
    }
}

/// Kills the private tmux server when the test ends (also on panic).
struct TmuxGuard {
    tmux: Tmux,
}

impl Drop for TmuxGuard {
    fn drop(&mut self) {
        let _ = self.tmux.kill_server_for_tests();
    }
}

#[test]
fn tmux_layout_created_on_private_socket() {
    let socket = format!(
        "vowp-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.subsec_nanos())
            .unwrap_or(0)
    );
    let tmux = Tmux::new(Some(socket));
    if !tmux.has_tmux() {
        eprintln!("SKIP: tmux not installed");
        return;
    }
    let guard = TmuxGuard { tmux: tmux.clone() };
    let cwd = tempfile::tempdir().expect("temp cwd");
    let program = vec!["sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()];

    vowp_cli::cmd::up::ensure_project_windows(&guard.tmux, "n09b-test", Some(cwd.path()), &program)
        .expect("layout on a private socket");
    let windows = guard.tmux.list_windows("vowp-n09b-test").expect("list windows");
    let names: Vec<&str> = windows.iter().map(|window| window.name.as_str()).collect();
    assert_eq!(names, ["board", "supervisor", "planner", "keeper", "inbox"]);
    assert!(windows.iter().all(|window| window.tagged), "{windows:?}");
    assert!(
        windows.iter().all(|window| window.pane_pid.is_some()),
        "every window runs a process"
    );

    // Idempotent: a second call changes nothing.
    vowp_cli::cmd::up::ensure_project_windows(&guard.tmux, "n09b-test", Some(cwd.path()), &program)
        .expect("second layout");
    assert_eq!(guard.tmux.list_windows("vowp-n09b-test").expect("windows").len(), 5);
}

/// Writes the fake planner harness, its mapping and a config that uses it.
fn install_fake_planner(env: &common::TestEnv) -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;

    let tools = tempfile::tempdir().expect("fake tools");
    let script = tools.path().join("fake-planner.sh");
    let body = format!(
        "#!/bin/sh\n\"{vowp}\" --home \"{home}\" job add --id planned-by-fake --title \"Fake planned job\" \
         --kind build --owns \"src/**\" >/dev/null\necho \"planned planned-by-fake\"\n",
        vowp = env!("CARGO_BIN_EXE_vowp"),
        home = env.home_path().display()
    );
    std::fs::write(&script, body).expect("fake planner script");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");

    let mapping = tools.path().join("mapping.toml");
    std::fs::write(
        &mapping,
        format!(
            "name = \"fake-planner\"\ncommand = {{ argv = [\"{}\"], prompt_via = \"stdin\" }}\n\n[output]\nformat = \"text\"\n",
            script.display()
        ),
    )
    .expect("mapping");

    let mut config = env.config.clone();
    config.harnesses.insert(
        "fake-planner".to_owned(),
        HarnessConfig {
            mapping: Some(mapping.display().to_string()),
            ..HarnessConfig::default()
        },
    );
    config.profiles.get_mut("planner").expect("planner profile").ladder =
        vec![Executor::parse("cli:fake-planner").expect("executor")];
    std::fs::write(
        env.project_path().join("vowp.toml"),
        toml::to_string_pretty(&config).expect("config toml"),
    )
    .expect("write config");
    tools
}

#[test]
fn attach_without_tty_falls_back_to_the_session_tail() {
    let env = common::TestEnv::new();
    env.insert_job("n01-alpha", "Alpha", JobKind::Build, &[]);

    let (code, _, stderr) = env.run(&["attach", "n01-alpha"]);
    assert_eq!(code, 3, "a job without a session is not found: {stderr}");
    assert!(stderr.contains("no session"), "{stderr}");
}

#[test]
fn window_board_renders_and_missing_job_session_exits_3() {
    let env = common::TestEnv::new();
    env.insert_job("n01-alpha", "Alpha", JobKind::Build, &[]);

    let (code, stdout, stderr) = env.run(&["__window", "board"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.starts_with("test @ "), "{stdout}");
    assert!(stdout.contains("jobs: pending 1"), "{stdout}");

    let (code, _, stderr) = env.run(&["__window", "job", "n01-alpha"]);
    assert_eq!(code, 3, "a job without a session is not found: {stderr}");
    assert!(stderr.contains("no session"), "{stderr}");
}

#[test]
fn plan_exits_6_when_every_rung_is_limited() {
    let env = common::TestEnv::new();
    let store = env.store();
    let now = store.clock().now();
    store
        .upsert_limit(&vowp_protocol::Limit {
            provider: "deepseek".to_owned(),
            harness: Some(HarnessKind::Opencode),
            kind: vowp_protocol::LimitRowKind::Quota,
            resets_at: None,
            detail: "test limit".to_owned(),
            since: now,
            last_probe_at: None,
            probe_fail_streak: 0,
            concurrency_cap: None,
            cap_until: None,
        })
        .expect("limit");

    let (code, _, stderr) = env.run(&["plan", "ship the feature"]);
    assert_eq!(code, 6, "every rung limited must exit 6: {stderr}");
    assert!(stderr.contains("limited"), "{stderr}");
    assert!(
        env.store()
            .job(&vowp_protocol::JobId::parse("planned-by-fake").expect("slug"))
            .expect("lookup")
            .is_none(),
        "no planner turn starts when every rung is limited"
    );
}

#[test]
fn plan_with_fake_planner_creates_jobs_through_job_add() {
    let env = common::TestEnv::new();
    let _tools = install_fake_planner(&env);

    let (code, stdout, stderr) = env.run(&["plan", "ship the feature"]);
    assert_eq!(code, 0, "plan failed: {stderr}");
    assert!(stdout.contains("created 1 job(s): planned-by-fake"), "{stdout}");
    assert!(stdout.contains("#2 "), "the planner events are streamed: {stdout}");

    let store = env.store();
    let job = store
        .job(&vowp_protocol::JobId::parse("planned-by-fake").expect("slug"))
        .expect("job lookup")
        .expect("the planner created the job through `vowp job add`");
    assert_eq!(job.kind, JobKind::Build);
    assert_eq!(job.state, JobState::Pending);
    assert!(env.brief_path("planned-by-fake").is_file(), "brief written");

    let sessions = store
        .list_sessions(&vowp_store::SessionFilter {
            project: Some(env.config.project.name.clone()),
            ..vowp_store::SessionFilter::default()
        })
        .expect("sessions");
    assert!(
        sessions
            .iter()
            .any(|session| session.role == Some(vowp_protocol::SessionRole::Planner)),
        "the planner turn owns a planner session: {sessions:?}"
    );
}
