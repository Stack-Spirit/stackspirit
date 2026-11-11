//! `vowp chat` fallback-surface tests.
//!
//! Every test gets a temporary node home and project, a private tmux directory and a fake
//! opencode script. Turns run through the real `vowp __exec` child, so the lease token
//! never appears in argv and the executor is the surface's own machine.

mod common;

use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use vowp_cli::chat::{ChatOptions, capture_follow_start};
use vowp_cli::{BufferIo, Ctx};
use vowp_core::surface::follow::follow_to;
use vowp_protocol::{
    Actor, CancelToken, CapsuleTrigger, EventBody, EventSource, Executor, HarnessKind, NodeName, Note, RunPurpose,
    SessionId, SystemClock,
};
use vowp_store::NodePaths;
use vowp_store::session_log::{NewEvent, SessionLog, index_row};

/// Writes a fake opencode binary that replies with `reply` and stops normally.
fn fake_opencode(env: &common::TestEnv, reply: &str) -> PathBuf {
    let dir = env.home_path().join("fake-bin");
    std::fs::create_dir_all(&dir).expect("fake bin dir");
    let events = [
        serde_json::json!({
            "type": "text",
            "timestamp": 1_794_168_421_621_i64,
            "sessionID": "ses_fake",
            "part": {"text": reply}
        }),
        serde_json::json!({
            "type": "step_finish",
            "timestamp": 1_794_168_421_622_i64,
            "sessionID": "ses_fake",
            "part": {"cost": 0.0, "tokens": {"input": 1, "output": 1}, "reason": "stop"}
        }),
    ];
    let fixture = dir.join("reply.jsonl");
    let text = events.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n");
    std::fs::write(&fixture, format!("{text}\n")).expect("fixture");
    let script = dir.join("opencode");
    let body = format!(
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then printf '%s\\n' '2.0.28'; exit 0; fi\ncat '{}'\n",
        fixture.display()
    );
    std::fs::write(&script, body).expect("fake script");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    script
}

/// A valid capsule draft, used when a switch must write a capsule.
fn capsule_reply() -> String {
    serde_json::json!({
        "intent": "continue the chat",
        "constraints": [],
        "decisions": [],
        "state": {"done": [], "in_progress": [], "blocked": [], "next_steps": ["answer"]},
        "files": [],
        "gotchas": [],
        "open_questions": []
    })
    .to_string()
}

/// Rewrites the project config to use the fake opencode binary.
fn configure(env: &common::TestEnv, reply: &str) -> PathBuf {
    let script = fake_opencode(env, reply);
    let mut config = env.config.clone();
    config.harnesses.insert(
        "opencode".to_owned(),
        vowp_protocol::HarnessConfig {
            binary: Some(script.display().to_string()),
            ..vowp_protocol::HarnessConfig::default()
        },
    );
    std::fs::write(
        env.project_path().join("vowp.toml"),
        toml::to_string_pretty(&config).expect("config"),
    )
    .expect("write config");
    script
}

/// Creates one interactive session and returns its id.
fn create(env: &common::TestEnv) -> String {
    let (code, stdout, stderr) = env.run(&[
        "session",
        "new",
        "--title",
        "chat test",
        "--executor",
        "opencode:deepseek/deepseek-flash#max",
    ]);
    assert_eq!(code, 0, "{stderr}");
    stdout.trim().to_owned()
}

/// Starts the real `vowp chat` binary with a writable stdin pipe.
fn spawn_chat(env: &common::TestEnv, args: &[&str]) -> Child {
    let mut command = env.command(env!("CARGO_BIN_EXE_vowp"));
    command
        .arg("--home")
        .arg(env.home_path())
        .arg("--project")
        .arg(env.project_path())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.spawn().expect("spawn vowp chat")
}

/// Sends one line to a running chat child.
fn send_chat(child: &mut Child, text: &str) {
    let stdin = child.stdin.as_mut().expect("chat stdin");
    stdin.write_all(text.as_bytes()).expect("write chat input");
    stdin.flush().expect("flush chat input");
}

/// Waits for a condition, polling briefly.
fn wait_until(timeout: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    condition()
}

/// Runs the real `vowp chat` binary with `input` on stdin.
fn run_chat(env: &common::TestEnv, args: &[&str], input: &str) -> (i32, String, String) {
    let mut child = spawn_chat(env, args);
    if let Some(mut stdin) = child.stdin.take() {
        // A lease refusal exits before the test writes the scripted input, so the pipe may
        // already be closed. That is the expected outcome for those runs, not a test bug.
        if let Err(error) = stdin.write_all(input.as_bytes()) {
            assert_eq!(
                error.kind(),
                std::io::ErrorKind::BrokenPipe,
                "write chat input: {error}"
            );
        }
    }
    let output = child.wait_with_output().expect("wait for vowp chat");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// Waits until `holder` owns the lease of `id`.
fn wait_for_lease(env: &common::TestEnv, id: &str, holder: &str) -> bool {
    let session = SessionId::parse(id).expect("session id");
    wait_until(Duration::from_secs(5), || {
        env.store()
            .lease(&session)
            .expect("lease query")
            .is_some_and(|lease| lease.holder == holder)
    })
}

/// Reads one session's canonical events.
fn events(env: &common::TestEnv, id: &str) -> Vec<vowp_protocol::EventEnvelope> {
    let session = SessionId::parse(id).expect("session id");
    SessionLog::open_read(&NodePaths::new(env.home_path()), &session)
        .expect("open session log")
        .read_all()
        .expect("read session log")
}

#[test]
fn chat_takes_over_and_prints_notice() {
    let env = common::TestEnv::new();
    configure(&env, &capsule_reply());
    let id = create(&env);

    // Leave a live lease held by another surface.
    let (code, token, stderr) = env.run(&[
        "session",
        "lease",
        &id,
        "--acquire",
        "chat",
        "--holder",
        "laptop",
        "--node",
        "local",
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert!(!token.trim().is_empty());

    let (code, stdout, stderr) = run_chat(&env, &["chat", &id, "--takeover"], "/status\n/quit\n");
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("took session"), "{stdout}");
    assert!(stdout.contains("from laptop"), "{stdout}");
    assert!(stdout.contains("lease: local (chat) on local"), "{stdout}");

    let session = SessionId::parse(&id).expect("session id");
    let last_seq = env.store().require_session(&session).expect("session").last_seq;
    assert!(
        stdout.contains(&format!("events: {last_seq}")),
        "/status used the pre-attach sequence: {stdout}"
    );
    assert!(
        env.store().lease(&session).expect("lease query").is_none(),
        "chat must release the lease on exit"
    );
    let notes: Vec<String> = events(&env, &id)
        .into_iter()
        .filter_map(|event| match event.body() {
            Ok(EventBody::Note(note)) => Some(note.text),
            _ => None,
        })
        .collect();
    assert!(
        notes.iter().any(|note| note.starts_with("taken over by local at ")),
        "takeover note missing: {notes:?}"
    );
}

#[test]
fn chat_live_lease_without_terminal_exits_4() {
    let env = common::TestEnv::new();
    configure(&env, "hello");
    let id = create(&env);
    let (code, _token, stderr) = env.run(&[
        "session",
        "lease",
        &id,
        "--acquire",
        "chat",
        "--holder",
        "laptop",
        "--node",
        "local",
    ]);
    assert_eq!(code, 0, "{stderr}");

    let (code, _stdout, stderr) = run_chat(&env, &["chat", &id], "/quit\n");
    assert_eq!(code, 4, "{stderr}");
    assert!(stderr.contains("active on laptop"), "{stderr}");
}

#[test]
fn chat_tty_prompts_and_accepts_takeover() {
    let env = common::TestEnv::new();
    configure(&env, &capsule_reply());
    let id = create(&env);
    let (code, _token, stderr) = env.run(&[
        "session",
        "lease",
        &id,
        "--acquire",
        "chat",
        "--holder",
        "laptop",
        "--node",
        "local",
    ]);
    assert_eq!(code, 0, "{stderr}");

    let session = SessionId::parse(&id).expect("session id");
    let global = env.global(false);
    let mut io = BufferIo::with_tty("y\n/quit\n", true);
    let mut ctx = Ctx::build(&global, &io).expect("build context");
    let code = vowp_cli::chat::run(
        &mut ctx,
        &mut io,
        &ChatOptions {
            session,
            executor: None,
            node: None,
            follow: false,
            takeover: false,
        },
    )
    .expect("chat runs");
    assert_eq!(code, 0);
    let stdout = io.stdout();
    assert!(stdout.contains("Active on laptop since "), "{stdout}");
    assert!(stdout.contains("Take over? [y/N]"), "{stdout}");
    assert!(stdout.contains("took session"), "{stdout}");
}

#[test]
fn chat_follow_mode_is_read_only() {
    let env = common::TestEnv::new();
    configure(&env, "hello");
    let id = create(&env);
    let session = SessionId::parse(&id).expect("session id");
    let before = env.store().require_session(&session).expect("session").last_seq;

    let (code, _stdout, stderr) = run_chat(&env, &["chat", &id, "--follow"], "please write this\n/quit\n");
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stderr.contains("This surface is read-only (follow mode)."),
        "follow mode must refuse writes: {stderr}"
    );
    let after = env.store().require_session(&session).expect("session").last_seq;
    assert_eq!(before, after, "follow mode wrote to the session");
    assert!(
        env.store().lease(&session).expect("lease query").is_none(),
        "follow mode must not take the lease"
    );
}

#[test]
fn chat_follow_prints_new_events_at_narrow_width() {
    let env = common::TestEnv::new();
    configure(&env, "hello");
    let id = create(&env);
    let session = SessionId::parse(&id).expect("session id");
    let store = env.store();
    let before = store.require_session(&session).expect("session").last_seq;
    let paths = NodePaths::new(env.home_path());
    let log = SessionLog::open(&paths, &session, Arc::new(SystemClock)).expect("writer");
    let appended = log
        .append(NewEvent {
            actor: Actor::System,
            source: EventSource {
                harness: HarnessKind::Vowp,
                model: None,
                node: Some(NodeName::parse("local").expect("node")),
                native_session_id: None,
            },
            body: EventBody::Note(Note {
                text: "follow output event ".repeat(20),
            }),
            raw: None,
            ts: None,
        })
        .expect("append note");
    store.index_event(&index_row(&appended)).expect("index event");
    log.sync().expect("sync log");

    let cancel = CancelToken::default();
    cancel.cancel();
    let mut out = Vec::new();
    let last = follow_to(&store, &session, before, &cancel, &mut out).expect("follow");
    assert_eq!(last, appended.envelope.seq);
    let text = String::from_utf8(out).expect("utf8");
    assert!(text.contains("follow output event"), "{text}");
    for line in text.lines() {
        assert!(
            line.chars().count() <= 80,
            "follower line is wider than 80 columns: {line:?}"
        );
    }
}

#[test]
fn chat_follow_command_releases_the_lease_and_prints_new_events() {
    let env = common::TestEnv::new();
    configure(&env, "hello");
    let id = create(&env);
    let session = SessionId::parse(&id).expect("session id");
    let mut child = spawn_chat(&env, &["chat", &id]);
    assert!(wait_for_lease(&env, &id, "local"), "chat did not take the lease");

    send_chat(&mut child, "/follow\n");
    assert!(
        wait_until(Duration::from_secs(3), || {
            env.store().lease(&session).expect("lease query").is_none()
        }),
        "/follow kept the lease"
    );

    // An event appended after `/follow` must reach the follower output.
    let paths = NodePaths::new(env.home_path());
    let log = SessionLog::open(&paths, &session, Arc::new(SystemClock)).expect("writer");
    let appended = log
        .append(NewEvent {
            actor: Actor::System,
            source: EventSource {
                harness: HarnessKind::Vowp,
                model: None,
                node: Some(NodeName::parse("local").expect("node")),
                native_session_id: None,
            },
            body: EventBody::Note(Note {
                text: "the follower sees this".to_owned(),
            }),
            raw: None,
            ts: None,
        })
        .expect("append note");
    env.store().index_event(&index_row(&appended)).expect("index event");
    log.sync().expect("sync log");

    send_chat(&mut child, "hello\n/quit\n");
    let output = child.wait_with_output().expect("wait for chat");
    assert_eq!(output.status.code().unwrap_or(-1), 0);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("following session"), "{stdout}");
    assert!(
        stdout.contains(&format!("#{} note", appended.envelope.seq)),
        "the follower did not print the new event: {stdout}"
    );
    assert!(stdout.contains("the follower sees this"), "{stdout}");
    assert!(
        stderr.contains("This surface is read-only (follow mode)."),
        "follow mode must refuse writes: {stderr}"
    );
    assert!(
        !stderr.contains("Taken over by local"),
        "a released lease must not look like a self-takeover: {stderr}"
    );
}

#[test]
fn chat_follow_captures_the_start_sequence_before_the_lease_release() {
    let env = common::TestEnv::new();
    configure(&env, "hello");
    let id = create(&env);
    let session = SessionId::parse(&id).expect("session id");
    let store = env.store();
    let before = store.require_session(&session).expect("session").last_seq;
    let paths = NodePaths::new(env.home_path());
    let log = SessionLog::open(&paths, &session, Arc::new(SystemClock)).expect("writer");

    // The closure stands in for the lease release. An event indexed here becomes
    // visible after the start sequence is captured but before the follower starts.
    let mut release_window_event = None;
    let start = capture_follow_start(&store, &session, || {
        let event = log
            .append(NewEvent {
                actor: Actor::System,
                source: EventSource {
                    harness: HarnessKind::Vowp,
                    model: None,
                    node: Some(NodeName::parse("local").expect("node")),
                    native_session_id: None,
                },
                body: EventBody::Note(Note {
                    text: "indexed during the release window".to_owned(),
                }),
                raw: None,
                ts: None,
            })
            .expect("append release-window note");
        store.index_event(&index_row(&event)).expect("index event");
        log.sync().expect("sync log");
        release_window_event = Some(event);
        Ok(())
    })
    .expect("capture follow start");

    let event = release_window_event.expect("release-window event");
    assert_eq!(
        start, before,
        "the follower must start from the pre-release sequence, not the post-release row"
    );
    assert!(
        event.envelope.seq > start,
        "the release-window event must be newer than the captured start: event {} start {start}",
        event.envelope.seq
    );

    let cancel = CancelToken::default();
    cancel.cancel();
    let mut out = Vec::new();
    follow_to(&store, &session, start, &cancel, &mut out).expect("follow");
    let text = String::from_utf8(out).expect("utf8");
    assert!(
        text.contains(&format!("#{} note", event.envelope.seq)),
        "the follower skipped an event indexed during the release window: {text}"
    );
    assert!(text.contains("indexed during the release window"), "{text}");
}

#[test]
fn chat_switch_after_takeover_is_read_only() {
    let env = common::TestEnv::new();
    configure(&env, &capsule_reply());
    let id = create(&env);
    let session = SessionId::parse(&id).expect("session id");
    let mut child = spawn_chat(&env, &["chat", &id]);
    assert!(wait_for_lease(&env, &id, "local"), "chat did not take the lease");

    let (code, _stdout, stderr) = env.run(&[
        "session", "takeover", &id, "--kind", "chat", "--holder", "phone", "--node", "local",
    ]);
    assert_eq!(code, 0, "{stderr}");
    let after_takeover = env.store().list_switches(&session).expect("switches").len();

    send_chat(
        &mut child,
        "/switch opencode:deepseek/deepseek-flash#max --node local\n/quit\n",
    );
    let output = child.wait_with_output().expect("wait for chat");
    assert_eq!(output.status.code().unwrap_or(-1), 0);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Taken over by phone at "), "{stderr}");
    assert!(stderr.contains("read-only"), "{stderr}");
    assert!(!stdout.contains("switched to"), "old surface switched: {stdout}");

    let switches = env.store().list_switches(&session).expect("switches");
    assert_eq!(
        switches.len(),
        after_takeover,
        "a revoked surface wrote another switch: {switches:?}"
    );
    assert!(
        !switches
            .iter()
            .any(|switch| switch.reason == vowp_protocol::SwitchReason::User),
        "a revoked surface wrote a user switch: {switches:?}"
    );
}

#[test]
fn chat_switch_to_other_node_is_refused() {
    let env = common::TestEnv::new();
    configure(&env, "hello");
    let id = create(&env);
    let session = SessionId::parse(&id).expect("session id");

    let (code, stdout, stderr) = run_chat(
        &env,
        &["chat", &id],
        "/switch opencode:deepseek/deepseek-flash#max --node other\n/quit\n",
    );
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stderr.contains("executor node other is not this surface (local)"),
        "{stderr}"
    );
    assert!(!stdout.contains("switched to"), "{stdout}");
    assert!(
        env.store().list_switches(&session).expect("switches").is_empty(),
        "a refused node must not record a switch"
    );
}

#[test]
fn chat_executor_node_is_refused_before_switching() {
    let env = common::TestEnv::new();
    configure(&env, "hello");
    let id = create(&env);
    let session = SessionId::parse(&id).expect("session id");

    let (code, _stdout, stderr) = run_chat(
        &env,
        &[
            "chat",
            &id,
            "--executor",
            "opencode:deepseek/deepseek-flash#max",
            "--node",
            "other",
        ],
        "/quit\n",
    );
    assert_eq!(code, 2, "{stderr}");
    assert!(
        stderr.contains("executor node other is not this surface (local)"),
        "{stderr}"
    );
    assert!(
        env.store().list_switches(&session).expect("switches").is_empty(),
        "the refused top-level node recorded a switch"
    );
}

#[test]
fn chat_slash_switch_status_capsule() {
    let env = common::TestEnv::new();
    configure(&env, &capsule_reply());
    let id = create(&env);
    let session = SessionId::parse(&id).expect("session id");

    let mut child = spawn_chat(&env, &["chat", &id]);
    assert!(wait_for_lease(&env, &id, "local"), "chat did not take the lease");
    send_chat(
        &mut child,
        "/status\n/switch opencode:deepseek/deepseek-flash#max --node local\n/capsule\n",
    );

    let store = env.store();
    assert!(
        wait_until(Duration::from_secs(5), || {
            store
                .latest_capsule(&session)
                .expect("capsules")
                .is_some_and(|capsule| capsule.trigger == CapsuleTrigger::Manual)
        }),
        "/capsule did not write a capsule"
    );
    assert!(
        !store.list_switches(&session).expect("switches").is_empty(),
        "/switch must record a switch row"
    );

    // A takeover revokes this surface's token. The named slash test must cover the
    // revoked switch, not only the happy path as the lease holder.
    let (code, _stdout, stderr) = env.run(&[
        "session", "takeover", &id, "--kind", "chat", "--holder", "phone", "--node", "local",
    ]);
    assert_eq!(code, 0, "{stderr}");
    let after_takeover = store.list_switches(&session).expect("switches").len();

    send_chat(
        &mut child,
        "/switch opencode:deepseek/deepseek-flash#max --node local\n/quit\n",
    );
    let output = child.wait_with_output().expect("wait for chat");
    assert_eq!(output.status.code().unwrap_or(-1), 0);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("session:"), "{stdout}");
    assert_eq!(
        stdout.matches("switched to").count(),
        1,
        "only the pre-takeover switch may succeed: {stdout}"
    );
    assert!(stderr.contains("Taken over by phone at "), "{stderr}");
    assert!(stderr.contains("read-only"), "{stderr}");

    let switches = store.list_switches(&session).expect("switches");
    assert_eq!(
        switches.len(),
        after_takeover,
        "a revoked surface wrote another switch: {switches:?}"
    );
    let lease = store
        .lease(&session)
        .expect("lease query")
        .expect("the new holder's lease");
    assert_eq!(
        lease.holder, "phone",
        "the revoked surface released the new holder's lease"
    );
    assert!(
        store.latest_capsule(&session).expect("capsules").is_some(),
        "/capsule must write a capsule"
    );
}

#[test]
fn chat_remote_executor_uses_surface_machine() {
    let env = common::TestEnv::new();
    configure(&env, "reply from the surface");
    let id = create(&env);

    let (code, stdout, stderr) = run_chat(&env, &["chat", &id], "hello\n/quit\n");
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("reply from the surface"), "{stdout}");

    let attached = events(&env, &id)
        .into_iter()
        .filter_map(|event| match event.body() {
            Ok(EventBody::SessionExecutor(payload)) => payload.run_id.map(|run| (run, payload.node)),
            _ => None,
        })
        .next_back()
        .expect("the turn records an executor attach");
    let run = env.store().require_run(&attached.0).expect("run row");
    assert_eq!(run.node.as_str(), "local", "the executor must be the surface machine");
    assert_eq!(
        run.purpose,
        RunPurpose::Start,
        "the first chat turn must start the session, not continue it"
    );
    assert_eq!(
        run.executor,
        Some(Executor::parse("opencode:deepseek/deepseek-flash#max").expect("executor"))
    );
    assert_eq!(attached.1.as_str(), "local");
    assert_eq!(
        run.executor.as_ref().map(|executor| executor.harness),
        Some(HarnessKind::Opencode)
    );
}

#[test]
fn chat_narrow_terminal_no_colour_or_wide_tables() {
    let env = common::TestEnv::new();
    let reply = "word ".repeat(40);
    configure(&env, &reply);
    let id = create(&env);

    let (code, stdout, stderr) = run_chat(&env, &["chat", &id], "talk\n/quit\n");
    assert_eq!(code, 0, "{stderr}");
    assert!(!stdout.contains('\u{1b}'), "chat printed ANSI colour: {stdout:?}");
    for line in stdout.lines() {
        assert!(
            line.chars().count() <= 80,
            "chat line is wider than 80 columns: {line:?}"
        );
    }
}

#[test]
fn chat_non_tty_reads_turns_until_eof() {
    let env = common::TestEnv::new();
    configure(&env, "reply");
    let id = create(&env);

    let (code, stdout, stderr) = run_chat(&env, &["chat", &id], "first\nsecond\n");
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout.matches("reply").count(), 2, "{stdout}");
    let user_messages = events(&env, &id)
        .into_iter()
        .filter(|event| matches!(event.body(), Ok(EventBody::UserMessage(_))))
        .count();
    assert_eq!(user_messages, 2, "chat must run every turn before EOF");
}
