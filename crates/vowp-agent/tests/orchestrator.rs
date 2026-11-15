//! Hermetic orchestrator-host tests: a fake app script, a temporary agent home and a fake
//! transport. No real opencode/claude binary, home directory, network or tmux server is used.

use std::collections::VecDeque;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use vowp_agent::{
    AgentClient, AgentClock, AgentConfig, AgentError, FrameRead, FrameStream, NodeKey, OrchestratorConfig,
    OrchestratorHost, RunOptions, RunOutcome, SurfaceAttachment, SurfaceBinding, Transport, validate_name,
};
use vowp_protocol::{
    AGENT_PROTOCOL, AgentFrame, AgentRole, NodeName, OrchestratorApp, OrchestratorCommand, OrchestratorError,
    OrchestratorNodeState, ProjectName, SessionId, SurfaceId, Timestamp,
};

const SESSION: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const SURFACE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
const SESSION_TWO: &str = "01ARZ3NDEKTSV4RRFFQ69G5FBV";
const SURFACE_TWO: &str = "01ARZ3NDEKTSV4RRFFQ69G5FBW";
const SESSION_THREE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FCV";
const SURFACE_THREE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FCW";
const PROJECT: &str = "vowp";

fn session(value: &str) -> SessionId {
    SessionId::parse(value).expect("session id")
}

fn surface(value: &str) -> SurfaceId {
    SurfaceId(value.to_owned())
}

fn project() -> ProjectName {
    ProjectName::parse(PROJECT).expect("project")
}

fn node() -> NodeName {
    NodeName::parse("mac-studio").expect("node slug")
}

/// Builds a service password at runtime so no secret-shaped literal is committed.
fn service_password() -> String {
    format!("vowp-test-{}", "P".repeat(24))
}

fn agent_state(root: &Path) -> PathBuf {
    let state = root.join("agent");
    fs::create_dir_all(&state).expect("state dir");
    fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).expect("state mode");
    state
}

fn project_root(root: &Path) -> PathBuf {
    let project = root.join("project");
    fs::create_dir_all(&project).expect("project dir");
    project
}

fn write_binding(state: &Path, surface: &SurfaceId, session: &SessionId) {
    let dir = state.join("surfaces").join(&surface.0);
    fs::create_dir_all(&dir).expect("surface dir");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).expect("surface mode");
    fs::write(dir.join("session"), session.as_str()).expect("session binding");
}

fn write_config(state: &Path, config: &OrchestratorConfig) {
    let path = OrchestratorHost::config_path(state, &config.name).expect("config path");
    fs::create_dir_all(path.parent().expect("config parent")).expect("config dir");
    fs::write(&path, serde_json::to_vec_pretty(config).expect("config json")).expect("config");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("config mode");
}

fn fake_app(root: &Path, label: &str, password: &str) -> (PathBuf, PathBuf) {
    let script = root.join(format!("{label}-app.sh"));
    let count = root.join(format!("{label}-pids"));
    let body = format!(
        "#!/bin/sh\n\
         set -eu\n\
         count=\"$1\"\n\
         password=\"{password}\"\n\
         printf '%s\\n' \"$$\" >> \"$count\"\n\
         state=\"${{XDG_STATE_HOME:?}}/opencode\"\n\
         mkdir -p \"$state\"\n\
         umask 077\n\
         printf '{{\"version\":\"2.0.28\",\"port\":54321,\"url\":\"http://127.0.0.1:54321\",\"password\":\"%s\",\"pid\":%s}}\\n' \"$password\" \"$$\" > \"$state/service.json\"\n\
         chmod 600 \"$state/service.json\"\n\
         while :; do sleep 0.05; done\n"
    );
    fs::write(&script, body).expect("fake app");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).expect("fake app mode");
    (script, count)
}

fn pid_count(count: &Path) -> usize {
    fs::read_to_string(count)
        .map(|text| text.lines().filter(|line| !line.is_empty()).count())
        .unwrap_or(0)
}

fn last_pid(count: &Path) -> u32 {
    fs::read_to_string(count)
        .expect("pid file")
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .next_back()
        .expect("a recorded pid")
}

fn process_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn wait_until(mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if check() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("condition did not become true");
}

fn start_command(name: &str, app: OrchestratorApp) -> OrchestratorCommand {
    OrchestratorCommand::Start {
        request_id: format!("req-start-{name}"),
        name: name.to_owned(),
        app,
        project: Some(project()),
    }
}

fn stop_command(name: &str) -> OrchestratorCommand {
    OrchestratorCommand::Stop {
        request_id: format!("req-stop-{name}"),
        name: name.to_owned(),
    }
}

fn resume_command(name: &str) -> OrchestratorCommand {
    OrchestratorCommand::Resume {
        request_id: format!("req-resume-{name}"),
        name: name.to_owned(),
    }
}

#[derive(Default)]
struct RecordingLink {
    attached: Vec<SurfaceBinding>,
    detached: Vec<SurfaceBinding>,
    fail_attach: bool,
}

impl SurfaceAttachment for RecordingLink {
    fn attach(&mut self, binding: &SurfaceBinding) -> Result<(), OrchestratorError> {
        if self.fail_attach {
            return Err(OrchestratorError::new(
                "attach_failed",
                "the surface attach frame could not be sent to the home",
                true,
            ));
        }
        self.attached.push(binding.clone());
        Ok(())
    }

    fn detach(&mut self, binding: &SurfaceBinding) -> Result<(), OrchestratorError> {
        self.detached.push(binding.clone());
        Ok(())
    }
}

fn host_with_app(state: &Path, script: &Path, count: &Path, _password: &str) -> OrchestratorHost {
    OrchestratorHost::new(state)
        .with_startup(Duration::from_secs(5))
        .with_stop_grace(Duration::from_millis(300))
        .with_app_command(
            OrchestratorApp::Opencode,
            script.display().to_string(),
            vec![count.display().to_string()],
        )
}

fn bind(state: &Path, name: &str, surface_id: &str, session_id: &str, working: &Path, script: &Path, count: &Path) {
    write_config(
        state,
        &OrchestratorConfig {
            name: name.to_owned(),
            app: OrchestratorApp::Opencode,
            project: Some(project()),
            session: Some(session(session_id)),
            surface: Some(surface(surface_id)),
            command: Some(script.display().to_string()),
            args: vec![count.display().to_string()],
            working_dir: Some(working.display().to_string()),
            service_json: Some(
                OrchestratorHost::dir(state, name)
                    .expect("dir")
                    .join(".opencode/state/opencode/service.json")
                    .display()
                    .to_string(),
            ),
        },
    );
}

#[test]
fn orchestrator_start_materializes_0600_config_and_starts_one_host() {
    let root = tempfile::tempdir().expect("temp dir");
    let state = agent_state(root.path());
    let working = project_root(root.path());
    write_binding(&state, &surface(SURFACE), &session(SESSION));
    let password = service_password();
    let (script, count) = fake_app(root.path(), "one", &password);
    let host = host_with_app(&state, &script, &count, &password);
    let mut link = RecordingLink::default();

    let result = host.start(
        &start_command("one", OrchestratorApp::Opencode),
        Some(&working),
        &mut link,
    );
    assert!(result.accepted, "{result:?}");
    assert_eq!(result.state, OrchestratorNodeState::Running);
    assert_eq!(link.attached.len(), 1);
    assert_eq!(link.attached[0].surface, surface(SURFACE));
    assert_eq!(link.attached[0].session, session(SESSION));

    let config_path = OrchestratorHost::config_path(&state, "one").expect("config path");
    let metadata = fs::metadata(&config_path).expect("config metadata");
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    let config: OrchestratorConfig =
        serde_json::from_slice(&fs::read(&config_path).expect("config bytes")).expect("config");
    assert_eq!(config.app, OrchestratorApp::Opencode);
    assert_eq!(config.project, Some(project()));
    assert_eq!(config.session, Some(session(SESSION)));
    assert_eq!(config.surface, Some(surface(SURFACE)));

    let host_path = OrchestratorHost::host_path(&state, "one").expect("host path");
    assert_eq!(
        fs::metadata(&host_path).expect("host metadata").permissions().mode() & 0o777,
        0o600
    );
    let host_state = host.state("one").expect("host state").expect("state exists");
    let pid = host_state.pid.expect("pid");
    assert_eq!(host_state.state, OrchestratorNodeState::Running);
    assert!(process_alive(pid));
    assert_eq!(pid_count(&count), 1);

    let result = host.stop(&stop_command("one"), &mut link);
    assert!(result.accepted);
    assert_eq!(result.state, OrchestratorNodeState::Stopped);
    wait_until(|| !process_alive(pid));
    assert!(!host_path.exists());
    assert_eq!(link.detached.len(), 1);
}

#[test]
fn orchestrator_duplicate_start_is_idempotent() {
    let root = tempfile::tempdir().expect("temp dir");
    let state = agent_state(root.path());
    let working = project_root(root.path());
    write_binding(&state, &surface(SURFACE), &session(SESSION));
    let password = service_password();
    let (script, count) = fake_app(root.path(), "dup", &password);
    let host = host_with_app(&state, &script, &count, &password);
    let mut link = RecordingLink::default();

    let first = host.start(
        &start_command("dup", OrchestratorApp::Opencode),
        Some(&working),
        &mut link,
    );
    assert!(first.accepted);
    let pid = host.state("dup").expect("state").expect("running").pid.expect("pid");
    let second = host.start(
        &start_command("dup", OrchestratorApp::Opencode),
        Some(&working),
        &mut link,
    );
    assert!(second.accepted, "{second:?}");
    assert_eq!(second.state, OrchestratorNodeState::Running);
    assert_eq!(pid_count(&count), 1, "a duplicate start spawned no second app");
    assert_eq!(host.state("dup").expect("state").expect("running").pid, Some(pid));
    assert_eq!(link.attached.len(), 1, "a running host is not attached twice");

    host.stop(&stop_command("dup"), &mut link);
}

#[test]
fn orchestrator_stop_kills_only_its_own_host_and_detaches() {
    let root = tempfile::tempdir().expect("temp dir");
    let state = agent_state(root.path());
    let working = project_root(root.path());
    let password = service_password();
    let (script, count_one) = fake_app(root.path(), "stop-one", &password);
    let (script_two, count_two) = fake_app(root.path(), "stop-two", &password);
    let host = host_with_app(&state, &script, &count_one, &password);
    bind(&state, "one", SURFACE, SESSION, &working, &script, &count_one);
    bind(
        &state,
        "two",
        SURFACE_TWO,
        SESSION_TWO,
        &working,
        &script_two,
        &count_two,
    );
    let mut link = RecordingLink::default();

    let first = host.start(
        &start_command("one", OrchestratorApp::Opencode),
        Some(&working),
        &mut link,
    );
    assert!(first.accepted, "{first:?}");
    let second = host.start(
        &start_command("two", OrchestratorApp::Opencode),
        Some(&working),
        &mut link,
    );
    assert!(second.accepted, "{second:?}");
    let one_pid = last_pid(&count_one);
    let two_pid = last_pid(&count_two);
    assert!(process_alive(one_pid) && process_alive(two_pid));

    let stopped = host.stop(&stop_command("one"), &mut link);
    assert!(stopped.accepted, "{stopped:?}");
    wait_until(|| !process_alive(one_pid));
    assert!(process_alive(two_pid), "the second host keeps running");
    assert_eq!(link.detached.len(), 1);
    assert_eq!(link.detached[0].surface, surface(SURFACE));
    assert_eq!(link.detached[0].session, session(SESSION));

    host.stop(&stop_command("two"), &mut link);
    wait_until(|| !process_alive(two_pid));
}

#[test]
fn orchestrator_resume_starts_again_after_stop() {
    let root = tempfile::tempdir().expect("temp dir");
    let state = agent_state(root.path());
    let working = project_root(root.path());
    write_binding(&state, &surface(SURFACE), &session(SESSION));
    let password = service_password();
    let (script, count) = fake_app(root.path(), "resume", &password);
    let host = host_with_app(&state, &script, &count, &password);
    let mut link = RecordingLink::default();

    let started = host.start(
        &start_command("resume", OrchestratorApp::Opencode),
        Some(&working),
        &mut link,
    );
    assert!(started.accepted);
    let first_pid = last_pid(&count);
    let stopped = host.stop(&stop_command("resume"), &mut link);
    assert!(stopped.accepted);
    wait_until(|| !process_alive(first_pid));

    let resumed = host.start(&resume_command("resume"), None, &mut link);
    assert!(resumed.accepted, "{resumed:?}");
    assert_eq!(resumed.state, OrchestratorNodeState::Running);
    let second_pid = last_pid(&count);
    assert_ne!(first_pid, second_pid);
    assert!(process_alive(second_pid));
    assert_eq!(pid_count(&count), 2);
    assert_eq!(link.attached.len(), 2);

    host.stop(&stop_command("resume"), &mut link);
    wait_until(|| !process_alive(second_pid));
}

#[test]
fn orchestrator_missing_binary_returns_a_typed_error_and_leaves_no_state() {
    let root = tempfile::tempdir().expect("temp dir");
    let state = agent_state(root.path());
    let working = project_root(root.path());
    write_binding(&state, &surface(SURFACE), &session(SESSION));
    let missing = root.path().join("no-such-app");
    let host = OrchestratorHost::new(&state)
        .with_startup(Duration::from_secs(2))
        .with_app_command(
            OrchestratorApp::Opencode,
            missing.display().to_string(),
            vec!["--fake".to_owned()],
        );
    let mut link = RecordingLink::default();

    let result = host.start(
        &start_command("missing", OrchestratorApp::Opencode),
        Some(&working),
        &mut link,
    );
    assert!(!result.accepted, "{result:?}");
    assert_eq!(result.state, OrchestratorNodeState::Failed);
    let error = result.error.expect("typed error");
    assert_eq!(error.code, "spawn_failed");
    assert!(!error.message.contains("no-such-app"), "{}", error.message);
    assert!(link.attached.is_empty());
    assert!(
        host.state("missing").expect("state").is_none(),
        "no partial host state is left"
    );
    assert!(fs::metadata(OrchestratorHost::host_path(&state, "missing").expect("path")).is_err());
}

#[test]
fn orchestrator_failed_attach_returns_a_typed_error_and_leaves_no_host() {
    let root = tempfile::tempdir().expect("temp dir");
    let state = agent_state(root.path());
    let working = project_root(root.path());
    write_binding(&state, &surface(SURFACE), &session(SESSION));
    let password = service_password();
    let (script, count) = fake_app(root.path(), "attach-fail", &password);
    let host = host_with_app(&state, &script, &count, &password);
    let mut link = RecordingLink {
        fail_attach: true,
        ..RecordingLink::default()
    };

    let result = host.start(
        &start_command("attach-fail", OrchestratorApp::Opencode),
        Some(&working),
        &mut link,
    );
    assert!(!result.accepted, "{result:?}");
    assert_eq!(result.state, OrchestratorNodeState::Failed);
    let error = result.error.expect("typed error");
    assert_eq!(error.code, "attach_failed");
    assert!(!error.message.contains(&state.display().to_string()));
    assert!(link.attached.is_empty());
    assert!(host.state("attach-fail").expect("state").is_none());
    let pid = last_pid(&count);
    wait_until(|| !process_alive(pid));
}

#[test]
fn orchestrator_bad_service_json_is_typed_and_leaves_no_host() {
    let root = tempfile::tempdir().expect("temp dir");
    let state = agent_state(root.path());
    let working = project_root(root.path());
    write_binding(&state, &surface(SURFACE), &session(SESSION));
    let script = root.path().join("bad-service.sh");
    fs::write(
        &script,
        "#!/bin/sh\n\
         state=\"${XDG_STATE_HOME:?}/opencode\"\n\
         mkdir -p \"$state\"\n\
         umask 077\n\
         printf '{not json}\\n' > \"$state/service.json\"\n\
         while :; do sleep 0.05; done\n",
    )
    .expect("bad script");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).expect("script mode");
    let host = OrchestratorHost::new(&state)
        .with_startup(Duration::from_secs(2))
        .with_app_command(OrchestratorApp::Opencode, script.display().to_string(), Vec::new());
    let mut link = RecordingLink::default();

    let result = host.start(
        &start_command("bad-json", OrchestratorApp::Opencode),
        Some(&working),
        &mut link,
    );
    assert!(!result.accepted, "{result:?}");
    let error = result.error.expect("typed error");
    assert_eq!(error.code, "config_invalid");
    assert!(!error.message.contains(&state.display().to_string()));
    assert!(host.state("bad-json").expect("state").is_none());
    assert!(link.attached.is_empty());
}

#[test]
fn orchestrator_never_logs_the_port_password_or_absolute_path() {
    let root = tempfile::tempdir().expect("temp dir");
    let state = agent_state(root.path());
    let working = project_root(root.path());
    write_binding(&state, &surface(SURFACE), &session(SESSION));
    let password = service_password();
    let (script, count) = fake_app(root.path(), "redact", &password);
    let host = host_with_app(&state, &script, &count, &password);
    let mut link = RecordingLink::default();

    let result = host.start(
        &start_command("redact", OrchestratorApp::Opencode),
        Some(&working),
        &mut link,
    );
    assert!(result.accepted, "{result:?}");
    let serialized = serde_json::to_string(&result).expect("result json");
    let host_state =
        fs::read_to_string(OrchestratorHost::host_path(&state, "redact").expect("path")).expect("host state");
    let config = fs::read_to_string(OrchestratorHost::config_path(&state, "redact").expect("path")).expect("config");
    let state_text = state.display().to_string();
    for text in [&serialized, &host_state] {
        assert!(!text.contains(&password), "{text}");
        assert!(!text.contains("54321"), "{text}");
        assert!(!text.contains("127.0.0.1"), "{text}");
        assert!(!text.contains(&state_text), "{text}");
        assert!(!text.contains(&working.display().to_string()), "{text}");
    }
    assert!(
        !config.contains(&password),
        "the service password is not copied into config"
    );

    let missing = root.path().join("absent-app");
    let other = OrchestratorHost::new(&state)
        .with_startup(Duration::from_secs(2))
        .with_app_command(OrchestratorApp::Opencode, missing.display().to_string(), Vec::new());
    let failed = other.start(
        &start_command("redact-missing", OrchestratorApp::Opencode),
        Some(&working),
        &mut link,
    );
    let serialized = serde_json::to_string(&failed).expect("failed json");
    assert!(!serialized.contains("absent-app"), "{serialized}");
    assert!(!serialized.contains(&state_text), "{serialized}");
    assert!(!serialized.contains(&password), "{serialized}");

    host.stop(&stop_command("redact"), &mut link);
}

#[test]
fn orchestrator_disconnect_reports_degraded_and_reconnect_does_not_spawn_a_second_host() {
    let root = tempfile::tempdir().expect("temp dir");
    let state = agent_state(root.path());
    let working = project_root(root.path());
    write_binding(&state, &surface(SURFACE), &session(SESSION));
    let password = service_password();
    let (script, count) = fake_app(root.path(), "degraded", &password);
    let host = host_with_app(&state, &script, &count, &password);
    let mut link = RecordingLink::default();

    let started = host.start(
        &start_command("degraded", OrchestratorApp::Opencode),
        Some(&working),
        &mut link,
    );
    assert!(started.accepted);
    let pid = last_pid(&count);
    host.mark_degraded().expect("mark degraded");
    let state_file = host.state("degraded").expect("state").expect("degraded state");
    assert_eq!(state_file.state, OrchestratorNodeState::Degraded);
    assert_eq!(state_file.pid, Some(pid));
    assert!(process_alive(pid));

    let live = host.reconcile().expect("reconcile");
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].name, "degraded");
    assert_eq!(live[0].binding.surface, surface(SURFACE));
    host.mark_running("degraded").expect("mark running");
    assert_eq!(
        host.state("degraded").expect("state").expect("running").state,
        OrchestratorNodeState::Running
    );
    assert_eq!(pid_count(&count), 1);
    assert!(process_alive(pid));

    // A duplicate Start after the reconnect still reports the existing state and never
    // spawns a second process.
    let again = host.start(
        &start_command("degraded", OrchestratorApp::Opencode),
        Some(&working),
        &mut link,
    );
    assert!(again.accepted);
    assert_eq!(pid_count(&count), 1);
    assert_eq!(last_pid(&count), pid);

    host.stop(&stop_command("degraded"), &mut link);
}

#[test]
fn names_are_checked_before_any_path_is_built() {
    for name in ["", ".", "..", "../escape", "/tmp/escape", "a/b", "a\\b"] {
        assert!(validate_name(name).is_err(), "{name:?}");
    }
    let result = OrchestratorHost::new(std::env::temp_dir()).prepare_start(
        &OrchestratorCommand::Start {
            request_id: "req".to_owned(),
            name: "../escape".to_owned(),
            app: OrchestratorApp::Opencode,
            project: None,
        },
        None,
    );
    match result {
        vowp_agent::PrepareOutcome::Failed(result) => {
            assert_eq!(result.error.expect("error").code, "invalid_name")
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[derive(Debug, Clone, Copy)]
struct FixedClock(Timestamp);

impl AgentClock for FixedClock {
    fn now(&self) -> Timestamp {
        self.0
    }

    fn sleep(&self, _duration: Duration) {}
}

#[derive(Default)]
struct FakeState {
    connections: usize,
    received: Vec<AgentFrame>,
}

#[derive(Clone)]
struct FakeHome {
    shared: Arc<Mutex<FakeState>>,
    command: OrchestratorCommand,
    clock: FixedClock,
}

impl FakeHome {
    fn new(command: OrchestratorCommand) -> Self {
        Self {
            shared: Arc::new(Mutex::new(FakeState::default())),
            command,
            clock: FixedClock(Timestamp::parse("2026-11-14T10:00:00Z").expect("timestamp")),
        }
    }

    fn frames(&self) -> Vec<AgentFrame> {
        self.shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .received
            .clone()
    }
}

impl Transport for FakeHome {
    fn open(&mut self) -> Result<Box<dyn FrameStream>, AgentError> {
        let mut state = self.shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let connection = state.connections;
        state.connections += 1;
        drop(state);
        Ok(Box::new(FakeStream {
            shared: Arc::clone(&self.shared),
            command: self.command.clone(),
            clock: self.clock,
            connection,
            outbound: VecDeque::new(),
            sent_command: false,
        }))
    }
}

struct FakeStream {
    shared: Arc<Mutex<FakeState>>,
    command: OrchestratorCommand,
    clock: FixedClock,
    connection: usize,
    outbound: VecDeque<AgentFrame>,
    sent_command: bool,
}

impl FrameStream for FakeStream {
    fn recv(&mut self, _timeout: Option<Duration>) -> Result<FrameRead, AgentError> {
        if let Some(frame) = self.outbound.pop_front() {
            return Ok(FrameRead::Frame(frame));
        }
        if !self.sent_command {
            self.sent_command = true;
            self.outbound
                .push_back(AgentFrame::OrchestratorCommand(self.command.clone()));
            self.outbound.push_back(AgentFrame::Ping);
            return Ok(FrameRead::Frame(self.outbound.pop_front().expect("the queued command")));
        }
        Ok(FrameRead::Timeout)
    }

    fn send(&mut self, frame: &AgentFrame) -> Result<(), AgentError> {
        self.shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .received
            .push(frame.clone());
        match frame {
            AgentFrame::AgentHello { .. } => self.outbound.push_back(AgentFrame::HomeChallenge {
                nonce: format!("nonce-{}", self.connection),
                home_id: "home-1".to_owned(),
                connection_id: format!("connection-{}", self.connection),
                server_time: self.clock.now(),
                heartbeat_secs: 10,
                max_frame_bytes: 1024 * 1024,
            }),
            AgentFrame::AgentAuth { .. } => self.outbound.push_back(AgentFrame::HomeWelcome {
                protocol: AGENT_PROTOCOL.to_owned(),
                home_id: "home-1".to_owned(),
                node: node(),
                connection_id: format!("connection-{}", self.connection),
            }),
            AgentFrame::OrchestratorResult(_) => self.outbound.push_back(AgentFrame::Revoked {
                reason: "test complete".to_owned(),
            }),
            _ => {}
        }
        Ok(())
    }
}

fn connection_host_config(
    state: &Path,
    name: &str,
    script: &Path,
    count: &Path,
    _password: &str,
) -> OrchestratorConfig {
    let dir = OrchestratorHost::dir(state, name).expect("dir");
    OrchestratorConfig {
        name: name.to_owned(),
        app: OrchestratorApp::Opencode,
        project: Some(project()),
        session: Some(session(SESSION_THREE)),
        surface: Some(surface(SURFACE_THREE)),
        command: Some(script.display().to_string()),
        args: vec![count.display().to_string()],
        working_dir: Some(dir.display().to_string()),
        service_json: Some(dir.join(".opencode/state/opencode/service.json").display().to_string()),
    }
}

#[test]
fn orchestrator_connection_result_is_correlated_and_revoke_cleans_the_host() {
    let root = tempfile::tempdir().expect("temp dir");
    let state = agent_state(root.path());
    let working = project_root(root.path());
    write_binding(&state, &surface(SURFACE_THREE), &session(SESSION_THREE));
    let password = service_password();
    let (script, count) = fake_app(root.path(), "connection", &password);
    write_config(
        &state,
        &connection_host_config(&state, "connection", &script, &count, &password),
    );
    let command = start_command("connection", OrchestratorApp::Opencode);
    let request_id = command.request_id().to_owned();
    let home = FakeHome::new(command);

    let key = NodeKey::generate(&state).expect("node key");
    let config = AgentConfig::new("home-1", node(), state.clone(), "example.invalid");
    let mut client = AgentClient::new(
        state.clone(),
        key,
        config,
        Box::new(home.clone()),
        Arc::new(FixedClock(Timestamp::parse("2026-11-14T10:00:00Z").expect("timestamp"))),
    );
    let outcome = client.run(&RunOptions::new(AgentRole::Surface)).expect("run outcome");
    assert!(matches!(outcome, RunOutcome::Revoked { .. }), "{outcome:?}");

    let frames = home.frames();
    let results: Vec<_> = frames
        .iter()
        .filter_map(|frame| match frame {
            AgentFrame::OrchestratorResult(result) => Some(result.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 1, "{frames:?}");
    assert_eq!(results[0].request_id, request_id);
    assert!(results[0].accepted);
    assert_eq!(results[0].state, OrchestratorNodeState::Running);
    assert!(
        frames.iter().any(|frame| matches!(frame, AgentFrame::Pong)),
        "the ping is answered while the start is in flight: {frames:?}"
    );
    assert!(
        frames.iter().any(
            |frame| matches!(frame, AgentFrame::SurfaceAttach(attach) if attach.surface == surface(SURFACE_THREE))
        ),
        "the surface is attached: {frames:?}"
    );

    assert_eq!(pid_count(&count), 1);
    let pid = last_pid(&count);
    wait_until(|| !process_alive(pid));
    assert!(
        fs::metadata(OrchestratorHost::dir(&state, "connection").expect("dir")).is_err(),
        "revoke removes the private orchestrator directory"
    );
    let serialized = serde_json::to_string(&results[0]).expect("result json");
    assert!(!serialized.contains(&state.display().to_string()));
    assert!(!serialized.contains(&password));
    assert!(!serialized.contains("54321"));
    let _ = working;
}
