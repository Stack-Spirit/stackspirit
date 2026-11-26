//! The StackSpirit relay machine side: identity, enrollment, the end-to-end channel and the
//! connector.
//!
//! Spec: `docs/design/relay-and-packaging.md` §6 and `docs/design/crates/vowp-agent.md`. This module
//! holds the shared error type, a small blocking network layer (plain TCP or TLS 1.3), the API
//! client and the `enroll`, `rotate_key`, `refresh_credential` and `status_report` operations.
//! Submodules:
//!
//! * [`key`] — `relay.key`/`relay.pub`/`relay.json` and the status file;
//! * [`auth`] — challenge signing, ES256 JWT and ticket verification, replay caches;
//! * [`crypto`] — P-256/HKDF/AES-GCM channel and the canonical transcript;
//! * [`connector`] — the machine connector loop.
//!
//! Nothing here logs or returns a token, credential, private key or payload.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use vowp_protocol::{MachineCapabilities, REMOTE_PROTOCOL, RelayConfig, Timestamp};

pub mod auth;
pub mod connector;
pub mod crypto;
pub mod key;

pub use auth::{
    CachedJwks, CredentialClaims, Jwks, JwksKey, Jwt, NonceSet, ReplayCache, TicketClaims, TicketContext, TicketError,
    credential_refresh_canonical, machine_auth_canonical, sign_es256, sign_machine_auth, verify_machine_auth,
    verify_ticket,
};
pub use connector::{Connector, ConnectorOptions, ConnectorOutcome, ConnectorReport};
pub use crypto::{
    AcceptInput, ChannelCipher, ChannelKeys, EphemeralKey, MachineAccept, Role, TranscriptInput, client_finish,
    jwk_from_public, jwk_thumbprint, machine_accept, machine_accept_with, public_from_jwk, random_nonce, transcript,
};
pub use key::{
    ConnectorState, KEY_ID_PREFIX, RELAY_JSON_FILE, RELAY_JWKS_FILE, RELAY_KEY_FILE, RELAY_PUB_FILE, RELAY_STATUS_FILE,
    RelayJson, RelayKey, RelayPaths, RelayStatusFile, verify_hex, write_atomic,
};

/// The agent version string sent in `machine.hello` and `enroll`.
pub const RELAY_AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Domain separator of the key-rotation signature.
pub const ROTATE_DOMAIN: &str = "STACKSPIRIT-ROTATE-V1";
/// Largest API response body the machine reads, in bytes.
pub const MAX_API_BODY_BYTES: usize = 1_048_576;
/// Connect and I/O timeout for an API call.
pub const API_TIMEOUT: Duration = Duration::from_secs(15);
/// Refresh a credential this many seconds before it expires.
pub const CREDENTIAL_REFRESH_MARGIN_SECS: i64 = 600;

/// Errors from the relay machine side. Messages never contain a secret value.
#[derive(Debug, thiserror::Error)]
pub enum RelayError {
    /// A local filesystem operation failed.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// A setting or file is invalid.
    #[error("config: {0}")]
    Config(String),
    /// `relay.key` or `relay.json` is missing.
    #[error("not enrolled: {0}")]
    NotEnrolled(String),
    /// `relay.json` is already present.
    #[error("already enrolled: {0}")]
    AlreadyEnrolled(String),
    /// The relay key file is unreadable or has the wrong mode.
    #[error("relay key: {0}")]
    Key(String),
    /// A signature, credential or ticket was rejected.
    #[error("authentication: {0}")]
    Auth(String),
    /// The end-to-end channel failed.
    #[error("crypto: {0}")]
    Crypto(String),
    /// The peer broke the protocol.
    #[error("protocol: {0}")]
    Protocol(String),
    /// A network operation failed.
    #[error("network: {0}")]
    Network(String),
    /// The API refused a request.
    #[error("api {status}: {code}")]
    Api {
        /// HTTP status.
        status: u16,
        /// A short sanitised error code from the response.
        code: String,
    },
    /// The owner revoked this machine.
    #[error("this machine was revoked")]
    Revoked,
}

// ----------------------------------------------------------------------------------------------
// Network layer
// ----------------------------------------------------------------------------------------------

/// A plain or TLS 1.3 client stream.
pub(crate) enum RelayStream {
    /// Cleartext TCP: loopback test endpoints and the loopback daemon only.
    Plain(TcpStream),
    /// TLS 1.3 over TCP.
    Tls(Box<StreamOwned<ClientConnection, TcpStream>>),
}

impl RelayStream {
    /// Sets the read timeout on the underlying socket.
    pub(crate) fn set_read_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.tcp().set_read_timeout(timeout)
    }

    fn tcp(&self) -> &TcpStream {
        match self {
            RelayStream::Plain(tcp) => tcp,
            RelayStream::Tls(tls) => tls.get_ref(),
        }
    }
}

impl Read for RelayStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            RelayStream::Plain(tcp) => tcp.read(buf),
            RelayStream::Tls(tls) => tls.read(buf),
        }
    }
}

impl Write for RelayStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            RelayStream::Plain(tcp) => tcp.write(buf),
            RelayStream::Tls(tls) => tls.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            RelayStream::Plain(tcp) => tcp.flush(),
            RelayStream::Tls(tls) => tls.flush(),
        }
    }
}

/// A parsed `http(s)://host:port/base` or `ws(s)://...` URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Endpoint {
    pub(crate) tls: bool,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) path: String,
}

impl Endpoint {
    /// Parses a URL whose scheme is one of `http`, `https`, `ws`, `wss`.
    pub(crate) fn parse(url: &str) -> Result<Self, RelayError> {
        let (tls, rest) = if let Some(rest) = url.strip_prefix("https://").or_else(|| url.strip_prefix("wss://")) {
            (true, rest)
        } else if let Some(rest) = url.strip_prefix("http://").or_else(|| url.strip_prefix("ws://")) {
            (false, rest)
        } else {
            return Err(RelayError::Config("the URL scheme must be http(s) or ws(s)".to_owned()));
        };
        let (authority, path) = match rest.find('/') {
            Some(index) => (&rest[..index], &rest[index..]),
            None => (rest, "/"),
        };
        if authority.is_empty() || authority.contains('@') {
            return Err(RelayError::Config("the URL host is invalid".to_owned()));
        }
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) if !host.ends_with(']') || host.starts_with('[') => (
                host.trim_start_matches('[').trim_end_matches(']').to_owned(),
                port.parse::<u16>()
                    .map_err(|_| RelayError::Config("the URL port is invalid".to_owned()))?,
            ),
            _ => (authority.to_owned(), if tls { 443 } else { 80 }),
        };
        Ok(Self {
            tls,
            host,
            port,
            path: path.to_owned(),
        })
    }

    /// True when the host is a loopback IP literal or `localhost`.
    pub(crate) fn is_loopback(&self) -> bool {
        self.host == "localhost"
            || self
                .host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    }

    /// The `Host` header value.
    pub(crate) fn host_header(&self) -> String {
        let default = if self.tls { 443 } else { 80 };
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        if self.port == default {
            host
        } else {
            format!("{host}:{}", self.port)
        }
    }

    /// Opens a connection with `timeout` for connect, read and write.
    pub(crate) fn connect(&self, timeout: Duration) -> Result<RelayStream, RelayError> {
        let network = |what: &str, error: std::io::Error| RelayError::Network(format!("{what}: {error}"));
        let mut last = None;
        let mut tcp = None;
        for address in (self.host.as_str(), self.port)
            .to_socket_addrs()
            .map_err(|error| network("resolve", error))?
        {
            match TcpStream::connect_timeout(&address, timeout) {
                Ok(stream) => {
                    tcp = Some(stream);
                    break;
                }
                Err(error) => last = Some(error),
            }
        }
        let tcp = tcp.ok_or_else(|| match last {
            Some(error) => network("connect", error),
            None => RelayError::Network("connect: no address".to_owned()),
        })?;
        tcp.set_nodelay(true).map_err(|error| network("nodelay", error))?;
        tcp.set_read_timeout(Some(timeout))
            .map_err(|error| network("timeout", error))?;
        tcp.set_write_timeout(Some(timeout))
            .map_err(|error| network("timeout", error))?;
        if !self.tls {
            return Ok(RelayStream::Plain(tcp));
        }
        let server_name = ServerName::try_from(self.host.clone())
            .map_err(|_| RelayError::Config("the URL host is not a usable TLS name".to_owned()))?;
        let connection = ClientConnection::new(Arc::new(tls_config()?), server_name)
            .map_err(|error| RelayError::Network(format!("tls: {error}")))?;
        Ok(RelayStream::Tls(Box::new(StreamOwned::new(connection, tcp))))
    }
}

/// TLS 1.3 with the system trust roots.
fn tls_config() -> Result<ClientConfig, RelayError> {
    let provider = rustls::crypto::CryptoProvider::get_default()
        .cloned()
        .unwrap_or_else(|| Arc::new(rustls::crypto::aws_lc_rs::default_provider()));
    let mut roots = RootCertStore::empty();
    let native = rustls_native_certs::load_native_certs();
    let (added, _ignored) = roots.add_parsable_certificates(native.certs);
    if added == 0 {
        return Err(RelayError::Config("no system trust roots are available".to_owned()));
    }
    Ok(ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|error| RelayError::Config(format!("tls 1.3 is unavailable: {error}")))?
        .with_root_certificates(roots)
        .with_no_client_auth())
}

// ----------------------------------------------------------------------------------------------
// Minimal HTTP/1.1 client (API calls and the loopback daemon)
// ----------------------------------------------------------------------------------------------

/// The status line and headers of an HTTP response.
#[derive(Debug, Clone)]
pub(crate) struct ResponseHead {
    pub(crate) status: u16,
    /// Header names are lowercased.
    pub(crate) headers: Vec<(String, String)>,
}

impl ResponseHead {
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// Writes one request. `headers` must not contain CR/LF; the caller validated them.
pub(crate) fn write_request(
    stream: &mut RelayStream,
    method: &str,
    endpoint_host: &str,
    path: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> Result<(), RelayError> {
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {endpoint_host}\r\nConnection: close\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if method == "POST" || !body.is_empty() {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    head.push_str("\r\n");
    stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(body))
        .and_then(|()| stream.flush())
        .map_err(|error| RelayError::Network(format!("write: {error}")))
}

pub(crate) fn is_idle(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut | std::io::ErrorKind::Interrupted
    )
}

/// `read_line` that keeps partial data across read timeouts. `idle` returns true to keep waiting.
fn read_line_idle(
    reader: &mut BufReader<RelayStream>,
    line: &mut String,
    idle: &mut dyn FnMut() -> bool,
) -> Result<usize, RelayError> {
    let start = line.len();
    loop {
        match reader.read_line(line) {
            Ok(_) => return Ok(line.len() - start),
            Err(error) if is_idle(&error) => {
                if !idle() {
                    return Err(RelayError::Network("read: timed out".to_owned()));
                }
            }
            Err(error) => return Err(RelayError::Network(format!("read: {error}"))),
        }
    }
}

/// Reads the response status line and headers. `idle` returns true to keep waiting after a
/// read timeout.
pub(crate) fn read_head(
    reader: &mut BufReader<RelayStream>,
    idle: &mut dyn FnMut() -> bool,
) -> Result<ResponseHead, RelayError> {
    let mut line = String::new();
    read_line_idle(reader, &mut line, idle)?;
    let status = line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| RelayError::Protocol("malformed HTTP status line".to_owned()))?;
    let mut headers = Vec::new();
    loop {
        line.clear();
        if read_line_idle(reader, &mut line, idle)? == 0 {
            return Err(RelayError::Protocol("truncated HTTP headers".to_owned()));
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if headers.len() >= 100 {
            return Err(RelayError::Protocol("too many HTTP headers".to_owned()));
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }
    Ok(ResponseHead { status, headers })
}

/// Reads a response body (content-length, chunked or until close), passing each piece to `sink`.
/// `sink` returns false to stop early. Reads at most `limit` bytes; more is an error. `idle`
/// returns true to keep waiting after a read timeout (streaming); false aborts with an error.
pub(crate) fn read_body(
    reader: &mut BufReader<RelayStream>,
    head: &ResponseHead,
    limit: usize,
    sink: &mut dyn FnMut(&[u8]) -> bool,
    idle: &mut dyn FnMut() -> bool,
) -> Result<(), RelayError> {
    let network = |error: std::io::Error| RelayError::Network(format!("read: {error}"));
    let mut total = 0usize;
    let mut buffer = [0u8; 16 * 1024];
    let mut emit = |bytes: &[u8], total: &mut usize| -> Result<bool, RelayError> {
        *total += bytes.len();
        if *total > limit {
            return Err(RelayError::Protocol("the response is over the size limit".to_owned()));
        }
        Ok(sink(bytes))
    };
    let chunked = head
        .header("transfer-encoding")
        .is_some_and(|value| value.to_ascii_lowercase().contains("chunked"));
    if chunked {
        loop {
            let mut size_line = String::new();
            if read_line_idle(reader, &mut size_line, idle)? == 0 {
                return Err(RelayError::Protocol("truncated chunked body".to_owned()));
            }
            let size_text = size_line.trim().split(';').next().unwrap_or("");
            let mut remaining = usize::from_str_radix(size_text, 16)
                .map_err(|_| RelayError::Protocol("malformed chunk size".to_owned()))?;
            if remaining == 0 {
                return Ok(());
            }
            while remaining > 0 {
                let want = remaining.min(buffer.len());
                let read = match reader.read(&mut buffer[..want]) {
                    Ok(read) => read,
                    Err(error) if is_idle(&error) => {
                        if idle() {
                            continue;
                        }
                        return Err(RelayError::Network("read: timed out".to_owned()));
                    }
                    Err(error) => return Err(network(error)),
                };
                if read == 0 {
                    return Err(RelayError::Protocol("truncated chunk".to_owned()));
                }
                remaining -= read;
                if !emit(&buffer[..read], &mut total)? {
                    return Ok(());
                }
            }
            let mut crlf = String::new();
            read_line_idle(reader, &mut crlf, idle)?;
        }
    }
    let mut remaining = head
        .header("content-length")
        .and_then(|value| value.parse::<usize>().ok());
    loop {
        let want = remaining.map_or(buffer.len(), |left| left.min(buffer.len()));
        if want == 0 {
            return Ok(());
        }
        let read = match reader.read(&mut buffer[..want]) {
            Ok(read) => read,
            Err(error) if is_idle(&error) => {
                if idle() {
                    continue;
                }
                return Err(RelayError::Network("read: timed out".to_owned()));
            }
            Err(error) => return Err(network(error)),
        };
        if read == 0 {
            return match remaining {
                Some(left) if left > 0 => Err(RelayError::Protocol("truncated body".to_owned())),
                _ => Ok(()),
            };
        }
        if let Some(left) = remaining.as_mut() {
            *left -= read;
        }
        if !emit(&buffer[..read], &mut total)? {
            return Ok(());
        }
    }
}

/// A buffered JSON request: connect, send, read the whole (size-limited) response.
fn http_json(
    base_url: &str,
    method: &str,
    path: &str,
    extra_headers: &[(String, String)],
    body: Option<&serde_json::Value>,
) -> Result<(u16, Vec<u8>), RelayError> {
    let endpoint = Endpoint::parse(base_url)?;
    if !endpoint.tls && !endpoint.is_loopback() {
        return Err(RelayError::Config("a non-loopback API URL must be https".to_owned()));
    }
    let prefix = endpoint.path.trim_end_matches('/');
    let mut headers = vec![("Accept".to_owned(), "application/json".to_owned())];
    headers.extend_from_slice(extra_headers);
    let payload = match body {
        Some(value) => {
            headers.push(("Content-Type".to_owned(), "application/json".to_owned()));
            serde_json::to_vec(value).map_err(|_| RelayError::Config("request body".to_owned()))?
        }
        None => Vec::new(),
    };
    let mut stream = endpoint.connect(API_TIMEOUT)?;
    write_request(
        &mut stream,
        method,
        &endpoint.host_header(),
        &format!("{prefix}{path}"),
        &headers,
        &payload,
    )?;
    let mut reader = BufReader::new(stream);
    let head = read_head(&mut reader, &mut || false)?;
    let mut bytes = Vec::new();
    read_body(
        &mut reader,
        &head,
        MAX_API_BODY_BYTES,
        &mut |chunk| {
            bytes.extend_from_slice(chunk);
            true
        },
        &mut || false,
    )?;
    Ok((head.status, bytes))
}

// ----------------------------------------------------------------------------------------------
// API client and operations
// ----------------------------------------------------------------------------------------------

/// The API's answer to `POST /v1/machines/enroll` (and to a key rotation).
#[derive(Clone, Deserialize)]
struct MachineGrant {
    #[serde(default)]
    machine_id: String,
    #[serde(default)]
    account_id: String,
    machine_credential: String,
    #[serde(default)]
    relay_url: String,
    #[serde(default)]
    api_url: String,
    expires_at: String,
}

/// Maps a non-2xx API answer to an error without echoing the body.
fn api_error(status: u16, body: &[u8]) -> RelayError {
    if status == 403 {
        return RelayError::Revoked;
    }
    let value: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
    let code = value
        .pointer("/error/code")
        .or_else(|| value.get("code"))
        .or_else(|| value.get("error"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("error");
    let code: String = code
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(48)
        .collect();
    RelayError::Api {
        status,
        code: if code.is_empty() { "error".to_owned() } else { code },
    }
}

fn call_json(
    api_url: &str,
    method: &str,
    path: &str,
    headers: &[(String, String)],
    body: Option<&serde_json::Value>,
) -> Result<serde_json::Value, RelayError> {
    let (status, bytes) = http_json(api_url, method, path, headers, body)?;
    if !(200..300).contains(&status) {
        return Err(api_error(status, &bytes));
    }
    serde_json::from_slice(&bytes).map_err(|_| RelayError::Protocol("the API answer is not JSON".to_owned()))
}

fn parse_grant(value: serde_json::Value) -> Result<MachineGrant, RelayError> {
    serde_json::from_value(value).map_err(|_| RelayError::Protocol("the API answer is missing fields".to_owned()))
}

/// Fetches the API signing keys from `/.well-known/jwks.json`.
pub fn fetch_jwks(api_url: &str) -> Result<Jwks, RelayError> {
    let value = call_json(api_url, "GET", "/.well-known/jwks.json", &[], None)?;
    serde_json::from_value(value).map_err(|_| RelayError::Protocol("the JWKS answer is malformed".to_owned()))
}

/// The capabilities a machine advertises for `config`.
pub fn capabilities_for(config: &RelayConfig) -> MachineCapabilities {
    MachineCapabilities {
        protocol: REMOTE_PROTOCOL.to_owned(),
        http: config.allow_http_read || config.allow_http_write,
        mcp: config.allow_mcp_read || config.allow_mcp_write,
        ssh: config.allow_ssh,
        exec: config.allow_exec,
        max_channels: config.max_channels,
        projects: if config.project.is_empty() {
            Vec::new()
        } else {
            vec![config.project.clone()]
        },
    }
}

/// Inputs to [`enroll`]. `token` is the single-use `ssm_...` value; it is sent once, never stored.
pub struct EnrollRequest<'a> {
    /// The node home.
    pub home: &'a Path,
    /// The `[relay]` table to enroll under.
    pub config: &'a RelayConfig,
    /// The enrollment token.
    pub token: &'a str,
    /// Machine display name.
    pub name: &'a str,
    /// Current time.
    pub now: Timestamp,
}

/// What `enroll` reports. It carries no credential and no token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnrollOutcome {
    /// Machine id.
    pub machine_id: String,
    /// Account id.
    pub account_id: String,
    /// `ed25519:<hex>`.
    pub key_id: String,
    /// Display name.
    pub name: String,
    /// API URL used.
    pub api_url: String,
    /// Relay URL to connect to.
    pub relay_url: String,
    /// Credential expiry.
    pub credential_expires_at: String,
}

/// Enrolls this machine: reuses or generates `relay.key`, calls `POST /v1/machines/enroll`, and
/// writes `relay.json` (0600). `relay.key` is 0600 and `relay.pub` 0644.
///
/// The key is written before the call and reused by a retry; the `Idempotency-Key` is derived from
/// the token and the public key so a lost reply can be retried without a `409`. An existing
/// `relay.json` is refused.
pub fn enroll(request: &EnrollRequest<'_>) -> Result<EnrollOutcome, RelayError> {
    request
        .config
        .validate()
        .map_err(|error| RelayError::Config(error.to_string()))?;
    if !request.token.starts_with("ssm_") || request.token.len() < 20 || request.token.chars().any(char::is_whitespace)
    {
        return Err(RelayError::Config(
            "the enrollment token is not an ssm_ token".to_owned(),
        ));
    }
    let paths = RelayPaths::new(request.home, request.config);
    if paths.credential().exists() {
        return Err(RelayError::AlreadyEnrolled(format!(
            "{} exists; remove it to enroll again",
            request.config.credential_file
        )));
    }
    let key = if request.home.join(&request.config.key_file).is_file() {
        RelayKey::load(request.home, &request.config.key_file)?
    } else {
        let key = RelayKey::generate();
        key.write(request.home, &request.config.key_file)?;
        key
    };
    let idempotency = hex::encode(Sha256::digest(
        format!("stackspirit-enroll\n{}\n{}", request.token, key.public_key_hex()).as_bytes(),
    ));
    let body = serde_json::json!({
        "token": request.token,
        "public_key": key.public_key_hex(),
        "key_id": key.key_id(),
        "name": request.name,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "agent_version": RELAY_AGENT_VERSION,
        "capabilities": capabilities_for(request.config),
    });
    let grant = parse_grant(call_json(
        &request.config.api_url,
        "POST",
        "/v1/machines/enroll",
        &[("Idempotency-Key".to_owned(), idempotency[..32].to_owned())],
        Some(&body),
    )?)?;
    if grant.machine_id.is_empty() || grant.account_id.is_empty() {
        return Err(RelayError::Protocol(
            "the enrollment answer has no machine id".to_owned(),
        ));
    }
    // The API's URLs are written to relay.json and used by the connector: hold them to the same
    // rules as the `[relay]` table before they are saved.
    RelayConfig {
        api_url: if grant.api_url.is_empty() {
            request.config.api_url.clone()
        } else {
            grant.api_url.clone()
        },
        relay_url: if grant.relay_url.is_empty() {
            request.config.relay_url.clone()
        } else {
            grant.relay_url.clone()
        },
        ..request.config.clone()
    }
    .validate()
    .map_err(|_| RelayError::Protocol("the enrollment answer has an unusable URL".to_owned()))?;
    let record = RelayJson {
        v: 1,
        api_url: if grant.api_url.is_empty() {
            request.config.api_url.clone()
        } else {
            grant.api_url.clone()
        },
        relay_url: if grant.relay_url.is_empty() {
            request.config.relay_url.clone()
        } else {
            grant.relay_url.clone()
        },
        machine_id: grant.machine_id.clone(),
        account_id: grant.account_id.clone(),
        key_id: key.key_id(),
        credential: grant.machine_credential.clone(),
        credential_expires_at: grant.expires_at.clone(),
        name: request.name.to_owned(),
        project: request.config.project.clone(),
        ssh_user: request.config.ssh_user.clone(),
        registered_at: request.now.as_rfc3339(),
    };
    record.save(&paths.credential())?;
    Ok(EnrollOutcome {
        machine_id: record.machine_id,
        account_id: record.account_id,
        key_id: record.key_id,
        name: record.name,
        api_url: record.api_url,
        relay_url: record.relay_url,
        credential_expires_at: record.credential_expires_at,
    })
}

/// Refreshes the machine credential with a signed request and updates `relay.json`.
///
/// A `403` means the owner revoked the machine and returns [`RelayError::Revoked`].
pub fn refresh_credential(paths: &RelayPaths, now: Timestamp) -> Result<RelayJson, RelayError> {
    let mut record = RelayJson::load(&paths.credential())?;
    let key = RelayKey::load(&paths.home, &paths.key_file)?;
    if key.key_id() != record.key_id {
        return Err(RelayError::Key("relay.key does not match relay.json".to_owned()));
    }
    let signed_at = now.as_rfc3339();
    let canonical = auth::credential_refresh_canonical(&record.machine_id, &record.key_id, &signed_at);
    let body = serde_json::json!({
        "key_id": record.key_id,
        "signed_at": signed_at,
        "signature": key.sign_hex(canonical.as_bytes()),
    });
    let grant = parse_grant(call_json(
        &record.api_url,
        "POST",
        &format!("/v1/machines/{}/credential", record.machine_id),
        &[],
        Some(&body),
    )?)?;
    record.credential = grant.machine_credential;
    record.credential_expires_at = grant.expires_at;
    record.save(&paths.credential())?;
    Ok(record)
}

/// Rotates the relay key: signs a request with the old key, calls `POST /v1/machines/{id}/keys`,
/// then writes the new `relay.key` atomically and updates `relay.json`. Returns the new key id.
///
/// The signed string is
/// `STACKSPIRIT-ROTATE-V1\n<machine_id>\n<old_key_id>\n<new_key_id>\n<new_public_key>\n<signed_at>`.
/// The old key stays valid at the API for 24 hours to drain connections. On any API error the old
/// key and `relay.json` are left untouched.
pub fn rotate_key(paths: &RelayPaths, now: Timestamp) -> Result<String, RelayError> {
    let mut record = RelayJson::load(&paths.credential())?;
    let old = RelayKey::load(&paths.home, &paths.key_file)?;
    if old.key_id() != record.key_id {
        return Err(RelayError::Key("relay.key does not match relay.json".to_owned()));
    }
    let new = RelayKey::generate();
    let signed_at = now.as_rfc3339();
    let canonical = [
        ROTATE_DOMAIN,
        &record.machine_id,
        &old.key_id(),
        &new.key_id(),
        &new.public_key_hex(),
        &signed_at,
    ]
    .join("\n");
    let body = serde_json::json!({
        "key_id": old.key_id(),
        "new_key_id": new.key_id(),
        "new_public_key": new.public_key_hex(),
        "signed_at": signed_at,
        "signature": old.sign_hex(canonical.as_bytes()),
        "new_signature": new.sign_hex(canonical.as_bytes()),
    });
    let grant = parse_grant(call_json(
        &record.api_url,
        "POST",
        &format!("/v1/machines/{}/keys", record.machine_id),
        &[],
        Some(&body),
    )?)?;
    new.write(&paths.home, &paths.key_file)?;
    record.key_id = new.key_id();
    record.credential = grant.machine_credential;
    record.credential_expires_at = grant.expires_at;
    record.save(&paths.credential())?;
    Ok(new.key_id())
}

/// `stackspirit relay status`: what is configured and what the connector last reported.
/// It never contains the credential, a token or a private key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusReport {
    /// `[relay].enabled`.
    pub enabled: bool,
    /// True when `relay.key` and `relay.json` exist.
    pub enrolled: bool,
    /// Machine id, when enrolled.
    pub machine_id: Option<String>,
    /// Account id, when enrolled.
    pub account_id: Option<String>,
    /// Key id, when enrolled.
    pub key_id: Option<String>,
    /// Display name.
    pub name: Option<String>,
    /// Served project.
    pub project: Option<String>,
    /// API URL.
    pub api_url: String,
    /// Relay URL.
    pub relay_url: String,
    /// Credential expiry, when enrolled.
    pub credential_expires_at: Option<String>,
    /// True when the credential has expired (the connector refreshes it on start).
    pub credential_expired: Option<bool>,
    /// Local scope policy.
    pub policy: StatusPolicy,
    /// The connector's last status file.
    pub connector: Option<RelayStatusFile>,
    /// True when the status file names a connector process that no longer exists.
    pub connector_stale: bool,
}

/// The local `allow_*` policy, as shown by status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusPolicy {
    /// `allow_http_read`.
    pub http_read: bool,
    /// `allow_http_write`.
    pub http_write: bool,
    /// `allow_mcp_read`.
    pub mcp_read: bool,
    /// `allow_mcp_write`.
    pub mcp_write: bool,
    /// `allow_ssh`.
    pub ssh: bool,
    /// `allow_exec`.
    pub exec: bool,
}

/// Builds the status report from the home's files. Reads no secret into the report.
pub fn status_report(paths: &RelayPaths, config: &RelayConfig, now: Timestamp) -> StatusReport {
    let record = RelayJson::load(&paths.credential()).ok();
    let enrolled = record.is_some() && paths.home.join(&config.key_file).is_file();
    let connector = RelayStatusFile::load(&paths.status());
    let connector_stale = connector.as_ref().is_some_and(|status| {
        matches!(
            status.state,
            ConnectorState::Online | ConnectorState::Connecting | ConnectorState::Backoff
        ) && Path::new("/proc").is_dir()
            && !Path::new(&format!("/proc/{}", status.pid)).exists()
    });
    let expired = record.as_ref().and_then(|record| {
        Timestamp::parse(&record.credential_expires_at)
            .ok()
            .map(|expires| expires.unix_secs() <= now.unix_secs())
    });
    StatusReport {
        enabled: config.enabled,
        enrolled,
        machine_id: record.as_ref().map(|r| r.machine_id.clone()),
        account_id: record.as_ref().map(|r| r.account_id.clone()),
        key_id: record.as_ref().map(|r| r.key_id.clone()),
        name: record.as_ref().map(|r| r.name.clone()),
        project: record.as_ref().map(|r| r.project.clone()),
        api_url: record
            .as_ref()
            .map_or_else(|| config.api_url.clone(), |r| r.api_url.clone()),
        relay_url: record
            .as_ref()
            .map_or_else(|| config.relay_url.clone(), |r| r.relay_url.clone()),
        credential_expires_at: record.as_ref().map(|r| r.credential_expires_at.clone()),
        credential_expired: expired,
        policy: StatusPolicy {
            http_read: config.allow_http_read,
            http_write: config.allow_http_write,
            mcp_read: config.allow_mcp_read,
            mcp_write: config.allow_mcp_write,
            ssh: config.allow_ssh,
            exec: config.allow_exec,
        },
        connector,
        connector_stale,
    }
}
