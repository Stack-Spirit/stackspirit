//! `stackspirit login|logout|whoami|machines ls|ssh` and the hidden `relay proxy`.
//! Spec: `docs/design/relay-and-packaging.md` §6.4–§6.7.
//!
//! Hermetic: a temporary `$HOME`, a loopback JSON API, a loopback WebSocket relay with a fake
//! machine identity, and a fake `ssh` binary. There is no live network call and no real home.
//!
//! The package root is the workspace root, so Cargo does not auto-discover
//! `crates/vowp-cli/tests/`; the root manifest registers this file as the `cloud_cli` `[[test]]`
//! target. Run it with `cargo test -p vowp-cli --test cloud_cli`.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use serde_json::{Value, json};
use tempfile::TempDir;
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::{Message, WebSocket, accept_hdr};
use vowp_agent::relay::{AcceptInput, ChannelCipher, Jwt, RelayKey, TicketClaims, jwk_thumbprint, machine_accept};
use vowp_protocol::{
    ClientKind, E2E_SUITE, Jwk, RELAY_FRAME_VERSION, RELAY_SUBPROTOCOL, REMOTE_FRAME_VERSION, RelayDataFrame,
    RelayFrame, RemoteFrame, Scope, Timestamp,
};

const MACHINE_ID: &str = "mch_01JQ8Z6K4W5N7R9T2V3X4Y5Z6A";
const MACHINE_ID_2: &str = "mch_01JQ8Z6K4W5N7R9T2V3X4Y5Z6B";
const ACCOUNT_ID: &str = "acct_01JQ9B7M5X6P8S1T3W4Y5Z6A7B";
const ACCESS_TOKEN: &str = "sst_access_cloud-cli-test-0123456789";
const REFRESH_TOKEN: &str = "sst_refresh_cloud-cli-test-0123456789";
const DEVICE_CODE: &str = "dev_cloud-cli-test-secret-0123456789";
const USER_CODE: &str = "ABCD-EFGH";
const TICKET_JTI: &str = "jti_cloud_cli_test";
const CHANNEL_ID: &str = "01JQ8Z6K4W5N7R9T2V3X4Y5Z6A";

/// A temporary home plus a private `bin/` for the fake `ssh`.
struct TestEnv {
    home: TempDir,
    bin: TempDir,
}

impl TestEnv {
    fn new() -> Self {
        let home = tempfile::tempdir().expect("temp home");
        let bin = tempfile::tempdir().expect("temp bin");
        Self { home, bin }
    }

    fn home(&self) -> &Path {
        self.home.path()
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_stackspirit"));
        let path = format!(
            "{}:{}",
            self.bin.path().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        command
            .env("HOME", self.home.path())
            .env("PATH", path)
            .env("NO_COLOR", "1")
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env_remove("VOWP_TMUX_SOCKET")
            .env_remove("STACKSPIRIT_ENROLL_TOKEN")
            .env_remove("STACKSPIRIT_HOME")
            .env_remove("VOWP_HOME");
        command
    }

    fn global_args(&self) -> Vec<String> {
        vec!["--home".to_owned(), self.home.path().display().to_string()]
    }

    fn run(&self, args: &[&str]) -> (i32, String, String) {
        self.command()
            .args(self.global_args())
            .args(args)
            .output()
            .map(|output| {
                (
                    output.status.code().unwrap_or(-1),
                    String::from_utf8_lossy(&output.stdout).into_owned(),
                    String::from_utf8_lossy(&output.stderr).into_owned(),
                )
            })
            .expect("run stackspirit")
    }

    fn run_with_stdin(&self, args: &[&str], input: &str) -> (i32, String, String) {
        let mut child = self
            .command()
            .args(self.global_args())
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn stackspirit");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(input.as_bytes())
            .expect("write stdin");
        let output = child.wait_with_output().expect("wait stackspirit");
        (
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    }

    /// Writes a fake `ssh` that records its argv and exits 0.
    fn fake_ssh(&self) -> PathBuf {
        let path = self.bin.path().join("ssh");
        std::fs::write(
            &path,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$STACKSPIRIT_SSH_ARGS_FILE\"\nexit 0\n",
        )
        .expect("write fake ssh");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod fake ssh");
        self.home.path().join("ssh-args.txt")
    }
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).expect("metadata").permissions().mode() & 0o777
}

fn session_path(env: &TestEnv) -> PathBuf {
    env.home().join("cloud-session.json")
}

fn session_json(env: &TestEnv) -> Value {
    let text = std::fs::read_to_string(session_path(env)).expect("session file");
    serde_json::from_str(&text).expect("session json")
}

fn login_with_token(env: &TestEnv, api: &MockApi) {
    let (code, stdout, stderr) = env.run_with_stdin(
        &["login", "--api-url", &api.url, "--token-stdin"],
        &format!("{ACCESS_TOKEN}\n"),
    );
    assert_eq!(code, 0, "login failed: {stdout}\n{stderr}");
}

/// Signs in through the device flow so the session also holds a refresh token.
fn login_with_device(env: &TestEnv, api: &MockApi) {
    let (code, stdout, stderr) = env.run(&["login", "--api-url", &api.url]);
    assert_eq!(code, 0, "device login failed: {stdout}\n{stderr}");
}

/// Every regular file under `dir`, excluding the session file itself.
fn other_file_text(dir: &Path) -> String {
    let mut text = String::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(path) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&path) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_none_or(|name| name != "cloud-session.json")
                && let Ok(content) = std::fs::read_to_string(&path)
            {
                text.push_str(&content);
            }
        }
    }
    text
}

// ---------------------------------------------------------------------------------------------
// Loopback JSON API
// ---------------------------------------------------------------------------------------------

struct MockApi {
    url: String,
    state: Arc<Mutex<ApiState>>,
}

struct ApiState {
    machine_key: Arc<RelayKey>,
    relay_url: String,
    machines: Value,
    access_tokens: Vec<String>,
    refresh_token: String,
    ticket_scopes: Vec<Scope>,
    expire_access_once: bool,
    pending_polls: usize,
    log: Vec<String>,
    last_ticket: Option<Value>,
    last_ticket_auth: Option<String>,
}

impl MockApi {
    fn start(machine_key: Arc<RelayKey>, relay_url: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind api");
        let port = listener.local_addr().expect("addr").port();
        let url = format!("http://127.0.0.1:{port}");
        let state = Arc::new(Mutex::new(ApiState {
            machine_key,
            relay_url,
            machines: json!({
                "machines": [
                    {
                        "id": MACHINE_ID,
                        "name": "contabo-1",
                        "online": true,
                        "last_seen_at": "2026-11-25T12:00:00.000Z",
                        "project": "stackspirit",
                        "ssh_user": "deploy"
                    },
                    {
                        "id": MACHINE_ID_2,
                        "name": "mac-mini",
                        "presence": {"state": "offline", "last_seen_at": "2026-11-20T08:00:00.000Z"}
                    }
                ]
            }),
            access_tokens: vec![ACCESS_TOKEN.to_owned()],
            refresh_token: REFRESH_TOKEN.to_owned(),
            ticket_scopes: vec![Scope::Ssh],
            expire_access_once: false,
            pending_polls: 0,
            log: Vec::new(),
            last_ticket: None,
            last_ticket_auth: None,
        }));
        let own_url = url.clone();
        let thread_state = Arc::clone(&state);
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let state = Arc::clone(&thread_state);
                let url = own_url.clone();
                std::thread::spawn(move || handle_api(&mut stream, &state, &url));
            }
        });
        Self { url, state }
    }

    fn log(&self) -> Vec<String> {
        self.state.lock().expect("lock").log.clone()
    }

    fn last_ticket(&self) -> Option<Value> {
        self.state.lock().expect("lock").last_ticket.clone()
    }

    fn last_ticket_auth(&self) -> Option<String> {
        self.state.lock().expect("lock").last_ticket_auth.clone()
    }

    fn expire_access_once(&self) {
        self.state.lock().expect("lock").expire_access_once = true;
    }

    fn set_ticket_scopes(&self, scopes: Vec<Scope>) {
        self.state.lock().expect("lock").ticket_scopes = scopes;
    }

    fn set_pending_polls(&self, count: usize) {
        self.state.lock().expect("lock").pending_polls = count;
    }
}

struct HttpRequest {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: Value,
}

fn read_http(stream: &mut TcpStream) -> Option<HttpRequest> {
    stream.set_read_timeout(Some(Duration::from_secs(10))).expect("timeout");
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(index) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break index;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
        }
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let mut lines = head.lines();
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?.to_owned();
    let path = first.next()?.to_owned();
    let mut headers = BTreeMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    let length: usize = headers
        .get("content-length")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let mut body = buffer[head_end + 4..].to_vec();
    while body.len() < length {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => body.extend_from_slice(&chunk[..read]),
        }
    }
    let body = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).unwrap_or(Value::Null)
    };
    Some(HttpRequest {
        method,
        path,
        headers,
        body,
    })
}

fn authorized(state: &ApiState, request: &HttpRequest) -> bool {
    request
        .headers
        .get("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| state.access_tokens.iter().any(|valid| valid == token))
}

fn handle_api(stream: &mut TcpStream, state: &Arc<Mutex<ApiState>>, own_url: &str) {
    let Some(request) = read_http(stream) else {
        return;
    };
    let path = request.path.split('?').next().unwrap_or_default().to_owned();
    {
        let mut state = state.lock().expect("lock");
        state.log.push(format!("{} {}", request.method, path));
    }
    let (status, reply) = respond(state, &request, &path, own_url);
    let text = reply.to_string();
    let _ = write!(
        stream,
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{text}",
        text.len()
    );
}

fn respond(state: &Arc<Mutex<ApiState>>, request: &HttpRequest, path: &str, own_url: &str) -> (u16, Value) {
    let mut state = state.lock().expect("lock");
    match (request.method.as_str(), path) {
        ("POST", "/v1/auth/device") => (
            200,
            json!({
                "device_code": DEVICE_CODE,
                "user_code": USER_CODE,
                "verification_uri": format!("{own_url}/device"),
                "expires_in": 30,
                "interval": 0
            }),
        ),
        ("POST", "/v1/auth/device/token") => {
            if state.pending_polls > 0 {
                state.pending_polls -= 1;
                (
                    400,
                    json!({"error": {"code": "authorization_pending", "message": "waiting for approval"}}),
                )
            } else if request.body["device_code"].as_str() != Some(DEVICE_CODE) {
                (401, json!({"error": {"code": "invalid_device_code"}}))
            } else {
                (
                    200,
                    json!({
                        "access_token": ACCESS_TOKEN,
                        "refresh_token": REFRESH_TOKEN,
                        "expires_in": 900,
                        "account": {"id": ACCOUNT_ID, "email": "owner@example.test"}
                    }),
                )
            }
        }
        ("POST", "/v1/auth/refresh") => {
            if request.body["refresh_token"].as_str() != Some(state.refresh_token.as_str()) {
                (401, json!({"error": {"code": "invalid_refresh_token"}}))
            } else {
                let next = format!("{ACCESS_TOKEN}-refreshed");
                state.access_tokens = vec![next.clone()];
                (
                    200,
                    json!({
                        "access_token": next,
                        "refresh_token": state.refresh_token.clone(),
                        "expires_in": 900
                    }),
                )
            }
        }
        ("GET", "/v1/me") => {
            if !authorized(&state, request) {
                (401, json!({"error": {"code": "invalid_token"}}))
            } else {
                (
                    200,
                    json!({"account": {"id": ACCOUNT_ID, "email": "owner@example.test"}}),
                )
            }
        }
        ("GET", "/v1/machines") => {
            if state.expire_access_once {
                state.expire_access_once = false;
                state.access_tokens.clear();
                (401, json!({"error": {"code": "invalid_token"}}))
            } else if !authorized(&state, request) {
                (401, json!({"error": {"code": "invalid_token"}}))
            } else {
                (200, state.machines.clone())
            }
        }
        ("POST", ticket_path) if ticket_path == format!("/v1/machines/{MACHINE_ID}/ticket") => {
            if !authorized(&state, request) {
                (401, json!({"error": {"code": "invalid_token"}}))
            } else if request.body["kind"].as_str() != Some("cli")
                || !request.body["scopes"]
                    .as_array()
                    .is_some_and(|scopes| scopes.iter().any(|scope| scope.as_str() == Some("ssh")))
            {
                (400, json!({"error": {"code": "bad_request"}}))
            } else {
                let Ok(client_key) = serde_json::from_value::<Jwk>(request.body["client_key"].clone()) else {
                    return (400, json!({"error": {"code": "bad_client_key"}}));
                };
                let ticket = build_ticket(&state, &client_key, own_url);
                state.last_ticket = Some(request.body.clone());
                state.last_ticket_auth = request.headers.get("authorization").cloned();
                (
                    200,
                    json!({
                        "ticket": ticket,
                        "expires_at": "2099-01-01T00:00:00.000Z",
                        "relay_url": state.relay_url.clone(),
                        "machine": {"id": MACHINE_ID, "name": "contabo-1", "ssh_user": "deploy"}
                    }),
                )
            }
        }
        _ => (404, json!({"error": {"code": "not_found"}})),
    }
}

fn build_ticket(state: &ApiState, client_key: &Jwk, own_url: &str) -> String {
    let now = Timestamp::now().unix_secs();
    let claims = json!({
        "iss": own_url,
        "aud": "relay.test",
        "sub": "client:cli_cloud_cli_test",
        "acct": ACCOUNT_ID,
        "machine": MACHINE_ID,
        "kind": "cli",
        "scopes": &state.ticket_scopes,
        "cnf": {"jkt": jwk_thumbprint(client_key)},
        "mkey": state.machine_key.public_key_hex(),
        "mkey_id": state.machine_key.key_id(),
        "iat": now,
        "exp": now + 300,
        "jti": TICKET_JTI
    });
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"ES256","typ":"JWT","kid":"test"}"#);
    let payload = URL_SAFE_NO_PAD.encode(claims.to_string());
    let signature = URL_SAFE_NO_PAD.encode([0u8; 64]);
    format!("{header}.{payload}.{signature}")
}

// ---------------------------------------------------------------------------------------------
// Loopback HTTPS API
// ---------------------------------------------------------------------------------------------

/// A local TLS listener with a self-signed certificate. The CLI rejects the certificate, but
/// the connection and TLS handshake prove the production HTTPS client has a crypto provider.
struct MockTlsApi {
    url: String,
    accepted: Arc<AtomicBool>,
}

impl MockTlsApi {
    fn start() -> Self {
        let certified = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_owned()]).expect("self-signed cert");
        let cert = certified.cert.der().clone();
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(certified.signing_key.serialize_der()));
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let config = ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .expect("TLS 1.3")
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .expect("server certificate");
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind tls api");
        let port = listener.local_addr().expect("addr").port();
        let url = format!("https://127.0.0.1:{port}");
        let accepted = Arc::new(AtomicBool::new(false));
        let thread_accepted = Arc::clone(&accepted);
        let _ = std::thread::spawn(move || {
            let Ok((tcp, _)) = listener.accept() else {
                return;
            };
            thread_accepted.store(true, Ordering::SeqCst);
            let Ok(server) = ServerConnection::new(Arc::new(config)) else {
                return;
            };
            let mut stream = StreamOwned::new(server, tcp);
            // Drive the handshake. The client rejects the self-signed certificate, so the
            // read ends in a TLS alert; the recorded connection is what this mock needs.
            let mut byte = [0u8; 1];
            let _ = stream.read(&mut byte);
        });
        Self { url, accepted }
    }
}

// ---------------------------------------------------------------------------------------------
// Loopback WebSocket relay with a fake machine
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct RelayLog {
    client_hello: bool,
    hello_scopes: Vec<String>,
    tcp_open: bool,
    tcp_target: Option<String>,
    bytes: Vec<u8>,
    tcp_closed: bool,
    error: Option<String>,
}

struct MockRelay {
    url: String,
    log: Arc<Mutex<RelayLog>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl MockRelay {
    fn start(machine_key: Arc<RelayKey>, ticket_scopes: Vec<Scope>, granted_scopes: Vec<Scope>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind relay");
        let port = listener.local_addr().expect("addr").port();
        let url = format!("ws://127.0.0.1:{port}/v1/client");
        let log = Arc::new(Mutex::new(RelayLog::default()));
        let thread_log = Arc::clone(&log);
        let handle = std::thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            if let Err(error) = serve_relay(stream, &thread_log, &machine_key, &ticket_scopes, &granted_scopes) {
                thread_log.lock().expect("lock").error = Some(error);
            }
        });
        Self {
            url,
            log,
            handle: Some(handle),
        }
    }

    fn log(&self) -> std::sync::MutexGuard<'_, RelayLog> {
        self.log.lock().expect("lock")
    }

    fn join(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for MockRelay {
    fn drop(&mut self) {
        // The server thread has a 10 s read timeout; do not block a passing test on a clean
        // thread join. The thread's state is inspected before this point.
        self.handle.take();
    }
}

#[allow(clippy::result_large_err)]
fn serve_relay(
    stream: TcpStream,
    log: &Arc<Mutex<RelayLog>>,
    machine_key: &RelayKey,
    ticket_scopes: &[Scope],
    granted_scopes: &[Scope],
) -> Result<(), String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|error| error.to_string())?;
    let mut socket = accept_hdr(
        stream,
        |request: &Request, mut response: Response| -> Result<Response, ErrorResponse> {
            let protocol = request
                .headers()
                .get("sec-websocket-protocol")
                .and_then(|value| value.to_str().ok());
            if protocol != Some(RELAY_SUBPROTOCOL) {
                let reply = tungstenite::http::Response::builder()
                    .status(400)
                    .body(None)
                    .expect("response");
                return Err(reply);
            }
            response.headers_mut().insert(
                "Sec-WebSocket-Protocol",
                tungstenite::http::HeaderValue::from_static(RELAY_SUBPROTOCOL),
            );
            Ok(response)
        },
    )
    .map_err(|error| format!("accept: {error}"))?;

    let hello = match socket.read().map_err(|error| error.to_string())? {
        Message::Text(text) => RelayFrame::from_json(text.as_str()).map_err(|error| error.to_string())?,
        other => return Err(format!("expected client.hello, got {other:?}")),
    };
    let (ticket, client_key, client_nonce) = match hello {
        RelayFrame::ClientHello {
            ticket,
            client_key,
            client_nonce,
            kind,
            ..
        } => {
            if kind != ClientKind::Cli {
                return Err("wrong client kind".to_owned());
            }
            (ticket, client_key, client_nonce)
        }
        other => return Err(format!("expected client.hello, got {}", other.type_name())),
    };
    let claims: TicketClaims = Jwt::parse(&ticket)
        .and_then(|jwt| jwt.claims())
        .map_err(|error| format!("ticket: {error}"))?;
    if claims.machine != MACHINE_ID || claims.cnf.jkt != jwk_thumbprint(&client_key) {
        return Err("client.hello does not match the ticket".to_owned());
    }
    {
        let mut log = log.lock().expect("lock");
        log.client_hello = true;
        log.hello_scopes = ticket_scopes.iter().map(|scope| scope.as_str().to_owned()).collect();
    }

    send_control(
        &mut socket,
        &RelayFrame::ClientWelcome {
            v: RELAY_FRAME_VERSION,
            relay_id: "relay-test".to_owned(),
            session_id: "session-test".to_owned(),
            channel_id: CHANNEL_ID.to_owned(),
            machine_id: MACHINE_ID.to_owned(),
            account_id: ACCOUNT_ID.to_owned(),
            expires_at: Timestamp::now(),
            features: vec!["ssh".to_owned()],
        },
    )?;
    let accept = machine_accept(
        &AcceptInput {
            channel_id: CHANNEL_ID,
            client_key: &client_key,
            client_nonce: &client_nonce,
            machine_id: MACHINE_ID,
            account_id: ACCOUNT_ID,
            ticket_jti: &claims.jti,
        },
        machine_key,
    )
    .map_err(|error| format!("accept: {error}"))?;
    send_control(
        &mut socket,
        &RelayFrame::ChannelAccept {
            v: RELAY_FRAME_VERSION,
            channel_id: CHANNEL_ID.to_owned(),
            machine_key: accept.machine_key.clone(),
            machine_nonce: accept.machine_nonce.clone(),
            machine_key_id: machine_key.key_id(),
            machine_signature: accept.machine_signature.clone(),
            suite: E2E_SUITE.to_owned(),
        },
    )?;

    let mut cipher = accept.cipher;
    let hello = next_remote(&mut socket, &mut cipher, Duration::from_secs(10))?;
    let RemoteFrame::Hello { scopes, .. } = hello else {
        return Err("expected Hello".to_owned());
    };
    if !scopes.contains(&Scope::Ssh) {
        return Err("client did not request ssh".to_owned());
    }
    send_remote(
        &mut socket,
        &mut cipher,
        &RemoteFrame::HelloAck {
            v: REMOTE_FRAME_VERSION,
            machine: MACHINE_ID.to_owned(),
            scopes: granted_scopes.to_vec(),
            capabilities: vec!["ssh".to_owned()],
        },
    )?;
    if !granted_scopes.contains(&Scope::Ssh) {
        // The proxy must abort before opening a stream; wait for it to disconnect.
        let _ = next_remote(&mut socket, &mut cipher, Duration::from_secs(5));
        return Ok(());
    }

    loop {
        let frame = next_remote(&mut socket, &mut cipher, Duration::from_secs(10))?;
        match frame {
            RemoteFrame::TcpOpen { target, .. } => {
                if target != "ssh" {
                    return Err(format!("wrong target {target}"));
                }
                {
                    let mut log = log.lock().expect("lock");
                    log.tcp_open = true;
                    log.tcp_target = Some(target);
                }
                send_remote(
                    &mut socket,
                    &mut cipher,
                    &RemoteFrame::TcpData {
                        stream: 1,
                        data: STANDARD.encode(b"SSH-2.0-stackspirit-test\r\n"),
                    },
                )?;
            }
            RemoteFrame::TcpData { data, .. } => {
                let bytes = STANDARD
                    .decode(data.as_bytes())
                    .map_err(|error| format!("base64: {error}"))?;
                log.lock().expect("lock").bytes.extend_from_slice(&bytes);
                send_remote(&mut socket, &mut cipher, &RemoteFrame::TcpData { stream: 1, data })?;
            }
            RemoteFrame::TcpClose { .. } => {
                log.lock().expect("lock").tcp_closed = true;
                send_remote(&mut socket, &mut cipher, &RemoteFrame::TcpClose { stream: 1 })?;
                let _ = socket.close(None);
                let _ = socket.flush();
                return Ok(());
            }
            RemoteFrame::Ping { nonce } => send_remote(&mut socket, &mut cipher, &RemoteFrame::Pong { nonce })?,
            _ => {}
        }
    }
}

fn send_control(socket: &mut WebSocket<TcpStream>, frame: &RelayFrame) -> Result<(), String> {
    let text = frame.to_json().map_err(|error| error.to_string())?;
    socket.send(Message::text(text)).map_err(|error| error.to_string())?;
    socket.flush().map_err(|error| error.to_string())
}

fn send_remote(
    socket: &mut WebSocket<TcpStream>,
    cipher: &mut ChannelCipher,
    frame: &RemoteFrame,
) -> Result<(), String> {
    let plain = frame.to_json().map_err(|error| error.to_string())?;
    let data = cipher.seal(&plain).map_err(|error| error.to_string())?;
    let bytes = data.to_bytes().map_err(|error| error.to_string())?;
    socket.send(Message::binary(bytes)).map_err(|error| error.to_string())?;
    socket.flush().map_err(|error| error.to_string())
}

fn next_remote(
    socket: &mut WebSocket<TcpStream>,
    cipher: &mut ChannelCipher,
    timeout: Duration,
) -> Result<RemoteFrame, String> {
    socket
        .get_ref()
        .set_read_timeout(Some(timeout))
        .map_err(|error| error.to_string())?;
    loop {
        match socket.read().map_err(|error| error.to_string())? {
            Message::Binary(bytes) => {
                let frame = RelayDataFrame::from_bytes(&bytes).map_err(|error| error.to_string())?;
                let plain = cipher.open(&frame).map_err(|error| error.to_string())?;
                return RemoteFrame::from_json(&plain).map_err(|error| error.to_string());
            }
            Message::Text(text) => {
                let frame = RelayFrame::from_json(text.as_str()).map_err(|error| error.to_string())?;
                if let RelayFrame::Ping { nonce, .. } = frame {
                    send_control(
                        socket,
                        &RelayFrame::Pong {
                            v: RELAY_FRAME_VERSION,
                            nonce,
                        },
                    )?;
                }
            }
            Message::Ping(payload) => {
                socket.send(Message::Pong(payload)).map_err(|error| error.to_string())?;
            }
            Message::Close(_) => return Err("closed".to_owned()),
            _ => {}
        }
    }
}

fn wait_timeout(child: &mut Child, timeout: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => return Some(status),
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

#[test]
fn login_uses_the_device_flow_and_stores_a_0600_session() {
    let env = TestEnv::new();
    let machine_key = Arc::new(RelayKey::generate());
    let api = MockApi::start(Arc::clone(&machine_key), "ws://127.0.0.1:1/v1/client".to_owned());
    api.set_pending_polls(1);

    let (code, stdout, stderr) = env.run(&["login", "--api-url", &api.url]);
    assert_eq!(code, 0, "login failed: {stdout}\n{stderr}");
    let text = format!("{stdout}\n{stderr}");
    assert!(text.contains(USER_CODE), "the user code must be shown: {text}");
    assert!(text.contains("/device"), "the verification URL must be shown: {text}");
    for secret in [ACCESS_TOKEN, REFRESH_TOKEN, DEVICE_CODE] {
        assert!(!text.contains(secret), "login printed a secret: {text}");
    }

    let path = session_path(&env);
    assert_eq!(mode(&path), 0o600, "the session file is private");
    let session = session_json(&env);
    assert_eq!(session["api_url"], api.url);
    assert_eq!(session["access_token"], ACCESS_TOKEN);
    assert_eq!(session["refresh_token"], REFRESH_TOKEN);
    assert!(
        !other_file_text(env.home()).contains(ACCESS_TOKEN),
        "the token is written only to the 0600 session file"
    );

    // `whoami` uses the access token and prints no token.
    let (code, stdout, stderr) = env.run(&["--json", "whoami"]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    let who: Value = serde_json::from_str(&stdout).expect("whoami json");
    assert_eq!(who["account_id"], ACCOUNT_ID);
    assert_eq!(who["email"], "owner@example.test");
    assert_eq!(who["api_url"], api.url);
    assert!(!stdout.contains(ACCESS_TOKEN) && !stderr.contains(ACCESS_TOKEN));

    // `logout` removes the file; `whoami` then needs a login and does not call the API.
    let (code, stdout, _) = env.run(&["logout"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("signed out"), "{stdout}");
    assert!(!path.exists());
    let calls = api.log().len();
    let (code, _, stderr) = env.run(&["whoami"]);
    assert_eq!(code, 6, "{stderr}");
    assert!(stderr.contains("not signed in"), "{stderr}");
    assert_eq!(api.log().len(), calls, "whoami must not call the API without a session");
}

#[test]
fn login_token_stdin_validates_and_stores_the_token() {
    let env = TestEnv::new();
    let machine_key = Arc::new(RelayKey::generate());
    let api = MockApi::start(machine_key, "ws://127.0.0.1:1/v1/client".to_owned());

    let (code, stdout, stderr) = env.run_with_stdin(
        &["login", "--api-url", &api.url, "--token-stdin"],
        &format!("{ACCESS_TOKEN}\n"),
    );
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(!stdout.contains(ACCESS_TOKEN) && !stderr.contains(ACCESS_TOKEN));
    assert_eq!(mode(&session_path(&env)), 0o600);
    assert_eq!(session_json(&env)["access_token"], ACCESS_TOKEN);

    let (code, _, stderr) = env.run_with_stdin(
        &["login", "--api-url", &api.url, "--token-stdin"],
        "sst_wrong-token-0123456789\n",
    );
    assert_eq!(code, 6, "{stderr}");
    assert!(stderr.contains("rejected"), "{stderr}");
    assert!(!stderr.contains("sst_wrong-token"));
    assert_eq!(
        session_json(&env)["access_token"],
        ACCESS_TOKEN,
        "a rejected token must not replace the stored session"
    );
}

#[test]
fn login_over_https_uses_the_tls_provider_and_fails_without_panicking() {
    let env = TestEnv::new();
    let api = MockTlsApi::start();

    // `login` and every other cloud command build their client through `Api::new`; an HTTPS
    // URL must reach the TLS handshake even when the certificate is not trusted.
    let (code, stdout, stderr) = env.run_with_stdin(
        &["login", "--api-url", &api.url, "--token-stdin"],
        &format!("{ACCESS_TOKEN}\n"),
    );
    assert_eq!(
        code, 7,
        "an HTTPS failure must be a normal remote error, not a panic: {stdout}\n{stderr}"
    );
    assert!(
        stderr.contains("the StackSpirit API is unreachable"),
        "missing the normal API error: {stderr}"
    );
    assert!(!stderr.contains("panicked"), "the HTTPS client panicked: {stderr}");
    assert!(
        !stderr.contains("CryptoProvider"),
        "the HTTPS client has no TLS provider: {stderr}"
    );
    assert!(!stdout.contains(ACCESS_TOKEN) && !stderr.contains(ACCESS_TOKEN));
    assert!(!session_path(&env).exists(), "a failed login must not write a session");
    assert!(
        api.accepted.load(Ordering::SeqCst),
        "the HTTPS client never opened a connection"
    );
}

#[test]
fn machines_ls_shows_presence_and_refreshes_an_expired_access_token() {
    let env = TestEnv::new();
    let machine_key = Arc::new(RelayKey::generate());
    let api = MockApi::start(machine_key, "ws://127.0.0.1:1/v1/client".to_owned());
    login_with_device(&env, &api);

    let (code, stdout, stderr) = env.run(&["--json", "machines", "ls"]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    let report: Value = serde_json::from_str(&stdout).expect("machines json");
    assert_eq!(report["api_url"], api.url);
    let machines = report["machines"].as_array().expect("machines");
    assert_eq!(machines.len(), 2);
    assert_eq!(machines[0]["id"], MACHINE_ID);
    assert_eq!(machines[0]["name"], "contabo-1");
    assert_eq!(machines[0]["state"], "online");
    assert_eq!(machines[0]["online"], true);
    assert_eq!(machines[0]["last_seen_at"], "2026-11-25T12:00:00.000Z");
    assert_eq!(machines[1]["state"], "offline");
    assert_eq!(machines[1]["online"], false);
    assert_eq!(machines[1]["last_seen_at"], "2026-11-20T08:00:00.000Z");

    let (code, text, stderr) = env.run(&["machines", "ls"]);
    assert_eq!(code, 0, "{stderr}");
    for expected in [
        MACHINE_ID,
        "contabo-1",
        "online",
        "mac-mini",
        "offline",
        "2026-11-25T12:00:00.000Z",
    ] {
        assert!(text.contains(expected), "missing {expected} in:\n{text}");
    }

    // Expire the access token once; the client must rotate it and retry the same read.
    let before = session_json(&env)["access_token"].clone();
    api.expire_access_once();
    let (code, stdout, stderr) = env.run(&["--json", "machines", "ls"]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    let after = session_json(&env)["access_token"].clone();
    assert_ne!(after, before, "the session token must be rotated");
    assert_eq!(after, format!("{ACCESS_TOKEN}-refreshed"));
    assert_eq!(mode(&session_path(&env)), 0o600);
    assert!(
        api.log().contains(&"POST /v1/auth/refresh".to_owned()),
        "the refresh endpoint was called: {:?}",
        api.log()
    );
}

#[test]
fn ssh_builds_the_proxy_command_without_a_token() {
    let env = TestEnv::new();
    let machine_key = Arc::new(RelayKey::generate());
    let api = MockApi::start(machine_key, "ws://127.0.0.1:1/v1/client".to_owned());
    login_with_token(&env, &api);
    let log = env.fake_ssh();

    let output = env
        .command()
        .args(env.global_args())
        .args(["ssh", "contabo-1", "--", "-p", "2222", "uptime"])
        .env("STACKSPIRIT_SSH_ARGS_FILE", &log)
        .output()
        .expect("run ssh command");
    let (code, stdout, stderr) = (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    );
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    let args = std::fs::read_to_string(&log).expect("ssh argv log");
    let lines: Vec<&str> = args.lines().collect();
    assert_eq!(lines.first(), Some(&"-o"), "{args}");
    let proxy = lines
        .get(1)
        .and_then(|line| line.strip_prefix("ProxyCommand="))
        .expect("ProxyCommand");
    assert!(proxy.contains("--home"), "{proxy}");
    assert!(proxy.contains("relay proxy"), "{proxy}");
    assert!(proxy.contains(&format!("--machine {MACHINE_ID}")), "{proxy}");
    assert!(proxy.ends_with("--stdio"), "{proxy}");
    assert_eq!(lines.get(2), Some(&"-o"), "{args}");
    assert_eq!(
        lines.get(3),
        Some(&format!("HostKeyAlias=stackspirit-{MACHINE_ID}").as_str()),
        "{args}"
    );
    assert_eq!(
        lines.get(4),
        Some(&format!("deploy@stackspirit-{MACHINE_ID}").as_str()),
        "{args}"
    );
    assert_eq!(&lines[5..], ["-p", "2222", "uptime"], "{args}");
    assert!(!args.contains(ACCESS_TOKEN), "a token must never be an argv value");
    assert!(!stdout.contains(ACCESS_TOKEN) && !stderr.contains(ACCESS_TOKEN));

    let (code, _, stderr) = env.run(&["ssh", "missing-box"]);
    assert_eq!(code, 3, "{stderr}");
    assert!(stderr.contains("machines ls"), "{stderr}");
}

#[test]
fn relay_proxy_carries_bytes_over_the_e2e_ssh_channel() {
    let env = TestEnv::new();
    let machine_key = Arc::new(RelayKey::generate());
    let mut relay = MockRelay::start(Arc::clone(&machine_key), vec![Scope::Ssh], vec![Scope::Ssh]);
    let relay_url = relay.url.clone();
    let api = MockApi::start(Arc::clone(&machine_key), relay_url);
    login_with_token(&env, &api);

    let mut child = env
        .command()
        .args(env.global_args())
        .args(["relay", "proxy", "--machine", MACHINE_ID, "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn proxy");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"hello over e2e\n")
        .expect("write stdin");
    drop(child.stdin.take());

    let mut stdout = child.stdout.take().expect("stdout");
    let stdout_thread = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        bytes
    });
    let status = wait_timeout(&mut child, Duration::from_secs(15)).expect("proxy exited");
    let bytes = stdout_thread.join().expect("stdout thread");
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    assert!(status.success(), "proxy failed: {stderr}");
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("SSH-2.0-stackspirit-test"),
        "the machine banner did not reach stdout: {text:?}"
    );
    assert!(
        bytes
            .windows(b"hello over e2e\n".len())
            .any(|w| w == b"hello over e2e\n")
    );
    assert!(!text.contains(ACCESS_TOKEN) && !stderr.contains(ACCESS_TOKEN));

    relay.join();
    let log = relay.log();
    assert!(log.error.is_none(), "relay error: {:?}", log.error);
    assert!(log.client_hello, "no client.hello");
    assert_eq!(log.hello_scopes, vec!["ssh".to_owned()]);
    assert!(log.tcp_open, "no tcp.open");
    assert_eq!(log.tcp_target.as_deref(), Some("ssh"));
    assert_eq!(log.bytes, b"hello over e2e\n");
    assert!(log.tcp_closed, "the proxy did not send tcp.close");

    let ticket = api.last_ticket().expect("ticket request");
    assert_eq!(ticket["kind"], "cli");
    assert_eq!(ticket["scopes"], json!(["ssh"]));
    assert!(ticket["client_key"].get("x").is_some());
    assert!(api.last_ticket_auth().is_some_and(|value| value.starts_with("Bearer ")));
    assert!(
        api.log().contains(&format!("POST /v1/machines/{MACHINE_ID}/ticket")),
        "{:?}",
        api.log()
    );
}

#[test]
fn relay_proxy_refuses_a_channel_without_the_ssh_scope_before_tcp_open() {
    let env = TestEnv::new();
    let machine_key = Arc::new(RelayKey::generate());
    // The ticket has ssh, but the machine grants only http.read: the proxy must stop at
    // HelloAck and never send tcp.open.
    let mut relay = MockRelay::start(Arc::clone(&machine_key), vec![Scope::Ssh], vec![Scope::HttpRead]);
    let relay_url = relay.url.clone();
    let api = MockApi::start(Arc::clone(&machine_key), relay_url);
    login_with_token(&env, &api);

    let mut child = env
        .command()
        .args(env.global_args())
        .args(["relay", "proxy", "--machine", MACHINE_ID, "--stdio"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn proxy");
    let status = wait_timeout(&mut child, Duration::from_secs(10)).expect("proxy exited");
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    assert_eq!(status.code(), Some(6), "{stderr}");
    assert!(stderr.contains("ssh scope") || stderr.contains("ssh"), "{stderr}");
    assert!(!stderr.contains(ACCESS_TOKEN));

    relay.join();
    let log = relay.log();
    assert!(log.error.is_none(), "relay error: {:?}", log.error);
    assert!(!log.tcp_open, "the proxy opened a TCP stream without the ssh scope");
}

#[test]
fn relay_proxy_refuses_a_machine_whose_key_changed() {
    let env = TestEnv::new();
    // The API issued a ticket for key A; the relay's machine signs with key B.
    let ticket_key = Arc::new(RelayKey::generate());
    let machine_key = Arc::new(RelayKey::generate());
    let mut relay = MockRelay::start(Arc::clone(&machine_key), vec![Scope::Ssh], vec![Scope::Ssh]);
    let relay_url = relay.url.clone();
    let api = MockApi::start(Arc::clone(&ticket_key), relay_url);
    login_with_token(&env, &api);

    let (code, stdout, stderr) = env.run(&["relay", "proxy", "--machine", MACHINE_ID, "--stdio"]);
    assert_eq!(code, 6, "{stdout}\n{stderr}");
    assert!(
        stderr.contains("handshake failed") && stderr.contains("signature"),
        "{stderr}"
    );
    assert!(!stderr.contains(ACCESS_TOKEN));

    relay.join();
    let log = relay.log();
    assert!(log.client_hello, "the relay never saw client.hello");
    assert!(
        !log.tcp_open,
        "the proxy opened a stream before verifying the machine key"
    );
}

#[test]
fn relay_proxy_refuses_a_ticket_without_ssh_before_connecting() {
    let env = TestEnv::new();
    let machine_key = Arc::new(RelayKey::generate());
    // The ticket has no ssh scope, so the proxy must stop before opening the WebSocket at all.
    let api = MockApi::start(Arc::clone(&machine_key), "ws://127.0.0.1:1/v1/client".to_owned());
    api.set_ticket_scopes(vec![Scope::HttpRead]);
    login_with_token(&env, &api);

    let (code, stdout, stderr) = env.run(&["relay", "proxy", "--machine", MACHINE_ID, "--stdio"]);
    assert_eq!(code, 6, "{stdout}\n{stderr}");
    assert!(stderr.contains("ssh scope"), "{stderr}");
    assert!(
        api.log().contains(&format!("POST /v1/machines/{MACHINE_ID}/ticket")),
        "{:?}",
        api.log()
    );
}
