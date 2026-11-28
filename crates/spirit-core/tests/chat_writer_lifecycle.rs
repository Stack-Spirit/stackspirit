//! Hermetic lifecycle tests for `docs/SESSION-TAKEOVER.md` Phase 4.
//!
//! Every test runs the checked-in fake `vowp` from `tests/fixtures/fake-vowp/` through
//! a private temporary `PATH` and `VOWP_AGENT_HOME`. No test touches a real VOWP home,
//! `~/.vowp`, `~/.vowp-agent`, the keychain, the network or a real engine. The loopback
//! "engine" is a local fake server on a private port.
#![cfg(unix)]

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde_json::{Value, json};

use spirit_core::engine::chat::{
    ChatAccess, ChatEngine, ChatEvent, ChatSessionId, ChatSessionLocation, ChatSessionView,
    ChatStartInput, ChatWriterConfig, ChatWriterReason, ChatWriterState, LocalChatProvider,
    VowpEndpointResolver,
};
use spirit_core::error::{SpiritError, SpiritErrorCode};

const HOME_SESSION: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const NATIVE: &str = "ses_fake_native";
const ENGINE_CREDENTIAL: &str = "fake-engine-credential";

static CANARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn canary() -> String {
    let sequence = CANARY_SEQUENCE.fetch_add(1, Ordering::SeqCst);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!(
        "fake-lease-canary-{}-{sequence}-{nanos}",
        std::process::id()
    )
}

/// Inherited `PATH` directories minus any that hold a host `stackspirit` or `vowp`, so a
/// host install cannot shadow the fake binary under test.
fn host_path_without_engines() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|existing| {
            std::env::split_paths(&existing)
                .filter(|dir| !dir.join("stackspirit").exists() && !dir.join("vowp").exists())
                .collect()
        })
        .unwrap_or_default()
}

fn path_with(dir: &Path) -> std::ffi::OsString {
    let mut paths = vec![dir.to_path_buf()];
    paths.extend(host_path_without_engines());
    std::env::join_paths(paths).expect("join fake PATH")
}

async fn wait_until(mut predicate: impl FnMut() -> bool, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if predicate() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    predicate()
}

async fn wait_for_writer_state(
    provider: &LocalChatProvider,
    session: &ChatSessionId,
    expected: ChatWriterState,
    timeout: Duration,
) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if provider
            .writer_view(session)
            .await
            .is_some_and(|view| view.state == expected)
        {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

struct FakeVowp {
    /// Keeps the private temporary tree alive for the whole test.
    #[allow(dead_code)]
    dir: tempfile::TempDir,
    bin: PathBuf,
    state: PathBuf,
    canary: String,
}

impl FakeVowp {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("fake vowp temp dir");
        let bin = dir.path().join("bin");
        let state = dir.path().join("state");
        std::fs::create_dir_all(&bin).expect("fake bin dir");
        std::fs::create_dir_all(&state).expect("fake state dir");
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-vowp/vowp");
        let binary = bin.join("vowp");
        std::fs::copy(&source, &binary).expect("copy fake vowp");
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake vowp");
        let canary = canary();
        let fake = Self {
            dir,
            bin,
            state,
            canary,
        };
        fake.write("mode", "attach-grant");
        fake.write("canary", &fake.canary);
        fake.write("session", HOME_SESSION);
        fake.write("native", NATIVE);
        fake.write("node", "fake-node");
        fake
    }

    fn write(&self, name: &str, value: &str) {
        std::fs::write(self.state.join(name), value).expect("write fake state");
    }

    fn set_mode(&self, mode: &str) {
        self.write("mode", mode);
    }

    fn set_url(&self, url: &str) {
        self.write("url", url);
    }

    fn path(&self) -> std::ffi::OsString {
        path_with(&self.bin)
    }

    fn config(&self) -> ChatWriterConfig {
        ChatWriterConfig::new()
            .with_path_env(self.path())
            .with_agent_home(self.state.clone())
            .with_home_env(self.state.clone())
            .with_probe_timeout(Duration::from_secs(2))
            .with_attach_timeout(Duration::from_secs(2))
            .with_release_timeout(Duration::from_secs(2))
            .with_attach_drain_timeout(Duration::from_millis(300))
            .with_watch_interval(Duration::from_millis(25))
            .with_watch_retry_delay(Duration::from_millis(20))
    }

    fn resolver(&self) -> VowpEndpointResolver {
        VowpEndpointResolver::with_path_env(self.path())
            .with_agent_home(self.state.clone())
            .with_home_env(self.state.clone())
    }

    fn attach_count(&self) -> u32 {
        std::fs::read_to_string(self.state.join("attach-count"))
            .ok()
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(0)
    }

    fn surface_ids(&self) -> Vec<String> {
        std::fs::read_to_string(self.state.join("surface-ids.log"))
            .map(|text| text.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    fn released(&self) -> bool {
        self.state.join("released").exists()
    }

    fn stdin_open(&self) -> bool {
        self.state.join("stdin-open").exists()
    }

    fn pid(&self) -> Option<u32> {
        std::fs::read_to_string(self.state.join("attach-pid"))
            .ok()
            .and_then(|value| value.trim().parse().ok())
    }

    /// Asserts the canary token appears in no app-visible file.
    ///
    /// `canary` is the test's planted input, so it is the one state file excluded.
    fn assert_no_token_leak(&self, app_dir: &Path) {
        for path in files_under(app_dir) {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            assert!(
                !text.contains(&self.canary),
                "the canary token leaked into {}",
                path.display()
            );
        }
        for path in files_under(&self.state) {
            if path.file_name().is_some_and(|name| name == "canary") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            assert!(
                !text.contains(&self.canary),
                "the canary token leaked into {}",
                path.display()
            );
        }
    }
}

fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(files_under(&path));
        } else {
            files.push(path);
        }
    }
    files
}

struct PanicEngine;

#[async_trait]
impl ChatEngine for PanicEngine {
    async fn ensure_ready(&self) -> Result<u16, SpiritError> {
        panic!("a global/project writer test must never start the bundled engine");
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
    requests: Vec<RequestRecord>,
    assistant_text: String,
    assistant_delay: Duration,
    append_assistant: bool,
}

struct FakeEngine {
    port: u16,
    state: Arc<Mutex<ServerState>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl FakeEngine {
    fn new(expected_auth: Option<String>) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind fake engine");
        listener
            .set_nonblocking(true)
            .expect("nonblocking fake engine");
        let port = listener.local_addr().expect("fake engine addr").port();
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
            requests: Vec::new(),
            assistant_text: "LATE ANSWER".to_owned(),
            assistant_delay: Duration::ZERO,
            append_assistant: false,
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

    fn set_assistant_delay(&self, delay: Duration) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .assistant_delay = delay;
    }

    /// Adds an authoritative assistant message to the native log, as the home engine
    /// would after a drained or continued turn.
    fn add_assistant(&self, text: &str) {
        if let Some(log) = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .sessions
            .get_mut(NATIVE)
        {
            log.push(json!({
                "info": {"id": format!("msg_home_{}", now_ms()), "role": "assistant",
                         "sessionID": NATIVE, "time": {"created": now_ms()}},
                "parts": [{"type": "text", "text": text}],
            }));
        }
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
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .requests
        .push(request.clone());
    match (request.method.as_str(), path) {
        ("GET", "/global/health") => {
            write_json(
                &mut stream,
                200,
                &json!({"healthy": true, "version": "1.19.5"}),
            );
        }
        ("GET", "/session") => {
            write_json(&mut stream, 200, &json!({"data": []}));
        }
        ("GET", "/api/session") => {
            write_json(&mut stream, 200, &json!({"data": []}));
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
                    &json!({"name": "BadRequest", "data": {"message": "missing messageID"}}),
                );
                return;
            };
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
            let (exists, assistant_text, delay, append) = {
                let mut state = state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let exists = state.sessions.contains_key(id);
                if exists && let Some(log) = state.sessions.get_mut(id) {
                    log.push(json!({
                        "info": {"id": user_id, "role": "user", "sessionID": id,
                                 "time": {"created": now_ms()}},
                        "parts": [{"type": "text", "text": text}],
                    }));
                }
                (
                    exists,
                    state.assistant_text.clone(),
                    state.assistant_delay,
                    state.append_assistant,
                )
            };
            if !exists {
                write_json(stream, 404, &json!({}));
                return;
            }
            // A late response is deliberately not appended to the home log: the test
            // proves the app must ignore it and backfill the authoritative log instead.
            thread::sleep(delay);
            let assistant = json!({
                "info": {"id": "msg_assistant", "role": "assistant", "sessionID": id,
                         "time": {"created": now_ms()}},
                "parts": [{"type": "text", "text": assistant_text}],
            });
            if append
                && let Some(log) = state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .sessions
                    .get_mut(id)
            {
                log.push(assistant.clone());
            }
            write_json(stream, 200, &assistant);
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
        write_json(stream, 200, &json!({"id": rest, "title": "Chat"}));
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

fn expected_auth() -> String {
    format!(
        "Basic {}",
        base64(format!("opencode:{ENGINE_CREDENTIAL}").as_bytes())
    )
}

fn provider_with_writer(
    fake: &FakeVowp,
    engine: Arc<dyn ChatEngine>,
    workspace: &Path,
) -> LocalChatProvider {
    LocalChatProvider::new(engine, workspace.to_path_buf())
        .with_endpoint_resolver(Arc::new(fake.resolver()))
        .with_chat_writer(fake.config())
}

async fn wait_for_lost_session(
    events: &mut tokio::sync::broadcast::Receiver<ChatEvent>,
    session: &ChatSessionId,
    timeout: Duration,
) -> Option<ChatSessionView> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .ok()?
            .ok()?;
        if let ChatEvent::Session { session: view, .. } = event
            && view.session == session.clone()
            && view
                .writer
                .as_ref()
                .is_some_and(|writer| writer.state == ChatWriterState::Lost)
        {
            return Some(*view);
        }
    }
}

fn joined_log(page: &spirit_core::engine::chat::ChatLogPage) -> String {
    page.messages
        .iter()
        .flat_map(|message| message.text.iter())
        .cloned()
        .collect::<Vec<String>>()
        .join("\n")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn watcher_loss_flips_the_session_lost_and_releases_the_child() {
    let fake = FakeVowp::new();
    let engine = FakeEngine::new(Some(expected_auth()));
    fake.set_url(&format!("http://127.0.0.1:{}", engine.port()));
    let workspace = tempfile::tempdir().unwrap();
    let provider = provider_with_writer(&fake, Arc::new(PanicEngine), workspace.path());
    let mut events = provider.subscribe();

    let available = provider.start(&global_input()).await.unwrap();
    assert_eq!(available.access, ChatAccess::ReadOnly);
    let held = provider
        .take_over(&ChatSessionId::Global, false)
        .await
        .unwrap();
    assert_eq!(held.access, ChatAccess::Owner);
    assert_eq!(
        held.writer.expect("writer view").state,
        ChatWriterState::Held
    );
    assert!(wait_until(|| fake.stdin_open(), Duration::from_secs(1)).await);

    fake.set_mode("endpoint-other");
    let lost = wait_for_lost_session(&mut events, &ChatSessionId::Global, Duration::from_secs(3))
        .await
        .expect("the watcher must emit a lost session view");
    assert_eq!(lost.access, ChatAccess::ReadOnly);
    let holder = lost.ownership.and_then(|owner| owner.holder);
    assert_eq!(
        holder.as_deref(),
        Some("other-writer"),
        "the lost view must name the surface that replaced the lease"
    );
    let writer = lost.writer.expect("writer view");
    assert_eq!(writer.state, ChatWriterState::Lost);
    assert_eq!(writer.reason, Some(ChatWriterReason::LeaseLost));
    assert!(!writer.can_release);
    assert!(
        wait_until(
            || fake.released() || fake.pid().is_none(),
            Duration::from_secs(2)
        )
        .await,
        "a lost lease must reap the attach child"
    );
    assert_eq!(fake.attach_count(), 1, "the manager must not auto-retry");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_lost_lease_fences_the_turn_and_backfills_from_the_home_log() {
    let fake = FakeVowp::new();
    let engine = FakeEngine::new(Some(expected_auth()));
    fake.set_url(&format!("http://127.0.0.1:{}", engine.port()));
    engine.set_assistant_delay(Duration::from_millis(350));
    let workspace = tempfile::tempdir().unwrap();
    let app = tempfile::tempdir().unwrap();
    let provider = provider_with_writer(&fake, Arc::new(PanicEngine), workspace.path());

    provider.start(&global_input()).await.unwrap();
    provider
        .take_over(&ChatSessionId::Global, false)
        .await
        .unwrap();
    provider
        .send(&ChatSessionId::Global, "Hello?")
        .await
        .expect("the held writer accepts the turn");
    assert!(
        wait_until(
            || engine
                .requests()
                .iter()
                .any(|request| request.method == "POST" && request.path.ends_with("/message")),
            Duration::from_secs(2)
        )
        .await,
        "the turn must reach the grant endpoint"
    );
    // The home log gains an authoritative message while the direct POST is still in
    // flight. The late POST body says "LATE ANSWER" and is never appended by the engine.
    engine.add_assistant("Backfilled from the home");
    fake.set_mode("endpoint-other");
    assert!(
        wait_for_writer_state(
            &provider,
            &ChatSessionId::Global,
            ChatWriterState::Lost,
            Duration::from_secs(3)
        )
        .await,
        "the watcher must notice the replaced lease mid-turn"
    );
    // Let the delayed response arrive after the loss.
    tokio::time::sleep(Duration::from_millis(500)).await;

    let page = provider
        .log(&ChatSessionId::Global, &Default::default())
        .await
        .expect("the home log stays readable after the loss");
    let text = joined_log(&page);
    assert!(
        text.contains("Backfilled from the home"),
        "the authoritative home log must be backfilled: {text}"
    );
    assert!(
        !text.contains("LATE ANSWER"),
        "a late response from a lost lease must not be appended: {text}"
    );
    assert!(
        text.contains("Hello?"),
        "the home log keeps the user turn: {text}"
    );
    assert!(
        wait_until(
            || fake.released() || fake.pid().is_none(),
            Duration::from_secs(2)
        )
        .await,
        "the fenced turn must still release the child"
    );
    fake.assert_no_token_leak(app.path());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_engine_restart_re_resolves_the_grant_without_a_new_attach() {
    let fake = FakeVowp::new();
    let first = FakeEngine::new(Some(expected_auth()));
    fake.set_url(&format!("http://127.0.0.1:{}", first.port()));
    let workspace = tempfile::tempdir().unwrap();
    let provider = provider_with_writer(&fake, Arc::new(PanicEngine), workspace.path());

    provider.start(&global_input()).await.unwrap();
    let held = provider
        .take_over(&ChatSessionId::Global, false)
        .await
        .unwrap();
    assert_eq!(held.access, ChatAccess::Owner);
    assert_eq!(fake.attach_count(), 1);

    let second = FakeEngine::new(Some(expected_auth()));
    fake.set_url(&format!("http://127.0.0.1:{}", second.port()));
    let refreshed = provider
        .attach(&ChatSessionId::Global)
        .await
        .expect("a held grant survives the engine restart");
    assert_eq!(refreshed.access, ChatAccess::Owner);
    assert_eq!(
        refreshed.writer.expect("writer view").state,
        ChatWriterState::Held,
        "the same grant is still held across the restart"
    );

    provider
        .send(&ChatSessionId::Global, "after restart")
        .await
        .unwrap();
    assert!(
        wait_until(
            || second
                .requests()
                .iter()
                .any(|request| request.method == "POST" && request.path.ends_with("/message")),
            Duration::from_secs(2)
        )
        .await,
        "the re-resolved endpoint must serve the next turn"
    );
    assert!(
        !first
            .requests()
            .iter()
            .any(|request| request.method == "POST" && request.path.ends_with("/message")),
        "the dead port must not be used after re-resolution"
    );
    assert_eq!(
        fake.attach_count(),
        1,
        "an engine restart is not a new attach"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sleep_past_the_stale_window_is_lost_and_a_new_attach_uses_a_new_surface() {
    let fake = FakeVowp::new();
    let engine = FakeEngine::new(Some(expected_auth()));
    fake.set_url(&format!("http://127.0.0.1:{}", engine.port()));
    let workspace = tempfile::tempdir().unwrap();
    let app = tempfile::tempdir().unwrap();
    // Keep the periodic watcher out of this test so the focus/resume probe is the first
    // check after the simulated sleep; a watcher loss would legitimately re-probe the
    // grant back to `available` before the assertion.
    let provider = LocalChatProvider::new(Arc::new(PanicEngine), workspace.path().to_path_buf())
        .with_endpoint_resolver(Arc::new(fake.resolver()))
        .with_chat_writer(fake.config().with_watch_interval(Duration::from_secs(30)));

    provider.start(&global_input()).await.unwrap();
    provider
        .take_over(&ChatSessionId::Global, false)
        .await
        .unwrap();
    let first_surface = fake.surface_ids().pop().expect("the first grant surface");

    // Resume/focus after sleeping past the 60 s stale window: still ours, but stale.
    fake.write("owner-state", "stale");
    let lost = provider
        .probe_writer(&ChatSessionId::Global)
        .await
        .expect("the focus check reads the view");
    assert_eq!(lost.access, ChatAccess::ReadOnly);
    let writer = lost.writer.expect("writer view");
    assert_eq!(writer.state, ChatWriterState::Lost);
    assert_eq!(writer.reason, Some(ChatWriterReason::LeaseLost));
    let error = provider
        .send(&ChatSessionId::Global, "too late")
        .await
        .expect_err("a lost lease must refuse the pre-send check");
    assert_eq!(error.code, SpiritErrorCode::Refused);
    assert!(error.message.contains("chat_read_only"));
    assert!(
        wait_until(
            || fake.released() || fake.pid().is_none(),
            Duration::from_secs(2)
        )
        .await,
        "a stale lease must reap the old child"
    );

    let available = provider.attach(&ChatSessionId::Global).await.unwrap();
    assert_eq!(available.access, ChatAccess::ReadOnly);
    assert_eq!(
        available.writer.expect("writer view").state,
        ChatWriterState::Available,
        "the next takeover is an explicit new grant"
    );
    let held = provider
        .take_over(&ChatSessionId::Global, false)
        .await
        .unwrap();
    assert_eq!(held.access, ChatAccess::Owner);
    let surfaces = fake.surface_ids();
    assert_eq!(surfaces.len(), 2, "the reissue is one new attach");
    assert_ne!(
        surfaces[1], first_surface,
        "the old surface must not be reused"
    );
    assert_ne!(surfaces[0], surfaces[1], "a reissue gets a new surface");
    assert_eq!(fake.attach_count(), 2);
    fake.assert_no_token_leak(app.path());
}
