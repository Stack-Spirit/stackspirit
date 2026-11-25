//! The relay machine side against a fake API, a fake relay and a fake loopback daemon.
//! Spec: `docs/design/relay-and-packaging.md` §6.2–§6.6 and §9.3.
//!
//! Everything runs on loopback in temporary directories: no Cloudflare, no real daemon and no
//! network. The fake relay and fake client use only the crate's public primitives, so the machine's
//! behaviour is proven against an independent peer script.

#![allow(clippy::result_large_err)]

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use p256::ecdsa::SigningKey;
use serde_json::{Value, json};
use tempfile::TempDir;
use tungstenite::protocol::WebSocket;
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::protocol::{CloseFrame, Message};
use vowp_agent::relay::auth::{
    CredentialClaims, Jwks, JwksKey, Jwt, NonceSet, ReplayCache, TicketContext, credential_refresh_canonical,
    machine_auth_canonical, sign_es256, sign_machine_auth, verify_machine_auth, verify_ticket,
};
use vowp_agent::relay::connector::validate_http_request;
use vowp_agent::relay::crypto::{AcceptInput, ChannelCipher, EphemeralKey, client_finish, jwk_thumbprint};
use vowp_agent::relay::{
    ConnectorOutcome, ConnectorState, EnrollRequest, RelayError, RelayJson, RelayKey, RelayPaths, RelayStatusFile,
    enroll, refresh_credential, rotate_key, status_report,
};
use vowp_agent::{Connector, ConnectorOptions, ConnectorReport};
use vowp_protocol::{
    ClientKind, Jwk, RELAY_SUBPROTOCOL, RelayConfig, RelayDataFrame, RelayFrame, RemoteFrame, Scope, Timestamp, codes,
};

// ---------------------------------------------------------------------------------------------
// small helpers
// ---------------------------------------------------------------------------------------------

const DAEMON_MARKER: &str = "PAYLOAD-MARKER-7f3a";
const MACHINE_ID: &str = "mch_01JQ8Z6K4W5N7R9T2V3X4Y5Z6A";
const ACCOUNT_ID: &str = "acct_01JQ9B7M5X6P8S1T3W4Y5Z6A7B";

fn now_secs() -> i64 {
    Timestamp::now().unix_secs()
}

fn eventually(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {what}");
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).expect("metadata").permissions().mode() & 0o777
}

struct HttpRequestIn {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

fn read_http_request(stream: &mut TcpStream) -> Option<HttpRequestIn> {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    let head_end = loop {
        if let Some(index) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break index;
        }
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let mut lines = head.lines();
    let mut request_line = lines.next()?.split_whitespace();
    let method = request_line.next()?.to_owned();
    let path = request_line.next()?.to_owned();
    let headers: BTreeMap<String, String> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let length: usize = headers
        .get("content-length")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let mut body = buffer[head_end + 4..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    Some(HttpRequestIn {
        method,
        path,
        headers,
        body,
    })
}

fn respond(stream: &mut TcpStream, status: u16, headers: &[(&str, &str)], body: &[u8]) {
    let mut head = format!(
        "HTTP/1.1 {status} X\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

fn serve(handler: impl Fn(TcpStream) + Send + Sync + 'static) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let handler = Arc::new(handler);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let handler = Arc::clone(&handler);
            std::thread::spawn(move || handler(stream));
        }
    });
    format!("127.0.0.1:{port}")
}

// ---------------------------------------------------------------------------------------------
// fake daemon
// ---------------------------------------------------------------------------------------------

/// Method, path and lowercased headers of one request the fake daemon saw.
type SeenRequest = (String, String, BTreeMap<String, String>);

/// Requests the fake daemon received.
#[derive(Default)]
struct DaemonLog {
    requests: Mutex<Vec<SeenRequest>>,
}

fn fake_daemon() -> (String, Arc<DaemonLog>) {
    let log = Arc::new(DaemonLog::default());
    let seen = Arc::clone(&log);
    let address = serve(move |mut stream| {
        let Some(request) = read_http_request(&mut stream) else {
            return;
        };
        seen.requests.lock().expect("lock").push((
            request.method.clone(),
            request.path.clone(),
            request.headers.clone(),
        ));
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/api/v1/health") => respond(
                &mut stream,
                200,
                &[
                    ("Content-Type", "application/json"),
                    ("Set-Cookie", "session=SECRET-COOKIE"),
                    ("X-Internal", "leak"),
                    ("ETag", "\"abc\""),
                ],
                format!("{{\"ok\":true,\"marker\":\"{DAEMON_MARKER}\"}}").as_bytes(),
            ),
            ("GET", "/api/v1/missing") => respond(&mut stream, 404, &[("Content-Type", "application/json")], b"{}"),
            ("GET", "/api/v1/big") => respond(&mut stream, 200, &[], &vec![b'x'; 700_000]),
            ("GET", "/api/v1/board/stream") => {
                let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
                let _ = stream.write_all(head.as_bytes());
                for event in ["data: one\n\n", "data: two\n\n"] {
                    let _ = stream.write_all(format!("{:x}\r\n{event}\r\n", event.len()).as_bytes());
                    let _ = stream.flush();
                    std::thread::sleep(Duration::from_millis(100));
                }
                let _ = stream.write_all(b"0\r\n\r\n");
            }
            ("POST", path) if path.starts_with("/api/v1/jobs/") => {
                respond(&mut stream, 200, &[("Content-Type", "application/json")], &request.body)
            }
            _ => respond(&mut stream, 404, &[], b""),
        }
    });
    (format!("http://{address}/"), log)
}

// ---------------------------------------------------------------------------------------------
// fake API
// ---------------------------------------------------------------------------------------------

struct ApiState {
    url: Mutex<String>,
    machine_pubkey: Mutex<String>,
    revoked: AtomicBool,
    credential_ttl: AtomicI64,
    issued: AtomicU64,
    log: Mutex<Vec<String>>,
    enroll_token: String,
    enroll_headers: Mutex<Vec<BTreeMap<String, String>>>,
    enroll_status: AtomicU64,
}

struct FakeApi {
    url: String,
    state: Arc<ApiState>,
    signing: Arc<SigningKey>,
}

const API_KID: &str = "test-kid-1";

fn api_signing_key() -> SigningKey {
    SigningKey::from_slice(&[0x77u8; 32]).expect("test signing key")
}

fn jwks_for(key: &SigningKey) -> Jwks {
    Jwks {
        keys: vec![JwksKey::from_verifying(API_KID, key.verifying_key())],
    }
}

fn mint_credential(url: &str, key: &SigningKey, machine_id: &str, mkey: &str, ttl: i64, n: u64) -> (String, String) {
    let now = now_secs();
    let claims = json!({
        "iss": url.trim_end_matches('/'), "aud": "127.0.0.1", "sub": format!("machine:{machine_id}"),
        "acct": ACCOUNT_ID, "key_id": format!("ed25519:{mkey}"), "mkey": mkey,
        "scopes": ["machine.connect"], "iat": now, "exp": now + ttl, "jti": format!("jti_cred{n}"),
    });
    (
        sign_es256(key, API_KID, &claims),
        Timestamp::from_unix_secs(now + ttl).as_rfc3339(),
    )
}

impl FakeApi {
    fn start(enroll_token: &str) -> Self {
        let state = Arc::new(ApiState {
            url: Mutex::new(String::new()),
            machine_pubkey: Mutex::new(String::new()),
            revoked: AtomicBool::new(false),
            credential_ttl: AtomicI64::new(86_400),
            issued: AtomicU64::new(0),
            log: Mutex::new(Vec::new()),
            enroll_token: enroll_token.to_owned(),
            enroll_headers: Mutex::new(Vec::new()),
            enroll_status: AtomicU64::new(200),
        });
        let signing = Arc::new(api_signing_key());
        let (shared, key) = (Arc::clone(&state), Arc::clone(&signing));
        let address = serve(move |mut stream| {
            let Some(request) = read_http_request(&mut stream) else {
                return;
            };
            handle_api(&mut stream, &shared, &key, &request);
        });
        let url = format!("http://{address}");
        *state.url.lock().expect("lock") = url.clone();
        Self { url, state, signing }
    }

    fn register(&self, pubkey: &str) {
        *self.state.machine_pubkey.lock().expect("lock") = pubkey.to_owned();
    }

    fn credential_for(&self, pubkey: &str, ttl: i64) -> (String, String) {
        let n = self.state.issued.fetch_add(1, Ordering::SeqCst);
        mint_credential(&self.url, &self.signing, MACHINE_ID, pubkey, ttl, n)
    }

    fn requests(&self) -> Vec<String> {
        self.state.log.lock().expect("lock").clone()
    }

    fn count(&self, needle: &str) -> usize {
        self.requests().iter().filter(|line| line.contains(needle)).count()
    }
}

fn handle_api(stream: &mut TcpStream, st: &Arc<ApiState>, key: &SigningKey, request: &HttpRequestIn) {
    st.log
        .lock()
        .expect("lock")
        .push(format!("{} {}", request.method, request.path));
    let url = st.url.lock().expect("lock").clone();
    let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
    let json_ok = |stream: &mut TcpStream, value: Value| {
        respond(
            stream,
            200,
            &[("Content-Type", "application/json")],
            value.to_string().as_bytes(),
        )
    };
    let ttl = st.credential_ttl.load(Ordering::SeqCst);
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/.well-known/jwks.json") => json_ok(stream, serde_json::to_value(jwks_for(key)).expect("jwks")),
        ("POST", "/v1/machines/enroll") => {
            st.enroll_headers.lock().expect("lock").push(request.headers.clone());
            let status = st.enroll_status.load(Ordering::SeqCst) as u16;
            if status != 200 {
                respond(
                    stream,
                    status,
                    &[],
                    json!({"error": {"code": "conflict"}}).to_string().as_bytes(),
                );
                return;
            }
            if body["token"] != st.enroll_token {
                respond(
                    stream,
                    401,
                    &[],
                    json!({"error": {"code": "invalid_token"}}).to_string().as_bytes(),
                );
                return;
            }
            let pubkey = body["public_key"].as_str().unwrap_or_default().to_owned();
            *st.machine_pubkey.lock().expect("lock") = pubkey.clone();
            let n = st.issued.fetch_add(1, Ordering::SeqCst);
            let (credential, expires) = mint_credential(&url, key, MACHINE_ID, &pubkey, ttl, n);
            json_ok(
                stream,
                json!({
                    "machine_id": MACHINE_ID, "account_id": ACCOUNT_ID, "machine_credential": credential,
                    "relay_url": "wss://relay.example.test/v1/machine", "api_url": "https://api.example.test",
                    "expires_at": expires,
                }),
            );
        }
        ("POST", path) if path == format!("/v1/machines/{MACHINE_ID}/credential") => {
            if st.revoked.load(Ordering::SeqCst) {
                respond(stream, 403, &[], b"{}");
                return;
            }
            let pubkey = st.machine_pubkey.lock().expect("lock").clone();
            let canonical = credential_refresh_canonical(
                MACHINE_ID,
                body["key_id"].as_str().unwrap_or_default(),
                body["signed_at"].as_str().unwrap_or_default(),
            );
            let fresh = Timestamp::parse(body["signed_at"].as_str().unwrap_or_default())
                .is_ok_and(|signed| (signed.unix_secs() - now_secs()).abs() <= 60);
            if !fresh
                || body["key_id"] != format!("ed25519:{pubkey}")
                || verify_machine_auth(&pubkey, &canonical, body["signature"].as_str().unwrap_or_default()).is_err()
            {
                respond(
                    stream,
                    401,
                    &[],
                    json!({"error": {"code": "bad_signature"}}).to_string().as_bytes(),
                );
                return;
            }
            let n = st.issued.fetch_add(1, Ordering::SeqCst);
            let (credential, expires) = mint_credential(&url, key, MACHINE_ID, &pubkey, ttl, n);
            json_ok(stream, json!({"machine_credential": credential, "expires_at": expires}));
        }
        ("POST", path) if path == format!("/v1/machines/{MACHINE_ID}/keys") => {
            let old = st.machine_pubkey.lock().expect("lock").clone();
            let new = body["new_public_key"].as_str().unwrap_or_default().to_owned();
            let canonical = [
                "STACKSPIRIT-ROTATE-V1",
                MACHINE_ID,
                body["key_id"].as_str().unwrap_or_default(),
                body["new_key_id"].as_str().unwrap_or_default(),
                &new,
                body["signed_at"].as_str().unwrap_or_default(),
            ]
            .join("\n");
            if body["key_id"] != format!("ed25519:{old}")
                || body["new_key_id"] != format!("ed25519:{new}")
                || verify_machine_auth(&old, &canonical, body["signature"].as_str().unwrap_or_default()).is_err()
                || verify_machine_auth(&new, &canonical, body["new_signature"].as_str().unwrap_or_default()).is_err()
            {
                respond(
                    stream,
                    401,
                    &[],
                    json!({"error": {"code": "bad_signature"}}).to_string().as_bytes(),
                );
                return;
            }
            *st.machine_pubkey.lock().expect("lock") = new.clone();
            let n = st.issued.fetch_add(1, Ordering::SeqCst);
            let (credential, expires) = mint_credential(&url, key, MACHINE_ID, &new, ttl, n);
            json_ok(stream, json!({"machine_credential": credential, "expires_at": expires}));
        }
        _ => respond(stream, 404, &[], b"{}"),
    }
}

// ---------------------------------------------------------------------------------------------
// fake relay
// ---------------------------------------------------------------------------------------------

type Ws = WebSocket<TcpStream>;

fn spawn_relay(script: impl Fn(usize, Ws) + Send + Sync + 'static) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let script = Arc::new(script);
    std::thread::spawn(move || {
        for (index, stream) in listener.incoming().flatten().enumerate() {
            let script = Arc::clone(&script);
            std::thread::spawn(move || {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(20)));
                let socket = tungstenite::accept_hdr(
                    stream,
                    |request: &tungstenite::handshake::server::Request,
                     mut response: tungstenite::handshake::server::Response| {
                        let offered = request
                            .headers()
                            .get("Sec-WebSocket-Protocol")
                            .and_then(|value| value.to_str().ok())
                            .unwrap_or_default()
                            .to_owned();
                        assert_eq!(
                            offered, RELAY_SUBPROTOCOL,
                            "the machine must offer the relay subprotocol"
                        );
                        assert!(
                            request.headers().get("Authorization").is_none() && request.uri().query().is_none(),
                            "no query-string or header credential is accepted"
                        );
                        response.headers_mut().insert(
                            "Sec-WebSocket-Protocol",
                            tungstenite::http::HeaderValue::from_static(RELAY_SUBPROTOCOL),
                        );
                        Ok(response)
                    },
                );
                if let Ok(socket) = socket {
                    script(index, socket);
                }
            });
        }
    });
    format!("ws://127.0.0.1:{port}/v1/machine")
}

fn send_frame(ws: &mut Ws, frame: &RelayFrame) {
    ws.send(Message::text(frame.to_json().expect("frame")))
        .expect("relay send");
}

fn read_frame(ws: &mut Ws) -> Option<RelayFrame> {
    loop {
        match ws.read() {
            Ok(Message::Text(text)) => return Some(RelayFrame::from_json(text.as_str()).expect("machine frame")),
            Ok(Message::Close(_)) | Err(_) => return None,
            Ok(_) => {}
        }
    }
}

fn close_with(ws: &mut Ws, code: u16, reason: &str) {
    let _ = ws.close(Some(CloseFrame {
        code: CloseCode::from(code),
        reason: reason.to_owned().into(),
    }));
    let _ = ws.flush();
}

struct Login {
    machine_id: String,
    account_id: String,
    key_id: String,
    mkey: String,
    credential: String,
}

/// The relay's side of machine login: verify the credential with the API JWKS, challenge, verify
/// the signature, refuse a replayed nonce, welcome.
fn relay_login(ws: &mut Ws, nonces: &mut NonceSet, jwks: &Jwks) -> Result<Login, String> {
    let Some(RelayFrame::MachineHello {
        machine_id,
        account_id,
        key_id,
        public_key,
        credential,
        ..
    }) = read_frame(ws)
    else {
        return Err("no machine.hello".to_owned());
    };
    let reject = |ws: &mut Ws, why: &str| {
        send_frame(
            ws,
            &RelayFrame::Error {
                v: 1,
                code: codes::UNAUTHENTICATED.to_owned(),
                message: "no".to_owned(),
                retryable: false,
            },
        );
        close_with(ws, 4001, "unauthenticated");
        Err(why.to_owned())
    };
    let jwt = Jwt::parse(&credential).map_err(|error| error.to_string())?;
    if jwt.verify(jwks).is_err() {
        return reject(ws, "credential signature");
    }
    let claims: CredentialClaims = jwt.claims().map_err(|error| error.to_string())?;
    if claims.exp <= now_secs() {
        return reject(ws, "credential expired");
    }
    if claims.mkey != public_key || claims.key_id != key_id || claims.sub != format!("machine:{machine_id}") {
        return reject(ws, "credential does not match hello");
    }
    let server_time = Timestamp::now();
    let nonce = format!("nonce-{}", server_time.unix_millis());
    send_frame(
        ws,
        &RelayFrame::RelayChallenge {
            v: 1,
            relay_id: "relay-test".to_owned(),
            session_id: "sess-test".to_owned(),
            nonce: nonce.clone(),
            server_time,
            heartbeat_secs: 1,
            max_frame_bytes: 1_048_576,
            features: vec![],
        },
    );
    let Some(RelayFrame::MachineAuth { signature, .. }) = read_frame(ws) else {
        return Err("no machine.auth".to_owned());
    };
    let canonical = machine_auth_canonical(
        &machine_id,
        &account_id,
        &key_id,
        "relay-test",
        "sess-test",
        &nonce,
        &server_time.as_rfc3339(),
    );
    if nonces.consume(&nonce).is_err() || verify_machine_auth(&claims.mkey, &canonical, &signature).is_err() {
        return reject(ws, "bad machine signature");
    }
    send_frame(
        ws,
        &RelayFrame::RelayWelcome {
            v: 1,
            relay_id: "relay-test".to_owned(),
            session_id: "sess-test".to_owned(),
            machine_id: machine_id.clone(),
            account_id: account_id.clone(),
            heartbeat_secs: 1,
            max_frame_bytes: 1_048_576,
            features: vec![],
        },
    );
    Ok(Login {
        machine_id,
        account_id,
        key_id,
        mkey: claims.mkey,
        credential,
    })
}

// ---------------------------------------------------------------------------------------------
// fake client
// ---------------------------------------------------------------------------------------------

struct TicketSpec {
    scopes: Vec<Scope>,
    kind: ClientKind,
    exp_in: i64,
    machine: String,
    mkey: String,
    jkt: Option<String>,
    jti: String,
    signing: Arc<SigningKey>,
    kid: String,
    alg_none: bool,
}

impl TicketSpec {
    fn new(login: &Login, scopes: &[Scope], jti: &str) -> Self {
        Self {
            scopes: scopes.to_vec(),
            kind: ClientKind::Cli,
            exp_in: 120,
            machine: login.machine_id.clone(),
            mkey: login.mkey.clone(),
            jkt: None,
            jti: jti.to_owned(),
            signing: Arc::new(api_signing_key()),
            kid: API_KID.to_owned(),
            alg_none: false,
        }
    }
}

fn mint_ticket(api_url: &str, login: &Login, spec: &TicketSpec, client_key: &Jwk) -> String {
    let now = now_secs();
    let claims = json!({
        "iss": api_url.trim_end_matches('/'), "aud": "127.0.0.1", "sub": "client:cli_test",
        "acct": login.account_id, "machine": spec.machine, "kind": spec.kind, "scopes": spec.scopes,
        "cnf": {"jkt": spec.jkt.clone().unwrap_or_else(|| jwk_thumbprint(client_key))},
        "mkey": spec.mkey, "mkey_id": format!("ed25519:{}", login.mkey),
        "iat": now, "exp": now + spec.exp_in, "jti": spec.jti,
    });
    if spec.alg_none {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"none","kid":"test-kid-1"}"#);
        return format!("{header}.{}.", URL_SAFE_NO_PAD.encode(claims.to_string()));
    }
    sign_es256(&spec.signing, &spec.kid, &claims)
}

struct ClientChannel {
    raw_id: [u8; 16],
    cipher: ChannelCipher,
}

enum Opened {
    Accepted(Box<ClientChannel>),
    Rejected(String),
}

fn channel_id(n: u8) -> String {
    RelayDataFrame::channel_string([n; 16])
}

/// Relay side: forward `channel.open` and complete the client half of the handshake.
fn client_open(
    ws: &mut Ws,
    login: &Login,
    ticket: &str,
    scopes: &[Scope],
    kind: ClientKind,
    client: &EphemeralKey,
    id: &str,
) -> Opened {
    let nonce = vowp_agent::relay::random_nonce();
    send_frame(
        ws,
        &RelayFrame::ChannelOpen {
            v: 1,
            channel_id: id.to_owned(),
            machine_id: login.machine_id.clone(),
            client_id: "cli_test".to_owned(),
            kind,
            ticket: ticket.to_owned(),
            scopes: scopes.to_vec(),
            client_key: client.jwk(),
            client_nonce: nonce.clone(),
        },
    );
    loop {
        match read_frame(ws) {
            Some(RelayFrame::ChannelAccept {
                machine_key,
                machine_nonce,
                machine_key_id,
                machine_signature,
                suite,
                ..
            }) => {
                assert_eq!(suite, vowp_protocol::E2E_SUITE);
                assert_eq!(
                    machine_key_id, login.key_id,
                    "machine_key_id is the long-lived identity id"
                );
                let claims: vowp_agent::relay::auth::TicketClaims =
                    Jwt::parse(ticket).expect("ticket").claims().expect("claims");
                let cipher = client_finish(
                    client,
                    &AcceptInput {
                        channel_id: id,
                        client_key: &client.jwk(),
                        client_nonce: &nonce,
                        machine_id: &login.machine_id,
                        account_id: &login.account_id,
                        ticket_jti: &claims.jti,
                    },
                    &machine_key,
                    &machine_nonce,
                    &machine_signature,
                    &login.mkey,
                )
                .expect("the machine signature verifies against the ticket mkey");
                return Opened::Accepted(Box::new(ClientChannel {
                    raw_id: RelayDataFrame::channel_bytes(id).expect("ulid"),
                    cipher,
                }));
            }
            Some(RelayFrame::ChannelReject { code, channel_id, .. }) => {
                assert_eq!(channel_id, id);
                return Opened::Rejected(code);
            }
            Some(_) => {}
            None => return Opened::Rejected("connection closed".to_owned()),
        }
    }
}

impl ClientChannel {
    fn send(&mut self, ws: &mut Ws, frame: &RemoteFrame) {
        let sealed = self.cipher.seal(&frame.to_json().expect("frame")).expect("seal");
        assert_eq!(sealed.channel_id, self.raw_id);
        ws.send(Message::binary(sealed.to_bytes().expect("bytes")))
            .expect("send");
    }

    /// The next remote frame, or the code of an operational `channel.close`.
    fn recv(&mut self, ws: &mut Ws) -> Result<RemoteFrame, String> {
        loop {
            match ws.read() {
                Ok(Message::Binary(bytes)) => {
                    let frame = RelayDataFrame::from_bytes(&bytes).expect("data frame");
                    let plain = self.cipher.open(&frame).map_err(|error| error.to_string())?;
                    return Ok(RemoteFrame::from_json(&plain).expect("remote frame"));
                }
                Ok(Message::Text(text)) => {
                    if let RelayFrame::ChannelClose { code, .. } = RelayFrame::from_json(text.as_str()).expect("frame")
                    {
                        return Err(code);
                    }
                }
                Ok(_) => {}
                Err(error) => return Err(format!("closed: {error}")),
            }
        }
    }

    fn hello(&mut self, ws: &mut Ws, login: &Login, scopes: &[Scope]) -> RemoteFrame {
        self.send(
            ws,
            &RemoteFrame::Hello {
                v: 1,
                client: "cli_test".to_owned(),
                machine: login.machine_id.clone(),
                scopes: scopes.to_vec(),
                capabilities: vec![],
            },
        );
        self.recv(ws).expect("hello answer")
    }
}

/// Status, headers and the decoded body of one proxied response.
type HttpAnswer = (u16, BTreeMap<String, String>, Vec<u8>);

/// Collects one full HTTP answer: the head and the decoded body bytes, or a stream error code.
fn http_get(chan: &mut ClientChannel, ws: &mut Ws, stream: u32, path: &str) -> Result<HttpAnswer, String> {
    chan.send(
        ws,
        &RemoteFrame::HttpRequest {
            stream,
            method: "GET".to_owned(),
            path: path.to_owned(),
            headers: BTreeMap::new(),
            body: String::new(),
        },
    );
    http_answer(chan, ws, stream)
}

fn http_answer(chan: &mut ClientChannel, ws: &mut Ws, stream: u32) -> Result<HttpAnswer, String> {
    let mut head = None;
    let mut body = Vec::new();
    loop {
        match chan.recv(ws)? {
            RemoteFrame::HttpResponse {
                stream: s,
                status,
                headers,
            } if s == stream => head = Some((status, headers)),
            RemoteFrame::HttpBody {
                stream: s, data, end, ..
            } if s == stream => {
                body.extend(BASE64.decode(data).expect("base64"));
                if end {
                    let (status, headers) = head.ok_or("body before head")?;
                    return Ok((status, headers, body));
                }
            }
            RemoteFrame::StreamError { stream: s, code, .. } if s == stream => return Err(code),
            RemoteFrame::Ping { .. } | RemoteFrame::Pong { .. } => {}
            other => return Err(format!("unexpected frame {other:?}")),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// harness
// ---------------------------------------------------------------------------------------------

struct Harness {
    home: TempDir,
    api: FakeApi,
    key: RelayKey,
    config: RelayConfig,
    stop: Arc<AtomicBool>,
    log: Arc<Mutex<Vec<String>>>,
    daemon_log: Arc<DaemonLog>,
}

impl Harness {
    /// An enrolled home, a fake API and a fake daemon. `relay_url` is filled in by `connect`.
    fn new() -> Self {
        let api = FakeApi::start("ssm_test-only-token-0123456789abcdef");
        let (daemon_url, daemon_log) = fake_daemon();
        let home = tempfile::tempdir().expect("home");
        let key = RelayKey::generate();
        key.write(home.path(), "relay.key").expect("key");
        api.register(&key.public_key_hex());
        let config = RelayConfig {
            enabled: true,
            api_url: api.url.clone(),
            relay_url: "ws://127.0.0.1:9/v1/machine".to_owned(),
            project: "vowp".to_owned(),
            daemon_url,
            heartbeat_secs: 1,
            max_channels: 4,
            ..RelayConfig::default()
        };
        let harness = Self {
            home,
            api,
            key,
            config,
            stop: Arc::new(AtomicBool::new(false)),
            log: Arc::new(Mutex::new(Vec::new())),
            daemon_log,
        };
        harness.write_record(86_400);
        harness
    }

    fn write_record(&self, ttl: i64) {
        let (credential, expires) = self.api.credential_for(&self.key.public_key_hex(), ttl);
        RelayJson {
            v: 1,
            api_url: self.config.api_url.clone(),
            relay_url: self.config.relay_url.clone(),
            machine_id: MACHINE_ID.to_owned(),
            account_id: ACCOUNT_ID.to_owned(),
            key_id: self.key.key_id(),
            credential,
            credential_expires_at: expires,
            name: "test-box".to_owned(),
            project: "vowp".to_owned(),
            ssh_user: "stackspirit".to_owned(),
            registered_at: Timestamp::now().as_rfc3339(),
        }
        .save(&self.home.path().join("relay.json"))
        .expect("relay.json");
    }

    fn login_record(&self) -> RelayJson {
        RelayJson::load(&self.home.path().join("relay.json")).expect("relay.json")
    }

    /// Points the machine at `relay_url` (rewriting relay.json, as enrollment would).
    fn point_at(&mut self, relay_url: &str) {
        self.config.relay_url = relay_url.to_owned();
        let mut record = self.login_record();
        record.relay_url = relay_url.to_owned();
        record.save(&self.home.path().join("relay.json")).expect("save");
    }

    fn options(&self) -> ConnectorOptions {
        let mut options = ConnectorOptions::new(self.home.path().to_owned(), self.config.clone());
        options.stop = Arc::clone(&self.stop);
        let log = Arc::clone(&self.log);
        options.log = Arc::new(move |line| log.lock().expect("lock").push(line.to_owned()));
        options.reconnect_min = Duration::from_millis(40);
        options.reconnect_max = Duration::from_millis(400);
        options.api_token = None;
        options
    }

    fn start(&self) -> JoinHandle<Result<ConnectorReport, RelayError>> {
        self.start_with(self.options())
    }

    fn start_with(&self, options: ConnectorOptions) -> JoinHandle<Result<ConnectorReport, RelayError>> {
        std::thread::spawn(move || Connector::new(options).run())
    }

    fn finish(&self, handle: JoinHandle<Result<ConnectorReport, RelayError>>) -> ConnectorReport {
        self.stop.store(true, Ordering::SeqCst);
        handle.join().expect("connector thread").expect("connector result")
    }

    fn logged(&self) -> String {
        self.log.lock().expect("lock").join("\n")
    }

    fn status_opt(&self) -> Option<RelayStatusFile> {
        RelayStatusFile::load(&RelayPaths::new(self.home.path(), &self.config).status())
    }

    fn status(&self) -> RelayStatusFile {
        self.status_opt().expect("status file")
    }
}

/// A relay that logs the machine in and runs `body` once, then closes.
fn relay_once<T: Send + 'static>(
    jwks: Jwks,
    body: impl Fn(&mut Ws, &Login) -> T + Send + Sync + 'static,
) -> (String, std::sync::mpsc::Receiver<T>) {
    let (tx, rx) = std::sync::mpsc::channel();
    let tx = Mutex::new(tx);
    let url = spawn_relay(move |index, mut ws| {
        let mut nonces = NonceSet::default();
        let Ok(login) = relay_login(&mut ws, &mut nonces, &jwks) else {
            return;
        };
        let result = body(&mut ws, &login);
        if index == 0 {
            let _ = tx.lock().expect("lock").send(result);
        }
        // Keep the connection open until the machine goes away so the test controls timing.
        while ws.read().is_ok() {}
    });
    (url, rx)
}

fn all_jwks() -> Jwks {
    jwks_for(&api_signing_key())
}

// ---------------------------------------------------------------------------------------------
// authentication primitives
// ---------------------------------------------------------------------------------------------

#[test]
fn relay_rejects_a_bad_machine_signature() {
    let key = RelayKey::generate();
    let other = RelayKey::generate();
    let canonical = machine_auth_canonical("m", "a", &key.key_id(), "r", "s", "n", "2026-11-25T12:00:00.000Z");
    let signature = sign_machine_auth(&key, &canonical);
    verify_machine_auth(&key.public_key_hex(), &canonical, &signature).expect("good signature");
    assert!(
        verify_machine_auth(&other.public_key_hex(), &canonical, &signature).is_err(),
        "wrong key"
    );
    assert!(
        verify_machine_auth(
            &key.public_key_hex(),
            &canonical,
            &sign_machine_auth(&other, &canonical)
        )
        .is_err(),
        "signature by another key"
    );
    let mut flipped = signature.clone().into_bytes();
    flipped[0] = if flipped[0] == b'0' { b'1' } else { b'0' };
    assert!(
        verify_machine_auth(
            &key.public_key_hex(),
            &canonical,
            std::str::from_utf8(&flipped).expect("utf8")
        )
        .is_err()
    );
    assert!(
        verify_machine_auth(&key.public_key_hex(), &canonical, "").is_err(),
        "empty signature"
    );
    assert!(
        verify_machine_auth(&key.public_key_hex(), &canonical, "zz").is_err(),
        "non-hex signature"
    );
    assert!(
        verify_machine_auth("00", &canonical, &signature).is_err(),
        "short public key"
    );
}

#[test]
fn relay_rejects_a_replayed_challenge_nonce() {
    let mut nonces = NonceSet::default();
    nonces.consume("nonce-1").expect("first use");
    nonces.consume("nonce-2").expect("another nonce");
    let error = nonces.consume("nonce-1").expect_err("a replayed nonce");
    assert!(matches!(error, RelayError::Auth(_)));
}

#[test]
fn relay_rejects_an_expired_credential() {
    let api = FakeApi::start("ssm_x");
    let now = now_secs();
    for (ttl, expired_now, expiring_soon) in [(-5, true, true), (300, false, true), (3_600, false, false)] {
        let (credential, _) = mint_credential(&api.url, &api.signing, MACHINE_ID, &"ab".repeat(32), ttl, 1);
        let claims = CredentialClaims::decode(&credential).expect("claims");
        assert_eq!(claims.expires_within(now, 0), expired_now, "ttl {ttl}");
        assert_eq!(claims.expires_within(now, 600), expiring_soon, "ttl {ttl}");
    }
    // The relay refuses an expired credential at login; the connector then never comes online.
    let mut h = Harness::new();
    h.write_record(-30);
    let (url, rx) = relay_once(all_jwks(), |_, _| ());
    h.point_at(&url);
    h.api.state.revoked.store(true, Ordering::SeqCst); // the refresh is refused, so the credential stays expired
    let report = h.finish_after_revocation();
    assert_eq!(report.outcome, ConnectorOutcome::Revoked);
    assert!(rx.try_recv().is_err(), "no session ever reached the channel stage");
}

impl Harness {
    fn finish_after_revocation(&self) -> ConnectorReport {
        let handle = self.start();
        handle.join().expect("connector thread").expect("connector result")
    }
}

// ---------------------------------------------------------------------------------------------
// ticket verification
// ---------------------------------------------------------------------------------------------

fn ticket_login() -> Login {
    let key = RelayKey::generate();
    Login {
        machine_id: MACHINE_ID.to_owned(),
        account_id: ACCOUNT_ID.to_owned(),
        key_id: key.key_id(),
        mkey: key.public_key_hex(),
        credential: String::new(),
    }
}

fn verify_with(
    login: &Login,
    spec: &TicketSpec,
    client: &Jwk,
    replay: &mut ReplayCache,
    jwks: &Jwks,
) -> Result<(), String> {
    let api_url = "http://127.0.0.1:1";
    let token = mint_ticket(api_url, login, spec, client);
    verify_ticket(
        &token,
        &TicketContext {
            jwks,
            issuer: api_url,
            audience: "127.0.0.1",
            machine_id: &login.machine_id,
            account_id: &login.account_id,
            key_id: &login.key_id,
            mkey: &login.mkey,
            client_key: client,
            now_unix: now_secs(),
        },
        replay,
    )
    .map(|_| ())
    .map_err(|error| error.code.to_owned())
}

#[test]
fn relay_rejects_an_expired_ticket() {
    let login = ticket_login();
    let client = EphemeralKey::generate().jwk();
    let mut replay = ReplayCache::default();
    let mut spec = TicketSpec::new(&login, &[Scope::HttpRead], "jti_a");
    verify_with(&login, &spec, &client, &mut replay, &all_jwks()).expect("a fresh ticket verifies");
    spec.exp_in = -1;
    spec.jti = "jti_b".to_owned();
    assert_eq!(
        verify_with(&login, &spec, &client, &mut replay, &all_jwks()),
        Err("ticket_expired".to_owned())
    );
    spec.exp_in = 301;
    spec.jti = "jti_c".to_owned();
    assert_eq!(
        verify_with(&login, &spec, &client, &mut replay, &all_jwks()),
        Err("ticket_invalid".to_owned()),
        "a ticket may live at most 300 seconds"
    );
}

#[test]
fn relay_ticket_binds_machine_key_client_key_and_signature() {
    let login = ticket_login();
    let client = EphemeralKey::generate().jwk();
    let jwks = all_jwks();
    let mut replay = ReplayCache::default();
    let base = |jti: &str| TicketSpec::new(&login, &[Scope::HttpRead], jti);

    // Key substitution: the ticket confirms a different client key.
    let mut spec = base("jti_1");
    spec.jkt = Some(jwk_thumbprint(&EphemeralKey::generate().jwk()));
    assert_eq!(
        verify_with(&login, &spec, &client, &mut replay, &jwks),
        Err("ticket_invalid".to_owned()),
        "cnf.jkt"
    );
    // Another machine, another machine key.
    let mut spec = base("jti_2");
    spec.machine = "mch_OTHER".to_owned();
    assert_eq!(
        verify_with(&login, &spec, &client, &mut replay, &jwks),
        Err("ticket_invalid".to_owned()),
        "machine"
    );
    let mut spec = base("jti_3");
    spec.mkey = RelayKey::generate().public_key_hex();
    assert_eq!(
        verify_with(&login, &spec, &client, &mut replay, &jwks),
        Err("ticket_invalid".to_owned()),
        "mkey"
    );
    // Signed by a key the JWKS does not publish, an unknown kid, and alg none.
    let mut spec = base("jti_4");
    spec.signing = Arc::new(SigningKey::from_slice(&[0x99u8; 32]).expect("key"));
    assert_eq!(
        verify_with(&login, &spec, &client, &mut replay, &jwks),
        Err("ticket_invalid".to_owned()),
        "foreign signer"
    );
    let mut spec = base("jti_5");
    spec.kid = "unknown".to_owned();
    assert_eq!(
        verify_with(&login, &spec, &client, &mut replay, &jwks),
        Err("ticket_invalid".to_owned()),
        "kid"
    );
    let mut spec = base("jti_6");
    spec.alg_none = true;
    assert_eq!(
        verify_with(&login, &spec, &client, &mut replay, &jwks),
        Err("ticket_invalid".to_owned()),
        "alg none"
    );
    assert_eq!(
        verify_with(&login, &base("jti_7"), &client, &mut replay, &Jwks::default()),
        Err("ticket_invalid".to_owned()),
        "no trust-any-key fallback"
    );

    // None of those burned a jti; the same ticket id then works once and only once.
    verify_with(&login, &base("jti_8"), &client, &mut replay, &jwks).expect("valid");
    assert_eq!(
        verify_with(&login, &base("jti_8"), &client, &mut replay, &jwks),
        Err("ticket_replayed".to_owned())
    );
}

#[test]
fn relay_replay_cache_forgets_after_ten_minutes() {
    let mut cache = ReplayCache::default();
    assert!(cache.first_use("jti_x", 1_000));
    assert!(!cache.first_use("jti_x", 1_599));
    assert!(cache.first_use("jti_x", 1_601), "a jti is remembered for 600 seconds");
}

// ---------------------------------------------------------------------------------------------
// connector: one HTTP read, scopes, tickets
// ---------------------------------------------------------------------------------------------

fn open_ok(
    ws: &mut Ws,
    login: &Login,
    api_url: &str,
    scopes: &[Scope],
    jti: &str,
    n: u8,
) -> (ClientChannel, EphemeralKey) {
    let client = EphemeralKey::generate();
    let ticket = mint_ticket(api_url, login, &TicketSpec::new(login, scopes, jti), &client.jwk());
    match client_open(ws, login, &ticket, scopes, ClientKind::Cli, &client, &channel_id(n)) {
        Opened::Accepted(chan) => (*chan, client),
        Opened::Rejected(code) => panic!("channel rejected: {code}"),
    }
}

#[test]
fn machine_connector_serves_one_http_read_through_the_relay() {
    let mut h = Harness::new();
    let api_url = h.api.url.clone();
    let (url, rx) = relay_once(all_jwks(), move |ws, login| {
        let (mut chan, _) = open_ok(ws, login, &api_url, &[Scope::HttpRead], "jti_read", 1);
        let ack = chan.hello(ws, login, &[Scope::HttpRead]);
        let read = http_get(&mut chan, ws, 1, "/api/v1/health");
        let missing = http_get(&mut chan, ws, 3, "/api/v1/missing");
        chan.send(ws, &RemoteFrame::Ping { nonce: "p1".to_owned() });
        let pong = chan.recv(ws);
        (format!("{ack:?}"), read, missing, format!("{pong:?}"))
    });
    h.point_at(&url);
    let handle = h.start();
    let (ack, read, missing, pong) = rx.recv_timeout(Duration::from_secs(20)).expect("relay script");
    assert!(
        ack.contains("HelloAck") && ack.contains("HttpRead") && !ack.contains("HttpWrite"),
        "{ack}"
    );
    let (status, headers, body) = read.expect("an HTTP read through the connector");
    assert_eq!(status, 200);
    let text = String::from_utf8(body).expect("utf8");
    assert!(text.contains(DAEMON_MARKER), "the daemon body arrives intact: {text}");
    assert_eq!(
        headers.get("Content-Type").map(String::as_str),
        Some("application/json")
    );
    assert_eq!(headers.get("ETag").map(String::as_str), Some("\"abc\""));
    assert!(
        !headers.keys().any(|name| name.eq_ignore_ascii_case("set-cookie")),
        "Set-Cookie is never forwarded"
    );
    assert!(
        !headers.keys().any(|name| name.eq_ignore_ascii_case("x-internal")),
        "unlisted headers are dropped"
    );
    let (status, _, _) = missing.expect("a 404 is a normal response, not a stream error");
    assert_eq!(status, 404);
    assert!(pong.contains("Pong"), "{pong}");

    // The daemon saw one clean GET with no credential header.
    let requests = h.daemon_log.requests.lock().expect("lock").clone();
    assert!(
        requests
            .iter()
            .any(|(method, path, _)| method == "GET" && path == "/api/v1/health")
    );
    for (_, _, headers) in &requests {
        assert!(!headers.contains_key("authorization") && !headers.contains_key("cookie"));
    }
    eventually("online status", || {
        RelayStatusFile::load(&RelayPaths::new(h.home.path(), &h.config).status())
            .is_some_and(|status| status.state == ConnectorState::Online && status.channels == 1)
    });
    let report = h.finish(handle);
    assert_eq!(report.outcome, ConnectorOutcome::Stopped);
    assert_eq!(report.connects, 1);

    // The connector never writes a payload, ticket, credential or key to its log or status file.
    let log = h.logged();
    assert!(!log.is_empty(), "the connector logs events");
    let record = h.login_record();
    let status = std::fs::read_to_string(h.home.path().join("relay-status.json")).expect("status");
    for secret in [
        DAEMON_MARKER,
        &record.credential,
        "jti_read",
        "api/v1/health",
        &hex::encode(h.key.public_key_hex()),
    ] {
        assert!(!log.contains(secret), "log leaked {secret}: {log}");
        assert!(!status.contains(secret), "status leaked {secret}");
    }
    assert!(!status.contains("credential"));
}

#[test]
fn machine_connector_streams_server_sent_events_and_stops_them_on_cancel() {
    let mut h = Harness::new();
    let api_url = h.api.url.clone();
    let (url, rx) = relay_once(all_jwks(), move |ws, login| {
        let (mut chan, _) = open_ok(ws, login, &api_url, &[Scope::HttpRead], "jti_sse", 2);
        chan.hello(ws, login, &[Scope::HttpRead]);
        let sse = http_get(&mut chan, ws, 1, "/api/v1/board/stream");
        let big = http_get(&mut chan, ws, 5, "/api/v1/big");
        (sse, big)
    });
    h.point_at(&url);
    let handle = h.start();
    let (sse, big) = rx.recv_timeout(Duration::from_secs(20)).expect("script");
    let (status, headers, body) = sse.expect("sse");
    assert_eq!(status, 200);
    assert_eq!(
        headers.get("Content-Type").map(String::as_str),
        Some("text/event-stream")
    );
    assert_eq!(String::from_utf8(body).expect("utf8"), "data: one\n\ndata: two\n\n");
    let (_, _, body) = big.expect("a 700 KB body is chunked and reassembled");
    assert_eq!(body.len(), 700_000);
    h.finish(handle);
}

#[test]
fn relay_channel_rejects_a_scope_escalation() {
    let mut h = Harness::new();
    h.config.allow_ssh = false; // local policy: ssh stays off
    let api_url = h.api.url.clone();
    let (url, rx) = relay_once(all_jwks(), move |ws, login| {
        let mut outcomes = Vec::new();
        // 1. The client asks for http.write but its ticket only grants http.read.
        let client = EphemeralKey::generate();
        let ticket = mint_ticket(
            &api_url,
            login,
            &TicketSpec::new(login, &[Scope::HttpRead], "jti_e1"),
            &client.jwk(),
        );
        outcomes.push(
            match client_open(
                ws,
                login,
                &ticket,
                &[Scope::HttpRead, Scope::HttpWrite],
                ClientKind::Cli,
                &client,
                &channel_id(1),
            ) {
                Opened::Rejected(code) => code,
                Opened::Accepted(_) => "accepted".to_owned(),
            },
        );
        // 2. The ticket grants ssh but the machine did not enable it locally.
        let client = EphemeralKey::generate();
        let ticket = mint_ticket(
            &api_url,
            login,
            &TicketSpec::new(login, &[Scope::HttpRead, Scope::Ssh], "jti_e2"),
            &client.jwk(),
        );
        outcomes.push(
            match client_open(
                ws,
                login,
                &ticket,
                &[Scope::Ssh],
                ClientKind::Cli,
                &client,
                &channel_id(2),
            ) {
                Opened::Rejected(code) => code,
                Opened::Accepted(_) => "accepted".to_owned(),
            },
        );
        // 3. http.write is in the ticket but allow_http_write is false.
        let client = EphemeralKey::generate();
        let ticket = mint_ticket(
            &api_url,
            login,
            &TicketSpec::new(login, &[Scope::HttpWrite], "jti_e3"),
            &client.jwk(),
        );
        outcomes.push(
            match client_open(
                ws,
                login,
                &ticket,
                &[Scope::HttpWrite],
                ClientKind::Cli,
                &client,
                &channel_id(3),
            ) {
                Opened::Rejected(code) => code,
                Opened::Accepted(_) => "accepted".to_owned(),
            },
        );
        // 4. No scopes at all.
        let client = EphemeralKey::generate();
        let ticket = mint_ticket(
            &api_url,
            login,
            &TicketSpec::new(login, &[Scope::HttpRead], "jti_e4"),
            &client.jwk(),
        );
        outcomes.push(
            match client_open(ws, login, &ticket, &[], ClientKind::Cli, &client, &channel_id(4)) {
                Opened::Rejected(code) => code,
                Opened::Accepted(_) => "accepted".to_owned(),
            },
        );
        // 5. A client kind that differs from the ticket.
        let client = EphemeralKey::generate();
        let ticket = mint_ticket(
            &api_url,
            login,
            &TicketSpec::new(login, &[Scope::HttpRead], "jti_e5"),
            &client.jwk(),
        );
        outcomes.push(
            match client_open(
                ws,
                login,
                &ticket,
                &[Scope::HttpRead],
                ClientKind::Mcp,
                &client,
                &channel_id(5),
            ) {
                Opened::Rejected(code) => code,
                Opened::Accepted(_) => "accepted".to_owned(),
            },
        );
        // 6. An accepted read channel whose Hello asks for more than it was granted.
        let (mut chan, _) = open_ok(ws, login, &api_url, &[Scope::HttpRead], "jti_e6", 6);
        chan.send(
            ws,
            &RemoteFrame::Hello {
                v: 1,
                client: "c".to_owned(),
                machine: login.machine_id.clone(),
                scopes: vec![Scope::HttpRead, Scope::McpWrite],
                capabilities: vec![],
            },
        );
        outcomes.push(format!("{:?}", chan.recv(ws)));
        outcomes
    });
    h.point_at(&url);
    let handle = h.start();
    let outcomes = rx.recv_timeout(Duration::from_secs(20)).expect("script");
    assert_eq!(outcomes[0], "scope_denied", "ticket lacks http.write");
    assert_eq!(outcomes[1], "scope_denied", "ssh not enabled locally");
    assert_eq!(outcomes[2], "scope_denied", "http.write not enabled locally");
    assert_eq!(outcomes[3], "bad_request", "no scopes");
    assert_eq!(outcomes[4], "ticket_invalid", "kind mismatch");
    assert!(
        outcomes[5].contains("Close") && outcomes[5].contains("scope_denied"),
        "{}",
        outcomes[5]
    );
    h.finish(handle);
}

#[test]
fn relay_channel_rejects_a_replayed_ticket() {
    let mut h = Harness::new();
    let api_url = h.api.url.clone();
    let (url, rx) = relay_once(all_jwks(), move |ws, login| {
        let client = EphemeralKey::generate();
        let ticket = mint_ticket(
            &api_url,
            login,
            &TicketSpec::new(login, &[Scope::HttpRead], "jti_once"),
            &client.jwk(),
        );
        let first = matches!(
            client_open(
                ws,
                login,
                &ticket,
                &[Scope::HttpRead],
                ClientKind::Cli,
                &client,
                &channel_id(1)
            ),
            Opened::Accepted(_)
        );
        let second = match client_open(
            ws,
            login,
            &ticket,
            &[Scope::HttpRead],
            ClientKind::Cli,
            &client,
            &channel_id(2),
        ) {
            Opened::Rejected(code) => code,
            Opened::Accepted(_) => "accepted".to_owned(),
        };
        (first, second)
    });
    h.point_at(&url);
    let handle = h.start();
    let (first, second) = rx.recv_timeout(Duration::from_secs(20)).expect("script");
    assert!(first, "the first use is accepted");
    assert_eq!(second, "ticket_replayed");
    h.finish(handle);
}

#[test]
fn relay_channel_rejects_expired_substituted_and_foreign_tickets() {
    let mut h = Harness::new();
    let api_url = h.api.url.clone();
    let (url, rx) = relay_once(all_jwks(), move |ws, login| {
        let mut codes = Vec::new();
        let mut attempt = |n: u8, tweak: &dyn Fn(&mut TicketSpec), client_for_open: Option<&EphemeralKey>| {
            let client = EphemeralKey::generate();
            let mut spec = TicketSpec::new(login, &[Scope::HttpRead], &format!("jti_x{n}"));
            tweak(&mut spec);
            let ticket = mint_ticket(&api_url, login, &spec, &client.jwk());
            let presenter = client_for_open.unwrap_or(&client);
            match client_open(
                ws,
                login,
                &ticket,
                &[Scope::HttpRead],
                ClientKind::Cli,
                presenter,
                &channel_id(n),
            ) {
                Opened::Rejected(code) => code,
                Opened::Accepted(_) => "accepted".to_owned(),
            }
        };
        codes.push(attempt(1, &|spec| spec.exp_in = -10, None));
        // The ticket confirms one key but the relay presents another: key substitution.
        let thief = EphemeralKey::generate();
        codes.push(attempt(2, &|_| {}, Some(&thief)));
        codes.push(attempt(3, &|spec| spec.mkey = "00".repeat(32), None));
        codes.push(attempt(
            4,
            &|spec| spec.signing = Arc::new(SigningKey::from_slice(&[0x42; 32]).expect("key")),
            None,
        ));
        codes.push(attempt(5, &|spec| spec.alg_none = true, None));
        codes
    });
    h.point_at(&url);
    let handle = h.start();
    let codes = rx.recv_timeout(Duration::from_secs(20)).expect("script");
    assert_eq!(
        codes,
        [
            "ticket_expired",
            "ticket_invalid",
            "ticket_invalid",
            "ticket_invalid",
            "ticket_invalid"
        ]
    );
    h.finish(handle);
}

#[test]
fn machine_connector_types_unwired_channels_and_enforces_http_policy() {
    let mut h = Harness::new();
    h.config.allow_http_write = true;
    h.config.allow_mcp_write = false;
    let api_url = h.api.url.clone();
    let (url, rx) = relay_once(all_jwks(), move |ws, login| {
        let (mut chan, _) = open_ok(
            ws,
            login,
            &api_url,
            &[Scope::HttpRead, Scope::HttpWrite, Scope::McpRead],
            "jti_pol",
            1,
        );
        chan.hello(ws, login, &[Scope::HttpRead, Scope::HttpWrite, Scope::McpRead]);
        let mut results: BTreeMap<&'static str, String> = BTreeMap::new();
        // MCP is granted but not wired; TCP and exec were never granted.
        chan.send(
            ws,
            &RemoteFrame::McpRequest {
                stream: 1,
                project: None,
                session: None,
                request: json!({"jsonrpc":"2.0","id":1,"method":"ping"}),
            },
        );
        results.insert("mcp", stream_error_code(chan.recv(ws)));
        chan.send(
            ws,
            &RemoteFrame::TcpOpen {
                stream: 2,
                target: "ssh".to_owned(),
            },
        );
        results.insert("tcp", stream_error_code(chan.recv(ws)));
        chan.send(
            ws,
            &RemoteFrame::ExecStart {
                stream: 3,
                argv: vec!["/bin/true".to_owned()],
                cwd: None,
                pty: false,
                cols: 0,
                rows: 0,
                env: BTreeMap::new(),
            },
        );
        results.insert("exec", stream_error_code(chan.recv(ws)));
        // Policy: each of these must be refused by the machine before the daemon sees it.
        for (key, path) in [
            ("dotdot", "/api/v1/../etc/passwd"),
            ("encoded", "/api/v1/jobs%2fx"),
            ("backslash", "/api/v1/jobs\\x"),
            ("token", "/api/v1/health?token=abc"),
            ("outside", "/health"),
            ("static", "/sw.js"),
        ] {
            let outcome = http_get(&mut chan, ws, 10, path);
            results.insert(key, outcome.err().unwrap_or_else(|| "served".to_owned()));
        }
        // A forbidden request header and a POST outside the write routes.
        let mut headers = BTreeMap::new();
        headers.insert("Authorization".to_owned(), "Bearer smuggled".to_owned());
        chan.send(
            ws,
            &RemoteFrame::HttpRequest {
                stream: 11,
                method: "GET".to_owned(),
                path: "/api/v1/health".to_owned(),
                headers,
                body: String::new(),
            },
        );
        results.insert("auth_header", stream_error_code(chan.recv(ws)));
        chan.send(
            ws,
            &RemoteFrame::HttpRequest {
                stream: 12,
                method: "POST".to_owned(),
                path: "/api/v1/board".to_owned(),
                headers: BTreeMap::new(),
                body: String::new(),
            },
        );
        results.insert("post_other", stream_error_code(chan.recv(ws)));
        chan.send(
            ws,
            &RemoteFrame::HttpRequest {
                stream: 13,
                method: "DELETE".to_owned(),
                path: "/api/v1/jobs/j1".to_owned(),
                headers: BTreeMap::new(),
                body: String::new(),
            },
        );
        results.insert("delete", stream_error_code(chan.recv(ws)));
        // A permitted write route works and carries the machine's own bearer token.
        chan.send(
            ws,
            &RemoteFrame::HttpRequest {
                stream: 14,
                method: "POST".to_owned(),
                path: "/api/v1/jobs/j1/cancel".to_owned(),
                headers: BTreeMap::new(),
                body: BASE64.encode(b"{\"reason\":\"x\"}"),
            },
        );
        results.insert(
            "write",
            format!(
                "{:?}",
                http_answer(&mut chan, ws, 14)
                    .map(|(status, _, body)| (status, String::from_utf8(body).expect("utf8")))
            ),
        );
        // Stream id 0 is reserved: that is a protocol violation and closes the channel.
        chan.send(
            ws,
            &RemoteFrame::HttpRequest {
                stream: 0,
                method: "GET".to_owned(),
                path: "/api/v1/health".to_owned(),
                headers: BTreeMap::new(),
                body: String::new(),
            },
        );
        results.insert(
            "stream0",
            format!(
                "{:?}",
                chan.recv(ws).map(|frame| matches!(frame, RemoteFrame::Close { .. }))
            ),
        );
        results
    });
    h.point_at(&url);
    let mut options = h.options();
    options.api_token = Some("daemon-token-value".to_owned());
    let handle = h.start_with(options);
    let results = rx.recv_timeout(Duration::from_secs(20)).expect("script");
    assert_eq!(results["mcp"], "not_implemented");
    assert_eq!(results["tcp"], "scope_denied");
    assert_eq!(results["exec"], "scope_denied");
    for key in [
        "dotdot",
        "encoded",
        "backslash",
        "token",
        "outside",
        "static",
        "auth_header",
        "post_other",
        "delete",
    ] {
        assert_eq!(results[key], "bad_request", "{key}");
    }
    assert!(
        results["write"].contains("200") && results["write"].contains("reason"),
        "{}",
        results["write"]
    );
    assert_eq!(results["stream0"], "Ok(true)");

    let requests = h.daemon_log.requests.lock().expect("lock").clone();
    assert_eq!(
        requests.len(),
        1,
        "only the permitted write reached the daemon: {requests:?}"
    );
    let (method, path, headers) = &requests[0];
    assert_eq!((method.as_str(), path.as_str()), ("POST", "/api/v1/jobs/j1/cancel"));
    assert_eq!(
        headers.get("authorization").map(String::as_str),
        Some("Bearer daemon-token-value")
    );
    assert!(!h.logged().contains("daemon-token-value"), "the token is never logged");
    h.finish(handle);
}

fn stream_error_code(frame: Result<RemoteFrame, String>) -> String {
    match frame {
        Ok(RemoteFrame::StreamError { code, .. }) => code,
        other => format!("not a stream error: {other:?}"),
    }
}

#[test]
fn machine_connector_never_starts_a_daemon_and_discovers_web_window() {
    let mut h = Harness::new();
    h.config.daemon_url = String::new(); // empty means read <home>/web-window
    let (daemon_url, _) = fake_daemon();
    let api_url = h.api.url.clone();
    let window = h.home.path().join("web-window");
    let window_for_script = window.clone();
    let (url, rx) = relay_once(all_jwks(), move |ws, login| {
        let (mut chan, _) = open_ok(ws, login, &api_url, &[Scope::HttpRead], "jti_win", 1);
        chan.hello(ws, login, &[Scope::HttpRead]);
        // No web-window yet: the connector reports daemon_unavailable and starts nothing.
        let absent = http_get(&mut chan, ws, 1, "/api/v1/health").err();
        // A stale web-window pointing at a closed port is also unavailable.
        std::fs::write(&window_for_script, "http://127.0.0.1:1/\n").expect("stale window");
        let stale = http_get(&mut chan, ws, 3, "/api/v1/health").err();
        // A non-loopback target is refused outright.
        std::fs::write(&window_for_script, "http://example.com/\n").expect("remote window");
        let remote = http_get(&mut chan, ws, 5, "/api/v1/health").err();
        // The live window works, and is re-read on every request (the daemon may restart).
        std::fs::write(&window_for_script, format!("{daemon_url}\n")).expect("window");
        let live = http_get(&mut chan, ws, 7, "/api/v1/health").map(|(status, ..)| status);
        (absent, stale, remote, live)
    });
    h.point_at(&url);
    let handle = h.start();
    let (absent, stale, remote, live) = rx.recv_timeout(Duration::from_secs(25)).expect("script");
    assert_eq!(absent.as_deref(), Some("daemon_unavailable"));
    assert_eq!(stale.as_deref(), Some("daemon_unavailable"));
    assert_eq!(remote.as_deref(), Some("daemon_unavailable"));
    assert_eq!(live, Ok(200));
    h.finish(handle);
}

#[test]
fn machine_connector_enforces_the_channel_limit() {
    let mut h = Harness::new();
    h.config.max_channels = 1;
    let api_url = h.api.url.clone();
    let (url, rx) = relay_once(all_jwks(), move |ws, login| {
        let (_first, _) = open_ok(ws, login, &api_url, &[Scope::HttpRead], "jti_c1", 1);
        let client = EphemeralKey::generate();
        let ticket = mint_ticket(
            &api_url,
            login,
            &TicketSpec::new(login, &[Scope::HttpRead], "jti_c2"),
            &client.jwk(),
        );
        match client_open(
            ws,
            login,
            &ticket,
            &[Scope::HttpRead],
            ClientKind::Cli,
            &client,
            &channel_id(2),
        ) {
            Opened::Rejected(code) => code,
            Opened::Accepted(_) => "accepted".to_owned(),
        }
    });
    h.point_at(&url);
    let handle = h.start();
    assert_eq!(rx.recv_timeout(Duration::from_secs(20)).expect("script"), "overloaded");
    h.finish(handle);
}

#[test]
fn relay_channel_closes_with_crypto_error_on_a_tampered_data_frame() {
    let mut h = Harness::new();
    let api_url = h.api.url.clone();
    let (url, rx) = relay_once(all_jwks(), move |ws, login| {
        let (mut chan, _) = open_ok(ws, login, &api_url, &[Scope::HttpRead], "jti_t", 1);
        chan.hello(ws, login, &[Scope::HttpRead]);
        // A relay that flips one ciphertext bit: the machine must close, never serve it.
        let mut sealed = chan
            .cipher
            .seal(
                &RemoteFrame::HttpRequest {
                    stream: 1,
                    method: "GET".to_owned(),
                    path: "/api/v1/health".to_owned(),
                    headers: BTreeMap::new(),
                    body: String::new(),
                }
                .to_json()
                .expect("json"),
            )
            .expect("seal");
        sealed.ciphertext[3] ^= 1;
        ws.send(Message::binary(sealed.to_bytes().expect("bytes")))
            .expect("send");
        chan.recv(ws).err()
    });
    h.point_at(&url);
    let handle = h.start();
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(20)).expect("script").as_deref(),
        Some("crypto_error")
    );
    assert!(
        h.daemon_log.requests.lock().expect("lock").is_empty(),
        "a frame that failed authentication never reaches the daemon"
    );
    h.finish(handle);
}

#[test]
fn relay_frame_cap_closes_without_partial_frame() {
    let mut h = Harness::new();
    let url = spawn_relay({
        let jwks = all_jwks();
        move |index, mut ws| {
            let mut nonces = NonceSet::default();
            let Ok(_login) = relay_login(&mut ws, &mut nonces, &jwks) else {
                return;
            };
            if index == 0 {
                // One byte over the 1 MiB cap: the machine must drop the connection, not buffer it.
                let big = format!(r#"{{"type":"ping","v":1,"nonce":"{}"}}"#, "a".repeat(1_048_577));
                let _ = ws.send(Message::text(big));
                let _ = ws.flush();
            }
            while ws.read().is_ok() {}
        }
    });
    h.point_at(&url);
    let handle = h.start();
    eventually(
        "the oversized frame to drop the connection and the machine to reconnect",
        || h.api.count("jwks") == 0 && h.logged().contains("dropped (too_large)"),
    );
    eventually("a second authenticated connection", || {
        h.status_opt().is_some_and(|status| status.connects >= 2)
    });
    h.finish(handle);
}

// ---------------------------------------------------------------------------------------------
// connector lifecycle
// ---------------------------------------------------------------------------------------------

#[test]
fn machine_connector_reconnects_with_backoff() {
    let mut h = Harness::new();
    let attempts = Arc::new(AtomicU64::new(0));
    let counter = Arc::clone(&attempts);
    let jwks = all_jwks();
    let url = spawn_relay(move |index, mut ws| {
        counter.fetch_add(1, Ordering::SeqCst);
        if index < 3 {
            // Drop the first three connections before login completes.
            let _ = read_frame(&mut ws);
            return;
        }
        let mut nonces = NonceSet::default();
        if relay_login(&mut ws, &mut nonces, &jwks).is_ok() {
            while ws.read().is_ok() {}
        }
    });
    h.point_at(&url);
    let handle = h.start();
    eventually("a successful session after three drops", || {
        h.status_opt()
            .is_some_and(|status| status.state == ConnectorState::Online)
    });
    assert!(attempts.load(Ordering::SeqCst) >= 4);
    let report = h.finish(handle);
    assert_eq!(report.connects, 1);
    assert!(report.delays.len() >= 3, "{:?}", report.delays);
    let min = Duration::from_millis(40);
    let max = Duration::from_millis(400);
    for (index, delay) in report.delays.iter().take(3).enumerate() {
        let base = min * (1u32 << index);
        assert!(
            *delay >= base && *delay <= base + base / 4,
            "delay {index} is {delay:?}, base {base:?}"
        );
    }
    assert!(report.delays[2] > report.delays[0], "the delay grows");
    assert!(
        report.delays.iter().all(|delay| *delay <= max + max / 4),
        "the delay is capped"
    );
}

#[test]
fn machine_connector_backoff_resets_after_a_good_session() {
    let mut h = Harness::new();
    let jwks = all_jwks();
    let url = spawn_relay(move |index, mut ws| {
        let mut nonces = NonceSet::default();
        if relay_login(&mut ws, &mut nonces, &jwks).is_err() {
            return;
        }
        if index < 2 {
            close_with(&mut ws, 1013, "overloaded");
            return;
        }
        while ws.read().is_ok() {}
    });
    h.point_at(&url);
    let handle = h.start();
    eventually("the third session", || {
        h.status_opt().is_some_and(|status| status.connects >= 3)
    });
    let report = h.finish(handle);
    assert!(report.delays.len() >= 2);
    for delay in report.delays.iter().take(2) {
        assert!(
            *delay <= Duration::from_millis(50),
            "a session that logged in resets the backoff: {delay:?}"
        );
    }
}

#[test]
fn machine_connector_refreshes_a_credential() {
    let mut h = Harness::new();
    h.write_record(60); // inside the 600 second refresh margin
    let before = h.login_record().credential;
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let recorder = Arc::clone(&seen);
    let jwks = all_jwks();
    let url = spawn_relay(move |_, mut ws| {
        let mut nonces = NonceSet::default();
        if let Ok(login) = relay_login(&mut ws, &mut nonces, &jwks) {
            recorder.lock().expect("lock").push(login.credential);
            while ws.read().is_ok() {}
        }
    });
    h.point_at(&url);
    let handle = h.start();
    eventually("the relay to see a credential", || {
        !seen.lock().expect("lock").is_empty()
    });
    let presented = seen.lock().expect("lock")[0].clone();
    assert_ne!(
        presented, before,
        "the connector must not present the near-expiry credential"
    );
    assert_eq!(
        h.api.count("/credential"),
        1,
        "refreshed exactly once, with a signed request"
    );
    let record = h.login_record();
    assert_eq!(
        record.credential, presented,
        "relay.json holds the refreshed credential"
    );
    assert!(CredentialClaims::decode(&presented).expect("claims").exp - now_secs() > 3_000);
    assert_eq!(
        mode(&h.home.path().join("relay.json")),
        0o600,
        "the refresh keeps relay.json private"
    );
    h.finish(handle);
}

#[test]
fn machine_connector_refreshes_after_the_relay_refuses_a_credential() {
    let mut h = Harness::new();
    let attempts = Arc::new(AtomicU64::new(0));
    let counter = Arc::clone(&attempts);
    let jwks = all_jwks();
    let url = spawn_relay(move |_, mut ws| {
        let n = counter.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            // The relay's clock disagrees: it answers unauthenticated to a credential that looks fine.
            let _ = read_frame(&mut ws);
            send_frame(
                &mut ws,
                &RelayFrame::Error {
                    v: 1,
                    code: codes::UNAUTHENTICATED.to_owned(),
                    message: "stale".to_owned(),
                    retryable: false,
                },
            );
            close_with(&mut ws, 4001, "unauthenticated");
            return;
        }
        let mut nonces = NonceSet::default();
        if relay_login(&mut ws, &mut nonces, &jwks).is_ok() {
            while ws.read().is_ok() {}
        }
    });
    h.point_at(&url);
    let handle = h.start();
    eventually("an online session", || {
        h.status_opt()
            .is_some_and(|status| status.state == ConnectorState::Online)
    });
    assert_eq!(
        h.api.count("/credential"),
        1,
        "an unauthenticated answer forces one credential refresh"
    );
    h.finish(handle);
}

#[test]
fn machine_connector_stops_when_the_machine_is_revoked() {
    // (a) The API refuses the refresh with 403.
    let mut h = Harness::new();
    h.write_record(30);
    h.api.state.revoked.store(true, Ordering::SeqCst);
    let url = spawn_relay(|_, _| {});
    h.point_at(&url);
    let report = h.start().join().expect("thread").expect("result");
    assert_eq!(report.outcome, ConnectorOutcome::Revoked);
    assert_eq!(report.connects, 0);
    assert_eq!(h.status().state, ConnectorState::Revoked);
    assert_eq!(h.api.count("/credential"), 1, "no retry loop after a revocation");

    // (b) The relay reports the revocation on a live connection.
    let mut h = Harness::new();
    let jwks = all_jwks();
    let url = spawn_relay(move |_, mut ws| {
        let mut nonces = NonceSet::default();
        if relay_login(&mut ws, &mut nonces, &jwks).is_ok() {
            send_frame(
                &mut ws,
                &RelayFrame::Error {
                    v: 1,
                    code: codes::REVOKED.to_owned(),
                    message: "revoked".to_owned(),
                    retryable: false,
                },
            );
            while ws.read().is_ok() {}
        }
    });
    h.point_at(&url);
    let report = h.start().join().expect("thread").expect("result");
    assert_eq!(report.outcome, ConnectorOutcome::Revoked);
    assert_eq!(report.connects, 1);
    assert!(report.delays.is_empty(), "a revoked machine does not reconnect");
}

#[test]
fn machine_connector_stops_when_superseded() {
    let mut h = Harness::new();
    let jwks = all_jwks();
    let url = spawn_relay(move |_, mut ws| {
        let mut nonces = NonceSet::default();
        if relay_login(&mut ws, &mut nonces, &jwks).is_ok() {
            send_frame(
                &mut ws,
                &RelayFrame::Superseded {
                    v: 1,
                    reason: "another connection".to_owned(),
                },
            );
            close_with(&mut ws, 4008, "superseded");
        }
    });
    h.point_at(&url);
    let report = h.start().join().expect("thread").expect("result");
    assert_eq!(report.outcome, ConnectorOutcome::Superseded);
    assert!(
        report.delays.is_empty(),
        "a superseded connector must not fight the new connection"
    );
    assert_eq!(h.status().state, ConnectorState::Superseded);
}

#[test]
fn machine_connector_refuses_to_start_unsafely() {
    let mut h = Harness::new();
    let url = spawn_relay(|_, _| {});
    h.point_at(&url);

    // Disabled by default.
    let mut options = h.options();
    options.config.enabled = false;
    let error = Connector::new(options).run().expect_err("disabled");
    assert!(error.to_string().contains("relay is disabled"), "{error}");

    // A group-readable relay key is refused before anything is sent.
    let key_path = h.home.path().join("relay.key");
    std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o644)).expect("chmod");
    let error = Connector::new(h.options()).run().expect_err("loose key");
    assert!(matches!(error, RelayError::Key(_)), "{error}");
    std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).expect("chmod");

    // relay.json readable by others is refused too: it holds the credential.
    let record_path = h.home.path().join("relay.json");
    std::fs::set_permissions(&record_path, std::fs::Permissions::from_mode(0o644)).expect("chmod");
    let error = Connector::new(h.options()).run().expect_err("loose credential");
    assert!(matches!(error, RelayError::Key(_)), "{error}");
    std::fs::set_permissions(&record_path, std::fs::Permissions::from_mode(0o600)).expect("chmod");

    // A key that does not match relay.json is refused.
    let other = RelayKey::generate();
    other.write(h.home.path(), "relay.key").expect("other key");
    let error = Connector::new(h.options()).run().expect_err("mismatched key");
    assert!(matches!(error, RelayError::Key(_)), "{error}");

    // Not enrolled.
    let empty = tempfile::tempdir().expect("empty home");
    let error = Connector::new(ConnectorOptions::new(empty.path().to_owned(), h.config.clone()))
        .run()
        .expect_err("not enrolled");
    assert!(matches!(error, RelayError::NotEnrolled(_)), "{error}");
}

// ---------------------------------------------------------------------------------------------
// enrollment, rotation, status
// ---------------------------------------------------------------------------------------------

fn enroll_config(api: &FakeApi) -> RelayConfig {
    RelayConfig {
        enabled: true,
        api_url: api.url.clone(),
        project: "vowp".to_owned(),
        ..RelayConfig::default()
    }
}

const ENROLL_TOKEN: &str = "ssm_test-only-token-0123456789abcdef";

#[test]
fn relay_enroll_writes_private_files_0600() {
    let api = FakeApi::start(ENROLL_TOKEN);
    let home = tempfile::tempdir().expect("home");
    let config = enroll_config(&api);
    let outcome = enroll(&EnrollRequest {
        home: home.path(),
        config: &config,
        token: ENROLL_TOKEN,
        name: "contabo-1",
        now: Timestamp::now(),
    })
    .expect("enroll");
    assert_eq!(outcome.machine_id, MACHINE_ID);
    assert_eq!(mode(&home.path().join("relay.key")), 0o600);
    assert_eq!(mode(&home.path().join("relay.pub")), 0o644);
    assert_eq!(mode(&home.path().join("relay.json")), 0o600);

    // relay.key is a fresh key, not node.key; relay.pub is its public half; key_id has the prefix.
    let key = RelayKey::load(home.path(), "relay.key").expect("load");
    assert_eq!(
        std::fs::read_to_string(home.path().join("relay.pub"))
            .expect("pub")
            .trim(),
        key.public_key_hex()
    );
    assert!(!home.path().join("node.key").exists());
    let record = RelayJson::load(&home.path().join("relay.json")).expect("record");
    assert_eq!(record.key_id, format!("ed25519:{}", key.public_key_hex()));
    assert_eq!(record.name, "contabo-1");
    assert_eq!(record.api_url, "https://api.example.test", "the API's answer wins");
    assert_eq!(record.relay_url, "wss://relay.example.test/v1/machine");
    assert_eq!(
        *api.state.machine_pubkey.lock().expect("lock"),
        key.public_key_hex(),
        "the API registered the public key"
    );
    let (credential_machine, expires) = (
        CredentialClaims::decode(&record.credential).expect("claims").sub,
        &record.credential_expires_at,
    );
    assert_eq!(credential_machine, format!("machine:{MACHINE_ID}"));
    assert!(Timestamp::parse(expires).is_ok());

    // The private key never went to the API.
    let private = std::fs::read_to_string(home.path().join("relay.key"))
        .expect("key")
        .trim()
        .to_owned();
    let sent = format!("{:?}", api.state.enroll_headers.lock().expect("lock"));
    assert!(!sent.contains(&private));
    let headers = api.state.enroll_headers.lock().expect("lock").clone();
    assert_eq!(headers.len(), 1);
    assert_eq!(
        headers[0]["idempotency-key"].len(),
        32,
        "an Idempotency-Key header is sent"
    );

    // A second enrollment is refused rather than overwriting the identity.
    let again = enroll(&EnrollRequest {
        home: home.path(),
        config: &config,
        token: ENROLL_TOKEN,
        name: "x",
        now: Timestamp::now(),
    });
    assert!(matches!(again, Err(RelayError::AlreadyEnrolled(_))), "{again:?}");
    assert_eq!(api.count("/enroll"), 1, "and it never called the API again");
}

#[test]
fn relay_enroll_does_not_log_the_token() {
    let api = FakeApi::start(ENROLL_TOKEN);
    let home = tempfile::tempdir().expect("home");
    let config = enroll_config(&api);
    let outcome = enroll(&EnrollRequest {
        home: home.path(),
        config: &config,
        token: ENROLL_TOKEN,
        name: "box",
        now: Timestamp::now(),
    })
    .expect("enroll");
    // Neither the outcome nor any file under the home contains the token.
    assert!(!format!("{outcome:?}").contains(ENROLL_TOKEN));
    assert!(!serde_json::to_string(&outcome).expect("json").contains(ENROLL_TOKEN));
    for entry in std::fs::read_dir(home.path()).expect("dir") {
        let path = entry.expect("entry").path();
        let text = std::fs::read_to_string(&path).expect("text");
        assert!(!text.contains(ENROLL_TOKEN), "{} contains the token", path.display());
    }

    // Failures never echo the token either: a rejected token, a conflict and a malformed token.
    let other_home = tempfile::tempdir().expect("home");
    let wrong = "ssm_wrong-token-0123456789abcdef";
    let error = enroll(&EnrollRequest {
        home: other_home.path(),
        config: &config,
        token: wrong,
        name: "b",
        now: Timestamp::now(),
    })
    .expect_err("wrong token");
    assert!(matches!(error, RelayError::Api { status: 401, .. }), "{error}");
    assert!(!error.to_string().contains(wrong) && !format!("{error:?}").contains(wrong));
    api.state.enroll_status.store(409, Ordering::SeqCst);
    let error = enroll(&EnrollRequest {
        home: other_home.path(),
        config: &config,
        token: ENROLL_TOKEN,
        name: "b",
        now: Timestamp::now(),
    })
    .expect_err("conflict");
    assert!(
        matches!(&error, RelayError::Api { status: 409, code } if code == "conflict"),
        "{error}"
    );
    assert!(!error.to_string().contains(ENROLL_TOKEN));
    let error = enroll(&EnrollRequest {
        home: other_home.path(),
        config: &config,
        token: "not-a-token with spaces",
        name: "b",
        now: Timestamp::now(),
    })
    .expect_err("malformed");
    assert!(!error.to_string().contains("not-a-token"), "{error}");
    assert_eq!(api.count("/enroll"), 3, "the malformed token never left the machine");
    // A failed enrollment leaves the key (so a retry reuses it) but no relay.json.
    assert!(!other_home.path().join("relay.json").exists());
    assert_eq!(mode(&other_home.path().join("relay.key")), 0o600);
}

#[test]
fn relay_enroll_retry_reuses_the_key_and_the_idempotency_key() {
    let api = FakeApi::start(ENROLL_TOKEN);
    let home = tempfile::tempdir().expect("home");
    let config = enroll_config(&api);
    api.state.enroll_status.store(500, Ordering::SeqCst);
    let request = EnrollRequest {
        home: home.path(),
        config: &config,
        token: ENROLL_TOKEN,
        name: "box",
        now: Timestamp::now(),
    };
    assert!(enroll(&request).is_err(), "the first attempt loses its reply");
    api.state.enroll_status.store(200, Ordering::SeqCst);
    enroll(&request).expect("the retry succeeds");
    let headers = api.state.enroll_headers.lock().expect("lock").clone();
    assert_eq!(
        headers[0]["idempotency-key"], headers[1]["idempotency-key"],
        "same token and key, same idempotency key"
    );
}

#[test]
fn relay_enroll_refuses_a_non_loopback_cleartext_api() {
    let home = tempfile::tempdir().expect("home");
    // The table validator refuses it first; even a hand-built table must not reach the network.
    let config = RelayConfig {
        api_url: "http://api.example.test".to_owned(),
        ..RelayConfig::default()
    };
    let error = enroll(&EnrollRequest {
        home: home.path(),
        config: &config,
        token: ENROLL_TOKEN,
        name: "b",
        now: Timestamp::now(),
    })
    .expect_err("cleartext API");
    assert!(matches!(error, RelayError::Config(_)), "{error}");
    assert!(
        !home.path().join("relay.key").exists(),
        "nothing is written when the request is refused"
    );
}

#[test]
fn relay_rotate_key_replaces_the_key_atomically_and_updates_the_record() {
    let h = Harness::new();
    let paths = RelayPaths::new(h.home.path(), &h.config);
    let old_id = h.key.key_id();
    let new_id = rotate_key(&paths, Timestamp::now()).expect("rotate");
    assert_ne!(new_id, old_id);
    let key = RelayKey::load(h.home.path(), "relay.key").expect("new key");
    assert_eq!(key.key_id(), new_id);
    assert_eq!(mode(&h.home.path().join("relay.key")), 0o600);
    assert_eq!(mode(&h.home.path().join("relay.pub")), 0o644);
    assert_eq!(
        std::fs::read_to_string(h.home.path().join("relay.pub"))
            .expect("pub")
            .trim(),
        key.public_key_hex()
    );
    let record = h.login_record();
    assert_eq!(record.key_id, new_id);
    assert_eq!(
        CredentialClaims::decode(&record.credential).expect("claims").mkey,
        key.public_key_hex()
    );
    assert_eq!(mode(&h.home.path().join("relay.json")), 0o600);
    assert_eq!(
        *h.api.state.machine_pubkey.lock().expect("lock"),
        key.public_key_hex(),
        "the API verified both signatures"
    );
    // The temporary files are gone.
    let stray: Vec<_> = std::fs::read_dir(h.home.path())
        .expect("dir")
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(stray.is_empty(), "{stray:?}");

    // A rejected rotation leaves the (new) key and record untouched.
    h.api.state.revoked.store(true, Ordering::SeqCst);
    *h.api.state.machine_pubkey.lock().expect("lock") = "00".repeat(32);
    let before = std::fs::read_to_string(h.home.path().join("relay.key")).expect("key");
    assert!(rotate_key(&paths, Timestamp::now()).is_err());
    assert_eq!(
        std::fs::read_to_string(h.home.path().join("relay.key")).expect("key"),
        before
    );
}

#[test]
fn relay_refresh_credential_requires_the_matching_key() {
    let h = Harness::new();
    let paths = RelayPaths::new(h.home.path(), &h.config);
    let refreshed = refresh_credential(&paths, Timestamp::now()).expect("refresh");
    assert_ne!(refreshed.credential, "");
    // The API rejects a request signed by a key it does not know.
    let other = RelayKey::generate();
    other.write(h.home.path(), "relay.key").expect("key");
    let mut record = h.login_record();
    record.key_id = other.key_id();
    record.save(&h.home.path().join("relay.json")).expect("save");
    let error = refresh_credential(&paths, Timestamp::now()).expect_err("unknown key");
    assert!(matches!(error, RelayError::Api { status: 401, .. }), "{error}");
}

#[test]
fn relay_status_report_holds_no_credential_token_or_private_key() {
    let h = Harness::new();
    let paths = RelayPaths::new(h.home.path(), &h.config);
    let record = h.login_record();
    let private = std::fs::read_to_string(h.home.path().join("relay.key"))
        .expect("key")
        .trim()
        .to_owned();
    let report = status_report(&paths, &h.config, Timestamp::now());
    assert!(report.enrolled && report.enabled);
    assert_eq!(report.machine_id.as_deref(), Some(MACHINE_ID));
    assert_eq!(report.credential_expired, Some(false));
    assert!(report.connector.is_none());
    let text = serde_json::to_string(&report).expect("json");
    for secret in [record.credential.as_str(), private.as_str(), ENROLL_TOKEN, "ssm_"] {
        assert!(!text.contains(secret), "status leaked a secret");
    }
    assert!(!text.contains("credential\""), "no credential field: {text}");
    assert!(text.contains(&record.key_id), "the public key id is shown");

    // An expired credential is reported, a missing enrollment is reported.
    h.write_record(-10);
    assert_eq!(
        status_report(&paths, &h.config, Timestamp::now()).credential_expired,
        Some(true)
    );
    let empty = tempfile::tempdir().expect("home");
    let none = status_report(&RelayPaths::new(empty.path(), &h.config), &h.config, Timestamp::now());
    assert!(!none.enrolled && none.machine_id.is_none());
}

#[test]
fn relay_http_policy_table() {
    let none = BTreeMap::new();
    for path in [
        "/api/v1/health",
        "/api/v1/jobs?state=ready",
        "/api/v1/sessions/ses_01/events?limit=20",
        "/api/v1/board/stream",
    ] {
        validate_http_request("GET", path, &none).unwrap_or_else(|error| panic!("{path}: {error}"));
    }
    for path in [
        "/api/v1/",
        "/api/v1",
        "/api/v2/health",
        "api/v1/health",
        "//api/v1/health",
        "/api/v1//health",
        "/api/v1/../x",
        "/api/v1/a/..",
        "/api/v1/a%2Fb",
        "/api/v1/a%5cb",
        "/api/v1/a%2eb",
        "/api/v1/a%00b",
        "/api/v1/a\0b",
        "/api/v1/a\r\nb",
        "/api/v1/a b",
        "/api/v1/a#frag",
        "/api/v1/a\\b",
        "/api/v1/health?token=x",
        "/api/v1/health?a=1&KEY=x",
        "/api/v1/health?secret",
        "/api/v1/health?%61uth=x",
        "/api/v1/health?code=1",
    ] {
        assert!(
            validate_http_request("GET", path, &none).is_err(),
            "{path:?} must be refused"
        );
    }
    let long = format!("/api/v1/health?q={}", "a".repeat(2_100));
    assert!(
        validate_http_request("GET", &long, &none).is_err(),
        "a query over 2 KiB"
    );
    for method in ["PUT", "DELETE", "PATCH", "HEAD", "OPTIONS", "get"] {
        assert!(
            validate_http_request(method, "/api/v1/health", &none).is_err(),
            "{method}"
        );
    }
    for path in [
        "/api/v1/inbox/inb_1/answer",
        "/api/v1/sessions/ses_1/switch",
        "/api/v1/sessions/import/asif",
        "/api/v1/jobs/job_1/retry",
        "/api/v1/jobs/job_1/park",
        "/api/v1/jobs/job_1/cancel",
        "/api/v1/jobs/job_1/nudge",
        "/api/v1/jobs/job_1/restart",
        "/api/v1/jobs/job_1/replan",
        "/api/v1/jobs/job_1/fail",
    ] {
        validate_http_request("POST", path, &none).unwrap_or_else(|error| panic!("{path}: {error}"));
    }
    for path in [
        "/api/v1/health",
        "/api/v1/jobs/job_1",
        "/api/v1/jobs/job_1/delete",
        "/api/v1/jobs//cancel",
        "/api/v1/jobs/a/b/cancel",
        "/api/v1/jobs/job_1/cancel?x=1",
        "/api/v1/sessions/import/asif/extra",
    ] {
        assert!(validate_http_request("POST", path, &none).is_err(), "POST {path}");
    }
    let header = |name: &str| BTreeMap::from([(name.to_owned(), "x".to_owned())]);
    for name in ["Accept", "content-type", "Last-Event-ID", "If-None-Match"] {
        validate_http_request("GET", "/api/v1/health", &header(name)).unwrap_or_else(|error| panic!("{name}: {error}"));
    }
    for name in [
        "Authorization",
        "Cookie",
        "Host",
        "X-Forwarded-For",
        "x-forwarded-host",
        "Origin",
        "Connection",
    ] {
        assert!(
            validate_http_request("GET", "/api/v1/health", &header(name)).is_err(),
            "{name}"
        );
    }
    let mut bad_value = BTreeMap::new();
    bad_value.insert("Accept".to_owned(), "a\r\nX-Evil: 1".to_owned());
    assert!(
        validate_http_request("GET", "/api/v1/health", &bad_value).is_err(),
        "header injection"
    );
}
