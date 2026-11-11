//! `vowp setup verify`: the fixture checks run after `setup home` and `setup control`.
//!
//! Spec: `docs/design/v0.2/home-and-surfaces.md` §11.2. Verify answers three questions:
//!
//! 1. does a fixture job run one fake turn, pass its gate and merge through the real
//!    merge queue;
//! 2. is the home snapshot readable from the control machine, with an `as of` time;
//! 3. does an orchestrator session answer through `vowp chat`.
//!
//! The first and third checks run in their own temporary node home, with a fake harness
//! script, so they never read or write the owner's projects, database or tmux server.
//! The gc dry run is read-only. The snapshot check is the only check that reads the
//! control directory.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use vowp_core::deps::SystemRng;
use vowp_core::lifecycle::{GcOptions, gc};
use vowp_core::loops::Supervisor;
use vowp_core::surface::orchestrator::{NewOrchestrator, Orchestrator};
use vowp_core::{AdapterRegistry, Deps, SystemGit, SystemRunner};
use vowp_protocol::{
    Clock, CmdGate, EventBody, Gate, HarnessConfig, JobId, JobKind, JobState, MemoryLog, NewJob, Node, NodeName,
    OrchestratorApp, Project, ProjectName, Role, SNAPSHOT_FORMAT, SNAPSHOT_VERSION, Snapshot, SystemClock, Timestamp,
    VowpConfig,
};
use vowp_store::session_log::SessionLog;
use vowp_store::{NodePaths, Store};
use vowp_tmux::fake::FakeWindows;

use super::{SetupCheck, SetupOutcome, print_outcome};
use crate::cmd::setup::SetupVerifyArgs;
use crate::{CliError, Ctx, IoStreams};

/// The fake harness reply used by the chat check.
const CHAT_REPLY: &str = "setup verify orchestrator answered";

/// The fixture job id.
const FIXTURE_JOB: &str = "setup-verify";

/// A unique scratch directory removed when it goes out of scope.
struct Scratch {
    path: PathBuf,
}

impl Scratch {
    /// Creates a private scratch directory under the system temporary directory.
    fn new(label: &str) -> Result<Self, CliError> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| CliError::Invalid(format!("system clock before 1970: {error}")))?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "vowp-setup-verify-{label}-{}-{nanos}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// One isolated node home, project checkout and its injected dependencies.
///
/// The harness is a shell script that emits the opencode event shape the adapter reads.
/// The fixture job version also writes and commits one file, so a merge that does no
/// work cannot make the check pass.
struct Fixture {
    scratch: Scratch,
    paths: NodePaths,
    project: ProjectName,
    repo: PathBuf,
    store: Store,
    deps: Deps,
    log: Arc<MemoryLog>,
}

impl Fixture {
    /// Builds a temporary home, git checkout, bare origin and store.
    fn new(label: &str, reply: &str, commit_file: bool) -> Result<Self, CliError> {
        let scratch = Scratch::new(label)?;
        let repo = scratch.path.join("project");
        let origin = scratch.path.join("origin.git");
        fs::create_dir_all(&repo)?;
        git(&repo, &["init", "--quiet"])?;
        git(&repo, &["checkout", "--quiet", "-b", "main"])?;
        git(
            &repo,
            &[
                "-c",
                "user.name=setup verify",
                "-c",
                "user.email=setup@example.invalid",
                "commit",
                "--quiet",
                "--allow-empty",
                "-m",
                "fixture base",
            ],
        )?;
        git(&scratch.path, &["init", "--quiet", "--bare", "origin.git"])?;
        let origin_text = origin.display().to_string();
        git(&repo, &["remote", "add", "origin", &origin_text])?;
        git(&repo, &["push", "--quiet", "-u", "origin", "main"])?;

        let harness = scratch.path.join("fake-opencode");
        write_exec(&harness, &harness_script(reply, commit_file))?;

        let mut config = VowpConfig::with_defaults_for_tests();
        let project = ProjectName::parse("setup-verify")?;
        let node_name = NodeName::parse("setup-verify")?;
        config.node.name = node_name.clone();
        config.node.tick_secs = 1;
        config.node.max_workers = 1;
        config.node.max_verifiers = 1;
        config.project.name = project.clone();
        config.project.path = Some(repo.display().to_string());
        config.project.remote = "origin".to_owned();
        config.project.default_branch = "main".to_owned();
        config.project.push = false;
        config.project.worktree_root = Some(scratch.path.join("worktrees").display().to_string());
        config.node.cwd_allow.push(scratch.path.display().to_string());
        config.harnesses.insert(
            "opencode".to_owned(),
            HarnessConfig {
                binary: Some(harness.display().to_string()),
                ..HarnessConfig::default()
            },
        );
        config.validate()?;
        fs::write(
            repo.join("vowp.toml"),
            toml::to_string_pretty(&config)
                .map_err(|error| CliError::Invalid(format!("fixture vowp.toml: {error}")))?,
        )?;
        fs::write(repo.join(".gitignore"), ".vowp/\n")?;
        git(&repo, &["add", "vowp.toml", ".gitignore"])?;
        git(
            &repo,
            &[
                "-c",
                "user.name=setup verify",
                "-c",
                "user.email=setup@example.invalid",
                "commit",
                "--quiet",
                "-m",
                "fixture config",
            ],
        )?;
        git(&repo, &["push", "--quiet", "origin", "main"])?;

        let paths = NodePaths::new(scratch.path.join(".vowp"));
        paths.ensure()?;
        let now = SystemClock.now();
        let global = Store::open_global(&paths)?;
        global.upsert_node(&Node {
            name: node_name.clone(),
            ssh_target: None,
            is_local: true,
            capabilities: serde_json::json!({"vowp": env!("CARGO_PKG_VERSION")}),
            last_seen_at: Some(now),
            created_at: now,
        })?;
        global.upsert_project(&Project {
            name: project.clone(),
            path: repo.display().to_string(),
            remote_url: Some(origin.display().to_string()),
            default_branch: "main".to_owned(),
            config: serde_json::to_value(&config).map_err(vowp_protocol::ProtocolError::from)?,
            created_at: now,
            updated_at: now,
        })?;
        Store::create_project_db(&paths, &project)?;
        let store = Store::open(&paths).map_err(vowp_core::CoreError::from)?;

        let command: Arc<dyn vowp_core::CommandRunner> = Arc::new(SystemRunner);
        let adapters = AdapterRegistry::from_config(&config)?;
        let log = Arc::new(MemoryLog::new());
        let deps = Deps {
            paths: paths.clone(),
            config: Arc::new(config),
            node: node_name,
            clock: Arc::new(SystemClock),
            log: log.clone(),
            adapters: Arc::new(adapters),
            windows: Arc::new(FakeWindows::new()),
            git: Arc::new(SystemGit::new(Arc::clone(&command))),
            cmd: command,
            rng: Arc::new(SystemRng::new()),
        };
        Ok(Self {
            scratch,
            paths,
            project,
            repo,
            store,
            deps,
            log,
        })
    }

    /// Returns the scratch root used as the fake machine's `HOME`.
    fn home(&self) -> &Path {
        &self.scratch.path
    }
}

/// Runs one check and turns a setup error into a failed check instead of aborting.
fn push_check(checks: &mut Vec<SetupCheck>, name: &str, result: Result<SetupCheck, CliError>) {
    match result {
        Ok(check) => checks.push(check),
        Err(error) => checks.push(SetupCheck {
            name: name.to_owned(),
            status: "fail".to_owned(),
            detail: error.to_string(),
        }),
    }
}

/// Runs `vowp setup verify` and returns 1 when any check failed.
pub fn run(ctx: &Ctx, io: &mut dyn IoStreams, _args: &SetupVerifyArgs) -> Result<i32, CliError> {
    let mut checks = Vec::new();
    push_check(&mut checks, "gc.dry_run", gc_dry_run(ctx));
    push_check(&mut checks, "fixture.job_merge", fixture_job_merges());
    push_check(&mut checks, "control.snapshot", snapshot_check(ctx));
    push_check(&mut checks, "orchestrator.chat", orchestrator_answers_chat());

    let failed = checks.iter().any(|check| check.status == "fail");
    let outcome = SetupOutcome {
        status: if failed { "failed" } else { "ok" }.to_owned(),
        phase: "verify".to_owned(),
        changed: Vec::new(),
        handoff: None,
        checks,
    };
    if ctx.json() {
        crate::output::print_json(io, &outcome)?;
    } else {
        print_outcome(io, &outcome)?;
    }
    Ok(if failed { 1 } else { 0 })
}

/// Runs the read-only gc report the setup contract prints first.
pub fn gc_dry_run(ctx: &Ctx) -> Result<SetupCheck, CliError> {
    if ctx.control().is_some() {
        return Ok(check(
            "gc.dry_run",
            "warn",
            "the gc dry run reads the home database; run it on the home or inspect the cached snapshot",
        ));
    }
    let store = ctx.fleet_store()?;
    let report = gc(ctx.deps(), &store, &GcOptions::default())?;
    let files: u64 = report.classes.iter().map(|class| class.files).sum();
    let detail = if report.classes.is_empty() {
        "dry run found no lifecycle files to collect".to_owned()
    } else {
        format!(
            "dry run lists {} file(s) in {} class(es); nothing was deleted",
            files,
            report.classes.len()
        )
    };
    Ok(check(
        "gc.dry_run",
        if report.errors.is_empty() { "ok" } else { "warn" },
        if report.errors.is_empty() {
            detail
        } else {
            format!("{detail}; warnings: {}", report.errors.join("; "))
        },
    ))
}

/// Runs a fixture job through one fake turn, its gate and the real merge queue.
pub fn fixture_job_merges() -> Result<SetupCheck, CliError> {
    let fixture = Fixture::new("job", "setup verify fixture answered", true)?;
    let job_id = JobId::parse(FIXTURE_JOB)?;
    let mut supervisor = Supervisor::new(fixture.deps.clone(), fixture.project.clone())?;
    supervisor.startup()?;
    fixture.store.insert_job(
        &fixture.project,
        &NewJob {
            id: job_id.clone(),
            title: "setup verify fixture".to_owned(),
            kind: JobKind::Build,
            role: Role::Worker,
            profile: Some("worker".to_owned()),
            needs: Vec::new(),
            owns: Vec::new(),
            reads: Vec::new(),
            gates: Some(vec![Gate::Cmd(CmdGate {
                run: "sh -c 'echo fixture-gate-ok'".to_owned(),
                ..CmdGate::default()
            })]),
            priority: None,
            loop_: None,
            brief_text: None,
        },
        "setup verify",
    )?;

    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        let _ = supervisor.tick();
        let job = fixture.store.require_job(&job_id)?;
        match job.state {
            JobState::Done => break,
            JobState::Failed | JobState::Parked | JobState::Cancelled | JobState::Replanned => {
                let last_run = fixture.store.last_run_of_job(&job_id)?;
                return Ok(check(
                    "fixture.job_merge",
                    "fail",
                    format!(
                        "fixture job ended in {}{}; note {:?}; last run {}",
                        job.state.as_str(),
                        job.parked_reason
                            .as_ref()
                            .map(|reason| format!(" ({reason:?})"))
                            .unwrap_or_default(),
                        job.note,
                        last_run
                            .as_ref()
                            .map(|run| format!("{:?}: {}", run.outcome, first_line(&run.detail, "")))
                            .unwrap_or_else(|| "none".to_owned())
                    ),
                ));
            }
            _ => {}
        }
        if Instant::now() >= deadline {
            let last_run = fixture.store.last_run_of_job(&job_id)?;
            let log = fixture
                .log
                .lines()
                .last()
                .map(|(level, target, message)| format!("{} {target}: {message}", level.as_str()))
                .unwrap_or_default();
            return Ok(check(
                "fixture.job_merge",
                "fail",
                format!(
                    "fixture job did not merge within 45s (last state {}; continues {}; last run {}; log {})",
                    job.state.as_str(),
                    job.continues,
                    last_run
                        .as_ref()
                        .map(|run| format!("{:?}: {}", run.outcome, first_line(&run.detail, "")))
                        .unwrap_or_else(|| "none".to_owned()),
                    if log.is_empty() { "empty" } else { &log }
                ),
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let merged = fixture.repo.join("setup-verify.txt");
    let content = fs::read_to_string(&merged).unwrap_or_default();
    if !content.contains("setup verify fixture turn") {
        return Ok(check(
            "fixture.job_merge",
            "fail",
            format!(
                "fixture job reached done but {} is missing or empty after the merge",
                merged.display()
            ),
        ));
    }
    Ok(check(
        "fixture.job_merge",
        "ok",
        format!("fixture job {} merged into main at {}", job_id, fixture.repo.display()),
    ))
}

/// Creates an orchestrator chat session and sends it one turn through `vowp chat`.
pub fn orchestrator_answers_chat() -> Result<SetupCheck, CliError> {
    let fixture = Fixture::new("chat", CHAT_REPLY, false)?;
    let orchestrator = Orchestrator::new(fixture.deps.clone());
    let info = orchestrator.create(&NewOrchestrator {
        name: "setup-verify".to_owned(),
        project: Some(fixture.project.clone()),
        app: OrchestratorApp::Chat,
        executor: None,
        force: false,
    })?;

    let binary = vowp_binary()?;
    let mut command = Command::new(&binary);
    command
        .arg("--home")
        .arg(fixture.paths.root())
        .arg("--project")
        .arg(&fixture.repo)
        .arg("--no-color")
        .arg("chat")
        .arg(info.session.as_str())
        .current_dir(fixture.home())
        .env("HOME", fixture.home())
        .env("TMUX_TMPDIR", fixture.home().join("tmux"))
        .env("NO_COLOR", "1")
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .env_remove("VOWP_CONTROL_HOME");
    fs::create_dir_all(fixture.home().join("tmux"))?;
    let output = run_with_input(command, "hello\n/quit\n")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return Ok(check(
            "orchestrator.chat",
            "fail",
            format!(
                "vowp chat exited {:?}: {}",
                output.status.code(),
                first_line(&stderr, &stdout)
            ),
        ));
    }
    if !stdout.contains(CHAT_REPLY) {
        return Ok(check(
            "orchestrator.chat",
            "fail",
            format!(
                "vowp chat did not print the orchestrator's answer: {}",
                first_line(&stdout, &stderr)
            ),
        ));
    }

    let events = SessionLog::open_read(&fixture.paths, &info.session)?.read_all()?;
    let answered = events.iter().any(|event| {
        matches!(
            event.body(),
            Ok(EventBody::AssistantMessage(message)) if message.text.contains(CHAT_REPLY)
        )
    });
    if !answered {
        return Ok(check(
            "orchestrator.chat",
            "fail",
            format!("orchestrator session {} has no assistant answer event", info.session),
        ));
    }
    Ok(check(
        "orchestrator.chat",
        "ok",
        format!(
            "orchestrator {} answered through `vowp chat` in session {}",
            info.card.name, info.session
        ),
    ))
}

/// Reads the snapshot cache written by `setup control`, or reports a failing check.
pub fn snapshot_check(ctx: &Ctx) -> Result<SetupCheck, CliError> {
    match ctx.control() {
        Some(control) => snapshot_readable_from_control(&control.root),
        None if ctx.role() == crate::node_ctx::NodeRole::Home => snapshot_readable_from_home(ctx),
        None => Ok(check(
            "control.snapshot",
            "fail",
            "vowp home is not initialised; run `vowp setup home` and `vowp setup control` first",
        )),
    }
}

/// Checks the control cache written by `setup control --apply`.
pub fn snapshot_readable_from_control(root: &Path) -> Result<SetupCheck, CliError> {
    let Some(cached) = crate::remote::cached_snapshot(root)? else {
        return Ok(check(
            "control.snapshot",
            "fail",
            format!(
                "no readable snapshot cache in {}; run `vowp setup control --apply` or `vowp fleet status --refresh`",
                root.join("snapshot.json").display()
            ),
        ));
    };
    if let Some(config) = crate::remote::read_control_config(root)?
        && config.home != cached.snapshot.home.as_str()
    {
        return Ok(check(
            "control.snapshot",
            "fail",
            format!(
                "cached snapshot names home {} but control.json names {}",
                cached.snapshot.home, config.home
            ),
        ));
    }
    validate_snapshot(&cached.snapshot, cached.fetched_at, "control cache")
}

/// Checks a snapshot that exists on the home when no control machine is configured.
fn snapshot_readable_from_home(ctx: &Ctx) -> Result<SetupCheck, CliError> {
    if !ctx.paths().root().join("snapshot.json").is_file() {
        return Ok(check(
            "control.snapshot",
            "warn",
            "no control machine is configured and the home has no snapshot.json yet; the control check was not verified",
        ));
    }
    let snapshot = vowp_core::fleet::read_snapshot(ctx.paths())?;
    let mut result = validate_snapshot(&snapshot, snapshot.generated_at, "home snapshot")?;
    result.status = "warn".to_owned();
    result.detail = format!(
        "{}; no control machine is configured, so readability from a control machine was not verified",
        result.detail
    );
    Ok(result)
}

/// Validates the snapshot document and returns an `ok` check with its `as of` time.
fn validate_snapshot(snapshot: &Snapshot, fetched_at: Timestamp, source: &str) -> Result<SetupCheck, CliError> {
    if snapshot.schema != SNAPSHOT_FORMAT || snapshot.version != SNAPSHOT_VERSION {
        return Ok(check(
            "control.snapshot",
            "fail",
            format!(
                "unsupported snapshot {} version {}; this build reads {} version {}",
                snapshot.schema, snapshot.version, SNAPSHOT_FORMAT, SNAPSHOT_VERSION
            ),
        ));
    }
    Ok(check(
        "control.snapshot",
        "ok",
        format!(
            "snapshot as of {} (home {}, project(s) {}, source {source})",
            fetched_at.as_rfc3339(),
            snapshot.home,
            snapshot.projects.len()
        ),
    ))
}

/// Builds one check row.
fn check(name: &str, status: &str, detail: impl Into<String>) -> SetupCheck {
    SetupCheck {
        name: name.to_owned(),
        status: status.to_owned(),
        detail: detail.into(),
    }
}

/// Returns the first non-empty line of a command result, for a short check detail.
fn first_line(primary: &str, fallback: &str) -> String {
    let text = if primary.trim().is_empty() {
        fallback.trim()
    } else {
        primary.trim()
    };
    text.lines().next().unwrap_or_default().chars().take(300).collect()
}

/// Writes an executable shell script.
fn write_exec(path: &Path, body: &str) -> Result<(), CliError> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::write(path, body)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

/// Returns the fake opencode script.
fn harness_script(reply: &str, commit_file: bool) -> String {
    let commit = if commit_file {
        r#"sleep 1
mkdir -p .vowp/jobs/setup-verify
cat > .vowp/jobs/setup-verify/report.md <<'VOWP_REPORT_EOF'
---
job: setup-verify
status: done
branch: job/setup-verify
---
setup verify fixture turn
VOWP_REPORT_EOF
printf '%s\n' 'setup verify fixture turn' > setup-verify.txt
git add .vowp/jobs/setup-verify/report.md setup-verify.txt >/dev/null 2>&1
git -c user.name='setup verify' -c user.email='setup@example.invalid' commit -m 'setup verify fixture turn' >/dev/null 2>&1
"#
    } else {
        ""
    };
    let event = serde_json::json!({
        "type": "text",
        "timestamp": 1_794_168_421_621_i64,
        "sessionID": "ses_setup_verify",
        "part": {"text": reply}
    });
    let finish = serde_json::json!({
        "type": "step_finish",
        "timestamp": 1_794_168_421_622_i64,
        "sessionID": "ses_setup_verify",
        "part": {"cost": 0.0, "tokens": {"input": 1, "output": 1}, "reason": "stop"}
    });
    format!(
        "#!/bin/sh\n\
         if [ \"${{1:-}}\" = \"--version\" ]; then printf 'opencode 2.0.28\\n'; exit 0; fi\n\
         {commit}\
         printf '%s\\n' '{event}'\n\
         printf '%s\\n' '{finish}'\n"
    )
}

/// Runs one git command in `cwd` and returns its stdout.
fn git(cwd: &Path, args: &[&str]) -> Result<String, CliError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if !output.status.success() {
        return Err(CliError::Remote(format!(
            "git {} failed: {}",
            args.join(" "),
            first_line(
                &String::from_utf8_lossy(&output.stderr),
                &String::from_utf8_lossy(&output.stdout)
            )
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Locates the `vowp` binary built for this checkout.
fn vowp_binary() -> Result<PathBuf, CliError> {
    if let Some(path) = std::env::var_os("CARGO_BIN_EXE_vowp") {
        return Ok(PathBuf::from(path));
    }
    let current = std::env::current_exe()?;
    if current.file_name().is_some_and(|name| name == "vowp") {
        return Ok(current);
    }
    if let Some(debug) = current.parent().and_then(Path::parent) {
        let candidate = debug.join("vowp");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Ok(PathBuf::from("vowp"))
}

/// Runs a child with `input` on stdin and captures both streams.
fn run_with_input(mut command: Command, input: &str) -> Result<Output, CliError> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(error) = stdin.write_all(input.as_bytes()) {
            if error.kind() != std::io::ErrorKind::BrokenPipe {
                return Err(CliError::Io(error));
            }
        }
    }
    child.wait_with_output().map_err(CliError::Io)
}

#[cfg(test)]
#[path = "../../tests/setup_verify.rs"]
mod integration_tests;
