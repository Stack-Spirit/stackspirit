//! Hermetic tests for the VOWP local-orchestrator endpoint resolver and the Local
//! `global`/`project` attach path.
//!
//! The tests run a fake `vowp` executable on an injected `PATH` and a fake OpenCode v1
//! loopback server. No real VOWP, agent, orchestrator, provider key or owner state is
//! touched. Every reported port, credential and session id in this file is generated at
//! runtime or is an obvious placeholder, and the assertions never print them.
#![cfg(unix)]

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde_json::{Value, json};

use spirit_core::engine::chat::{
    ChatEndpointResolver, ChatEngine, ChatEvent, ChatLogQuery, ChatModelView, ChatRole,
    ChatSessionId, ChatSessionLocation, ChatSessionState, ChatStartInput, LocalChatProvider,
    VowpEndpointResolver,
};
use spirit_core::error::{SpiritError, SpiritErrorCode};

const NATIVE: &str = "ses_mirrored_fake_1";
const HOME_SESSION: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const WORKSPACE: &str = "/fake/orchestrator/workspace";
const OTHER_WORKSPACE: &str = "/fake/other/workspace";

fn secret() -> String {
    ["not", "a", "real", "credential"].join("-")
}

fn model() -> ChatModelView {
    ChatModelView {
        id: "opencode/big-pickle".to_owned(),
        label: "Big Pickle".to_owned(),
        provider: "opencode".to_owned(),
        tier: spirit_core::engine::chat::ChatModelTier::Included,
        context_tokens: Some(128_000),
    }
}

fn setup_input() -> ChatStartInput {
    ChatStartInput {
        session: ChatSessionId::Setup,
        project: None,
        location: ChatSessionLocation::Local,
        resume: None,
    }
}

fn global_input() -> ChatStartInput {
    ChatStartInput {
        session: ChatSessionId::Global,
        project: None,
        location: ChatSessionLocation::Local,
        resume: None,
    }
}

fn project_input(name: &str) -> ChatStartInput {
    ChatStartInput {
        session: ChatSessionId::Project(name.to_owned()),
        project: Some(name.to_owned()),
        location: ChatSessionLocation::Local,
        resume: None,
    }
}

/// The setup engine must never be asked for a port on a VOWP orchestrator session.
struct PanicEngine;

#[async_trait]
impl ChatEngine for PanicEngine {
    async fn ensure_ready(&self) -> Result<u16, SpiritError> {
        panic!("a global/project session must never start or reuse the bundled engine");
    }

    fn selected_model(&self) -> Option<ChatModelView> {
        panic!("a global/project session must not copy the setup model");
    }
}

struct SetupEngine {
    port: u16,
}

#[async_trait]
impl ChatEngine for SetupEngine {
    async fn ensure_ready(&self) -> Result<u16, SpiritError> {
        Ok(self.port)
    }

    fn selected_model(&self) -> Option<ChatModelView> {
        Some(model())
    }
}

#[derive(Clone)]
struct RequestRecord {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: String,
}

struct ServerState {
    expected_auth: Option<String>,
    sessions: HashMap<String, Vec<Value>>,
    /// The `/api/session` entries, newest first or in any order.
    list: Vec<Value>,
    /// A direct `/api/session` body override for malformed-shape tests.
    list_body: Option<Value>,
    list_status: u16,
    next_session: usize,
    requests: Vec<RequestRecord>,
    assistant_text: String,
}

struct FakeServer {
    port: u16,
    state: Arc<Mutex<ServerState>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl FakeServer {
    fn new(expected_auth: Option<String>) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind fake server");
        listener
            .set_nonblocking(true)
            .expect("set fake listener nonblocking");
        let port = listener.local_addr().expect("fake local addr").port();
        let mut sessions = HashMap::new();
        sessions.insert(
            NATIVE.to_owned(),
            vec![json!({
                "info": {"id": "msg_welcome", "role": "assistant", "sessionID": NATIVE,
                         "time": {"created": 1000}},
                "parts": [{"type": "text", "text": "Welcome back"}],
            })],
        );
        let state = Arc::new(Mutex::new(ServerState {
            expected_auth,
            sessions,
            list: vec![json!({
                "id": NATIVE,
                "title": "Mirrored",
                "time": {"created": 1000, "updated": 1000},
                "location": {"directory": WORKSPACE},
            })],
            list_body: None,
            list_status: 200,
            next_session: 1,
            requests: Vec::new(),
            assistant_text: "Hello world".to_owned(),
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_state = Arc::clone(&state);
        let thread_stop = Arc::clone(&stop);
        let handle = thread::spawn(move || serve(listener, thread_state, thread_stop));
        Self {
            port,
            state,
            stop,
            thread: Some(handle),
        }
    }

    fn port(&self) -> u16 {
        self.port
    }

    fn requests(&self) -> Vec<RequestRecord> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .requests
            .clone()
    }

    fn created_sessions(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .next_session
            .saturating_sub(1)
    }

    fn set_list(&self, list: Vec<Value>) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .list = list;
    }

    fn set_list_body(&self, body: Value) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .list_body = Some(body);
    }

    fn set_list_status(&self, status: u16) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .list_status = status;
    }

    fn add_log(&self, id: &str, messages: Vec<Value>) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .sessions
            .insert(id.to_owned(), messages);
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn serve(listener: TcpListener, state: Arc<Mutex<ServerState>>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let state = Arc::clone(&state);
                thread::spawn(move || handle_connection(stream, state));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2));
            }
            Err(_) => return,
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Option<RequestRecord> {
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
    Some(RequestRecord {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn handle_connection(mut stream: TcpStream, state: Arc<Mutex<ServerState>>) {
    let Some(request) = read_request(&mut stream) else {
        return;
    };
    let path = request.path.split('?').next().unwrap_or(&request.path);
    let auth_ok = {
        let state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .expected_auth
            .as_ref()
            .is_none_or(|expected| request.headers.get("authorization") == Some(expected))
    };
    if !auth_ok {
        write_json(&mut stream, 401, &json!({"error": "auth"}));
        return;
    }
    {
        let mut state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.requests.push(request.clone());
    }
    match (request.method.as_str(), path) {
        ("GET", "/global/health") => {
            write_json(
                &mut stream,
                200,
                &json!({"healthy": true, "version": "1.19.5"}),
            );
        }
        ("POST", "/session") => {
            let id = {
                let mut state = state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let id = format!("ses_setup_{}", state.next_session);
                state.next_session = state.next_session.saturating_add(1);
                state.sessions.insert(id.clone(), Vec::new());
                id
            };
            write_json(
                &mut stream,
                200,
                &json!({"id": id, "title": "StackSpirit setup"}),
            );
        }
        ("GET", "/api/session") => {
            let (status, body) = {
                let state = state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                (
                    state.list_status,
                    state
                        .list_body
                        .clone()
                        .unwrap_or_else(|| json!({"data": state.list})),
                )
            };
            write_json(&mut stream, status, &body);
        }
        ("GET", "/event") => {
            write_json(&mut stream, 404, &json!({}));
        }
        _ => handle_session_route(&mut stream, &state, &request.method, path, &request.body),
    }
}

fn handle_session_route(
    stream: &mut TcpStream,
    state: &Arc<Mutex<ServerState>>,
    method: &str,
    path: &str,
    body: &str,
) {
    let Some(rest) = path.strip_prefix("/session/") else {
        write_json(stream, 404, &json!({}));
        return;
    };
    if let Some(id) = rest.strip_suffix("/message") {
        if method == "POST" {
            let parsed: Value = serde_json::from_str(body).unwrap_or(Value::Null);
            let Some(user_id) = parsed.get("messageID").and_then(Value::as_str) else {
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
            let text = parsed
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
            let (exists, assistant_text) = {
                let mut state = state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let exists = state.sessions.contains_key(id);
                if exists {
                    let text_clone = text.clone();
                    if let Some(log) = state.sessions.get_mut(id) {
                        log.push(json!({
                            "info": {"id": user_id, "role": "user", "sessionID": id,
                                     "time": {"created": now_ms()}},
                            "parts": [{"type": "text", "text": text_clone}],
                        }));
                    }
                }
                (exists, state.assistant_text.clone())
            };
            if !exists {
                write_json(stream, 404, &json!({}));
                return;
            }
            if let Some(log) = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .sessions
                .get_mut(id)
            {
                log.push(json!({
                    "info": {"id": "msg_assistant", "role": "assistant", "sessionID": id,
                             "time": {"created": now_ms()}},
                    "parts": [{"type": "text", "text": assistant_text}],
                }));
            }
            write_json(
                stream,
                200,
                &json!({
                    "info": {"id": "msg_assistant", "role": "assistant", "sessionID": id,
                             "time": {"created": now_ms()}},
                    "parts": [{"type": "text", "text": assistant_text}],
                }),
            );
            return;
        }
        let messages = {
            let state = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.sessions.get(id).cloned()
        };
        match messages {
            Some(messages) => write_json(stream, 200, &Value::Array(messages)),
            None => write_json(stream, 404, &json!({})),
        }
        return;
    }
    if method != "GET" {
        write_json(stream, 405, &json!({}));
        return;
    }
    let exists = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .sessions
        .contains_key(rest);
    if exists {
        write_json(stream, 200, &json!({"id": rest, "title": "Mirrored"}));
    } else {
        write_json(stream, 404, &json!({}));
    }
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

fn write_vowp(dir: &Path, body: &str) -> PathBuf {
    let binary = dir.join("vowp");
    std::fs::write(&binary, format!("#!/bin/sh\n{body}\n")).expect("write fake vowp");
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))
        .expect("chmod fake vowp");
    binary
}

fn write_endpoint_vowp(dir: &Path, document: &Value) {
    let serialized = serde_json::to_string(document).expect("serialize endpoint");
    write_vowp(
        dir,
        &format!("cat <<'VOWP_ENDPOINT_EOF'\n{serialized}\nVOWP_ENDPOINT_EOF"),
    );
}

fn path_with(dir: &Path) -> std::ffi::OsString {
    let mut paths = vec![dir.to_path_buf()];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(paths).expect("join fake PATH")
}

fn provider_with_vowp(
    engine: Arc<dyn ChatEngine>,
    workspace: &Path,
    vowp_dir: &Path,
) -> LocalChatProvider {
    LocalChatProvider::new(engine, workspace.to_path_buf()).with_endpoint_resolver(Arc::new(
        VowpEndpointResolver::with_path_env(path_with(vowp_dir)),
    ))
}

fn endpoint_document(
    name: &str,
    app: &str,
    state: &str,
    url: Option<&str>,
    native: Option<&str>,
    session_id: &str,
) -> Value {
    json!({
        "name": name,
        "app": app,
        "session_id": session_id,
        "location": "node",
        "node": "mac-studio",
        "state": state,
        "url": url,
        "native_session_id": native,
        "auth": null,
    })
}

fn with_auth(mut document: Value, credential: &str) -> Value {
    document["auth"] = json!({
        "header": "Authorization",
        "scheme": "Basic",
        "username": "opencode",
        "password": credential,
    });
    document
}

fn base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let first = u32::from(chunk[0]);
        let second = u32::from(chunk.get(1).copied().unwrap_or(0));
        let third = u32::from(chunk.get(2).copied().unwrap_or(0));
        let value = (first << 16) | (second << 8) | third;
        output.push(TABLE[((value >> 18) & 63) as usize] as char);
        output.push(TABLE[((value >> 12) & 63) as usize] as char);
        output.push(if chunk.len() > 1 {
            TABLE[((value >> 6) & 63) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[(value & 63) as usize] as char
        } else {
            '='
        });
    }
    output
}

fn assert_safe_error(error: &SpiritError, forbidden: &[&str]) {
    let rendered = format!("{} {:?}", error.message, error.hint);
    for value in forbidden {
        assert!(!value.is_empty());
        assert!(
            !rendered.contains(value),
            "the typed error leaked a transport value"
        );
    }
    assert!(!rendered.contains("127.0.0.1"));
    assert!(!rendered.contains("http://"));
    assert!(!rendered.contains("Authorization"));
    assert!(!rendered.contains("Basic "));
}

fn assert_safe_view(view: &Value, forbidden: &[&str]) {
    let text = serde_json::to_string(view).unwrap_or_default();
    for value in forbidden {
        assert!(!value.is_empty());
        assert!(
            !text.contains(value),
            "a serialized chat value leaked: {text}"
        );
    }
    assert!(!text.contains("127.0.0.1"));
    assert!(!text.contains("http://"));
    assert!(!text.contains("\"url\""));
    assert!(!text.contains("\"port\""));
    assert!(!text.contains("native_session_id"));
    assert!(!text.contains("x-opencode-directory"));
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
            "timed out; {} events received",
            events.len()
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
            Err(_) => panic!("timed out; {} events received", events.len()),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn global_endpoint_attaches_and_sends_to_the_reported_native_session() {
    let credential = secret();
    let expected_auth = format!(
        "Basic {}",
        base64(format!("opencode:{credential}").as_bytes())
    );
    let server = FakeServer::new(Some(expected_auth.clone()));
    let workspace = tempfile::tempdir().unwrap();
    let vowp_dir = tempfile::tempdir().unwrap();
    let url = format!("http://127.0.0.1:{}", server.port());
    let document = with_auth(
        endpoint_document(
            "global",
            "opencode",
            "running",
            Some(&url),
            Some(NATIVE),
            HOME_SESSION,
        ),
        &credential,
    );
    write_endpoint_vowp(vowp_dir.path(), &document);
    let provider = provider_with_vowp(Arc::new(PanicEngine), workspace.path(), vowp_dir.path());
    let mut receiver = provider.subscribe();

    let view = provider.start(&global_input()).await.unwrap();
    assert_eq!(view.session, ChatSessionId::Global);
    assert_eq!(view.location, ChatSessionLocation::Local);
    assert_eq!(view.state, ChatSessionState::Ready);
    assert!(view.attached);
    assert!(view.model.is_none());
    assert_safe_view(
        &serde_json::to_value(&view).unwrap(),
        &[
            &credential,
            NATIVE,
            HOME_SESSION,
            &server.port().to_string(),
        ],
    );

    // A reattach resolves the same home session and still never creates one.
    let reattached = provider.attach(&ChatSessionId::Global).await.unwrap();
    assert_eq!(reattached.state, ChatSessionState::Ready);
    assert_eq!(server.created_sessions(), 0);

    let requests = server.requests();
    let attach = requests
        .iter()
        .find(|request| request.method == "GET" && request.path == format!("/session/{NATIVE}"))
        .expect("the endpoint's native session must be read");
    assert_eq!(
        attach.headers.get("authorization"),
        Some(&expected_auth),
        "the loopback credential must be sent"
    );
    assert!(
        !attach.headers.contains_key("x-opencode-directory"),
        "a VOWP endpoint must not receive the app's setup workspace"
    );
    assert!(
        !requests
            .iter()
            .any(|request| request.method == "POST" && request.path == "/session"),
        "global chat must never create a native session"
    );

    let receipt = provider
        .send(&ChatSessionId::Global, "Hello?")
        .await
        .unwrap();
    let events = collect_until_done(&mut receiver, Duration::from_secs(3)).await;
    let assistant = events.iter().find_map(|event| match event {
        ChatEvent::Message { message, .. } if message.role == ChatRole::Assistant => {
            Some(message.clone())
        }
        _ => None,
    });
    let assistant = assistant.expect("the final response must produce one assistant message");
    assert_eq!(assistant.joined_text(), "Hello world");
    assert_safe_view(
        &serde_json::to_value(&receipt).unwrap(),
        &[
            &credential,
            NATIVE,
            HOME_SESSION,
            &server.port().to_string(),
        ],
    );
    for event in &events {
        assert_safe_view(
            &serde_json::to_value(event).unwrap(),
            &[
                &credential,
                NATIVE,
                HOME_SESSION,
                &server.port().to_string(),
            ],
        );
    }

    let sent = server
        .requests()
        .into_iter()
        .find(|request| request.method == "POST" && request.path.ends_with("/message"))
        .expect("the turn must use the reported native session");
    assert_eq!(sent.path, format!("/session/{NATIVE}/message"));
    assert_eq!(sent.headers.get("authorization"), Some(&expected_auth));
    assert!(!sent.headers.contains_key("x-opencode-directory"));
    let body: Value = serde_json::from_str(&sent.body).unwrap();
    assert!(
        body.get("model").is_none(),
        "the orchestrator must choose its own model"
    );
    assert_eq!(body["messageID"], receipt.message.id);

    let page = provider
        .log(
            &ChatSessionId::Global,
            &ChatLogQuery {
                cursor: None,
                limit: Some(50),
            },
        )
        .await
        .unwrap();
    assert_eq!(page.messages.len(), 3);
    assert_safe_view(
        &serde_json::to_value(&page).unwrap(),
        &[
            &credential,
            NATIVE,
            HOME_SESSION,
            &server.port().to_string(),
        ],
    );

    // A reattach must not silently follow VOWP to another home session log.
    let other_session = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
    let mismatched = endpoint_document(
        "global",
        "opencode",
        "running",
        Some(&url),
        Some(NATIVE),
        other_session,
    );
    write_endpoint_vowp(vowp_dir.path(), &mismatched);
    let error = provider.attach(&ChatSessionId::Global).await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Gate);
    assert!(error.message.contains("chat_not_mirrored"));
    assert_safe_error(
        &error,
        &[
            &credential,
            NATIVE,
            HOME_SESSION,
            other_session,
            &server.port().to_string(),
        ],
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn project_endpoint_uses_the_project_name_as_the_orchestrator() {
    let server = FakeServer::new(None);
    let workspace = tempfile::tempdir().unwrap();
    let vowp_dir = tempfile::tempdir().unwrap();
    let url = format!("http://127.0.0.1:{}", server.port());
    let document = endpoint_document(
        "meadow",
        "opencode",
        "degraded",
        Some(&url),
        Some(NATIVE),
        HOME_SESSION,
    );
    write_endpoint_vowp(vowp_dir.path(), &document);
    let provider = provider_with_vowp(Arc::new(PanicEngine), workspace.path(), vowp_dir.path());

    let view = provider.start(&project_input("meadow")).await.unwrap();
    assert_eq!(view.session, ChatSessionId::Project("meadow".to_owned()));
    assert_eq!(view.location, ChatSessionLocation::Local);
    assert_eq!(view.state, ChatSessionState::Ready);
    assert!(view.attached);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn setup_assistant_still_creates_and_streams_on_the_bundled_engine() {
    let server = FakeServer::new(None);
    let workspace = tempfile::tempdir().unwrap();
    let engine = Arc::new(SetupEngine {
        port: server.port(),
    });
    let provider = LocalChatProvider::new(engine, workspace.path().to_path_buf());
    let mut receiver = provider.subscribe();

    let view = provider.start(&setup_input()).await.unwrap();
    assert_eq!(view.session, ChatSessionId::Setup);
    assert_eq!(view.location, ChatSessionLocation::Local);
    assert_eq!(view.state, ChatSessionState::Ready);
    assert!(view.attached);
    assert_eq!(view.model, Some(model()));
    assert_eq!(server.created_sessions(), 1);

    let receipt = provider
        .send(&ChatSessionId::Setup, "Hello?")
        .await
        .unwrap();
    let events = collect_until_done(&mut receiver, Duration::from_secs(3)).await;
    let assistant = events.iter().find_map(|event| match event {
        ChatEvent::Message { message, .. } if message.role == ChatRole::Assistant => {
            Some(message.clone())
        }
        _ => None,
    });
    assert_eq!(
        assistant.expect("the setup reply").joined_text(),
        "Hello world"
    );
    let create = server
        .requests()
        .into_iter()
        .find(|request| request.method == "POST" && request.path == "/session")
        .expect("the setup assistant must create its own native session");
    assert_eq!(
        create
            .headers
            .get("x-opencode-directory")
            .map(String::as_str),
        Some(workspace.path().to_str().unwrap()),
        "the setup workspace header must remain unchanged"
    );
    assert!(receipt.accepted);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn null_url_stopped_state_and_non_opencode_app_are_chat_gated() {
    let workspace = tempfile::tempdir().unwrap();

    for (label, document) in [
        (
            "null url",
            endpoint_document(
                "global",
                "opencode",
                "running",
                None,
                Some(NATIVE),
                HOME_SESSION,
            ),
        ),
        (
            "stopped",
            endpoint_document(
                "global",
                "opencode",
                "stopped",
                Some("http://127.0.0.1:1"),
                None,
                HOME_SESSION,
            ),
        ),
        (
            "other app",
            endpoint_document(
                "global",
                "claude_code",
                "running",
                Some("http://127.0.0.1:1"),
                Some(NATIVE),
                HOME_SESSION,
            ),
        ),
    ] {
        let vowp_dir = tempfile::tempdir().unwrap();
        write_endpoint_vowp(vowp_dir.path(), &document);
        let provider = provider_with_vowp(Arc::new(PanicEngine), workspace.path(), vowp_dir.path());
        let error = provider
            .start(&global_input())
            .await
            .expect_err("the endpoint must be refused");
        assert_eq!(error.code, SpiritErrorCode::Gate, "{label}");
        assert!(error.message.contains("chat_gated"), "{label}: {error}");
        assert_safe_error(&error, &[NATIVE, HOME_SESSION]);
    }
}

fn session_entry(id: &str, updated: i64, workspace: Option<&str>) -> Value {
    let mut entry = json!({
        "id": id,
        "title": "Orchestrator conversation",
        "time": {"created": updated.saturating_sub(100), "updated": updated},
    });
    if let Some(workspace) = workspace {
        entry["location"] = json!({"directory": workspace});
    }
    entry
}

fn with_workspace(mut document: Value, workspace: &str) -> Value {
    document["workspace"] = json!(workspace);
    document
}

fn no_session_creates(requests: &[RequestRecord]) -> bool {
    !requests
        .iter()
        .any(|request| request.method == "POST" && request.path == "/session")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_null_native_id_continues_the_newest_host_session() {
    let credential = secret();
    let expected_auth = format!(
        "Basic {}",
        base64(format!("opencode:{credential}").as_bytes())
    );
    let server = FakeServer::new(Some(expected_auth.clone()));
    server.add_log(
        "ses_older_fake",
        vec![json!({
            "info": {"id": "msg_older", "role": "assistant", "time": {"created": 1000}},
            "parts": [{"type": "text", "text": "The older conversation"}],
        })],
    );
    server.add_log(
        "ses_newest_fake",
        vec![json!({
            "info": {"id": "msg_newest", "role": "assistant", "time": {"created": 5000}},
            "parts": [{"type": "text", "text": "The owner's existing conversation"}],
        })],
    );
    server.set_list(vec![
        session_entry("ses_older_fake", 1000, Some(WORKSPACE)),
        session_entry("ses_newest_fake", 5000, Some(WORKSPACE)),
    ]);
    let workspace = tempfile::tempdir().unwrap();
    let vowp_dir = tempfile::tempdir().unwrap();
    let url = format!("http://127.0.0.1:{}", server.port());
    let document = with_auth(
        endpoint_document(
            "global",
            "opencode",
            "running",
            Some(&url),
            None,
            HOME_SESSION,
        ),
        &credential,
    );
    write_endpoint_vowp(vowp_dir.path(), &document);
    let provider = provider_with_vowp(Arc::new(PanicEngine), workspace.path(), vowp_dir.path());

    let view = provider.start(&global_input()).await.unwrap();
    assert_eq!(view.state, ChatSessionState::Ready);
    assert!(view.attached);
    assert_safe_view(
        &serde_json::to_value(&view).unwrap(),
        &[
            &credential,
            "ses_older_fake",
            "ses_newest_fake",
            HOME_SESSION,
            WORKSPACE,
        ],
    );

    let requests = server.requests();
    let list = requests
        .iter()
        .find(|request| request.method == "GET" && request.path == "/api/session")
        .expect("a null native id must list the host sessions");
    assert_eq!(
        list.headers.get("authorization"),
        Some(&expected_auth),
        "the discovery request must use the reported Basic credential"
    );
    assert!(
        !list.headers.contains_key("x-opencode-directory"),
        "VOWP reported no workspace, so the list request must not invent one"
    );
    assert!(
        no_session_creates(&requests),
        "discovery must never create a native session"
    );
    let get = requests
        .iter()
        .find(|request| request.method == "GET" && request.path == "/session/ses_newest_fake")
        .expect("the newest session must be checked");
    assert_eq!(
        get.headers.get("x-opencode-directory").map(String::as_str),
        Some(WORKSPACE),
        "the discovered workspace must scope the attach path"
    );
    let log = requests
        .iter()
        .find(|request| {
            request.method == "GET" && request.path == "/session/ses_newest_fake/message"
        })
        .expect("the newest session's log must be read");
    assert_eq!(
        log.headers.get("x-opencode-directory").map(String::as_str),
        Some(WORKSPACE)
    );
    assert!(
        !requests.iter().any(|request| {
            request.method == "GET" && request.path == "/session/ses_older_fake/message"
        }),
        "the older session must not be read"
    );

    let page = provider
        .log(
            &ChatSessionId::Global,
            &ChatLogQuery {
                cursor: None,
                limit: Some(50),
            },
        )
        .await
        .unwrap();
    assert_eq!(page.messages.len(), 1);
    assert_eq!(
        page.messages[0].joined_text(),
        "The owner's existing conversation"
    );
    assert_safe_view(
        &serde_json::to_value(&page).unwrap(),
        &[
            &credential,
            "ses_older_fake",
            "ses_newest_fake",
            HOME_SESSION,
            WORKSPACE,
        ],
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reported_native_id_wins_and_never_lists() {
    let server = FakeServer::new(None);
    server.add_log(
        "ses_newest_fake",
        vec![json!({
            "info": {"id": "msg_newest", "role": "assistant", "time": {"created": 5000}},
            "parts": [{"type": "text", "text": "A newer session that must be ignored"}],
        })],
    );
    server.set_list(vec![
        session_entry("ses_newest_fake", 9000, Some(WORKSPACE)),
        session_entry(NATIVE, 1000, Some(WORKSPACE)),
    ]);
    let workspace = tempfile::tempdir().unwrap();
    let vowp_dir = tempfile::tempdir().unwrap();
    let url = format!("http://127.0.0.1:{}", server.port());
    let document = endpoint_document(
        "global",
        "opencode",
        "running",
        Some(&url),
        Some(NATIVE),
        HOME_SESSION,
    );
    write_endpoint_vowp(vowp_dir.path(), &document);
    let provider = provider_with_vowp(Arc::new(PanicEngine), workspace.path(), vowp_dir.path());

    let view = provider.start(&global_input()).await.unwrap();
    assert_eq!(view.state, ChatSessionState::Ready);
    let requests = server.requests();
    assert!(
        !requests
            .iter()
            .any(|request| request.method == "GET" && request.path == "/api/session"),
        "a reported native id must never trigger listing"
    );
    assert!(
        requests
            .iter()
            .any(|request| request.method == "GET" && request.path == format!("/session/{NATIVE}")),
        "the reported native session must be used"
    );
    assert!(no_session_creates(&requests));
    let page = provider
        .log(
            &ChatSessionId::Global,
            &ChatLogQuery {
                cursor: None,
                limit: Some(50),
            },
        )
        .await
        .unwrap();
    assert_eq!(page.messages.len(), 1);
    assert_eq!(page.messages[0].joined_text(), "Welcome back");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reported_workspace_scopes_discovery_and_is_sent() {
    let server = FakeServer::new(None);
    server.add_log(
        "ses_orchestrator_fake",
        vec![json!({
            "info": {"id": "msg_orchestrator", "role": "assistant", "time": {"created": 1000}},
            "parts": [{"type": "text", "text": "The orchestrator conversation"}],
        })],
    );
    server.add_log(
        "ses_other_fake",
        vec![json!({
            "info": {"id": "msg_other", "role": "assistant", "time": {"created": 9000}},
            "parts": [{"type": "text", "text": "Another directory's conversation"}],
        })],
    );
    server.set_list(vec![
        session_entry("ses_other_fake", 9000, Some(OTHER_WORKSPACE)),
        session_entry("ses_orchestrator_fake", 1000, Some(WORKSPACE)),
    ]);
    let workspace = tempfile::tempdir().unwrap();
    let vowp_dir = tempfile::tempdir().unwrap();
    let url = format!("http://127.0.0.1:{}", server.port());
    let document = with_workspace(
        endpoint_document(
            "global",
            "opencode",
            "running",
            Some(&url),
            None,
            HOME_SESSION,
        ),
        WORKSPACE,
    );
    write_endpoint_vowp(vowp_dir.path(), &document);
    let provider = provider_with_vowp(Arc::new(PanicEngine), workspace.path(), vowp_dir.path());

    provider.start(&global_input()).await.unwrap();
    let requests = server.requests();
    let list = requests
        .iter()
        .find(|request| request.method == "GET" && request.path == "/api/session")
        .expect("the discovery list");
    assert_eq!(
        list.headers.get("x-opencode-directory").map(String::as_str),
        Some(WORKSPACE),
        "a reported workspace must be sent on the discovery request"
    );
    assert!(
        requests
            .iter()
            .any(|request| request.method == "GET"
                && request.path == "/session/ses_orchestrator_fake"),
        "only the session in the orchestrator workspace may be chosen"
    );
    assert!(
        !requests
            .iter()
            .any(|request| request.method == "GET" && request.path == "/session/ses_other_fake"),
        "a session in another workspace must be ignored"
    );
    assert!(no_session_creates(&requests));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn empty_or_malformed_session_lists_are_chat_not_configured() {
    let workspace = tempfile::tempdir().unwrap();
    for (label, list, body, status) in [
        ("empty", Some(Vec::new()), None, None),
        (
            "not an array",
            None,
            Some(json!({"data": "not-a-list"})),
            None,
        ),
        ("no data", None, Some(json!({"sessions": []})), None),
        (
            "entry without an id",
            Some(vec![
                json!({"time": {"updated": 1000}, "location": {"directory": WORKSPACE}}),
            ]),
            None,
            None,
        ),
        (
            "entry without updated",
            Some(vec![
                json!({"id": "ses_missing_time", "location": {"directory": WORKSPACE}}),
            ]),
            None,
            None,
        ),
        ("server refused the list", None, None, Some(500)),
    ] {
        let server = FakeServer::new(None);
        if let Some(list) = list {
            server.set_list(list);
        }
        if let Some(body) = body {
            server.set_list_body(body);
        }
        if let Some(status) = status {
            server.set_list_status(status);
        }
        let vowp_dir = tempfile::tempdir().unwrap();
        let url = format!("http://127.0.0.1:{}", server.port());
        let document = endpoint_document(
            "global",
            "opencode",
            "running",
            Some(&url),
            None,
            HOME_SESSION,
        );
        write_endpoint_vowp(vowp_dir.path(), &document);
        let provider = provider_with_vowp(Arc::new(PanicEngine), workspace.path(), vowp_dir.path());
        let error = provider.start(&global_input()).await.unwrap_err();
        assert_eq!(error.code, SpiritErrorCode::Gate, "{label}");
        assert!(
            error.message.contains("chat_not_configured"),
            "{label}: {error}"
        );
        assert_safe_error(
            &error,
            &[
                HOME_SESSION,
                "ses_missing_time",
                WORKSPACE,
                &server.port().to_string(),
            ],
        );
        assert!(
            no_session_creates(&server.requests()),
            "{label}: a bad list must never create a session"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_or_mixed_workspaces_are_chat_not_configured() {
    let credential = secret();
    let expected_auth = format!(
        "Basic {}",
        base64(format!("opencode:{credential}").as_bytes())
    );
    let workspace = tempfile::tempdir().unwrap();
    for (label, list) in [
        (
            "missing",
            vec![json!({
                "id": "ses_no_workspace",
                "time": {"created": 1000, "updated": 1000},
            })],
        ),
        (
            "mixed",
            vec![
                session_entry("ses_workspace_a", 1000, Some(WORKSPACE)),
                session_entry("ses_workspace_b", 2000, Some(OTHER_WORKSPACE)),
            ],
        ),
    ] {
        let server = FakeServer::new(Some(expected_auth.clone()));
        server.set_list(list);
        let vowp_dir = tempfile::tempdir().unwrap();
        let url = format!("http://127.0.0.1:{}", server.port());
        let document = with_auth(
            endpoint_document(
                "global",
                "opencode",
                "running",
                Some(&url),
                None,
                HOME_SESSION,
            ),
            &credential,
        );
        write_endpoint_vowp(vowp_dir.path(), &document);
        let provider = provider_with_vowp(Arc::new(PanicEngine), workspace.path(), vowp_dir.path());
        let error = provider.start(&global_input()).await.unwrap_err();
        assert_eq!(error.code, SpiritErrorCode::Gate, "{label}");
        assert!(
            error.message.contains("chat_not_configured"),
            "{label}: {error}"
        );
        assert_safe_error(
            &error,
            &[
                &credential,
                HOME_SESSION,
                "ses_no_workspace",
                "ses_workspace_a",
                "ses_workspace_b",
                WORKSPACE,
                OTHER_WORKSPACE,
                &server.port().to_string(),
            ],
        );
        assert!(
            no_session_creates(&server.requests()),
            "{label}: no session may be created"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rejected_discovery_credential_is_a_safe_chat_auth_error() {
    let credential = secret();
    let expected_auth = format!(
        "Basic {}",
        base64("opencode:a-different-credential".as_bytes())
    );
    let server = FakeServer::new(Some(expected_auth));
    let workspace = tempfile::tempdir().unwrap();
    let vowp_dir = tempfile::tempdir().unwrap();
    let url = format!("http://127.0.0.1:{}", server.port());
    let document = with_auth(
        endpoint_document(
            "global",
            "opencode",
            "running",
            Some(&url),
            None,
            HOME_SESSION,
        ),
        &credential,
    );
    write_endpoint_vowp(vowp_dir.path(), &document);
    let provider = provider_with_vowp(Arc::new(PanicEngine), workspace.path(), vowp_dir.path());

    let error = provider.start(&global_input()).await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Auth);
    assert!(error.message.contains("chat_auth"));
    assert_safe_error(
        &error,
        &[
            &credential,
            HOME_SESSION,
            WORKSPACE,
            &server.port().to_string(),
        ],
    );
    assert!(no_session_creates(&server.requests()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unreachable_discovery_endpoint_is_chat_not_configured() {
    let workspace = tempfile::tempdir().unwrap();
    let vowp_dir = tempfile::tempdir().unwrap();
    let document = endpoint_document(
        "global",
        "opencode",
        "running",
        Some("http://127.0.0.1:1"),
        None,
        HOME_SESSION,
    );
    write_endpoint_vowp(vowp_dir.path(), &document);
    let provider = provider_with_vowp(Arc::new(PanicEngine), workspace.path(), vowp_dir.path());
    let error = provider.start(&global_input()).await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Gate);
    assert!(error.message.contains("chat_not_configured"));
    assert_safe_error(&error, &[HOME_SESSION, "127.0.0.1:1"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_different_home_session_is_chat_not_mirrored() {
    let vowp_dir = tempfile::tempdir().unwrap();
    let document = endpoint_document(
        "global",
        "opencode",
        "running",
        Some("http://127.0.0.1:1"),
        Some(NATIVE),
        HOME_SESSION,
    );
    write_endpoint_vowp(vowp_dir.path(), &document);
    let resolver = VowpEndpointResolver::with_path_env(path_with(vowp_dir.path()));
    let error = resolver
        .resolve("global", Some("01ARZ3NDEKTSV4RRFFQ69G5FAW"))
        .await
        .unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Gate);
    assert!(error.message.contains("chat_not_mirrored"));
    assert_safe_error(
        &error,
        &[NATIVE, HOME_SESSION, "01ARZ3NDEKTSV4RRFFQ69G5FAW"],
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn command_failure_and_the_installed_040_shape_are_chat_not_configured() {
    let workspace = tempfile::tempdir().unwrap();

    for (label, script) in [
        (
            "unrecognized subcommand",
            "printf '%s\\n' 'error: unrecognized subcommand orchestrator' >&2\nexit 2",
        ),
        (
            "not enrolled",
            "printf '%s\\n' 'error: the local agent is not enrolled' >&2\nexit 3",
        ),
        (
            "command failed",
            "printf '%s\\n' 'error: the local agent failed' >&2\nexit 1",
        ),
    ] {
        let vowp_dir = tempfile::tempdir().unwrap();
        write_vowp(vowp_dir.path(), script);
        let provider = provider_with_vowp(Arc::new(PanicEngine), workspace.path(), vowp_dir.path());
        let error = provider.start(&global_input()).await.unwrap_err();
        assert_eq!(error.code, SpiritErrorCode::Gate, "{label}");
        assert!(error.message.contains("chat_not_configured"), "{label}");
        assert!(!error.message.contains("unrecognized subcommand"));
        assert!(!error.message.contains("not enrolled"));
        assert_safe_error(&error, &[]);
    }

    // A missing executable takes the same typed path.
    let empty = tempfile::tempdir().unwrap();
    let provider = provider_with_vowp(Arc::new(PanicEngine), workspace.path(), empty.path());
    let error = provider.start(&global_input()).await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Gate);
    assert!(error.message.contains("chat_not_configured"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_resolver_child_environment_has_no_inherited_secrets() {
    let vowp_dir = tempfile::tempdir().unwrap();
    let env_out = vowp_dir.path().join("child-env.txt");
    let args_out = vowp_dir.path().join("child-args.txt");
    write_vowp(
        vowp_dir.path(),
        &format!(
            "printf '%s\\n' \"$@\" > '{}'\nenv | sort > '{}'\nprintf '%s\\n' '{{\"name\":\"global\",\"app\":\"opencode\",\"session_id\":\"{HOME_SESSION}\",\"location\":\"node\",\"node\":null,\"state\":\"running\",\"url\":\"http://127.0.0.1:1\",\"native_session_id\":\"{NATIVE}\",\"auth\":null}}'",
            args_out.display(),
            env_out.display()
        ),
    );
    let resolver = VowpEndpointResolver::with_path_env(path_with(vowp_dir.path()));
    let resolved = resolver.resolve("global", None).await.unwrap();
    let debug = format!("{resolved:?}");
    assert!(!debug.contains("127.0.0.1"));
    assert!(!debug.contains(NATIVE));
    assert!(!debug.contains(HOME_SESSION));

    let args = std::fs::read_to_string(&args_out).expect("the fake vowp argv");
    let argv: Vec<&str> = args.lines().collect();
    assert_eq!(
        argv,
        vec!["agent", "orchestrator", "endpoint", "global", "--json"],
        "the resolver must use argv only"
    );

    let environment = std::fs::read_to_string(&env_out).expect("the fake vowp env dump");
    assert!(
        environment.contains("PATH="),
        "the child needs PATH to find VOWP"
    );
    assert!(
        environment.contains("HOME="),
        "the child needs HOME to find the agent state"
    );
    for inherited in [
        "CARGO_MANIFEST_DIR=",
        "OPENCODE_AUTH_CONTENT=",
        "OPENAI_API_KEY=",
        "ANTHROPIC_API_KEY=",
        "VOWP_TOKEN=",
    ] {
        assert!(
            !environment.contains(inherited),
            "the child inherited a non-allow-listed variable: {inherited}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_and_setup_server_gates_are_unchanged() {
    let workspace = tempfile::tempdir().unwrap();
    let server = FakeServer::new(None);
    let engine = Arc::new(SetupEngine {
        port: server.port(),
    });
    let provider = LocalChatProvider::new(engine, workspace.path().to_path_buf());

    let server_global = ChatStartInput {
        session: ChatSessionId::Global,
        project: None,
        location: ChatSessionLocation::Server,
        resume: None,
    };
    let error = provider.start(&server_global).await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Gate);
    assert!(error.message.contains("chat_gated"));

    let setup_server = ChatStartInput {
        session: ChatSessionId::Setup,
        project: None,
        location: ChatSessionLocation::Server,
        resume: None,
    };
    let error = provider.start(&setup_server).await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Gate);
    assert!(error.message.contains("chat_gated"));
}
