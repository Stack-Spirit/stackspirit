//! Hermetic tests for the setup assistant's free-model default and usage projection.
//!
//! A fake OpenCode v1 server runs on loopback. It reports only the models a test
//! supplies; the engine adapter is a fake, so no sidecar, provider key or real turn is
//! touched.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde_json::{Value, json};

use spirit_core::engine::chat::{
    ChatEngine, ChatEvent, ChatModelTier, ChatModelView, ChatSessionId, ChatSessionLocation,
    ChatStartInput, LocalChatProvider,
};
use spirit_core::error::{SpiritError, SpiritErrorCode};

const SETUP: ChatSessionId = ChatSessionId::Setup;

fn setup_input() -> ChatStartInput {
    ChatStartInput {
        session: SETUP,
        project: None,
        location: ChatSessionLocation::Local,
        resume: None,
    }
}

fn model(id: &str, label: &str, tier: ChatModelTier, context_tokens: Option<u64>) -> ChatModelView {
    ChatModelView {
        id: id.to_owned(),
        label: label.to_owned(),
        provider: id
            .split_once('/')
            .map_or("opencode", |(provider, _)| provider)
            .to_owned(),
        tier,
        context_tokens,
    }
}

fn included(id: &str) -> ChatModelView {
    model(id, id, ChatModelTier::Included, Some(128_000))
}

#[derive(Debug, Default)]
struct EngineState {
    models: Vec<ChatModelView>,
    selected: Option<ChatModelView>,
    selections: Vec<(String, bool)>,
}

struct FakeChatEngine {
    port: u16,
    state: Mutex<EngineState>,
    persist_path: Option<PathBuf>,
}

impl FakeChatEngine {
    fn new(port: u16, models: Vec<ChatModelView>) -> Self {
        Self {
            port,
            state: Mutex::new(EngineState {
                models,
                ..EngineState::default()
            }),
            persist_path: None,
        }
    }

    fn persisting(port: u16, models: Vec<ChatModelView>, path: PathBuf) -> Self {
        Self {
            persist_path: Some(path),
            ..Self::new(port, models)
        }
    }

    fn selections(&self) -> Vec<(String, bool)> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .selections
            .clone()
    }

    fn selected_id(&self) -> Option<String> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .selected
            .as_ref()
            .map(|model| model.id.clone())
    }
}

#[async_trait]
impl ChatEngine for FakeChatEngine {
    async fn ensure_ready(&self) -> Result<u16, SpiritError> {
        Ok(self.port)
    }

    fn selected_model(&self) -> Option<ChatModelView> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .selected
            .clone()
    }

    fn available_models(&self) -> Vec<ChatModelView> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .models
            .clone()
    }

    async fn select_model(
        &self,
        model_id: &str,
        persist: bool,
    ) -> Result<ChatModelView, SpiritError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let selected = state
            .models
            .iter()
            .find(|model| model.id == model_id)
            .cloned()
            .ok_or_else(|| {
                SpiritError::new(
                    SpiritErrorCode::NotFound,
                    "that model is not available in this engine build",
                )
            })?;
        state.selected = Some(selected.clone());
        state.selections.push((model_id.to_owned(), persist));
        if persist && let Some(path) = &self.persist_path {
            std::fs::write(path, model_id).unwrap();
        }
        Ok(selected)
    }
}

struct ServerState {
    log: Vec<Value>,
    assistant: Value,
    message_posts: usize,
    last_body: Option<Value>,
}

struct FakeServer {
    port: u16,
    state: Arc<Mutex<ServerState>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl FakeServer {
    fn new(log: Vec<Value>, assistant: Value) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind fake engine");
        listener
            .set_nonblocking(true)
            .expect("set fake listener nonblocking");
        let port = listener.local_addr().expect("fake local addr").port();
        let state = Arc::new(Mutex::new(ServerState {
            log,
            assistant,
            message_posts: 0,
            last_body: None,
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

    fn message_posts(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .message_posts
    }

    fn last_body(&self) -> Option<Value> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .last_body
            .clone()
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

struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: String,
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
    let mut parts = lines.next()?.split_whitespace();
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
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn handle_connection(mut stream: TcpStream, state: Arc<Mutex<ServerState>>) {
    let Some(request) = read_request(&mut stream) else {
        return;
    };
    let path = request.path.split('?').next().unwrap_or(&request.path);
    if request.headers.contains_key("x-opencode-directory") {
        // The setup workspace header is expected; its value is never asserted or printed.
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
            write_json(&mut stream, 200, &json!({"id": "ses_model_test_1"}));
        }
        ("GET", "/event") => write_json(&mut stream, 404, &json!({})),
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
    if let Some(_id) = rest.strip_suffix("/message") {
        if method == "POST" {
            handle_message_post(stream, state, body);
        } else {
            let log = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .log
                .clone();
            write_json(stream, 200, &Value::Array(log));
        }
        return;
    }
    if method == "GET" {
        write_json(
            stream,
            200,
            &json!({"id": rest, "title": "StackSpirit setup"}),
        );
    } else {
        write_json(stream, 405, &json!({}));
    }
}

fn handle_message_post(stream: &mut TcpStream, state: &Arc<Mutex<ServerState>>, body: &str) {
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
    let assistant = {
        let mut state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.message_posts = state.message_posts.saturating_add(1);
        state.last_body = Some(request);
        state.log.push(json!({
            "info": {
                "id": user_id,
                "role": "user",
                "sessionID": "ses_model_test_1",
                "time": {"created": now_ms()},
            },
            "parts": [{"type": "text", "text": text}],
        }));
        let mut assistant = state.assistant.clone();
        assistant["sessionID"] = json!("ses_model_test_1");
        assistant["time"] = json!({"created": now_ms()});
        state.log.push(json!({
            "info": assistant.clone(),
            "parts": [{"type": "text", "text": "Hello world"}],
        }));
        assistant
    };
    write_json(
        stream,
        200,
        &json!({
            "info": assistant,
            "parts": [{"type": "text", "text": "Hello world"}],
        }),
    );
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

fn provider(
    server: &FakeServer,
    models: Vec<ChatModelView>,
    workspace: &Path,
) -> (LocalChatProvider, Arc<FakeChatEngine>) {
    let engine = Arc::new(FakeChatEngine::new(server.port(), models));
    (
        LocalChatProvider::new(engine.clone(), workspace.to_path_buf()),
        engine,
    )
}

fn assistant(tokens: Value, cost: Option<f64>) -> Value {
    let mut info = json!({
        "id": "msg_assistant",
        "role": "assistant",
        "sessionID": "ses_model_test_1",
        "time": {"created": now_ms()},
    });
    if !tokens.is_null() {
        info["tokens"] = tokens;
    }
    if let Some(cost) = cost {
        info["cost"] = json!(cost);
    }
    info
}

fn log_entry(role: &str, id: &str, text: &str, info_extra: Value) -> Value {
    let mut info = json!({
        "id": id,
        "role": role,
        "sessionID": "ses_model_test_1",
        "time": {"created": now_ms()},
    });
    if let (Some(target), Some(extra)) = (info.as_object_mut(), info_extra.as_object()) {
        for (key, value) in extra {
            target.insert(key.clone(), value.clone());
        }
    }
    json!({
        "info": info,
        "parts": [{"type": "text", "text": text}],
    })
}

async fn wait_for_done(
    receiver: &mut tokio::sync::broadcast::Receiver<ChatEvent>,
    timeout: Duration,
) {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(!remaining.is_zero(), "the turn never finished");
        match tokio::time::timeout(remaining, receiver.recv()).await {
            Ok(Ok(ChatEvent::Done { .. })) => return,
            Ok(Ok(_)) => {}
            Ok(Err(error)) => panic!("chat event bus closed: {error}"),
            Err(_) => panic!("the turn never finished"),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unsaved_setup_defaults_to_the_first_verified_free_model() {
    let server = FakeServer::new(
        Vec::new(),
        assistant(json!({"input": 10, "output": 5}), Some(0.0)),
    );
    let workspace = tempfile::tempdir().unwrap();
    // The BYOK entry comes first to prove the default prefers an included free model.
    let byok = model(
        "deepseek/deepseek-flash",
        "DeepSeek V4.1 Flash",
        ChatModelTier::Byok,
        Some(64_000),
    );
    let free = included("opencode/deepseek-v4.1-flash-free");
    let (provider, engine) = provider(&server, vec![byok, free.clone()], workspace.path());

    let view = provider.start(&setup_input()).await.unwrap();
    assert_eq!(
        view.state,
        spirit_core::engine::chat::ChatSessionState::Ready
    );
    let selected = view.model.expect("the setup assistant must have a model");
    assert_eq!(selected.id, free.id);
    assert_eq!(selected.tier, ChatModelTier::Included);
    assert_eq!(selected.context_tokens, Some(128_000));
    assert_eq!(view.available_models.as_ref().map(Vec::len), Some(2));

    let mut receiver = provider.subscribe();
    let receipt = provider.send(&SETUP, "Hello?").await.unwrap();
    assert!(receipt.accepted);
    wait_for_done(&mut receiver, Duration::from_secs(3)).await;
    assert_eq!(server.message_posts(), 1);

    let body = server.last_body().expect("the message request body");
    assert_eq!(body["model"]["providerID"], "opencode");
    assert_eq!(body["model"]["modelID"], "deepseek-v4.1-flash-free");
    assert_eq!(
        engine.selections(),
        vec![(free.id, false)],
        "the automatic default is an in-memory choice, not a saved one"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn zero_models_stay_honestly_locked_without_a_fake_free_model() {
    let server = FakeServer::new(
        Vec::new(),
        assistant(json!({"input": 10, "output": 5}), Some(0.0)),
    );
    let workspace = tempfile::tempdir().unwrap();
    let (provider, engine) = provider(&server, Vec::new(), workspace.path());

    let view = provider.start(&setup_input()).await.unwrap();
    assert_eq!(
        view.state,
        spirit_core::engine::chat::ChatSessionState::Ready
    );
    assert!(view.model.is_none());
    assert_eq!(view.available_models.as_ref().map(Vec::len), Some(0));

    let error = provider.send(&SETUP, "Hello?").await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Engine);
    assert!(error.message.contains("no_model"));
    assert!(!error.message.contains("opencode/"));
    assert_eq!(server.message_posts(), 0);
    assert!(engine.selections().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn set_model_switches_and_persists_an_explicit_choice() {
    let server = FakeServer::new(
        Vec::new(),
        assistant(json!({"input": 10, "output": 5}), Some(0.0)),
    );
    let workspace = tempfile::tempdir().unwrap();
    let preferences = workspace.path().join("engine.json");
    let first = included("opencode/big-pickle");
    let second = included("opencode/iceman-free");
    let engine = Arc::new(FakeChatEngine::persisting(
        server.port(),
        vec![first.clone(), second.clone()],
        preferences.clone(),
    ));
    let provider = LocalChatProvider::new(engine.clone(), workspace.path().to_path_buf());

    let view = provider.start(&setup_input()).await.unwrap();
    assert_eq!(view.model.map(|model| model.id), Some(first.id.clone()));

    let switched = provider.set_model(&SETUP, &second.id).await.unwrap();
    assert_eq!(
        switched.model.map(|model| model.id),
        Some(second.id.clone())
    );
    assert_eq!(engine.selected_id(), Some(second.id.clone()));
    assert_eq!(
        engine.selections().last(),
        Some(&(second.id.clone(), true)),
        "an explicit switch persists"
    );
    assert_eq!(std::fs::read_to_string(&preferences).unwrap(), second.id);

    let missing = provider
        .set_model(&SETUP, "opencode/not-reported")
        .await
        .unwrap_err();
    assert_eq!(missing.code, SpiritErrorCode::NotFound);

    let gated = provider
        .set_model(&ChatSessionId::Global, &second.id)
        .await
        .unwrap_err();
    assert_eq!(gated.code, SpiritErrorCode::Conflict);
    assert!(gated.message.contains("chat_model_gated"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn usage_comes_from_message_metadata_and_missing_fields_stay_null() {
    let tokens = json!({
        "input": 1200,
        "output": 300,
        "cache": {"read": 800, "write": 100},
    });
    let log = vec![
        log_entry("user", "msg_user", "Hello?", Value::Null),
        log_entry(
            "assistant",
            "msg_assistant",
            "Hello world",
            json!({"tokens": tokens, "cost": 0.0015}),
        ),
    ];
    let server = FakeServer::new(
        log,
        assistant(
            json!({
                "input": 1200,
                "output": 300,
                "cache": {"read": 800, "write": 100},
            }),
            Some(0.0015),
        ),
    );
    let workspace = tempfile::tempdir().unwrap();
    let (provider, _engine) = provider(
        &server,
        vec![included("opencode/deepseek-v4.1-flash-free")],
        workspace.path(),
    );
    let mut receiver = provider.subscribe();
    let view = provider.start(&setup_input()).await.unwrap();
    let usage = view.usage.expect("reported usage");
    assert_eq!(usage.context_tokens_used, Some(2_400));
    assert_eq!(usage.context_limit, Some(128_000));
    assert_eq!(usage.input_tokens, Some(1_200));
    assert_eq!(usage.output_tokens, Some(300));
    assert_eq!(usage.cache_read_tokens, Some(800));
    assert_eq!(usage.turns, 1);
    assert_eq!(usage.cost_usd, Some(0.0015));

    provider.send(&SETUP, "Again").await.unwrap();
    wait_for_done(&mut receiver, Duration::from_secs(3)).await;
    let updated = provider.attach(&SETUP).await.unwrap().usage.unwrap();
    assert_eq!(updated.input_tokens, Some(2_400));
    assert_eq!(updated.output_tokens, Some(600));
    assert_eq!(updated.cache_read_tokens, Some(1_600));
    assert_eq!(updated.turns, 2);
    assert_eq!(updated.cost_usd, Some(0.003));
    assert_eq!(updated.context_tokens_used, Some(2_400));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_usage_fields_are_null_not_zero() {
    let log = vec![
        log_entry("user", "msg_user", "Hello?", Value::Null),
        // Only the input count is reported; output, cache and cost stay unknown.
        log_entry(
            "assistant",
            "msg_assistant",
            "Hello world",
            json!({"tokens": {"input": 7}}),
        ),
    ];
    let server = FakeServer::new(log, assistant(json!({"input": 7}), None));
    let workspace = tempfile::tempdir().unwrap();
    let (provider, _engine) = provider(
        &server,
        vec![included("opencode/deepseek-v4.1-flash-free")],
        workspace.path(),
    );
    let view = provider.start(&setup_input()).await.unwrap();
    let usage = view.usage.expect("reported usage");
    assert_eq!(usage.input_tokens, Some(7));
    assert_eq!(usage.output_tokens, None);
    assert_eq!(usage.cache_read_tokens, None);
    assert_eq!(usage.context_tokens_used, Some(7));
    assert_eq!(usage.cost_usd, None);
    assert_eq!(usage.turns, 1);

    let body = serde_json::to_value(usage).unwrap();
    assert!(body["output_tokens"].is_null());
    assert!(body["cache_read_tokens"].is_null());
    assert!(body["cost_usd"].is_null());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn step_finish_parts_are_used_when_the_message_omits_tokens() {
    let log = vec![
        log_entry("user", "msg_user", "Hello?", Value::Null),
        json!({
            "info": {
                "id": "msg_assistant",
                "role": "assistant",
                "sessionID": "ses_model_test_1",
                "time": {"created": now_ms()},
            },
            "parts": [
                {"type": "text", "text": "Hello world"},
                {
                    "type": "step-finish",
                    "cost": 0.002,
                    "tokens": {
                        "input": 20,
                        "output": 8,
                        "cache": {"read": 200, "write": 0},
                    },
                },
            ],
        }),
    ];
    let server = FakeServer::new(
        log,
        assistant(json!({"input": 20, "output": 8}), Some(0.002)),
    );
    let workspace = tempfile::tempdir().unwrap();
    let (provider, _engine) = provider(
        &server,
        vec![included("opencode/deepseek-v4.1-flash-free")],
        workspace.path(),
    );
    let view = provider.start(&setup_input()).await.unwrap();
    let usage = view.usage.expect("step-finish usage");
    assert_eq!(usage.input_tokens, Some(20));
    assert_eq!(usage.output_tokens, Some(8));
    assert_eq!(usage.cache_read_tokens, Some(200));
    assert_eq!(usage.context_tokens_used, Some(228));
    assert_eq!(usage.cost_usd, Some(0.002));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_live_turn_uses_a_msg_id_and_parses_the_reported_total() {
    // Exact token shape from a real bundled OpenCode turn. `total` includes the
    // reasoning tokens, so the context occupancy must use it rather than re-summing.
    let tokens = json!({
        "total": 12260,
        "input": 12247,
        "output": 3,
        "reasoning": 10,
        "cache": {"read": 0, "write": 0},
    });
    let server = FakeServer::new(Vec::new(), assistant(tokens, Some(0.0)));
    let workspace = tempfile::tempdir().unwrap();
    let (provider, _engine) = provider(
        &server,
        vec![included("opencode/deepseek-v4.1-flash-free")],
        workspace.path(),
    );
    let mut receiver = provider.subscribe();
    provider.start(&setup_input()).await.unwrap();

    let receipt = provider.send(&SETUP, "Hello?").await.unwrap();
    assert!(receipt.message.id.starts_with("msg_"));
    wait_for_done(&mut receiver, Duration::from_secs(3)).await;

    let body = server.last_body().expect("the message request body");
    assert_eq!(body["messageID"], receipt.message.id);
    assert!(
        body["messageID"]
            .as_str()
            .is_some_and(|id| id.starts_with("msg")),
        "the fake OpenCode server rejects ids without the msg prefix"
    );

    let usage = provider
        .attach(&SETUP)
        .await
        .unwrap()
        .usage
        .expect("live turn usage");
    assert_eq!(usage.context_tokens_used, Some(12_260));
    assert_eq!(usage.input_tokens, Some(12_247));
    assert_eq!(usage.output_tokens, Some(3));
    assert_eq!(usage.cache_read_tokens, Some(0));
    assert_eq!(usage.turns, 1);
    assert_eq!(usage.cost_usd, Some(0.0));
}
