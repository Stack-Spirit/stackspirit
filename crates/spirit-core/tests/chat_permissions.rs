//! Hermetic tests for live tool activity and permission prompts.
//!
//! A fake loopback OpenCode serves the v1 HTTP/SSE routes this feature touches. Its turn
//! request blocks until the permission is answered, which is exactly the hang the owner
//! saw before StackSpirit had a permission card. No model, key, sidecar or owner state is
//! involved.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::broadcast::Receiver;

use spirit_core::engine::EngineStatus;
use spirit_core::engine::chat::{
    ChatEngine, ChatEvent, ChatLogQuery, ChatPermissionDecision, ChatSessionId,
    ChatSessionLocation, ChatSessionState, ChatSessionView, ChatStartInput, ChatToolStatus,
    LocalChatProvider, StreamSupport,
};
use spirit_core::error::SpiritError;

const SECRET: &str = "sk-live-9f8e7d6c5b4a39281716ABCDEFGH";

struct Engine(u16);

#[async_trait]
impl ChatEngine for Engine {
    async fn ensure_ready(&self) -> Result<u16, SpiritError> {
        Ok(self.0)
    }

    fn selected_model(&self) -> Option<spirit_core::engine::chat::ChatModelView> {
        Some(spirit_core::engine::chat::ChatModelView {
            id: "opencode/big-pickle".to_owned(),
            label: "Big Pickle".to_owned(),
            provider: "opencode".to_owned(),
            tier: spirit_core::engine::chat::ChatModelTier::Included,
            context_tokens: Some(128_000),
        })
    }

    fn status(&self) -> EngineStatus {
        EngineStatus::default()
    }
}

#[derive(Default)]
struct State {
    log: Vec<Value>,
    /// Pending permission requests, as `GET /permission` lists them.
    permissions: Vec<Value>,
    /// SSE frames waiting to be pushed.
    frames: Vec<Value>,
    replies: Vec<(String, String)>,
    /// When set, `POST /permission/{id}/reply` answers 404 so the legacy route is used.
    legacy_only: bool,
    answered: Option<String>,
    turn_with_permission: bool,
    turn_started: bool,
}

struct Fake {
    port: u16,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Fake {
    fn new(configure: impl FnOnce(&mut State)) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut state = State::default();
        configure(&mut state);
        let state = Arc::new(Mutex::new(state));
        let stop = Arc::new(AtomicBool::new(false));
        let (s, st) = (Arc::clone(&state), Arc::clone(&stop));
        let thread = thread::spawn(move || {
            while !st.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let (s, st) = (Arc::clone(&s), Arc::clone(&st));
                        thread::spawn(move || handle(stream, &s, &st));
                    }
                    Err(_) => thread::sleep(Duration::from_millis(2)),
                }
            }
        });
        Self {
            port,
            state,
            stop,
            thread: Some(thread),
        }
    }

    fn replies(&self) -> Vec<(String, String)> {
        lock(&self.state).replies.clone()
    }

    fn push_frame(&self, frame: Value) {
        lock(&self.state).frames.push(frame);
    }

    fn add_permission(&self, id: &str, session: &str, permission: &str, patterns: &[&str]) {
        lock(&self.state).permissions.push(json!({
            "id": id,
            "sessionID": session,
            "permission": permission,
            "patterns": patterns,
        }));
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Option<(String, String, Vec<u8>)> {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut raw = Vec::new();
    let mut buffer = [0_u8; 4096];
    let end = loop {
        if let Some(position) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break position;
        }
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 {
            return None;
        }
        raw.extend_from_slice(&buffer[..read]);
    };
    let head = String::from_utf8_lossy(&raw[..end]).into_owned();
    let mut parts = head.lines().next()?.split_whitespace();
    let method = parts.next()?.to_owned();
    let path = parts.next()?.to_owned();
    let headers: HashMap<String, String> = head
        .lines()
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let length = headers
        .get("content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = raw[end + 4..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&buffer[..read]);
    }
    Some((method, path, body))
}

fn respond(stream: &mut TcpStream, status: u16, value: &Value) {
    let body = serde_json::to_vec(value).unwrap();
    let _ = write!(
        stream,
        "HTTP/1.1 {status} Status\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(&body);
}

fn assistant(parts: Value) -> Value {
    json!({
        "info": {"id": "msg_assistant", "role": "assistant", "sessionID": "ses_1",
                 "time": {"created": 1_700_000_000_000_i64}},
        "parts": parts,
    })
}

fn tool_parts() -> Value {
    json!([
        {"type": "tool", "id": "prt_t1", "tool": "bash", "callID": "c1", "state": {
            "status": "completed",
            "input": {"command": format!("curl -H 'X-Api-Key: {SECRET}' https://example.test"),
                      "description": "Fetch the page"},
            "output": format!("raw private output {SECRET}")}},
        {"type": "tool", "id": "prt_t2", "tool": "read", "state": {
            "status": "error",
            "input": {"filePath": "/etc/hosts"},
            "error": format!("denied {SECRET}")}},
        {"type": "tool", "id": "prt_t3", "tool": "glob", "state": {
            "status": "running", "input": {"pattern": "**/*.rs"}}},
        {"type": "text", "text": "All done."},
    ])
}

fn handle(mut stream: TcpStream, state: &Arc<Mutex<State>>, stop: &Arc<AtomicBool>) {
    let Some((method, path, body)) = read_request(&mut stream) else {
        return;
    };
    let path = path.split('?').next().unwrap_or("").to_owned();
    let text = String::from_utf8_lossy(&body).into_owned();
    match (method.as_str(), path.as_str()) {
        ("GET", "/global/health") => respond(&mut stream, 200, &json!({"healthy": true})),
        ("GET", "/session") => respond(&mut stream, 200, &json!([{"id": "ses_1"}])),
        ("POST", "/session") => respond(&mut stream, 200, &json!({"id": "ses_1"})),
        ("GET", "/session/ses_1") => respond(&mut stream, 200, &json!({"id": "ses_1"})),
        ("GET", "/session/ses_1/message") => {
            let log = lock(state).log.clone();
            respond(&mut stream, 200, &Value::Array(log));
        }
        ("POST", "/session/ses_1/message") => turn(&mut stream, state, stop),
        ("GET", "/permission") => {
            let list = lock(state).permissions.clone();
            respond(&mut stream, 200, &Value::Array(list));
        }
        ("POST", route) if route.starts_with("/permission/") && route.ends_with("/reply") => {
            let id = route
                .trim_start_matches("/permission/")
                .trim_end_matches("/reply")
                .to_owned();
            let mut guard = lock(state);
            if guard.legacy_only {
                drop(guard);
                respond(&mut stream, 404, &json!({}));
                return;
            }
            let reply = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v["reply"].as_str().map(str::to_owned))
                .unwrap_or_default();
            answer(&mut guard, &id, &reply);
            drop(guard);
            respond(&mut stream, 200, &json!(true));
        }
        ("POST", route) if route.starts_with("/session/ses_1/permissions/") => {
            let id = route.trim_start_matches("/session/ses_1/permissions/");
            let reply = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v["response"].as_str().map(str::to_owned))
                .unwrap_or_default();
            answer(&mut lock(state), id, &reply);
            respond(&mut stream, 200, &json!(true));
        }
        ("GET", "/event") => events(stream, state, stop),
        _ => respond(&mut stream, 404, &json!({})),
    }
}

fn answer(state: &mut State, id: &str, reply: &str) {
    state.permissions.retain(|p| p["id"] != id);
    state.replies.push((id.to_owned(), reply.to_owned()));
    state.answered = Some(reply.to_owned());
}

fn turn(stream: &mut TcpStream, state: &Arc<Mutex<State>>, stop: &Arc<AtomicBool>) {
    let blocking = {
        let mut guard = lock(state);
        guard.turn_started = true;
        guard.turn_with_permission
    };
    if blocking {
        let deadline = Instant::now() + Duration::from_secs(8);
        while lock(state).answered.is_none() && Instant::now() < deadline {
            if stop.load(Ordering::SeqCst) {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    let reply = lock(state).answered.clone();
    let parts = if reply.as_deref() == Some("reject") {
        json!([{"type": "text", "text": "The request was rejected."}])
    } else if blocking {
        json!([{"type": "text", "text": "Done after approval."}])
    } else {
        tool_parts()
    };
    let message = assistant(parts);
    lock(state).log.push(message.clone());
    respond(stream, 200, &message);
}

fn events(mut stream: TcpStream, state: &Arc<Mutex<State>>, stop: &Arc<AtomicBool>) {
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n";
    if stream.write_all(head.as_bytes()).is_err() {
        return;
    }
    let send = |stream: &mut TcpStream, frame: &Value| {
        let line = format!("data: {frame}\n\n");
        stream
            .write_all(line.as_bytes())
            .and_then(|()| stream.flush())
    };
    if send(
        &mut stream,
        &json!({"type": "server.connected", "properties": {}}),
    )
    .is_err()
    {
        return;
    }
    let deadline = Instant::now() + Duration::from_secs(12);
    while !stop.load(Ordering::SeqCst) && Instant::now() < deadline {
        let frames: Vec<Value> = std::mem::take(&mut lock(state).frames);
        for frame in frames {
            if send(&mut stream, &frame).is_err() {
                return;
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn provider(port: u16) -> LocalChatProvider {
    let workspace = std::env::temp_dir().join(format!("ss81-{port}"));
    LocalChatProvider::new(Arc::new(Engine(port)), workspace)
        .with_stream_support(StreamSupport::Pinned)
}

async fn start(provider: &LocalChatProvider) -> ChatSessionView {
    provider
        .start(&ChatStartInput {
            session: ChatSessionId::Setup,
            project: None,
            location: ChatSessionLocation::Local,
            resume: None,
        })
        .await
        .expect("start")
}

async fn next_view(
    events: &mut Receiver<ChatEvent>,
    mut matches: impl FnMut(&ChatSessionView) -> bool,
) -> ChatSessionView {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let event = tokio::time::timeout(remaining, events.recv())
            .await
            .expect("timed out waiting for a session view")
            .expect("event bus closed");
        if let ChatEvent::Session { session, .. } = event
            && matches(&session)
        {
            return *session;
        }
    }
}

#[tokio::test]
async fn tool_parts_become_compact_redacted_rows_in_the_log_and_the_turn() {
    let fake = Fake::new(|_| {});
    let provider = provider(fake.port);
    start(&provider).await;
    let mut events = provider.subscribe();
    provider.send(&ChatSessionId::Setup, "go").await.unwrap();
    next_view(&mut events, |view| view.state == ChatSessionState::Ready).await;

    let page = provider
        .log(&ChatSessionId::Setup, &ChatLogQuery::default())
        .await
        .unwrap();
    let message = page
        .messages
        .iter()
        .find(|message| message.id == "msg_assistant")
        .expect("assistant message");
    let tools = message.tools.as_ref().expect("tool rows");
    assert_eq!(tools.len(), 3);
    assert_eq!(tools[0].name, "bash");
    assert_eq!(tools[0].status, ChatToolStatus::Completed);
    assert_eq!(tools[1].status, ChatToolStatus::Error);
    assert_eq!(tools[2].status, ChatToolStatus::Running);
    assert_eq!(tools[2].summary.as_deref(), Some("**/*.rs"));
    assert_eq!(message.joined_text(), "All done.");

    let wire = serde_json::to_string(&page).unwrap();
    assert!(!wire.contains(SECRET), "a secret reached the wire: {wire}");
    assert!(!wire.contains("raw private output"));
    assert!(!wire.contains("denied"));
    assert!(wire.contains("\"status\":\"completed\""));
}

#[tokio::test]
async fn a_tool_part_event_refreshes_the_rows_while_the_turn_runs() {
    let fake = Fake::new(|state| state.turn_with_permission = true);
    let provider = provider(fake.port);
    start(&provider).await;
    let mut events = provider.subscribe();
    provider.send(&ChatSessionId::Setup, "go").await.unwrap();
    lock(&fake.state).log.push(assistant(json!([
        {"type": "tool", "id": "prt_t3", "tool": "glob",
         "state": {"status": "running", "input": {"pattern": "src/**"}}}
    ])));
    fake.push_frame(
        json!({"type": "message.part.updated", "properties": {"part": {
        "id": "prt_t3", "messageID": "msg_assistant", "sessionID": "ses_1", "type": "tool"}}}),
    );
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let event = tokio::time::timeout(
            deadline.saturating_duration_since(Instant::now()),
            events.recv(),
        )
        .await
        .expect("no tool row arrived")
        .unwrap();
        if let ChatEvent::Message { message, .. } = event
            && let Some(tools) = message.tools
        {
            assert_eq!(tools[0].name, "glob");
            assert_eq!(tools[0].status, ChatToolStatus::Running);
            break;
        }
    }
    // Unblock the fake turn so it can shut down cleanly.
    lock(&fake.state).answered = Some("once".to_owned());
}

#[tokio::test]
async fn a_pending_permission_is_surfaced_and_allow_once_resolves_the_turn() {
    let fake = Fake::new(|state| state.turn_with_permission = true);
    let provider = provider(fake.port);
    start(&provider).await;
    let mut events = provider.subscribe();
    provider.send(&ChatSessionId::Setup, "go").await.unwrap();
    // The engine also holds a request for another session; it must never show up here.
    fake.add_permission("per_other", "ses_other", "bash", &["rm -rf /"]);
    fake.add_permission(
        "per_1",
        "ses_1",
        "external_directory",
        &["/srv/data/*", &format!("token={SECRET}")],
    );
    fake.push_frame(json!({"type": "permission.asked", "properties": {
        "id": "per_1", "sessionID": "ses_1", "permission": "external_directory",
        "patterns": ["/srv/data/*", format!("token={SECRET}")]}}));

    let view = next_view(&mut events, |view| view.pending_permissions.is_some()).await;
    assert_eq!(
        view.state,
        ChatSessionState::Streaming,
        "the turn must wait"
    );
    let pending = view.pending_permissions.clone().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "per_1");
    assert_eq!(pending[0].permission, "external_directory");
    assert_eq!(pending[0].patterns[0], "/srv/data/*");
    let wire = serde_json::to_string(&view).unwrap();
    assert!(!wire.contains(SECRET), "a secret reached the wire");
    assert!(!wire.contains("per_other"));

    let after = provider
        .reply_permission(&ChatSessionId::Setup, "per_1", ChatPermissionDecision::Once)
        .await
        .unwrap();
    assert!(after.pending_permissions.is_none());
    assert_eq!(
        fake.replies(),
        vec![("per_1".to_owned(), "once".to_owned())]
    );

    let done = next_view(&mut events, |view| view.state == ChatSessionState::Ready).await;
    assert!(done.pending_permissions.is_none());
    let page = provider
        .log(&ChatSessionId::Setup, &ChatLogQuery::default())
        .await
        .unwrap();
    assert!(
        page.messages
            .iter()
            .any(|message| message.joined_text() == "Done after approval.")
    );
}

#[tokio::test]
async fn get_permission_discovers_a_request_the_stream_missed_and_reject_replies() {
    let fake = Fake::new(|state| state.turn_with_permission = true);
    let provider = provider(fake.port);
    start(&provider).await;
    provider.send(&ChatSessionId::Setup, "go").await.unwrap();
    fake.add_permission("per_9", "ses_1", "bash", &["git push *"]);

    // No SSE frame was sent; the reply path reconciles with `GET /permission`.
    let view = provider
        .reply_permission(
            &ChatSessionId::Setup,
            "per_9",
            ChatPermissionDecision::Reject,
        )
        .await
        .unwrap();
    assert!(view.pending_permissions.is_none());
    assert_eq!(
        fake.replies(),
        vec![("per_9".to_owned(), "reject".to_owned())]
    );

    let mut events = provider.subscribe();
    let done = next_view(&mut events, |view| view.state == ChatSessionState::Ready).await;
    assert!(done.pending_permissions.is_none());
}

#[tokio::test]
async fn an_older_engine_is_answered_on_the_session_permission_route() {
    let fake = Fake::new(|state| {
        state.turn_with_permission = true;
        state.legacy_only = true;
    });
    let provider = provider(fake.port);
    start(&provider).await;
    provider.send(&ChatSessionId::Setup, "go").await.unwrap();
    fake.add_permission("per_2", "ses_1", "edit", &["src/main.rs"]);
    provider
        .reply_permission(
            &ChatSessionId::Setup,
            "per_2",
            ChatPermissionDecision::Always,
        )
        .await
        .unwrap();
    assert_eq!(
        fake.replies(),
        vec![("per_2".to_owned(), "always".to_owned())]
    );
}

#[tokio::test]
async fn a_reply_for_a_request_that_is_already_gone_is_a_harmless_refresh() {
    let fake = Fake::new(|_| {});
    let provider = provider(fake.port);
    start(&provider).await;
    let view = provider
        .reply_permission(
            &ChatSessionId::Setup,
            "per_gone",
            ChatPermissionDecision::Once,
        )
        .await
        .unwrap();
    assert!(view.pending_permissions.is_none());
    assert!(
        fake.replies().is_empty(),
        "nothing may be sent for an unknown request"
    );
}
