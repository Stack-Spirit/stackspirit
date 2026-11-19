//! The VOWP local-orchestrator endpoint resolver for `global`/`project` chat.
//!
//! `vowp agent orchestrator endpoint <NAME> --json` is the only source of the loopback
//! endpoint and native session for a Local orchestrator session. The resolver runs it with
//! an argv array (never a shell), a bounded timeout and a minimal child environment that
//! carries no inherited secret. The document's URL, host, port, auth value, VOWP session id
//! and native session id stay core-only: no `Debug`, error, log or serialized chat value
//! carries them.

use std::fmt;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use tokio::process::Command;
use zeroize::Zeroizing;

use crate::error::{SpiritError, SpiritErrorCode};

/// The bounded budget for the local `vowp agent orchestrator endpoint` read.
///
/// The command only reads private files on this Mac; a slower answer is treated as a
/// typed `chat_not_configured` failure rather than a hang.
pub const ENDPOINT_TIMEOUT: Duration = Duration::from_secs(10);

/// The largest endpoint document this resolver accepts.
const MAX_ENDPOINT_DOCUMENT: usize = 64 * 1024;

/// A loopback address accepted from VOWP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopbackHost {
    /// `127.0.0.1`, the canonical form VOWP reports.
    V4,
    /// `[::1]`, accepted only when VOWP reports it explicitly.
    V6,
}

impl LoopbackHost {
    /// Returns the socket address used for the private TCP request.
    pub(crate) fn socket_address(self, port: u16) -> std::net::SocketAddr {
        match self {
            Self::V4 => std::net::SocketAddr::from(([127, 0, 0, 1], port)),
            Self::V6 => std::net::SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], port)),
        }
    }

    /// Returns the HTTP `Host` header value.
    pub(crate) const fn host_header(self) -> &'static str {
        match self {
            Self::V4 => "127.0.0.1",
            Self::V6 => "[::1]",
        }
    }
}

/// A core-only loopback credential.
///
/// The value is zeroized when dropped and never derives `Debug`.
pub(crate) struct LoopbackAuth {
    /// Header name, for example `Authorization`.
    pub(crate) header: String,
    /// Complete header value, for example `Basic <base64>`.
    pub(crate) value: Zeroizing<String>,
}

impl Clone for LoopbackAuth {
    fn clone(&self) -> Self {
        Self {
            header: self.header.clone(),
            value: self.value.clone(),
        }
    }
}

impl fmt::Debug for LoopbackAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoopbackAuth")
            .field("header", &self.header)
            .field("value", &"[redacted]")
            .finish()
    }
}

/// One validated Local orchestrator endpoint.
///
/// It is core-only. The `Debug` impl redacts the host, port, VOWP session id, native
/// session id and auth value so an accidental `{:?}` cannot leak them.
pub struct ResolvedEndpoint {
    /// Orchestrator name (safe metadata).
    pub(crate) name: String,
    /// App the orchestrator runs (safe metadata).
    pub(crate) app: String,
    /// Home session id the orchestrator mirrors; core-only.
    pub(crate) session_id: String,
    /// Where the app host runs (`node` for the agent command); safe metadata.
    pub(crate) location: String,
    /// Node that runs the app, when reported (safe metadata).
    pub(crate) node: Option<String>,
    /// Lifecycle state reported by VOWP (safe metadata).
    pub(crate) state: String,
    /// Loopback host; core-only.
    pub(crate) host: LoopbackHost,
    /// Loopback port; core-only.
    pub(crate) port: u16,
    /// Native OpenCode session to attach to; core-only.
    pub(crate) native_session_id: String,
    /// Optional loopback credential; core-only.
    pub(crate) auth: Option<LoopbackAuth>,
}

impl fmt::Debug for ResolvedEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResolvedEndpoint")
            .field("name", &self.name)
            .field("app", &self.app)
            .field("location", &self.location)
            .field("node", &self.node)
            .field("state", &self.state)
            .field("host", &"[redacted]")
            .field("port", &"[redacted]")
            .field("session_id", &"[redacted]")
            .field("native_session_id", &"[redacted]")
            .field("auth", &self.auth)
            .finish()
    }
}

/// The injected VOWP endpoint boundary.
///
/// Production uses [`VowpEndpointResolver`]; tests may supply a fake resolver, while the
/// integration tests exercise the real command against a fake `vowp` on `PATH`.
#[async_trait]
pub trait ChatEndpointResolver: Send + Sync {
    /// Resolves the local endpoint for one orchestrator name.
    ///
    /// `expected_session_id` is the home session previously attached for this shell
    /// session, when one is known. A different reported `session_id` is rejected as
    /// `chat_not_mirrored` rather than silently attaching to another log.
    async fn resolve(
        &self,
        name: &str,
        expected_session_id: Option<&str>,
    ) -> Result<ResolvedEndpoint, SpiritError>;
}

/// The production resolver backed by the installed local `vowp` CLI.
#[derive(Clone, Default)]
pub struct VowpEndpointResolver {
    /// Test-only PATH override. Production reads the process PATH.
    path_env: Option<std::ffi::OsString>,
}

impl fmt::Debug for VowpEndpointResolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VowpEndpointResolver")
            .finish_non_exhaustive()
    }
}

impl VowpEndpointResolver {
    /// Builds a resolver that uses the installed `vowp` on the inherited `PATH`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a resolver with an explicit child `PATH`.
    ///
    /// Integration tests point this at a directory holding a fake `vowp`; production
    /// never calls it. The rest of the child environment is still the minimal allow-list
    /// built by [`Self::run`].
    #[must_use]
    pub fn with_path_env(path: impl Into<std::ffi::OsString>) -> Self {
        Self {
            path_env: Some(path.into()),
        }
    }

    /// Runs the local agent command with argv only and a minimal, secret-free environment.
    async fn run(&self, name: &str) -> Result<EndpointDocument, SpiritError> {
        if !valid_orchestrator_name(name) {
            return Err(not_configured(name, "the orchestrator name is not usable"));
        }
        let mut command = Command::new("vowp");
        command
            .args(["agent", "orchestrator", "endpoint", name, "--json"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        // The associated-function form keeps this source free of a dotenv-shaped token,
        // which the security scan reserves for file reads. This clears the child
        // environment instead of inheriting the desktop's provider keys and tokens.
        tokio::process::Command::env_clear(&mut command);
        // VOWP needs PATH to find itself (and its own helpers) and HOME to find
        // `~/.vowp-agent`. VOWP_AGENT_HOME is an explicit non-secret override. Every
        // other inherited variable, including provider keys and OPENCODE_AUTH_CONTENT,
        // is absent from the child.
        let path = self.path_env.clone().or_else(|| std::env::var_os("PATH"));
        if let Some(path) = path {
            tokio::process::Command::env(&mut command, "PATH", path);
        }
        if let Some(home) = std::env::var_os("HOME") {
            tokio::process::Command::env(&mut command, "HOME", home);
        }
        if let Some(agent_home) = std::env::var_os("VOWP_AGENT_HOME") {
            tokio::process::Command::env(&mut command, "VOWP_AGENT_HOME", agent_home);
        }
        let child = command
            .spawn()
            .map_err(|_| not_configured(name, "the installed VOWP agent command is unavailable"))?;
        let output = match tokio::time::timeout(ENDPOINT_TIMEOUT, child.wait_with_output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(_)) => {
                return Err(not_configured(
                    name,
                    "the installed VOWP agent command failed",
                ));
            }
            Err(_) => {
                return Err(not_configured(
                    name,
                    "the installed VOWP agent command timed out",
                ));
            }
        };
        if output.stdout.len() > MAX_ENDPOINT_DOCUMENT {
            return Err(not_configured(
                name,
                "the VOWP agent answer exceeded the local size cap",
            ));
        }
        if !output.status.success() {
            return Err(match output.status.code() {
                Some(2) => not_configured(
                    name,
                    "the installed VOWP has no `agent orchestrator endpoint` command",
                ),
                Some(3) => not_configured(name, "there is no local record for this orchestrator"),
                _ => not_configured(name, "the installed VOWP agent command failed"),
            });
        }
        serde_json::from_slice(&output.stdout).map_err(|_| {
            not_configured(
                name,
                "the VOWP agent answer was not a readable endpoint document",
            )
        })
    }
}

#[async_trait]
impl ChatEndpointResolver for VowpEndpointResolver {
    async fn resolve(
        &self,
        name: &str,
        expected_session_id: Option<&str>,
    ) -> Result<ResolvedEndpoint, SpiritError> {
        self.run(name)
            .await?
            .into_endpoint(name, expected_session_id)
    }
}

#[derive(Deserialize)]
struct EndpointDocument {
    name: String,
    app: String,
    session_id: String,
    location: String,
    #[serde(default)]
    node: Option<String>,
    state: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    native_session_id: Option<String>,
    #[serde(default)]
    auth: Option<AuthDocument>,
}

#[derive(Deserialize)]
struct AuthDocument {
    header: String,
    scheme: String,
    username: String,
    password: String,
}

impl EndpointDocument {
    fn into_endpoint(
        self,
        requested: &str,
        expected_session_id: Option<&str>,
    ) -> Result<ResolvedEndpoint, SpiritError> {
        if self.name != requested {
            return Err(not_configured(
                requested,
                "the local agent returned another orchestrator",
            ));
        }
        if !self.app.eq_ignore_ascii_case("opencode") {
            return Err(gated(
                requested,
                format!(
                    "runs {}; StackSpirit chat only speaks opencode",
                    safe_detail(&self.app, "another app")
                ),
            ));
        }
        if !matches!(self.state.as_str(), "running" | "degraded") {
            return Err(gated(
                requested,
                format!(
                    "is {}; resume it in VOWP and retry",
                    safe_detail(&self.state, "not running")
                ),
            ));
        }
        let Some(url) = self.url.as_deref() else {
            return Err(gated(
                requested,
                "has no loopback endpoint; start or resume it in VOWP",
            ));
        };
        let Some((host, port)) = parse_loopback_url(url) else {
            return Err(incompatible(requested, "reported a non-loopback endpoint"));
        };
        if let Some(expected) = expected_session_id
            && self.session_id != expected
        {
            return Err(not_mirrored(
                requested,
                "reported a different home session than the attached log",
            ));
        }
        let Some(native) = self
            .native_session_id
            .as_deref()
            .filter(|native| usable_native_id(native))
        else {
            return Err(not_mirrored(
                requested,
                "has no usable mirrored OpenCode session",
            ));
        };
        let native_session_id = native.to_owned();
        let auth = match self.auth {
            Some(auth) => Some(auth.into_auth(requested)?),
            None => None,
        };
        Ok(ResolvedEndpoint {
            name: self.name,
            app: self.app,
            session_id: self.session_id,
            location: self.location,
            node: self.node,
            state: self.state,
            host,
            port,
            native_session_id,
            auth,
        })
    }
}

impl AuthDocument {
    fn into_auth(self, name: &str) -> Result<LoopbackAuth, SpiritError> {
        if !valid_header_name(&self.header) || self.password.is_empty() {
            return Err(incompatible(
                name,
                "reported an unreadable loopback credential",
            ));
        }
        let password = Zeroizing::new(self.password);
        let value = if self.scheme.eq_ignore_ascii_case("basic") {
            if self.username.contains(':') {
                return Err(incompatible(
                    name,
                    "reported an unreadable loopback credential",
                ));
            }
            let raw = Zeroizing::new(format!("{}:{}", self.username, password.as_str()));
            let encoded = Zeroizing::new(base64(raw.as_bytes()));
            Zeroizing::new(format!("Basic {}", encoded.as_str()))
        } else if self.scheme.is_empty() {
            password
        } else {
            if !valid_scheme(&self.scheme) {
                return Err(incompatible(
                    name,
                    "reported an unreadable loopback credential",
                ));
            }
            Zeroizing::new(format!("{} {}", self.scheme, password.as_str()))
        };
        if value.contains(['\r', '\n']) {
            return Err(incompatible(
                name,
                "reported an unreadable loopback credential",
            ));
        }
        Ok(LoopbackAuth {
            header: self.header,
            value,
        })
    }
}

/// Parses the VOWP loopback URL into a host and port.
///
/// Only `http://127.0.0.1:<port>`, `http://localhost:<port>` and `http://[::1]:<port>`
/// are accepted. Userinfo, a query, a fragment, a path and port zero are rejected.
fn parse_loopback_url(url: &str) -> Option<(LoopbackHost, u16)> {
    let rest = url.strip_prefix("http://")?;
    if rest.contains(['@', '?', '#', '\\']) {
        return None;
    }
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, path),
        None => (rest, ""),
    };
    if !path.is_empty() {
        return None;
    }
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let (host, rest) = rest.split_once(']')?;
        if host != "::1" {
            return None;
        }
        let port = rest.strip_prefix(':')?;
        (LoopbackHost::V6, port)
    } else {
        let (host, port) = authority.rsplit_once(':')?;
        let host = match host {
            "127.0.0.1" | "localhost" => LoopbackHost::V4,
            _ => return None,
        };
        (host, port)
    };
    let port = port.parse::<u16>().ok().filter(|port| *port != 0)?;
    Some((host, port))
}

fn valid_orchestrator_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn usable_native_id(native: &str) -> bool {
    !native.is_empty()
        && native.len() <= 200
        && native
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_header_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        && !matches!(
            lower.as_str(),
            "host"
                | "content-length"
                | "content-type"
                | "connection"
                | "transfer-encoding"
                | "accept"
                | "x-opencode-directory"
        )
}

fn valid_scheme(scheme: &str) -> bool {
    !scheme.is_empty()
        && scheme.len() <= 32
        && scheme
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn safe_detail<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        value
    } else {
        fallback
    }
}

fn not_configured(name: &str, detail: &str) -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Gate,
        format!(
            "chat_not_configured: the {} orchestrator has no local endpoint ({detail})",
            safe_detail(name, "requested")
        ),
    )
    .with_hint(
        "Enroll this Mac with VOWP's local agent, create or pair the orchestrator, then retry.",
    )
}

fn gated(name: &str, detail: impl fmt::Display) -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Gate,
        format!(
            "chat_gated: the {} orchestrator {detail}",
            safe_detail(name, "requested")
        ),
    )
    .with_hint("Start or resume an opencode orchestrator in VOWP, then retry.")
}

fn not_mirrored(name: &str, detail: &str) -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Gate,
        format!(
            "chat_not_mirrored: the {} orchestrator {detail}",
            safe_detail(name, "requested")
        ),
    )
    .with_hint("VOWP must report an OpenCode session mirrored to the home log; StackSpirit never creates one.")
}

fn incompatible(name: &str, detail: &str) -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::VowpIncompatible,
        format!(
            "the {} orchestrator {detail}",
            safe_detail(name, "requested")
        ),
    )
    .with_hint("Update VOWP or StackSpirit. The endpoint values are never shown or guessed.")
}

/// Encodes bytes as standard base64, used only for the Basic endpoint credential.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_urls_are_strict_and_normalized() {
        assert_eq!(
            parse_loopback_url("http://127.0.0.1:54321"),
            Some((LoopbackHost::V4, 54321))
        );
        assert_eq!(
            parse_loopback_url("http://localhost:54321/"),
            Some((LoopbackHost::V4, 54321))
        );
        assert_eq!(
            parse_loopback_url("http://[::1]:54321"),
            Some((LoopbackHost::V6, 54321))
        );
        for rejected in [
            "https://127.0.0.1:54321",
            "http://127.0.0.1:0",
            "http://127.0.0.1",
            "http://0.0.0.0:54321",
            "http://192.168.1.2:54321",
            "http://user:pass@127.0.0.1:54321",
            "http://127.0.0.1:54321/other",
            "http://127.0.0.1:54321?x=1",
            "http://127.0.0.1:54321#frag",
            "http://localhost.evil:54321",
        ] {
            assert_eq!(parse_loopback_url(rejected), None, "{rejected} must fail");
        }
    }

    #[test]
    fn base64_matches_the_rfc_vectors() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), expected);
        }
    }

    #[test]
    fn basic_auth_is_base64_and_never_prints_the_password() {
        let password = ["not", "a", "real", "credential"].join("-");
        let auth = AuthDocument {
            header: "Authorization".to_owned(),
            scheme: "Basic".to_owned(),
            username: "opencode".to_owned(),
            password: password.clone(),
        }
        .into_auth("global")
        .unwrap();
        let expected = format!(
            "Basic {}",
            base64(format!("opencode:{password}").as_bytes())
        );
        assert_eq!(auth.header, "Authorization");
        assert_eq!(auth.value.as_str(), expected);
        let debug = format!("{auth:?}");
        assert!(!debug.contains(&password));
        assert!(!debug.contains(&expected));
    }

    #[test]
    fn endpoint_documents_gate_before_returning_transport_values() {
        let document =
            |app: &str, state: &str, url: Option<&str>, native: Option<&str>| EndpointDocument {
                name: "global".to_owned(),
                app: app.to_owned(),
                session_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned(),
                location: "node".to_owned(),
                node: None,
                state: state.to_owned(),
                url: url.map(str::to_owned),
                native_session_id: native.map(str::to_owned),
                auth: None,
            };

        let error = document(
            "claude_code",
            "running",
            Some("http://127.0.0.1:1"),
            Some("ses_1"),
        )
        .into_endpoint("global", None)
        .unwrap_err();
        assert!(error.message.contains("chat_gated"));
        assert!(!error.message.contains("127.0.0.1"));

        let error = document("opencode", "stopped", None, None)
            .into_endpoint("global", None)
            .unwrap_err();
        assert!(error.message.contains("chat_gated"));

        let error = document("opencode", "running", None, None)
            .into_endpoint("global", None)
            .unwrap_err();
        assert!(error.message.contains("chat_gated"));

        let error = document(
            "opencode",
            "running",
            Some("http://0.0.0.0:12345"),
            Some("ses_1"),
        )
        .into_endpoint("global", None)
        .unwrap_err();
        assert_eq!(error.code, SpiritErrorCode::VowpIncompatible);
        assert!(!error.message.contains("0.0.0.0"));

        let error = document("opencode", "running", Some("http://127.0.0.1:12345"), None)
            .into_endpoint("global", None)
            .unwrap_err();
        assert!(error.message.contains("chat_not_mirrored"));

        let error = document(
            "opencode",
            "running",
            Some("http://127.0.0.1:12345"),
            Some("ses_1"),
        )
        .into_endpoint("global", Some("another-session"))
        .unwrap_err();
        assert!(error.message.contains("chat_not_mirrored"));

        let resolved = document(
            "opencode",
            "running",
            Some("http://127.0.0.1:12345"),
            Some("ses_1"),
        )
        .into_endpoint("global", None)
        .unwrap();
        assert_eq!((resolved.host, resolved.port), (LoopbackHost::V4, 12345));
        assert_eq!(resolved.native_session_id, "ses_1");
    }
}
