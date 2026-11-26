//! Hermetic tests for the optional StackSpirit Account/Cloud Relay client.
//!
//! Every request goes to a loopback mock HTTP server on an ephemeral port. No
//! test resolves or contacts `api.stackspirit.io` or `relay.stackspirit.io`.

use std::fmt;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use async_trait::async_trait;
use spirit_core::cloud::{
    CLOUD_SESSION_FILE, ClientKind, CloudClient, CloudError, CloudErrorCode, CloudMethod,
    CloudRequest, CloudResponse, CloudSession, CloudSessionStore, CloudTransport, DevicePollState,
    JsonCloudSessionStore, MachinePresence, RelayPublicJwk, RelayScope, RelayTicketRequest,
};
use spirit_core::time::Timestamp;
use tempfile::TempDir;

type Responder = Arc<dyn Fn(&RecordedRequest) -> (u16, String) + Send + Sync>;

#[derive(Debug, Clone)]
struct RecordedRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl RecordedRequest {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(header, _)| header.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

struct MockServer {
    addr: SocketAddr,
    recorded: Arc<Mutex<Vec<RecordedRequest>>>,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl MockServer {
    fn start(responder: Responder) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock cloud API");
        listener
            .set_nonblocking(true)
            .expect("nonblocking mock cloud API");
        let addr = listener.local_addr().expect("mock address");
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let join = {
            let recorded = Arc::clone(&recorded);
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            if stop.load(Ordering::SeqCst) {
                                break;
                            }
                            handle_connection(stream, &responder, &recorded);
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(2));
                        }
                        Err(_) => break,
                    }
                }
            })
        };
        Self {
            addr,
            recorded,
            stop,
            join: Some(join),
        }
    }

    fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn requests(&self) -> Vec<RecordedRequest> {
        self.recorded.lock().expect("requests").clone()
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.addr);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn handle_connection(
    mut stream: TcpStream,
    responder: &Responder,
    recorded: &Arc<Mutex<Vec<RecordedRequest>>>,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut reader = match stream.try_clone() {
        Ok(reader) => BufReader::new(reader),
        Err(_) => return,
    };
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or_default().to_owned();
    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }
    let length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0; length];
    if length > 0 && reader.read_exact(&mut body).is_err() {
        return;
    }
    let record = RecordedRequest {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    };
    let (status, response_body) = responder(&record);
    recorded.lock().expect("record request").push(record);
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        _ => "Status",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
        response_body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

#[derive(Clone, Copy)]
struct LoopbackTransport;

impl fmt::Debug for LoopbackTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LoopbackTransport")
    }
}

#[async_trait]
impl CloudTransport for LoopbackTransport {
    async fn send(&self, request: CloudRequest) -> Result<CloudResponse, CloudError> {
        let Some(rest) = request.url.strip_prefix("http://") else {
            return Err(CloudError::new(
                CloudErrorCode::Unreachable,
                "the test transport only reaches a loopback http API",
            ));
        };
        let (authority, path) = match rest.find('/') {
            Some(index) => (&rest[..index], &rest[index..]),
            None => (rest, "/"),
        };
        let mut stream = TcpStream::connect(authority).map_err(|_| {
            CloudError::new(
                CloudErrorCode::Unreachable,
                "the mock cloud API could not be reached",
            )
        })?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut head = format!(
            "{} {} HTTP/1.1\r\nHost: {authority}\r\n",
            request.method.as_str(),
            path
        );
        for (name, value) in &request.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        if let Some(body) = &request.body {
            head.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        head.push_str("Connection: close\r\n\r\n");
        stream.write_all(head.as_bytes()).map_err(|_| {
            CloudError::new(
                CloudErrorCode::Unreachable,
                "the mock cloud API refused the request",
            )
        })?;
        if let Some(body) = &request.body {
            stream.write_all(body.as_bytes()).map_err(|_| {
                CloudError::new(
                    CloudErrorCode::Unreachable,
                    "the mock cloud API refused the body",
                )
            })?;
        }
        stream.flush().ok();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).map_err(|_| {
            CloudError::new(
                CloudErrorCode::Unreachable,
                "the mock cloud API did not answer",
            )
        })?;
        let split = bytes
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .ok_or_else(|| {
                CloudError::new(
                    CloudErrorCode::Parse,
                    "the mock cloud API returned no HTTP header",
                )
            })?;
        let header_text = String::from_utf8_lossy(&bytes[..split]);
        let status = header_text
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|status| status.parse::<u16>().ok())
            .ok_or_else(|| {
                CloudError::new(
                    CloudErrorCode::Parse,
                    "the mock cloud API returned no HTTP status",
                )
            })?;
        let body = String::from_utf8_lossy(&bytes[split + 4..]).into_owned();
        Ok(CloudResponse::new(status, body))
    }
}

fn client(server: &MockServer) -> CloudClient<LoopbackTransport> {
    CloudClient::new(LoopbackTransport, server.url()).expect("client")
}

fn temp_store() -> (TempDir, JsonCloudSessionStore) {
    let directory = tempfile::tempdir().expect("temp dir");
    let store = JsonCloudSessionStore::new(directory.path().join(CLOUD_SESSION_FILE));
    (directory, store)
}

fn seed_session(server: &MockServer, store: &JsonCloudSessionStore, token: &str) {
    let session = CloudSession::new(server.url(), token);
    store.save(&session).expect("seed session");
}

fn json_body(value: &serde_json::Value) -> String {
    serde_json::to_string(value).expect("json")
}

#[tokio::test]
async fn device_flow_persists_a_0600_session_and_redacts_tokens() {
    let server = MockServer::start(Arc::new(|request: &RecordedRequest| {
        match (request.method.as_str(), request.path.as_str()) {
            ("POST", "/v1/auth/device") => (
                200,
                json_body(&serde_json::json!({
                    "device_code": "dev_secret_123",
                    "user_code": "ABCD-EFGH",
                    "verification_uri": "https://stackspirit.io/device",
                    "verification_uri_complete": "https://stackspirit.io/device?code=ABCD-EFGH",
                    "expires_in": 600,
                    "interval": 5
                })),
            ),
            ("POST", "/v1/auth/device/token") => (
                200,
                json_body(&serde_json::json!({
                    "access_token": "access_secret_123",
                    "refresh_token": "refresh_secret_123",
                    "expires_in": 900
                })),
            ),
            ("GET", "/v1/me") => {
                assert_eq!(
                    request.header("authorization"),
                    Some("Bearer access_secret_123")
                );
                (
                    200,
                    json_body(&serde_json::json!({
                        "account": {
                            "id": "acct_1",
                            "email": "owner@example.test",
                            "name": "Owner"
                        }
                    })),
                )
            }
            _ => (404, "{}".to_owned()),
        }
    }));
    let (_directory, store) = temp_store();
    let client = client(&server);

    let device = client.start_device_sign_in().await.expect("device start");
    assert_eq!(device.user_code, "ABCD-EFGH");
    assert_eq!(device.interval, 5);
    assert_eq!(device.expires_in, 600);
    assert!(!format!("{device:?}").contains("dev_secret_123"));

    let state = client
        .poll_device_sign_in(&store, device.device_code.as_str())
        .await
        .expect("device poll");
    assert_eq!(state, DevicePollState::Authorized);
    assert!(client.has_session());

    let account = client.account().await.expect("account");
    assert_eq!(account.email, "owner@example.test");
    assert_eq!(account.name.as_deref(), Some("Owner"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(store.path())
            .expect("session metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
    let text = std::fs::read_to_string(store.path()).expect("session file");
    assert!(text.contains("access_secret_123"));
    assert!(text.contains("refresh_secret_123"));

    assert!(!format!("{client:?}").contains("access_secret_123"));
    let session = CloudSession::new("https://api.stackspirit.io", "access_secret_123")
        .with_refresh_token("refresh_secret_123");
    let debug = format!("{session:?}");
    assert!(!debug.contains("access_secret_123"), "{debug}");
    assert!(!debug.contains("refresh_secret_123"), "{debug}");
    assert!(debug.contains("[redacted]"), "{debug}");

    let request = CloudRequest::new(CloudMethod::Post, "https://api.stackspirit.io/v1/test")
        .with_header("Authorization", "Bearer access_secret_123")
        .with_body("{\"refresh_token\":\"refresh_secret_123\"}");
    let debug = format!("{request:?}");
    assert!(!debug.contains("access_secret_123"), "{debug}");
    assert!(!debug.contains("refresh_secret_123"), "{debug}");

    assert!(client.sign_out(&store).expect("sign out"));
    assert!(!client.has_session());
    assert!(!store.path().exists());
}

#[tokio::test]
async fn plans_are_public_and_sorted_by_price() {
    let server = MockServer::start(Arc::new(|request: &RecordedRequest| {
        assert_eq!(request.path, "/v1/plans");
        assert_eq!(request.header("authorization"), None);
        (
            200,
            json_body(&serde_json::json!({
                "plans": [
                    {
                        "id": "home-12",
                        "name": "Home 12",
                        "vcpu": 6,
                        "ram_gb": 12,
                        "disk_gb": 200,
                        "disk_kind": "SSD",
                        "monthly_eur": 9.0,
                        "setup_fee_eur": 0.0,
                        "currency": "EUR",
                        "provider_spec": "Cloud VPS 6"
                    },
                    {
                        "id": "home-8",
                        "name": "Home 8",
                        "vcpu": 4,
                        "ram_gb": 8,
                        "disk_gb": 100,
                        "disk_kind": "SSD",
                        "monthly_eur": 6.6,
                        "setup_fee_eur": 0.7,
                        "currency": "EUR",
                        "provider_spec": "Cloud VPS 4"
                    },
                    {
                        "id": "home-24",
                        "name": "Home 24",
                        "vcpu": 8,
                        "ram_gb": 24,
                        "disk_gb": 300,
                        "disk_kind": "NVMe",
                        "monthly_eur": 17.0,
                        "setup_fee_eur": null,
                        "currency": "EUR",
                        "provider_spec": "Cloud VPS 8"
                    }
                ],
                "backups": {
                    "monthly_eur": 3.5,
                    "recommended": true,
                    "provider": "cloudflare-r2"
                }
            })),
        )
    }));
    let client = client(&server);

    let catalog = client.get_plans().await.expect("plans");
    let names: Vec<&str> = catalog
        .plans
        .iter()
        .map(|plan| plan.name.as_str())
        .collect();
    assert_eq!(names, ["Home 8", "Home 12", "Home 24"]);
    assert_eq!(catalog.plans[0].setup_fee_eur, Some(0.7));
    assert!(catalog.backups.recommended);
    assert_eq!(catalog.backups.monthly_eur, Some(3.5));
    assert_eq!(catalog.backups.provider.as_deref(), Some("cloudflare-r2"));
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn machines_require_a_bearer_and_parse_presence() {
    let server = MockServer::start(Arc::new(|request: &RecordedRequest| {
        assert_eq!(request.path, "/v1/machines");
        assert_eq!(request.header("authorization"), Some("Bearer stored_token"));
        (
            200,
            json_body(&serde_json::json!({
                "machines": [{
                    "id": "mch_1",
                    "name": "contabo-1",
                    "status": "online",
                    "project": "vowp",
                    "last_seen_at": "2026-11-26T12:00:00.000Z",
                    "provisioning": {
                        "machine_id": "mch_1",
                        "phase": "online",
                        "step": "ready",
                        "progress_pct": 100,
                        "detail": "ready at /Users/owner/private",
                        "updated_at": "2026-11-26T12:00:00.000Z",
                        "home_url": "https://mch_1.stackspirit.io"
                    }
                }]
            })),
        )
    }));
    let (_directory, store) = temp_store();
    seed_session(&server, &store, "stored_token");
    let signed_in = client(&server);
    assert!(signed_in.restore_session(&store).expect("restore"));

    let machines = signed_in.list_machines().await.expect("machines");
    assert_eq!(machines.len(), 1);
    assert_eq!(machines[0].status, MachinePresence::Online);
    assert_eq!(
        machines[0].last_seen_at,
        Some(Timestamp::parse("2026-11-26T12:00:00.000Z").expect("timestamp"))
    );
    let provisioning = machines[0].provisioning.as_ref().expect("provisioning");
    assert_eq!(provisioning.detail.as_deref(), Some("ready at [path]"));
    assert_eq!(provisioning.progress_pct, Some(100));

    let unsigned = client(&server);
    unsigned.clear_session();
    let error = unsigned.list_machines().await.expect_err("auth required");
    assert_eq!(error.code, CloudErrorCode::Auth);
    assert_eq!(server.requests().len(), 1, "no request without a bearer");
}

#[tokio::test]
async fn a_ticket_falls_back_only_on_404_and_carries_public_values() {
    let server = MockServer::start(Arc::new(|request: &RecordedRequest| {
        match request.path.as_str() {
            "/v1/machines/mch_1/ticket" => (
                404,
                json_body(&serde_json::json!({
                    "error": {"code": "not_found", "message": "use relay-tickets"}
                })),
            ),
            "/v1/relay-tickets" => (
                200,
                json_body(&serde_json::json!({
                    "ticket": "eyJhbGciOiJFUzI1NiJ9.ticket.signature",
                    "expires_at": "2026-11-26T12:05:00.000Z",
                    "relay_url": "wss://relay.stackspirit.io/v1/client",
                    "machine": {
                        "id": "mch_1",
                        "name": "contabo-1",
                        "key_id": "ed25519:abc",
                        "public_key": "abcdef",
                        "project": "vowp"
                    }
                })),
            ),
            _ => (404, "{}".to_owned()),
        }
    }));
    let (_directory, store) = temp_store();
    seed_session(&server, &store, "stored_token");
    let client = client(&server);
    assert!(client.restore_session(&store).expect("restore"));

    let ticket = client
        .mint_relay_ticket(
            "mch_1",
            RelayTicketRequest {
                kind: ClientKind::Desktop,
                scopes: vec![RelayScope::HttpRead],
                client_key: RelayPublicJwk::p256("public-x", "public-y"),
                client_nonce: "public-nonce".to_owned(),
                ttl_secs: 900,
            },
        )
        .await
        .expect("ticket");
    assert_eq!(ticket.machine.name, "contabo-1");
    assert_eq!(ticket.machine.project.as_deref(), Some("vowp"));
    assert_eq!(
        ticket.expires_at,
        Timestamp::parse("2026-11-26T12:05:00.000Z").expect("timestamp")
    );
    assert!(!format!("{ticket:?}").contains("eyJhbGciOiJFUzI1NiJ9"));

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    let scoped = serde_json::from_str::<serde_json::Value>(&requests[0].body).expect("scoped body");
    assert_eq!(scoped["ttl_secs"], 300);
    assert_eq!(scoped["client_key"]["kty"], "EC");
    assert_eq!(scoped["client_key"]["crv"], "P-256");
    assert_eq!(scoped["client_key"]["x"], "public-x");
    assert_eq!(scoped["client_nonce"], "public-nonce");
    assert_eq!(scoped["kind"], "desktop");
    assert_eq!(scoped["scopes"][0], "http.read");
    assert!(scoped.get("machine_id").is_none());
    assert!(!requests[0].body.contains("private"));
    assert!(!requests[0].body.contains("password"));
    let fallback =
        serde_json::from_str::<serde_json::Value>(&requests[1].body).expect("fallback body");
    assert_eq!(fallback["machine_id"], "mch_1");
}

#[tokio::test]
async fn a_ticket_auth_failure_never_falls_back() {
    let server = MockServer::start(Arc::new(|request: &RecordedRequest| {
        if request.path == "/v1/machines/mch_1/ticket" {
            return (
                401,
                json_body(&serde_json::json!({
                    "error": {"code": "unauthorized", "message": "session expired"}
                })),
            );
        }
        panic!("the fallback route must not be called after a 401");
    }));
    let (_directory, store) = temp_store();
    seed_session(&server, &store, "stored_token");
    let client = client(&server);
    assert!(client.restore_session(&store).expect("restore"));

    let error = client
        .mint_relay_ticket(
            "mch_1",
            RelayTicketRequest {
                kind: ClientKind::Desktop,
                scopes: vec![RelayScope::HttpRead],
                client_key: RelayPublicJwk::p256("public-x", "public-y"),
                client_nonce: "public-nonce".to_owned(),
                ttl_secs: 60,
            },
        )
        .await
        .expect_err("auth error");
    assert_eq!(error.code, CloudErrorCode::Auth);
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn device_poll_reports_pending_slow_down_and_authorized() {
    let calls = Arc::new(Mutex::new(0_u32));
    let calls_for_server = Arc::clone(&calls);
    let server = MockServer::start(Arc::new(move |request: &RecordedRequest| {
        assert_eq!(request.path, "/v1/auth/device/token");
        let mut count = calls_for_server.lock().expect("calls");
        *count += 1;
        match *count {
            1 => (
                400,
                json_body(&serde_json::json!({"error": "authorization_pending"})),
            ),
            2 => (400, json_body(&serde_json::json!({"error": "slow_down"}))),
            _ => (
                200,
                json_body(&serde_json::json!({
                    "access_token": "access_after_wait",
                    "refresh_token": "refresh_after_wait",
                    "expires_in": 900
                })),
            ),
        }
    }));
    let (_directory, store) = temp_store();
    let client = client(&server);

    assert_eq!(
        client
            .poll_device_sign_in(&store, "dev_1")
            .await
            .expect("pending"),
        DevicePollState::Pending
    );
    assert_eq!(
        client
            .poll_device_sign_in(&store, "dev_1")
            .await
            .expect("slow"),
        DevicePollState::SlowDown
    );
    assert_eq!(
        client
            .poll_device_sign_in(&store, "dev_1")
            .await
            .expect("authorized"),
        DevicePollState::Authorized
    );
    assert!(client.has_session());
    assert!(store.path().exists());
}

#[tokio::test]
async fn http_errors_are_typed_and_never_echo_a_token() {
    let server = MockServer::start(Arc::new(|request: &RecordedRequest| {
        match request.path.as_str() {
            "/v1/me" => (
                401,
                json_body(&serde_json::json!({
                    "error": {
                        "message": "token secret_access leaked at /Users/owner/private"
                    }
                })),
            ),
            "/v1/machines" => (429, json_body(&serde_json::json!({"message": "slow down"}))),
            "/v1/plans" => (500, "not json".to_owned()),
            _ => (404, "{}".to_owned()),
        }
    }));
    let (_directory, store) = temp_store();
    seed_session(&server, &store, "stored_token");
    let client = client(&server);
    assert!(client.restore_session(&store).expect("restore"));

    let auth = client.account().await.expect_err("401");
    assert_eq!(auth.code, CloudErrorCode::Auth);
    assert_eq!(auth.status, Some(401));
    assert!(!auth.message.contains("/Users/"), "{auth:?}");
    assert!(auth.hint.is_some());

    let limited = client.list_machines().await.expect_err("429");
    assert_eq!(limited.code, CloudErrorCode::RateLimited);
    assert!(limited.retryable);
    assert_eq!(limited.status, Some(429));

    let server_error = client.get_plans().await.expect_err("500");
    assert_eq!(server_error.code, CloudErrorCode::Server);
    assert!(server_error.retryable);
}

#[tokio::test]
async fn loose_or_linked_session_files_are_refused() {
    let (_directory, store) = temp_store();
    let server = MockServer::start(Arc::new(|_request: &RecordedRequest| {
        (200, "{}".to_owned())
    }));
    let session = CloudSession::new(server.url(), "access_secret_123");
    store.save(&session).expect("save");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(store.path(), std::fs::Permissions::from_mode(0o644))
            .expect("chmod");
        let error = store.load().expect_err("loose mode");
        assert_eq!(error.code, CloudErrorCode::Auth);
        assert!(!error.message.contains("access_secret_123"), "{error:?}");

        std::fs::set_permissions(store.path(), std::fs::Permissions::from_mode(0o600))
            .expect("chmod");
        let link = store.path().with_extension("link");
        std::os::unix::fs::symlink(store.path(), &link).expect("symlink");
        let linked = JsonCloudSessionStore::new(&link);
        let error = linked.load().expect_err("symlink");
        assert_eq!(error.code, CloudErrorCode::Auth);
        assert!(!error.message.contains("access_secret_123"), "{error:?}");
    }
}

#[tokio::test]
async fn refresh_rotates_the_stored_token() {
    let server = MockServer::start(Arc::new(|request: &RecordedRequest| {
        match request.path.as_str() {
            "/v1/auth/refresh" => {
                let body =
                    serde_json::from_str::<serde_json::Value>(&request.body).expect("refresh body");
                assert_eq!(body["refresh_token"], "refresh_one");
                (
                    200,
                    json_body(&serde_json::json!({
                        "access_token": "access_two",
                        "refresh_token": "refresh_two",
                        "expires_in": 900
                    })),
                )
            }
            "/v1/me" => {
                assert_eq!(request.header("authorization"), Some("Bearer access_two"));
                (
                    200,
                    json_body(&serde_json::json!({
                        "id": "acct_1",
                        "email": "owner@example.test",
                        "name": null
                    })),
                )
            }
            _ => (404, "{}".to_owned()),
        }
    }));
    let (_directory, store) = temp_store();
    let session = CloudSession::new(server.url(), "access_one").with_refresh_token("refresh_one");
    store.save(&session).expect("save");
    let client = client(&server);
    assert!(client.restore_session(&store).expect("restore"));

    client.refresh_session(&store).await.expect("refresh");
    let account = client.account().await.expect("account");
    assert_eq!(account.email, "owner@example.test");
    let text = std::fs::read_to_string(store.path()).expect("session file");
    assert!(text.contains("access_two"));
    assert!(text.contains("refresh_two"));
    assert!(!text.contains("access_one"));
    assert!(!text.contains("refresh_one"));
}

#[tokio::test]
async fn api_bases_are_normalized_and_plain_http_is_loopback_only() {
    let valid = CloudClient::new(LoopbackTransport, "https://api.stackspirit.io/")
        .expect("production base");
    assert_eq!(valid.api_base_url(), "https://api.stackspirit.io");
    let loopback =
        CloudClient::new(LoopbackTransport, "http://127.0.0.1:4567").expect("loopback base");
    assert_eq!(loopback.api_base_url(), "http://127.0.0.1:4567");

    for invalid in [
        "",
        "ftp://api.stackspirit.io",
        "https://api.stackspirit.io/v1",
        "https://user:pass@api.stackspirit.io",
        "http://api.stackspirit.io",
        "http://127.0.0.1.evil.test",
        "https://api.stackspirit.io?x=1",
    ] {
        let error =
            CloudClient::new(LoopbackTransport, invalid).expect_err("invalid base must be refused");
        assert_eq!(error.code, CloudErrorCode::Unavailable, "{invalid}");
    }
}

#[tokio::test]
async fn a_stored_session_for_another_api_is_not_used() {
    let server = MockServer::start(Arc::new(|_request: &RecordedRequest| {
        (200, "{}".to_owned())
    }));
    let (_directory, store) = temp_store();
    store
        .save(&CloudSession::new("https://other.example", "other_token"))
        .expect("save");
    let client = client(&server);
    assert!(!client.restore_session(&store).expect("restore"));
    assert!(!client.has_session());
    assert_eq!(server.requests().len(), 0);
}
