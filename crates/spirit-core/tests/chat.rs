//! Hermetic tests for the bundled-engine chat transport.
//!
//! The fake server speaks the v1 HTTP/SSE surface over loopback. No real model, no
//! provider key, no sidecar and no owner state is touched.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde_json::{Value, json};

use spirit_core::engine::chat::{
    ApplyOutcome, ChatEndpointResolver, ChatEngine, ChatEvent, ChatEventApplier, ChatEventBus,
    ChatLogQuery, ChatMessage, ChatSessionId, ChatSessionLocation, ChatSessionState,
    ChatStartInput, LocalChatProvider, ResolvedEndpoint, StreamSupport,
};
use spirit_core::engine::{
    EngineChoice, EngineRuntime, EngineStatus, ExternalHarnessKind, PrivateEngineTree,
    ProcessEngineRuntime,
};
use spirit_core::error::{SpiritError, SpiritErrorCode};
use spirit_core::time::Timestamp;

const SETUP: ChatSessionId = ChatSessionId::Setup;

fn setup_input() -> ChatStartInput {
    ChatStartInput {
        session: SETUP,
        project: None,
        location: ChatSessionLocation::Local,
        resume: None,
    }
}

fn model() -> spirit_core::engine::chat::ChatModelView {
    spirit_core::engine::chat::ChatModelView {
        id: "opencode/big-pickle".to_owned(),
        label: "Big Pickle".to_owned(),
        provider: "opencode".to_owned(),
        tier: spirit_core::engine::chat::ChatModelTier::Included,
        context_tokens: Some(128_000),
        variants: None,
    }
}

struct FakeChatEngine {
    port: u16,
    ready_calls: AtomicUsize,
    spawns: AtomicUsize,
    external: bool,
}

impl FakeChatEngine {
    fn new(port: u16) -> Self {
        Self {
            port,
            ready_calls: AtomicUsize::new(0),
            spawns: AtomicUsize::new(1),
            external: false,
        }
    }

    fn external(port: u16) -> Self {
        Self {
            external: true,
            ..Self::new(port)
        }
    }

    fn ready_calls(&self) -> usize {
        self.ready_calls.load(Ordering::SeqCst)
    }

    fn spawns(&self) -> usize {
        self.spawns.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ChatEngine for FakeChatEngine {
    async fn ensure_ready(&self) -> Result<u16, SpiritError> {
        self.ready_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.port)
    }

    fn selected_model(&self) -> Option<spirit_core::engine::chat::ChatModelView> {
        Some(model())
    }

    fn status(&self) -> EngineStatus {
        if self.external {
            EngineStatus {
                choice: EngineChoice::External {
                    path: PathBuf::from("/decoy/opencode"),
                    kind: ExternalHarnessKind::Opencode,
                },
                ..EngineStatus::default()
            }
        } else {
            EngineStatus::default()
        }
    }
}

fn build_provider(
    server: &FakeEngine,
    workspace: &Path,
    pinned: bool,
) -> (LocalChatProvider, Arc<FakeChatEngine>) {
    let engine = Arc::new(FakeChatEngine::new(server.port()));
    let provider = if pinned {
        LocalChatProvider::new(engine.clone(), workspace.to_path_buf())
            .with_stream_support(StreamSupport::Pinned)
    } else {
        LocalChatProvider::new(engine.clone(), workspace.to_path_buf())
    };
    (provider, engine)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SseScript {
    None,
    Deltas,
    Reconnect,
    Html,
}

#[derive(Debug, Clone)]
struct RequestRecord {
    method: String,
    path: String,
    directory: Option<String>,
    body: String,
}

struct FakeState {
    health_status: u16,
    sessions: HashMap<String, Vec<Value>>,
    order: Vec<String>,
    next_session: usize,
    message_error: Option<u16>,
    message_delay: Duration,
    abort_status: u16,
    abort_count: usize,
    event_connections: usize,
    message_started: bool,
    missing_session: bool,
    sse: SseScript,
    assistant_text: String,
    requests: Vec<RequestRecord>,
}

impl Default for FakeState {
    fn default() -> Self {
        Self {
            health_status: 200,
            sessions: HashMap::new(),
            order: Vec::new(),
            next_session: 1,
            message_error: None,
            message_delay: Duration::from_millis(40),
            abort_status: 200,
            abort_count: 0,
            event_connections: 0,
            message_started: false,
            missing_session: false,
            sse: SseScript::None,
            assistant_text: "Hello world".to_owned(),
            requests: Vec::new(),
        }
    }
}

struct FakeEngine {
    port: u16,
    state: Arc<Mutex<FakeState>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl FakeEngine {
    fn new() -> Self {
        Self::with(|_| {})
    }

    fn with(configure: impl FnOnce(&mut FakeState)) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind fake engine");
        listener
            .set_nonblocking(true)
            .expect("set fake listener nonblocking");
        let port = listener.local_addr().expect("fake local addr").port();
        let mut state = FakeState::default();
        configure(&mut state);
        let state = Arc::new(Mutex::new(state));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_state = Arc::clone(&state);
        let thread_stop = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            serve(listener, thread_state, thread_stop);
        });
        Self {
            port,
            state,
            stop,
            thread: Some(handle),
        }
    }

    fn sse(sse: SseScript) -> Self {
        Self::with(|state| state.sse = sse)
    }

    fn port(&self) -> u16 {
        self.port
    }

    fn created_sessions(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .next_session
            .saturating_sub(1)
    }

    fn event_connections(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .event_connections
    }

    fn abort_count(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .abort_count
    }

    fn message_started(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .message_started
    }

    fn requests(&self) -> Vec<RequestRecord> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .requests
            .clone()
    }

    fn set_missing_session(&self, missing: bool) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .missing_session = missing;
    }
}

impl Drop for FakeEngine {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn serve(listener: TcpListener, state: Arc<Mutex<FakeState>>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let state = Arc::clone(&state);
                let stop = Arc::clone(&stop);
                thread::spawn(move || {
                    handle_connection(stream, state, stop);
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2));
            }
            Err(_) => return,
        }
    }
}

struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut raw = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        if let Some(position) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
            break position;
        }
        if raw.len() > 64 * 1024 {
            return None;
        }
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 {
            return None;
        }
        raw.extend_from_slice(&buffer[..read]);
    };
    let head = String::from_utf8_lossy(&raw[..header_end]);
    let mut lines = head.lines();
    let request_line = lines.next()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_owned();
    let path = parts.next()?.to_owned();
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    let length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = raw[header_end + 4..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&buffer[..read]);
    }
    Some(Request {
        method,
        path,
        headers,
        body,
    })
}

fn handle_connection(mut stream: TcpStream, state: Arc<Mutex<FakeState>>, stop: Arc<AtomicBool>) {
    let Some(request) = read_request(&mut stream) else {
        return;
    };
    let directory = request.headers.get("x-opencode-directory").cloned();
    let body = String::from_utf8_lossy(&request.body).into_owned();
    {
        let mut state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.requests.push(RequestRecord {
            method: request.method.clone(),
            path: request.path.clone(),
            directory,
            body: body.clone(),
        });
    }
    let path = request.path.split('?').next().unwrap_or(&request.path);
    match (request.method.as_str(), path) {
        ("GET", "/global/health") => {
            let status = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .health_status;
            if status == 200 {
                write_json(
                    &mut stream,
                    200,
                    &json!({"healthy": true, "version": "1.19.5"}),
                );
            } else {
                write_json(&mut stream, status, &json!({"healthy": false}));
            }
        }
        ("GET", "/session") => {
            let state = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let sessions: Vec<Value> = state
                .order
                .iter()
                .map(|id| json!({"id": id, "title": "StackSpirit setup"}))
                .collect();
            drop(state);
            write_json(&mut stream, 200, &Value::Array(sessions));
        }
        ("POST", "/session") => {
            let id = {
                let mut state = state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let id = format!("ses_fake_{}", state.next_session);
                state.next_session = state.next_session.saturating_add(1);
                state.sessions.insert(id.clone(), Vec::new());
                state.order.push(id.clone());
                id
            };
            write_json(
                &mut stream,
                200,
                &json!({
                    "id": id,
                    "slug": "fake",
                    "projectID": "fake",
                    "directory": "/private/owner/opencode/sessions",
                    "title": "StackSpirit setup",
                    "version": "1.19.5",
                }),
            );
        }
        ("GET", "/event") => handle_event(stream, state, stop),
        _ => handle_session_route(stream, state, stop, &request.method, path, &body),
    }
}

fn handle_session_route(
    mut stream: TcpStream,
    state: Arc<Mutex<FakeState>>,
    _stop: Arc<AtomicBool>,
    method: &str,
    path: &str,
    body: &str,
) {
    let Some(rest) = path.strip_prefix("/session/") else {
        write_json(&mut stream, 404, &json!({}));
        return;
    };
    if let Some(_id) = rest.strip_suffix("/abort") {
        if method != "POST" {
            write_json(&mut stream, 405, &json!({}));
            return;
        }
        let status = {
            let mut state = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.abort_count = state.abort_count.saturating_add(1);
            state.abort_status
        };
        if (200..300).contains(&status) {
            write_json(&mut stream, status, &json!(true));
        } else {
            write_json(&mut stream, status, &json!({}));
        }
        return;
    }
    if let Some(id) = rest.strip_suffix("/message") {
        if method == "POST" {
            handle_message_post(&mut stream, &state, id, body);
        } else {
            let messages = {
                let state = state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                state.sessions.get(id).cloned()
            };
            match messages {
                Some(messages) => write_json(&mut stream, 200, &Value::Array(messages)),
                None => write_json(&mut stream, 404, &json!({})),
            }
        }
        return;
    }
    if method != "GET" {
        write_json(&mut stream, 405, &json!({}));
        return;
    }
    let exists = {
        let state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        !state.missing_session && state.sessions.contains_key(rest)
    };
    if exists {
        write_json(
            &mut stream,
            200,
            &json!({
                "id": rest,
                "directory": "/private/owner/opencode/sessions",
                "title": "StackSpirit setup",
            }),
        );
    } else {
        write_json(&mut stream, 404, &json!({}));
    }
}

fn handle_message_post(
    stream: &mut TcpStream,
    state: &Arc<Mutex<FakeState>>,
    id: &str,
    body: &str,
) {
    let request: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let Some(user_id) = request.get("messageID").and_then(Value::as_str) else {
        write_json(
            stream,
            400,
            &json!({"name": "BadRequest", "data": {
                "message": "Expected a string starting with \"msg\", got nothing at [\"messageID\"]",
                "kind": "Payload",
            }}),
        );
        return;
    };
    if !user_id.starts_with("msg") {
        write_json(
            stream,
            400,
            &json!({"name": "BadRequest", "data": {
                "message": format!("Expected a string starting with \"msg\", got \"{user_id}\" at [\"messageID\"]"),
                "kind": "Payload",
            }}),
        );
        return;
    }
    let user_id = user_id.to_owned();
    let text = request
        .get("parts")
        .and_then(Value::as_array)
        .and_then(|parts| {
            parts
                .iter()
                .find(|part| part.get("type").and_then(Value::as_str) == Some("text"))
        })
        .and_then(|part| part.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let (error, delay, assistant_text) = {
        let mut state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.sessions.contains_key(id) {
            drop(state);
            write_json(stream, 404, &json!({}));
            return;
        }
        if let Some(log) = state.sessions.get_mut(id) {
            log.push(json!({
                "info": {
                    "id": user_id,
                    "role": "user",
                    "sessionID": id,
                    "time": {"created": now_ms()},
                },
                "parts": [{"type": "text", "text": text}],
            }));
        }
        state.message_started = true;
        (
            state.message_error,
            state.message_delay,
            state.assistant_text.clone(),
        )
    };
    let started = Instant::now();
    while started.elapsed() < delay {
        thread::sleep(Duration::from_millis(5));
    }
    if let Some(status) = error {
        write_json(
            stream,
            200,
            &json!({
                "info": {
                    "id": "msg_assistant",
                    "role": "assistant",
                    "sessionID": id,
                    "error": {"statusCode": status, "message": "Invalid API key"},
                }
            }),
        );
        return;
    }
    {
        let mut state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(log) = state.sessions.get_mut(id) {
            log.push(json!({
                "info": {
                    "id": "msg_assistant",
                    "role": "assistant",
                    "sessionID": id,
                    "time": {"created": now_ms()},
                },
                "parts": [{"type": "text", "text": assistant_text.clone()}],
            }));
        }
    }
    write_json(
        stream,
        200,
        &json!({
            "info": {
                "id": "msg_assistant",
                "role": "assistant",
                "sessionID": id,
                "time": {"created": now_ms()},
            },
            "parts": [{"type": "text", "text": assistant_text}],
        }),
    );
}

fn handle_event(mut stream: TcpStream, state: Arc<Mutex<FakeState>>, stop: Arc<AtomicBool>) {
    let (sse, connection, native) = {
        let mut state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.event_connections = state.event_connections.saturating_add(1);
        (
            state.sse,
            state.event_connections,
            state.order.first().cloned(),
        )
    };
    if sse == SseScript::None {
        write_json(&mut stream, 404, &json!({}));
        return;
    }
    if sse == SseScript::Html {
        let body = "<!doctype html><title>app</title>";
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(reply.as_bytes());
        return;
    }
    let Some(native) = native else {
        write_json(&mut stream, 404, &json!({}));
        return;
    };
    let headers = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n";
    if stream.write_all(headers.as_bytes()).is_err() {
        return;
    }
    if write_sse(
        &mut stream,
        &json!({"type": "server.connected", "properties": {}}),
    )
    .is_err()
    {
        return;
    }
    // Wait for the turn to be accepted so the frames follow the user message.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !stop.load(Ordering::SeqCst) && Instant::now() < deadline {
        let started = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .message_started;
        if started {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let result = match sse {
        SseScript::Deltas => send_deltas(&mut stream, &native),
        SseScript::Reconnect if connection == 1 => send_partial(&mut stream, &native),
        SseScript::Reconnect => {
            thread::sleep(Duration::from_millis(300));
            send_recovered(&mut stream, &native)
        }
        SseScript::None | SseScript::Html => Ok(()),
    };
    if result.is_err() {
        return;
    }
    if sse == SseScript::Reconnect && connection == 1 {
        // Close the first stream so the client reconnects and backfills.
        return;
    }
    let hold = Instant::now() + Duration::from_secs(2);
    while !stop.load(Ordering::SeqCst) && Instant::now() < hold {
        thread::sleep(Duration::from_millis(50));
    }
}

fn send_deltas(stream: &mut TcpStream, native: &str) -> std::io::Result<()> {
    write_sse(
        stream,
        &json!({
            "type": "message.updated",
            "properties": {"info": {
                "id": "msg_assistant",
                "role": "assistant",
                "sessionID": native,
                "time": {"created": now_ms()},
            }},
        }),
    )?;
    for text in ["Hel", "Hello", "Hello world"] {
        write_sse(stream, &part_frame(native, text))?;
    }
    write_sse(
        stream,
        &json!({"type": "session.idle", "properties": {"sessionID": native}}),
    )
}

fn send_partial(stream: &mut TcpStream, native: &str) -> std::io::Result<()> {
    write_sse(
        stream,
        &json!({
            "type": "message.updated",
            "properties": {"info": {
                "id": "msg_assistant",
                "role": "assistant",
                "sessionID": native,
                "time": {"created": now_ms()},
            }},
        }),
    )?;
    write_sse(stream, &part_frame(native, "Hel"))
}

fn send_recovered(stream: &mut TcpStream, native: &str) -> std::io::Result<()> {
    write_sse(stream, &part_frame(native, "Hello world"))?;
    write_sse(
        stream,
        &json!({"type": "session.idle", "properties": {"sessionID": native}}),
    )
}

fn part_frame(native: &str, text: &str) -> Value {
    json!({
        "type": "message.part.updated",
        "properties": {"part": {
            "id": "prt_1",
            "messageID": "msg_assistant",
            "sessionID": native,
            "type": "text",
            "text": text,
        }},
    })
}

fn write_sse(stream: &mut TcpStream, value: &Value) -> std::io::Result<()> {
    let frame = format!(
        "data: {}\n\n",
        serde_json::to_string(value).unwrap_or_default()
    );
    stream.write_all(frame.as_bytes())?;
    stream.flush()
}

fn write_json(stream: &mut TcpStream, status: u16, value: &Value) {
    let body = serde_json::to_vec(value).unwrap_or_default();
    let _ = write!(
        stream,
        "HTTP/1.1 {status} Status\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(&body);
    let _ = stream.flush();
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as i64)
}

async fn wait_until(mut condition: impl FnMut() -> bool, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("condition was not met within {timeout:?}");
}

async fn collect_until_done(
    receiver: &mut tokio::sync::broadcast::Receiver<ChatEvent>,
    timeout: Duration,
) -> Vec<ChatEvent> {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut events = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(
            !remaining.is_zero(),
            "timed out; events so far: {events:#?}"
        );
        match tokio::time::timeout(remaining, receiver.recv()).await {
            Ok(Ok(event)) => {
                let done = matches!(event, ChatEvent::Done { .. });
                events.push(event);
                if done {
                    return events;
                }
            }
            Ok(Err(error)) => panic!("chat event bus closed: {error}"),
            Err(_) => panic!("timed out; events so far: {events:#?}"),
        }
    }
}

async fn collect_until_error(
    receiver: &mut tokio::sync::broadcast::Receiver<ChatEvent>,
    timeout: Duration,
) -> Vec<ChatEvent> {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut events = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(
            !remaining.is_zero(),
            "timed out; events so far: {events:#?}"
        );
        match tokio::time::timeout(remaining, receiver.recv()).await {
            Ok(Ok(event)) => {
                let failed = matches!(event, ChatEvent::Error { .. });
                events.push(event);
                if failed {
                    return events;
                }
            }
            Ok(Err(error)) => panic!("chat event bus closed: {error}"),
            Err(_) => panic!("timed out; events so far: {events:#?}"),
        }
    }
}

fn project(events: &[ChatEvent]) -> ChatEventApplier {
    let mut applier = ChatEventApplier::new();
    for event in events {
        let _ = applier.apply(event, Timestamp::now());
    }
    applier
}

fn assert_no_transport_leak(value: &Value, port: u16, workspace: &Path) {
    let text = serde_json::to_string(value).unwrap_or_default();
    assert!(!text.contains(&format!("127.0.0.1:{port}")), "{text}");
    assert!(!text.contains(&format!(":{port}")), "{text}");
    assert!(!text.contains("127.0.0.1"), "{text}");
    assert!(!text.contains("http://"), "{text}");
    assert!(!text.contains("x-opencode-directory"), "{text}");
    assert!(!text.contains("native_session_id"), "{text}");
    assert!(!text.contains("binary_path"), "{text}");
    assert!(!text.contains("identity_file"), "{text}");
    assert!(!text.contains("/private/owner/opencode"), "{text}");
    let workspace = workspace.to_string_lossy();
    assert!(!text.contains(workspace.as_ref()), "{text}");
}

#[test]
fn revision_gap_requests_a_log_reload_and_backfill_has_no_duplicates() {
    let bus = ChatEventBus::new(16);
    let mut applier = ChatEventApplier::new();
    let now = Timestamp::epoch();
    let first = bus.message(SETUP, ChatMessage::user("u1", "hi", now));
    assert_eq!(applier.apply(&first, now), ApplyOutcome::Applied);
    // These revisions are never applied, simulating a dropped stream.
    let _skipped_one = bus.message(SETUP, ChatMessage::user("u2", "skip", now));
    let _skipped_two = bus.message(SETUP, ChatMessage::user("u3", "skip", now));
    let gap = bus.delta(SETUP, "u1", None, "!");
    assert_eq!(applier.apply(&gap, now), ApplyOutcome::Backfill);
    applier.replace_log(vec![ChatMessage::user("u1", "hi!", now)]);
    let done = bus.done(SETUP, "u1", false);
    assert_eq!(applier.apply(&done, now), ApplyOutcome::Applied);
    assert_eq!(applier.messages().len(), 1);
    assert_eq!(applier.messages()[0].joined_text(), "hi!");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_turn_streams_deltas_and_done_without_duplicates() {
    let server = FakeEngine::sse(SseScript::Deltas);
    let workspace = tempfile::tempdir().unwrap();
    let (provider, engine) = build_provider(&server, workspace.path(), true);
    let mut receiver = provider.subscribe();
    let view = provider.start(&setup_input()).await.unwrap();
    assert_eq!(view.session, SETUP);
    assert_eq!(view.state, ChatSessionState::Ready);
    assert!(view.attached);
    assert!(view.streaming_supported);
    assert!(view.error.is_none());
    assert_eq!(view.location, ChatSessionLocation::Local);

    wait_until(|| server.event_connections() >= 1, Duration::from_secs(2)).await;
    let receipt = provider.send(&SETUP, "Hello?").await.unwrap();
    assert!(receipt.accepted);
    assert!(receipt.message.id.starts_with("msg_"));
    assert_eq!(receipt.message.text, vec!["Hello?"]);

    let events = collect_until_done(&mut receiver, Duration::from_secs(5)).await;
    let applier = project(&events);
    assert_eq!(
        applier.messages().len(),
        2,
        "messages={:#?} events={events:#?}",
        applier.messages()
    );
    assert_eq!(applier.messages()[0].text, vec!["Hello?"]);
    assert_eq!(applier.messages()[1].id, "msg_assistant");
    assert_eq!(applier.messages()[1].joined_text(), "Hello world");
    assert!(!applier.messages()[1].streaming);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ChatEvent::Delta { .. }))
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ChatEvent::Done { .. }))
            .count(),
        1
    );

    // A second start reuses the same native session and never spawns a second engine.
    let again = provider.start(&setup_input()).await.unwrap();
    assert_eq!(again.state, ChatSessionState::Ready);
    assert_eq!(server.created_sessions(), 1);
    assert_eq!(engine.spawns(), 1);
    assert_eq!(engine.ready_calls(), 3);

    let requests = server.requests();
    let create = requests
        .iter()
        .find(|request| request.method == "POST" && request.path == "/session")
        .expect("session create request");
    assert_eq!(
        create.directory.as_deref(),
        workspace.path().to_str(),
        "the private workspace header must select the setup workspace"
    );
    let message = requests
        .iter()
        .find(|request| request.method == "POST" && request.path.ends_with("/message"))
        .expect("message request");
    assert_eq!(message.directory.as_deref(), workspace.path().to_str());
    let body: Value = serde_json::from_str(&message.body).unwrap();
    assert_eq!(body["messageID"], receipt.message.id);
    assert_eq!(body["model"]["providerID"], "opencode");
    assert_eq!(body["model"]["modelID"], "big-pickle");
    assert_eq!(body["parts"][0]["text"], "Hello?");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_fake_engine_rejects_message_ids_without_the_msg_prefix() {
    let server = FakeEngine::new();
    let body = json!({
        "messageID": "local:01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "parts": [{"type": "text", "text": "Hello?"}],
    })
    .to_string();
    let mut stream = TcpStream::connect(("127.0.0.1", server.port())).expect("connect fake engine");
    write!(
        stream,
        "POST /session/ses_fake_1/message HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .expect("write fake message request");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("read fake message response");
    assert!(response.starts_with("HTTP/1.1 400"), "{response}");
    assert!(response.contains("BadRequest"), "{response}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnect_backfills_without_duplicating_the_assistant() {
    let server = FakeEngine::with(|state| {
        state.sse = SseScript::Reconnect;
        state.message_delay = Duration::from_millis(900);
    });
    let workspace = tempfile::tempdir().unwrap();
    let (provider, _engine) = build_provider(&server, workspace.path(), true);
    let mut receiver = provider.subscribe();
    provider.start(&setup_input()).await.unwrap();
    wait_until(|| server.event_connections() >= 1, Duration::from_secs(2)).await;
    provider.send(&SETUP, "Hello?").await.unwrap();

    let events = collect_until_done(&mut receiver, Duration::from_secs(6)).await;
    let applier = project(&events);
    let assistants: Vec<_> = applier
        .messages()
        .iter()
        .filter(|message| message.role == spirit_core::engine::chat::ChatRole::Assistant)
        .collect();
    assert_eq!(assistants.len(), 1, "{events:#?}");
    assert_eq!(assistants[0].id, "msg_assistant");
    assert_eq!(assistants[0].joined_text(), "Hello world");
    assert_eq!(applier.messages().len(), 2, "{events:#?}");
    assert!(
        server.event_connections() >= 2,
        "the pump must reconnect after the dropped stream"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn abort_stops_the_turn_and_late_text_is_ignored() {
    let server = FakeEngine::with(|state| {
        state.message_delay = Duration::from_millis(800);
    });
    let workspace = tempfile::tempdir().unwrap();
    let (provider, _engine) = build_provider(&server, workspace.path(), false);
    let mut receiver = provider.subscribe();
    provider.start(&setup_input()).await.unwrap();
    provider.send(&SETUP, "Start a long turn").await.unwrap();
    wait_until(|| server.message_started(), Duration::from_secs(2)).await;

    // An idempotent start must not reset a turn that is already in flight.
    let in_flight = provider.start(&setup_input()).await.unwrap();
    assert_eq!(in_flight.state, ChatSessionState::Streaming);

    let stopped = provider.stop(&SETUP).await.unwrap();
    assert_eq!(stopped.state, ChatSessionState::Stopped);
    assert!(stopped.error.is_none());
    assert_eq!(server.abort_count(), 1);

    // Drain the stop events, then prove the late POST response adds no text.
    let closed = tokio::time::timeout(Duration::from_secs(2), async {
        while let Ok(event) = receiver.recv().await {
            if matches!(event, ChatEvent::Closed { .. }) {
                return event;
            }
        }
        panic!("event bus closed before the closed event");
    })
    .await
    .expect("closed event");
    assert!(matches!(
        closed,
        ChatEvent::Closed {
            reason: spirit_core::engine::chat::ChatCloseReason::Stopped,
            ..
        }
    ));
    tokio::time::sleep(Duration::from_millis(1100)).await;
    while let Ok(event) = receiver.try_recv() {
        assert!(
            !matches!(
                event,
                ChatEvent::Message { .. } | ChatEvent::Done { .. } | ChatEvent::Delta { .. }
            ),
            "a stopped turn must not emit late text: {event:#?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn abort_unsupported_reports_stop_unconfirmed() {
    let server = FakeEngine::with(|state| {
        state.message_delay = Duration::from_millis(600);
        state.abort_status = 404;
    });
    let workspace = tempfile::tempdir().unwrap();
    let (provider, _engine) = build_provider(&server, workspace.path(), false);
    provider.start(&setup_input()).await.unwrap();
    provider.send(&SETUP, "Start a long turn").await.unwrap();
    wait_until(|| server.message_started(), Duration::from_secs(2)).await;
    let stopped = provider.stop(&SETUP).await.unwrap();
    assert_eq!(stopped.state, ChatSessionState::Stopped);
    let error = stopped.error.expect("an unconfirmed stop is a notice");
    assert_eq!(error.code, SpiritErrorCode::Conflict);
    assert!(error.message.contains("stop_unconfirmed"));
    assert!(!error.message.contains("127.0.0.1"));
    assert_eq!(server.abort_count(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn provider_error_in_http_200_is_a_safe_auth_error() {
    let server = FakeEngine::with(|state| {
        state.message_error = Some(401);
    });
    let workspace = tempfile::tempdir().unwrap();
    let (provider, _engine) = build_provider(&server, workspace.path(), false);
    let mut receiver = provider.subscribe();
    provider.start(&setup_input()).await.unwrap();
    provider.send(&SETUP, "Hello?").await.unwrap();
    let events = collect_until_error(&mut receiver, Duration::from_secs(3)).await;
    let ChatEvent::Error { error, .. } = events
        .iter()
        .find(|event| matches!(event, ChatEvent::Error { .. }))
        .unwrap()
    else {
        panic!("expected an error event");
    };
    assert_eq!(error.code, SpiritErrorCode::Auth);
    assert!(!error.message.contains("Invalid API key"));
    assert_no_transport_leak(
        &serde_json::to_value(error).unwrap(),
        server.port(),
        workspace.path(),
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn engine_reuse_uses_one_session_and_one_engine() {
    let server = FakeEngine::with(|state| {
        state.message_delay = Duration::from_millis(20);
    });
    let workspace = tempfile::tempdir().unwrap();
    let (provider, engine) = build_provider(&server, workspace.path(), false);
    provider.start(&setup_input()).await.unwrap();
    provider.start(&setup_input()).await.unwrap();
    let attached = provider.attach(&SETUP).await.unwrap();
    assert_eq!(attached.state, ChatSessionState::Ready);
    provider.send(&SETUP, "Hello?").await.unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(server.created_sessions(), 1);
    assert_eq!(engine.spawns(), 1);
    assert_eq!(server.event_connections(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_failure_and_external_engine_are_typed() {
    let server = FakeEngine::with(|state| state.health_status = 500);
    let workspace = tempfile::tempdir().unwrap();
    let (provider, _engine) = build_provider(&server, workspace.path(), false);
    let error = provider.start(&setup_input()).await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Engine);
    assert!(error.message.contains("engine_not_ready"));
    assert!(!error.message.contains("127.0.0.1"));
    assert!(!error.message.contains(&server.port().to_string()));
    assert!(
        !error
            .message
            .contains(&workspace.path().to_string_lossy().to_string())
    );

    let server = FakeEngine::new();
    let engine = Arc::new(FakeChatEngine::external(server.port()));
    let provider = LocalChatProvider::new(engine, workspace.path().to_path_buf());
    let error = provider.start(&setup_input()).await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Engine);
    assert!(error.message.contains("engine_external"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_session_and_missing_start_are_typed() {
    let server = FakeEngine::new();
    let workspace = tempfile::tempdir().unwrap();
    let (provider, _engine) = build_provider(&server, workspace.path(), false);
    let error = provider.attach(&SETUP).await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Conflict);
    assert!(error.message.contains("chat_not_started"));

    provider.start(&setup_input()).await.unwrap();
    server.set_missing_session(true);
    let error = provider.attach(&SETUP).await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::NotFound);
    assert!(error.message.contains("session_lost"));
    assert!(!error.message.contains("127.0.0.1"));

    // A stale mapping must not silently create a second setup session.
    let server = FakeEngine::new();
    let (provider, _engine) = build_provider(&server, workspace.path(), false);
    provider.start(&setup_input()).await.unwrap();
    server.set_missing_session(true);
    let error = provider.start(&setup_input()).await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::NotFound);
    assert!(error.message.contains("session_lost"));
    assert_eq!(server.created_sessions(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serialized_views_events_logs_and_errors_never_carry_transport_data() {
    let server = FakeEngine::with(|state| {
        state.message_delay = Duration::from_millis(20);
    });
    let workspace = tempfile::tempdir().unwrap();
    let (provider, _engine) = build_provider(&server, workspace.path(), false);
    let mut receiver = provider.subscribe();
    let view = provider.start(&setup_input()).await.unwrap();
    assert_no_transport_leak(
        &serde_json::to_value(&view).unwrap(),
        server.port(),
        workspace.path(),
    );
    let receipt = provider.send(&SETUP, "Hello?").await.unwrap();
    assert_no_transport_leak(
        &serde_json::to_value(&receipt).unwrap(),
        server.port(),
        workspace.path(),
    );
    let events = collect_until_done(&mut receiver, Duration::from_secs(3)).await;
    for event in &events {
        assert_no_transport_leak(
            &serde_json::to_value(event).unwrap(),
            server.port(),
            workspace.path(),
        );
    }
    let page = provider
        .log(
            &SETUP,
            &ChatLogQuery {
                cursor: None,
                limit: Some(50),
            },
        )
        .await
        .unwrap();
    assert_eq!(page.messages.len(), 2);
    assert_no_transport_leak(
        &serde_json::to_value(&page).unwrap(),
        server.port(),
        workspace.path(),
    );
    let sessions = provider.native_sessions().await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert!(sessions[0].id.starts_with("ses_"));
    assert_eq!(sessions[0].title, "StackSpirit setup");
    let requests = server.requests();
    let log_request = requests
        .iter()
        .find(|request| request.method == "GET" && request.path.ends_with("/message"))
        .expect("log request");
    assert_eq!(
        log_request.directory.as_deref(),
        workspace.path().to_str(),
        "the log request must carry the private workspace header"
    );
    let list_request = requests
        .iter()
        .find(|request| request.method == "GET" && request.path == "/session")
        .expect("session list request");
    assert_eq!(
        list_request.directory.as_deref(),
        workspace.path().to_str(),
        "the session list request must carry the private workspace header"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn streaming_is_honestly_disabled_when_the_probe_is_unpinned() {
    let server = FakeEngine::new();
    let workspace = tempfile::tempdir().unwrap();
    let (provider, _engine) = build_provider(&server, workspace.path(), false);
    let view = provider.start(&setup_input()).await.unwrap();
    assert!(!view.streaming_supported);
    let reason = provider
        .streaming_support_reason()
        .expect("an unpinned stream carries a reason");
    assert!(reason.contains("unproven"));
    assert!(!reason.contains("127.0.0.1"));
    assert!(!reason.contains('/'));
    let mut receiver = provider.subscribe();
    provider.send(&SETUP, "Hello?").await.unwrap();
    let events = collect_until_done(&mut receiver, Duration::from_secs(3)).await;
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ChatEvent::Delta { .. })),
        "the honest fallback must not invent deltas"
    );
    let applier = project(&events);
    assert_eq!(applier.messages().len(), 2);
    assert_eq!(applier.messages()[1].joined_text(), "Hello world");
    assert_eq!(server.event_connections(), 0);
}

#[tokio::test]
async fn non_sse_event_endpoint_stays_ready_without_reconnect_flicker() {
    let server = FakeEngine::sse(SseScript::Html);
    let workspace = tempfile::tempdir().unwrap();
    let (provider, _engine) = build_provider(&server, workspace.path(), true);
    let mut receiver = provider.subscribe();
    let view = provider.start(&setup_input()).await.unwrap();
    wait_until(|| server.event_connections() >= 1, Duration::from_secs(2)).await;
    // Several backoff periods: a reconnect loop would open more connections.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(server.event_connections(), 1, "must not loop-reconnect");
    let mut events = Vec::new();
    while let Ok(event) = receiver.try_recv() {
        events.push(event);
    }
    for event in &events {
        if let ChatEvent::Session { session, .. } = event {
            assert!(
                !matches!(
                    session.state,
                    spirit_core::engine::chat::ChatSessionState::Connecting
                        | spirit_core::engine::chat::ChatSessionState::Error
                ),
                "no Reconnecting/Error flicker: {events:#?}"
            );
        }
    }
    assert!(
        matches!(
            view.state,
            spirit_core::engine::chat::ChatSessionState::Ready
        ),
        "session starts Ready: {view:#?}"
    );
}

#[cfg(unix)]
fn python_available() -> bool {
    std::process::Command::new("python3")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn private_xdg_tree_is_used_and_the_owner_home_is_not_the_engine_root() {
    use std::os::unix::fs::PermissionsExt;

    if !python_available() {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let app_data = temp.path().join("app-data");
    let output = temp.path().join("child-env.json");
    let binary = temp.path().join("opencode-chat-fake");
    let script = format!(
        r#"#!/usr/bin/env python3
import http.server, json, os, sys
payload = {{
    "XDG_DATA_HOME": os.environ.get("XDG_DATA_HOME"),
    "XDG_STATE_HOME": os.environ.get("XDG_STATE_HOME"),
    "XDG_CONFIG_HOME": os.environ.get("XDG_CONFIG_HOME"),
    "HOME": os.environ.get("HOME"),
    "cwd": os.getcwd(),
}}
with open({output:?}, "w") as handle:
    json.dump(payload, handle)
port = int(sys.argv[sys.argv.index("--port") + 1])
class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/global/health":
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(b'{{"healthy": true, "version": "1.19.5"}}')
        else:
            self.send_error(404)
    def log_message(self, *args):
        pass
http.server.HTTPServer(("127.0.0.1", port), Handler).serve_forever()
"#,
        output = output.display().to_string()
    );
    std::fs::write(&binary, script).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let runtime = ProcessEngineRuntime::with_binary(binary).with_private_state(app_data.clone());
    let info = runtime.spawn(0).await.unwrap();
    let mut healthy = false;
    for _ in 0..100 {
        if runtime.probe_health(&info).await.is_ok() {
            healthy = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(healthy, "the fake engine should become healthy");
    let payload: Value = serde_json::from_str(&std::fs::read_to_string(&output).unwrap()).unwrap();
    let tree = PrivateEngineTree::prepare(&app_data).unwrap();
    assert_eq!(
        payload["XDG_DATA_HOME"].as_str(),
        tree.data.to_str(),
        "the child must receive the private data root"
    );
    assert_eq!(payload["XDG_STATE_HOME"].as_str(), tree.state.to_str());
    assert_eq!(payload["XDG_CONFIG_HOME"].as_str(), tree.config.to_str());
    let child_cwd = std::fs::canonicalize(payload["cwd"].as_str().unwrap()).unwrap();
    let setup = std::fs::canonicalize(&tree.setup_workspace).unwrap();
    assert_eq!(child_cwd, setup);
    for directory in [
        &tree.data,
        &tree.state,
        &tree.config,
        &tree.setup_workspace,
        &tree.global_workspace,
        &tree.project_workspace_root,
    ] {
        assert_eq!(
            std::fs::metadata(directory).unwrap().permissions().mode() & 0o777,
            0o700,
            "the private tree must be 0700: {}",
            directory.display()
        );
    }
    assert!(
        !tree.opencode_config_dir().join("AGENTS.md").exists(),
        "a global AGENTS.md would be loaded by every engine session"
    );
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        assert!(!tree.data.starts_with(home.join(".local/share")));
        assert!(!tree.state.starts_with(home.join(".local/state")));
        assert!(!tree.config.starts_with(home.join(".config")));
        assert_eq!(
            payload["HOME"].as_str(),
            home.to_str(),
            "the private XDG roots must not require replacing the owner's HOME"
        );
    }
    runtime.shutdown(&info).await.unwrap();
}

/// A host with no VOWP local record for any orchestrator.
struct NoLocalRecord;

#[async_trait]
impl ChatEndpointResolver for NoLocalRecord {
    async fn resolve(
        &self,
        name: &str,
        _expected_session_id: Option<&str>,
    ) -> Result<ResolvedEndpoint, SpiritError> {
        Err(SpiritError::new(
            SpiritErrorCode::Gate,
            format!(
                "chat_not_configured: the {name} orchestrator has no local endpoint (there is no local record for this orchestrator)"
            ),
        ))
    }
}

fn unconfigured(provider: LocalChatProvider) -> LocalChatProvider {
    provider.with_endpoint_resolver(Arc::new(NoLocalRecord))
}

fn project_session(name: &str) -> ChatSessionId {
    ChatSessionId::Project(name.to_owned())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn project_without_a_local_record_is_a_dead_end_until_it_is_provisioned() {
    let server = FakeEngine::sse(SseScript::Deltas);
    let workspace = tempfile::tempdir().unwrap();
    let (provider, _engine) =
        build_provider(&server, &workspace.path().join("workspaces/setup"), true);
    let provider = unconfigured(provider);
    let session = project_session("stackspirit");

    // Starting through the attach-only path still reports the typed gate; no session is
    // invented behind the owner's back.
    let err = provider
        .start(&ChatStartInput {
            session: session.clone(),
            project: Some("stackspirit".to_owned()),
            location: ChatSessionLocation::Local,
            resume: None,
        })
        .await
        .unwrap_err();
    assert!(err.message.starts_with("chat_not_configured"), "{err:?}");
    assert_eq!(server.created_sessions(), 0);

    // One-click Start falls back to a project session on the bundled engine.
    let view = provider
        .provision_orchestrator("stackspirit")
        .await
        .unwrap();
    assert_eq!(view.session, session);
    assert_eq!(view.state, ChatSessionState::Ready);
    assert!(view.attached);
    assert!(view.error.is_none());
    assert_eq!(view.location, ChatSessionLocation::Local);
    assert!(view.model.is_some(), "the connected model is shown");
    assert_eq!(server.created_sessions(), 1);
    let created = server
        .requests()
        .into_iter()
        .find(|request| request.method == "POST" && request.path == "/session")
        .expect("session create request");
    assert!(
        created.body.contains("stackspirit orchestrator"),
        "{}",
        created.body
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn provisioned_project_chats_and_is_reused_without_chat_not_configured() {
    let server = FakeEngine::sse(SseScript::Deltas);
    let workspace = tempfile::tempdir().unwrap();
    let (provider, engine) =
        build_provider(&server, &workspace.path().join("workspaces/setup"), true);
    let provider = unconfigured(provider);
    let session = project_session("stackspirit");
    let mut receiver = provider.subscribe();
    provider
        .provision_orchestrator("stackspirit")
        .await
        .unwrap();
    wait_until(|| server.event_connections() >= 1, Duration::from_secs(2)).await;

    let receipt = provider.send(&session, "Hello?").await.unwrap();
    assert!(receipt.accepted);
    let events = collect_until_done(&mut receiver, Duration::from_secs(5)).await;
    let applier = project(&events);
    assert_eq!(applier.messages().len(), 2, "{events:#?}");
    assert_eq!(applier.messages()[1].joined_text(), "Hello world");
    let message = server
        .requests()
        .into_iter()
        .find(|request| request.method == "POST" && request.path.ends_with("/message"))
        .expect("message request");
    let body: Value = serde_json::from_str(&message.body).unwrap();
    assert_eq!(body["model"]["providerID"], "opencode");

    // Idempotent: provisioning, starting and attaching again reuse the same session.
    provider
        .provision_orchestrator("stackspirit")
        .await
        .unwrap();
    let started = provider
        .start(&ChatStartInput {
            session: session.clone(),
            project: Some("stackspirit".to_owned()),
            location: ChatSessionLocation::Local,
            resume: None,
        })
        .await
        .unwrap();
    assert_eq!(started.state, ChatSessionState::Ready);
    let attached = provider.attach(&session).await.unwrap();
    assert_eq!(attached.state, ChatSessionState::Ready);
    assert_eq!(server.created_sessions(), 1);
    assert_eq!(engine.spawns(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn provisioning_rejects_an_unsafe_project_name() {
    let server = FakeEngine::new();
    let workspace = tempfile::tempdir().unwrap();
    let (provider, _engine) = build_provider(&server, workspace.path(), false);
    let err = provider.provision_orchestrator("../etc").await.unwrap_err();
    assert_eq!(err.code, SpiritErrorCode::InvalidInput);
    assert_eq!(server.created_sessions(), 0);
}

/// A bundled engine whose port and usable models can change, like a sidecar restart
/// after a provider key is saved.
struct RestartableEngine {
    port: std::sync::atomic::AtomicU16,
    models: Mutex<Vec<spirit_core::engine::chat::ChatModelView>>,
}

impl RestartableEngine {
    fn restart(&self, port: u16, models: Vec<spirit_core::engine::chat::ChatModelView>) {
        *self
            .models
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = models;
        self.port.store(port, Ordering::SeqCst);
    }
}

#[async_trait]
impl ChatEngine for RestartableEngine {
    async fn ensure_ready(&self) -> Result<u16, SpiritError> {
        Ok(self.port.load(Ordering::SeqCst))
    }

    fn selected_model(&self) -> Option<spirit_core::engine::chat::ChatModelView> {
        Some(model())
    }

    fn available_models(&self) -> Vec<spirit_core::engine::chat::ChatModelView> {
        self.models
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

fn deepseek_model() -> spirit_core::engine::chat::ChatModelView {
    spirit_core::engine::chat::ChatModelView {
        id: "deepseek/deepseek-chat".to_owned(),
        label: "DeepSeek Chat".to_owned(),
        provider: "deepseek".to_owned(),
        tier: spirit_core::engine::chat::ChatModelTier::Included,
        context_tokens: Some(64_000),
        variants: None,
    }
}

/// A second engine process that already holds the first one's persisted sessions.
fn restarted_engine_with_sessions() -> FakeEngine {
    FakeEngine::with(|state| {
        state.sse = SseScript::Deltas;
        for id in ["ses_fake_1", "ses_fake_2"] {
            state.sessions.insert(id.to_owned(), Vec::new());
            state.order.push(id.to_owned());
        }
        state.next_session = 3;
    })
}

async fn bundled_setup_and_project(
    old: &FakeEngine,
    workspace: &Path,
) -> (
    LocalChatProvider,
    Arc<RestartableEngine>,
    tokio::sync::broadcast::Receiver<ChatEvent>,
) {
    let engine = Arc::new(RestartableEngine {
        port: std::sync::atomic::AtomicU16::new(old.port()),
        models: Mutex::new(vec![model()]),
    });
    let provider = unconfigured(
        LocalChatProvider::new(engine.clone(), workspace.to_path_buf())
            .with_stream_support(StreamSupport::Pinned),
    );
    let receiver = provider.subscribe();
    provider.start(&setup_input()).await.unwrap();
    provider
        .provision_orchestrator("stackspirit")
        .await
        .unwrap();
    wait_until(|| old.event_connections() >= 1, Duration::from_secs(2)).await;
    (provider, engine, receiver)
}

fn drain_events(receiver: &mut tokio::sync::broadcast::Receiver<ChatEvent>) -> Vec<ChatEvent> {
    let mut events = Vec::new();
    while let Ok(event) = receiver.try_recv() {
        events.push(event);
    }
    events
}

fn assert_rebound(provider_views: &[(ChatSessionId, ChatSessionState, usize)]) {
    for (session, state, models) in provider_views {
        assert_eq!(*state, ChatSessionState::Ready, "{session:?}");
        assert_eq!(*models, 2, "{session:?} must list the new model");
    }
}

async fn views_of(provider: &LocalChatProvider) -> Vec<(ChatSessionId, ChatSessionState, usize)> {
    let mut out = Vec::new();
    for session in [SETUP, project_session("stackspirit")] {
        let view = provider.attach(&session).await.unwrap();
        assert!(view.error.is_none(), "{view:?}");
        out.push((
            session,
            view.state,
            view.available_models.map_or(0, |models| models.len()),
        ));
    }
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn key_save_restart_rebinds_setup_and_project_sessions_without_stream_lost() {
    let old = FakeEngine::sse(SseScript::Deltas);
    let workspace = tempfile::tempdir().unwrap();
    let (provider, engine, mut receiver) = bundled_setup_and_project(&old, workspace.path()).await;
    drain_events(&mut receiver);

    let new = restarted_engine_with_sessions();
    engine.restart(new.port(), vec![model(), deepseek_model()]);
    drop(old);
    provider.refresh_setup_models().await;
    wait_until(|| new.event_connections() >= 1, Duration::from_secs(3)).await;
    // Give a stale pump the full reconnect window to wrongly report stream_lost.
    tokio::time::sleep(Duration::from_millis(1500)).await;

    let events = drain_events(&mut receiver);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ChatEvent::Error { .. })),
        "{events:#?}"
    );
    for session in [SETUP, project_session("stackspirit")] {
        let view = events
            .iter()
            .rev()
            .find_map(|event| match event {
                ChatEvent::Session { session: view, .. } if view.session == session => {
                    Some(view.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("no refreshed view for {session:?}: {events:#?}"));
        assert_eq!(view.state, ChatSessionState::Ready);
        assert_eq!(view.available_models.as_ref().map(Vec::len), Some(2));
    }
    assert_rebound(&views_of(&provider).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dead_bundled_port_pump_probes_the_engine_and_rebinds_instead_of_stream_lost() {
    let old = FakeEngine::sse(SseScript::Deltas);
    let workspace = tempfile::tempdir().unwrap();
    let (provider, engine, mut receiver) = bundled_setup_and_project(&old, workspace.path()).await;
    drain_events(&mut receiver);

    // The engine restarts without anyone calling refresh: only the pump notices.
    let new = restarted_engine_with_sessions();
    engine.restart(new.port(), vec![model(), deepseek_model()]);
    drop(old);
    wait_until(|| new.event_connections() >= 1, Duration::from_secs(5)).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;

    let events = drain_events(&mut receiver);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ChatEvent::Error { .. })),
        "{events:#?}"
    );
    assert_rebound(&views_of(&provider).await);
}

struct RecordingProvisioner {
    calls: Mutex<Vec<String>>,
    result: fn() -> Result<(), SpiritError>,
}

#[async_trait::async_trait]
impl spirit_core::engine::ChatOrchestratorProvisioner for RecordingProvisioner {
    async fn ensure_orchestrator(&self, project: &str) -> Result<(), SpiritError> {
        self.calls.lock().unwrap().push(project.to_owned());
        (self.result)()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn provision_creates_the_vowp_orchestrator_before_the_endpoint_lookup() {
    let server = FakeEngine::sse(SseScript::Deltas);
    let workspace = tempfile::tempdir().unwrap();
    let (provider, _engine) =
        build_provider(&server, &workspace.path().join("workspaces/setup"), true);
    let provisioner = Arc::new(RecordingProvisioner {
        calls: Mutex::new(Vec::new()),
        result: || Ok(()),
    });
    let provider = unconfigured(provider).with_orchestrator_provisioner(provisioner.clone());
    provider
        .provision_orchestrator("stackspirit")
        .await
        .unwrap();
    assert_eq!(*provisioner.calls.lock().unwrap(), vec!["stackspirit"]);

    // A real provisioning failure is shown to the owner, not hidden by the local fallback.
    let (provider, _engine) =
        build_provider(&server, &workspace.path().join("workspaces/setup"), true);
    let failing = Arc::new(RecordingProvisioner {
        calls: Mutex::new(Vec::new()),
        result: || {
            Err(SpiritError::new(
                SpiritErrorCode::Engine,
                "vowp orchestrator new failed",
            ))
        },
    });
    let provider = unconfigured(provider).with_orchestrator_provisioner(failing);
    let err = provider
        .provision_orchestrator("stackspirit")
        .await
        .unwrap_err();
    assert!(
        err.message.contains("vowp orchestrator new failed"),
        "{err:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn engine_backed_project_session_is_not_the_setup_assistant() {
    let server = FakeEngine::sse(SseScript::Deltas);
    let workspace = tempfile::tempdir().unwrap();
    let setup = workspace.path().join("workspaces/setup");
    let (provider, _engine) = build_provider(&server, &setup, true);
    let provider = unconfigured(provider);
    let session = project_session("stackspirit");
    let mut receiver = provider.subscribe();
    provider
        .provision_orchestrator("stackspirit")
        .await
        .unwrap();
    wait_until(|| server.event_connections() >= 1, Duration::from_secs(2)).await;

    let project_dir = workspace.path().join("workspaces/project/stackspirit");
    let agents = std::fs::read_to_string(project_dir.join("AGENTS.md")).unwrap();
    assert!(agents.contains("stackspirit"));
    assert!(!agents.contains("setup assistant"));

    let created = server
        .requests()
        .into_iter()
        .find(|r| r.method == "POST" && r.path == "/session")
        .unwrap();
    assert_eq!(created.directory.as_deref(), project_dir.to_str());
    let body: Value = serde_json::from_str(&created.body).unwrap();
    assert_eq!(body["permission"][0]["action"], "ask", "{body}");

    provider.send(&session, "Hello?").await.unwrap();
    let _ = collect_until_done(&mut receiver, Duration::from_secs(5)).await;
    let message = server
        .requests()
        .into_iter()
        .find(|r| r.method == "POST" && r.path.ends_with("/message"))
        .unwrap();
    let body: Value = serde_json::from_str(&message.body).unwrap();
    let system = body["system"].as_str().unwrap();
    assert!(system.contains("orchestrator") && !system.contains("setup assistant"));
    assert_eq!(message.directory.as_deref(), project_dir.to_str());
}

#[test]
fn provider_variants_are_parsed_into_runtime_models() {
    let body = json!({
        "all": [{"id": "deepseek", "models": {
            "v4-flash": {"name": "V4 Flash", "variants": {"low": {}, "high": {}, "max": {"disabled": true}}},
            "plain": {"name": "Plain"}
        }}],
        "connected": ["deepseek"]
    })
    .to_string();
    let models = spirit_core::engine::process::parse_models(body.as_bytes());
    let flash = models.iter().find(|m| m.id == "deepseek/v4-flash").unwrap();
    let mut variants = flash.variants.clone();
    variants.sort();
    assert_eq!(variants, vec!["high", "low"]);
    let plain = models.iter().find(|m| m.id == "deepseek/plain").unwrap();
    assert!(plain.variants.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn send_turn_forwards_variant_and_file_parts() {
    use spirit_core::engine::chat::ChatAttachment;
    let server = FakeEngine::sse(SseScript::Deltas);
    let workspace = tempfile::tempdir().unwrap();
    let (provider, _engine) = build_provider(&server, workspace.path(), true);
    provider.start(&setup_input()).await.unwrap();
    let attachments = vec![
        ChatAttachment {
            filename: "shot.png".into(),
            mime: "image/png".into(),
            url: "data:image/png;base64,AAAA".into(),
        },
        ChatAttachment {
            filename: "notes.txt".into(),
            mime: "text/plain".into(),
            url: "data:text/plain;base64,aGk=".into(),
        },
    ];
    provider
        .send_turn(&SETUP, "look", Some("high"), &attachments)
        .await
        .unwrap();
    wait_until(
        || {
            server
                .requests()
                .iter()
                .any(|r| r.method == "POST" && r.path.ends_with("/message"))
        },
        Duration::from_secs(3),
    )
    .await;
    let requests = server.requests();
    let message = requests
        .iter()
        .find(|r| r.method == "POST" && r.path.ends_with("/message"))
        .expect("message request");
    let body: Value = serde_json::from_str(&message.body).unwrap();
    assert_eq!(body["variant"], "high");
    assert_eq!(body["parts"][0], json!({"type": "text", "text": "look"}));
    assert_eq!(
        body["parts"][1],
        json!({"type": "file", "mime": "image/png", "filename": "shot.png", "url": "data:image/png;base64,AAAA"})
    );
    assert_eq!(body["parts"][2]["filename"], "notes.txt");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn send_turn_rejects_bad_attachments_and_variants() {
    use spirit_core::engine::chat::ChatAttachment;
    let server = FakeEngine::sse(SseScript::Deltas);
    let workspace = tempfile::tempdir().unwrap();
    let (provider, _engine) = build_provider(&server, workspace.path(), true);
    provider.start(&setup_input()).await.unwrap();
    let remote = vec![ChatAttachment {
        filename: "x".into(),
        mime: "image/png".into(),
        url: "https://example.com/x.png".into(),
    }];
    assert!(
        provider
            .send_turn(&SETUP, "hi", None, &remote)
            .await
            .is_err()
    );
    assert!(
        provider
            .send_turn(&SETUP, "hi", Some("a b"), &[])
            .await
            .is_err()
    );
}
