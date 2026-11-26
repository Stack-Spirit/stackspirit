//! The typed StackSpirit Account and Cloud Relay client.
//!
//! Every call is explicit and opt-in: constructing a client makes no request,
//! and account-free local/SSH mode never constructs one. The bearer session is
//! kept in memory and, when the caller supplies a [`CloudSessionStore`], in the
//! host's private `0600` file. It is never written to a URL, a log, an error or
//! a view model.
//!
//! The HTTP transport is injected through [`CloudTransport`]. The Tauri/Web
//! renderer uses the webview's own `fetch`, the CLI can use its HTTPS adapter,
//! and tests use a loopback transport against a local mock server. This keeps
//! the crate free of a second TLS stack and makes every request observable
//! without a live network call.
//!
//! The client sends no SSH private key, passphrase, password or provider
//! secret. The only key material it ever transmits is the public half of the
//! browser's ephemeral P-256 ECDH key when minting a relay ticket.

use std::fmt;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Map, Value};
use zeroize::{Zeroize, Zeroizing};

use super::error::{CloudError, CloudErrorCode};
use super::session::{CloudSession, CloudSessionStore};
use super::types::{
    BackupPlan, ClientKind, CloudAccount, CloudMachine, DeviceAuthorization, DevicePollState,
    MachinePresence, ManagedPlan, ManagedPlanCatalog, ProvisioningPhase, ProvisioningStatus,
    RelayTicket, RelayTicketMachine, RelayTicketRequest,
};
use crate::time::Timestamp;

/// The production API base. A host may override it with `api_base_url`.
pub const DEFAULT_API_BASE_URL: &str = "https://api.stackspirit.io";

/// The client id this core build presents to the device flow.
pub const DEFAULT_DEVICE_CLIENT_ID: &str = "stackspirit-core";

/// The default request deadline.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// The largest response document the client will parse.
const MAX_RESPONSE_BYTES: usize = 1_000_000;

/// One HTTP method used by the cloud API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudMethod {
    /// `GET`
    Get,
    /// `POST`
    Post,
}

impl CloudMethod {
    /// Returns the canonical HTTP method text.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }
}

/// One request handed to a [`CloudTransport`].
///
/// `Debug` never prints a header value or a body: both can carry a bearer token,
/// a device code or a refresh token. The values are zeroized when the request is
/// dropped.
pub struct CloudRequest {
    /// HTTP method.
    pub method: CloudMethod,
    /// Absolute request URL.
    pub url: String,
    /// Header name/value pairs. The `Authorization` value is a credential.
    pub headers: Vec<(String, String)>,
    /// JSON request body, when the method carries one.
    pub body: Option<String>,
}

impl CloudRequest {
    /// Builds a request with no headers or body.
    #[must_use]
    pub fn new(method: CloudMethod, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            headers: Vec::new(),
            body: None,
        }
    }

    /// Adds one header.
    #[must_use]
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Adds a JSON body.
    #[must_use]
    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }
}

impl fmt::Debug for CloudRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CloudRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field(
                "headers",
                &self
                    .headers
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>(),
            )
            .field("body", &self.body.as_ref().map(|body| body.len()))
            .finish()
    }
}

impl Drop for CloudRequest {
    fn drop(&mut self) {
        for (_, value) in &mut self.headers {
            value.zeroize();
        }
        if let Some(body) = &mut self.body {
            body.zeroize();
        }
    }
}

/// One response returned by a [`CloudTransport`].
///
/// `Debug` never prints the body: a successful device-token response contains a
/// bearer token. The body is zeroized when the response is dropped.
pub struct CloudResponse {
    /// HTTP status.
    pub status: u16,
    /// Response body, decoded as UTF-8 by the transport.
    pub body: String,
}

impl CloudResponse {
    /// Builds a response.
    #[must_use]
    pub fn new(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into(),
        }
    }
}

impl fmt::Debug for CloudResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CloudResponse")
            .field("status", &self.status)
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

impl Drop for CloudResponse {
    fn drop(&mut self) {
        self.body.zeroize();
    }
}

/// The injected HTTP boundary.
///
/// Implementations must send the request as given, return non-2xx responses
/// instead of turning them into transport errors, and never log a header or a
/// body. A transport timeout is returned as [`CloudErrorCode::Timeout`]; an
/// unreachable API as [`CloudErrorCode::Unreachable`].
#[async_trait]
pub trait CloudTransport: Send + Sync {
    /// Sends one request and returns the API's response.
    async fn send(&self, request: CloudRequest) -> Result<CloudResponse, CloudError>;
}

/// The optional StackSpirit Account and Cloud Relay client.
///
/// `T` is the host's HTTPS transport. The client is inert until a method is
/// called; it never starts a background poll or a WebSocket.
pub struct CloudClient<T: CloudTransport> {
    transport: T,
    api_base_url: String,
    device_client_id: String,
    kind: ClientKind,
    timeout: Duration,
    session: Mutex<Option<CloudSession>>,
}

impl<T: CloudTransport> fmt::Debug for CloudClient<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CloudClient")
            .field("api_base_url", &self.api_base_url)
            .field("device_client_id", &self.device_client_id)
            .field("kind", &self.kind)
            .field("has_session", &self.has_session())
            .finish_non_exhaustive()
    }
}

impl<T: CloudTransport> CloudClient<T> {
    /// Builds a client for `api_base_url` with the production defaults.
    pub fn new(transport: T, api_base_url: impl Into<String>) -> Result<Self, CloudError> {
        Ok(Self {
            transport,
            api_base_url: validate_api_base(&api_base_url.into())?,
            device_client_id: DEFAULT_DEVICE_CLIENT_ID.to_owned(),
            kind: ClientKind::Desktop,
            timeout: DEFAULT_REQUEST_TIMEOUT,
            session: Mutex::new(None),
        })
    }

    /// Builds a client for [`DEFAULT_API_BASE_URL`].
    pub fn with_default_base(transport: T) -> Result<Self, CloudError> {
        Self::new(transport, DEFAULT_API_BASE_URL)
    }

    /// Returns the normalized API base.
    #[must_use]
    pub fn api_base_url(&self) -> &str {
        &self.api_base_url
    }

    /// Overrides the client id the device flow presents.
    pub fn set_device_client_id(&mut self, client_id: impl Into<String>) {
        self.device_client_id = client_id.into();
    }

    /// Overrides the client kind carried by ticket requests.
    pub fn set_client_kind(&mut self, kind: ClientKind) {
        self.kind = kind;
    }

    /// Overrides the request deadline.
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    /// Returns true when this client holds an in-memory bearer session.
    #[must_use]
    pub fn has_session(&self) -> bool {
        self.lock_session().is_some()
    }

    /// Loads a session from a private store and makes it current.
    ///
    /// A stored session for another API base is never used; the call returns
    /// `false` and leaves the file untouched.
    pub fn restore_session(&self, store: &dyn CloudSessionStore) -> Result<bool, CloudError> {
        let Some(session) = store.load()? else {
            return Ok(false);
        };
        if session.api_url != self.api_base_url {
            return Ok(false);
        }
        *self.lock_session() = Some(session);
        Ok(true)
    }

    /// Drops the in-memory session without touching the private store.
    pub fn clear_session(&self) {
        *self.lock_session() = None;
    }

    /// Signs out: clears memory first, then removes the private session file.
    pub fn sign_out(&self, store: &dyn CloudSessionStore) -> Result<bool, CloudError> {
        *self.lock_session() = None;
        store.remove()
    }

    /// Starts the device authorization grant.
    pub async fn start_device_sign_in(&self) -> Result<DeviceAuthorization, CloudError> {
        let body = serde_json::json!({
            "client_id": self.device_client_id,
            "kind": self.kind.as_str(),
        });
        let value = self
            .request_json(CloudMethod::Post, "/v1/auth/device", Some(body), false)
            .await?;
        parse_device_authorization(&value)
    }

    /// Polls the device grant once.
    ///
    /// On `Authorized`, the issued session is written to `store` before it is
    /// made current. A failed write leaves the client signed out.
    pub async fn poll_device_sign_in(
        &self,
        store: &dyn CloudSessionStore,
        device_code: &str,
    ) -> Result<DevicePollState, CloudError> {
        if device_code.trim().is_empty() {
            return Err(CloudError::new(
                CloudErrorCode::InvalidInput,
                "device_code: must not be empty",
            ));
        }
        let body = serde_json::json!({ "device_code": device_code });
        let (status, value) = self
            .request_raw(
                CloudMethod::Post,
                "/v1/auth/device/token",
                Some(body),
                false,
            )
            .await?;
        if !(200..300).contains(&status) {
            let code = device_error_code(&value);
            if matches!(status, 400 | 428) {
                match code.as_deref() {
                    Some("authorization_pending") | None => return Ok(DevicePollState::Pending),
                    Some("slow_down") => return Ok(DevicePollState::SlowDown),
                    Some("expired_token") => {
                        return Err(CloudError::new(
                            CloudErrorCode::Auth,
                            "The StackSpirit sign-in code expired. Start again.",
                        )
                        .with_status(status));
                    }
                    Some("access_denied") => {
                        return Err(CloudError::new(
                            CloudErrorCode::Auth,
                            "The StackSpirit sign-in was declined.",
                        )
                        .with_status(status));
                    }
                    Some(_) => {}
                }
            }
            return Err(status_error(status, &value));
        }
        let access_token = require_string(&value, "access_token")?;
        if access_token.chars().any(char::is_whitespace) {
            return Err(CloudError::parse(
                "device token",
                "the access token contains whitespace",
            ));
        }
        let refresh_token = optional_string(&value, "refresh_token")?;
        let expires_at =
            optional_u64(&value, "expires_in")?.map(|seconds| Timestamp::now().plus_secs(seconds));
        let mut session = CloudSession::new(self.api_base_url.clone(), access_token);
        session.refresh_token = refresh_token.map(Zeroizing::new);
        session.expires_at = expires_at;
        store.save(&session)?;
        *self.lock_session() = Some(session);
        Ok(DevicePollState::Authorized)
    }

    /// Rotates the current access token with the stored refresh token.
    pub async fn refresh_session(&self, store: &dyn CloudSessionStore) -> Result<(), CloudError> {
        let refresh_token = {
            let session = self.lock_session();
            session
                .as_ref()
                .and_then(|session| session.refresh_token.as_ref())
                .map(|token| Zeroizing::new(token.as_str().to_owned()))
        };
        let Some(refresh_token) = refresh_token else {
            return Err(
                CloudError::new(CloudErrorCode::Auth, "Sign in to StackSpirit again.")
                    .with_hint("The stored session has no refresh token."),
            );
        };
        let body = serde_json::json!({ "refresh_token": refresh_token.as_str() });
        let value = self
            .request_json(CloudMethod::Post, "/v1/auth/refresh", Some(body), false)
            .await?;
        let access_token = Zeroizing::new(require_string(&value, "access_token")?);
        if access_token.chars().any(char::is_whitespace) {
            return Err(CloudError::parse(
                "refresh response",
                "the access token contains whitespace",
            ));
        }
        let rotated = optional_string(&value, "refresh_token")?.map(Zeroizing::new);
        let expires_at =
            optional_u64(&value, "expires_in")?.map(|seconds| Timestamp::now().plus_secs(seconds));
        let mut session = self.lock_session();
        let Some(session) = session.as_mut() else {
            return Err(CloudError::new(
                CloudErrorCode::Auth,
                "Sign in to StackSpirit again.",
            ));
        };
        session.access_token = access_token;
        if rotated.is_some() {
            session.refresh_token = rotated;
        }
        if expires_at.is_some() {
            session.expires_at = expires_at;
        }
        store.save(session)
    }

    /// Reads the signed-in account profile.
    pub async fn account(&self) -> Result<CloudAccount, CloudError> {
        let value = self
            .request_json(CloudMethod::Get, "/v1/me", None, true)
            .await?;
        parse_account(&value)
    }

    /// Lists the account's enrolled Always-On Homes.
    pub async fn list_machines(&self) -> Result<Vec<CloudMachine>, CloudError> {
        let value = self
            .request_json(CloudMethod::Get, "/v1/machines", None, true)
            .await?;
        parse_machines(&value)
    }

    /// Reads the public Managed Home plan catalogue. No bearer is attached.
    pub async fn get_plans(&self) -> Result<ManagedPlanCatalog, CloudError> {
        let value = self
            .request_json(CloudMethod::Get, "/v1/plans", None, false)
            .await?;
        parse_catalog(&value)
    }

    /// Reads live provisioning status for one Managed Home.
    pub async fn get_provisioning(
        &self,
        machine_id: &str,
    ) -> Result<ProvisioningStatus, CloudError> {
        let id = validate_machine_id(machine_id)?;
        let value = self
            .request_json(
                CloudMethod::Get,
                &format!("/v1/machines/{id}/provisioning"),
                None,
                true,
            )
            .await?;
        parse_provisioning(&value)
    }

    /// Mints one short-lived relay ticket.
    ///
    /// The body contains a public P-256 JWK, a public nonce and scopes only.
    /// The machine-scoped route is tried first; the design's account-wide
    /// `POST /v1/relay-tickets` is used only when that route is absent (404),
    /// never as a silent retry after an auth or scope failure.
    pub async fn mint_relay_ticket(
        &self,
        machine_id: &str,
        request: RelayTicketRequest,
    ) -> Result<RelayTicket, CloudError> {
        let id = validate_machine_id(machine_id)?;
        validate_ticket_request(&request)?;
        let mut body = serde_json::to_value(&request)
            .map_err(|error| CloudError::parse("relay ticket request", error))?;
        if let Some(object) = body.as_object_mut() {
            object.insert(
                "ttl_secs".to_owned(),
                Value::from(request.ttl_secs.clamp(30, 300)),
            );
        }
        match self
            .request_json(
                CloudMethod::Post,
                &format!("/v1/machines/{id}/ticket"),
                Some(body.clone()),
                true,
            )
            .await
        {
            Ok(value) => parse_relay_ticket(&value),
            Err(error) if error.status == Some(404) => {
                if let Some(object) = body.as_object_mut() {
                    object.insert("machine_id".to_owned(), Value::from(id));
                }
                let value = self
                    .request_json(CloudMethod::Post, "/v1/relay-tickets", Some(body), true)
                    .await?;
                parse_relay_ticket(&value)
            }
            Err(error) => Err(error),
        }
    }

    fn lock_session(&self) -> MutexGuard<'_, Option<CloudSession>> {
        match self.session.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn bearer_header(&self) -> Result<String, CloudError> {
        let session = self.lock_session();
        let Some(session) = session.as_ref() else {
            return Err(
                CloudError::new(CloudErrorCode::Auth, "Sign in to StackSpirit first.")
                    .with_hint("Local server mode needs no account."),
            );
        };
        Ok(format!("Bearer {}", session.access_token.as_str()))
    }

    async fn request_json(
        &self,
        method: CloudMethod,
        path: &str,
        body: Option<Value>,
        authenticated: bool,
    ) -> Result<Value, CloudError> {
        let (status, value) = self.request_raw(method, path, body, authenticated).await?;
        if !(200..300).contains(&status) {
            return Err(status_error(status, &value));
        }
        Ok(value)
    }

    async fn request_raw(
        &self,
        method: CloudMethod,
        path: &str,
        body: Option<Value>,
        authenticated: bool,
    ) -> Result<(u16, Value), CloudError> {
        let mut headers = vec![("Accept".to_owned(), "application/json".to_owned())];
        let body = match body {
            Some(body) => {
                headers.push(("Content-Type".to_owned(), "application/json".to_owned()));
                Some(
                    serde_json::to_string(&body)
                        .map_err(|error| CloudError::parse("cloud request", error))?,
                )
            }
            None => None,
        };
        if authenticated {
            headers.push(("Authorization".to_owned(), self.bearer_header()?));
        }
        let mut request = CloudRequest::new(method, format!("{}{path}", self.api_base_url));
        request.headers = headers;
        request.body = body;
        let response = match tokio::time::timeout(self.timeout, self.transport.send(request)).await
        {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => return Err(error),
            Err(_) => {
                return Err(CloudError::new(
                    CloudErrorCode::Timeout,
                    "The StackSpirit API did not answer in time.",
                )
                .with_hint("Retry when the network is reachable.")
                .retryable());
            }
        };
        let status = response.status;
        let text = response.body.as_str();
        if text.len() > MAX_RESPONSE_BYTES {
            return Err(CloudError::new(
                CloudErrorCode::Parse,
                "The StackSpirit API returned an oversized document.",
            ));
        }
        let value = if text.trim().is_empty() {
            Value::Null
        } else {
            match serde_json::from_str(text) {
                Ok(value) => value,
                Err(_) if status >= 400 => Value::Null,
                Err(error) => {
                    return Err(CloudError::parse("the StackSpirit API response", error));
                }
            }
        };
        Ok((status, value))
    }
}

fn validate_api_base(raw: &str) -> Result<String, CloudError> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(CloudError::new(
            CloudErrorCode::Unavailable,
            "The StackSpirit API address is empty.",
        )
        .with_hint("Use https://api.stackspirit.io or a loopback test URL."));
    }
    if trimmed
        .chars()
        .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(CloudError::new(
            CloudErrorCode::Unavailable,
            "The StackSpirit API address contains whitespace.",
        ));
    }
    let (scheme, rest) = if let Some(rest) = trimmed.strip_prefix("https://") {
        ("https", rest)
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        ("http", rest)
    } else {
        return Err(CloudError::new(
            CloudErrorCode::Unavailable,
            "The StackSpirit API must use https.",
        )
        .with_hint("Plain http is accepted only for a loopback test server."));
    };
    if rest.is_empty() || rest.contains('@') || rest.contains('?') || rest.contains('#') {
        return Err(CloudError::new(
            CloudErrorCode::Unavailable,
            "The StackSpirit API address is not a plain origin.",
        ));
    }
    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, ""),
    };
    if authority.is_empty() || !(path.is_empty() || path == "/") {
        return Err(CloudError::new(
            CloudErrorCode::Unavailable,
            "The StackSpirit API address must be an origin without a path.",
        ));
    }
    if scheme == "http" && !is_loopback_authority(authority) {
        return Err(CloudError::new(
            CloudErrorCode::Unavailable,
            "The StackSpirit API must use https.",
        )
        .with_hint("Plain http is accepted only for 127.0.0.1, localhost or [::1]."));
    }
    Ok(trimmed.to_owned())
}

fn is_loopback_authority(authority: &str) -> bool {
    let host = if let Some(rest) = authority.strip_prefix('[') {
        match rest.split_once(']') {
            Some((host, port)) => {
                if !(port.is_empty() || port.starts_with(':')) {
                    return false;
                }
                host
            }
            None => return false,
        }
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

fn validate_machine_id(machine_id: &str) -> Result<&str, CloudError> {
    let valid = !machine_id.is_empty()
        && machine_id.len() <= 128
        && machine_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'));
    if valid {
        Ok(machine_id)
    } else {
        Err(CloudError::new(
            CloudErrorCode::InvalidInput,
            "machine_id: expected 1..=128 letters, digits, `_` or `-`",
        ))
    }
}

fn validate_ticket_request(request: &RelayTicketRequest) -> Result<(), CloudError> {
    if request.scopes.is_empty() {
        return Err(CloudError::new(
            CloudErrorCode::InvalidInput,
            "A relay ticket needs at least one scope.",
        ));
    }
    if request.client_key.x.trim().is_empty() || request.client_key.y.trim().is_empty() {
        return Err(CloudError::new(
            CloudErrorCode::InvalidInput,
            "A relay ticket needs a P-256 public key.",
        ));
    }
    if request.client_key.x.chars().any(char::is_whitespace)
        || request.client_key.y.chars().any(char::is_whitespace)
    {
        return Err(CloudError::new(
            CloudErrorCode::InvalidInput,
            "The relay public key contains whitespace.",
        ));
    }
    if request.client_nonce.trim().is_empty()
        || request.client_nonce.chars().any(char::is_whitespace)
    {
        return Err(CloudError::new(
            CloudErrorCode::InvalidInput,
            "client_nonce: must not be empty or contain whitespace",
        ));
    }
    Ok(())
}

fn parse_device_authorization(value: &Value) -> Result<DeviceAuthorization, CloudError> {
    let object = require_object(value, "device authorization")?;
    let expires_in = require_u64(object, "expires_in", "device authorization")?;
    let interval = require_u64(object, "interval", "device authorization")?.max(1);
    let device_code = require_string(value, "device_code")?;
    let user_code = require_string(value, "user_code")?;
    let verification_uri = require_string(value, "verification_uri")?;
    let verification_uri_complete = optional_string(value, "verification_uri_complete")?;
    if !safe_web_url(&verification_uri)
        || verification_uri_complete
            .as_deref()
            .is_some_and(|url| !safe_web_url(url))
    {
        return Err(CloudError::parse(
            "device authorization",
            "the verification URL is not an https or loopback URL",
        ));
    }
    if device_code.chars().any(char::is_whitespace)
        || device_code.len() > 1024
        || user_code.is_empty()
        || user_code.len() > 256
    {
        return Err(CloudError::parse(
            "device authorization",
            "the device or user code is unreadable",
        ));
    }
    Ok(DeviceAuthorization {
        device_code: Zeroizing::new(device_code),
        user_code,
        verification_uri,
        verification_uri_complete,
        expires_in,
        interval,
        expires_at: Timestamp::now().plus_secs(expires_in),
    })
}

fn parse_account(value: &Value) -> Result<CloudAccount, CloudError> {
    let body = value
        .as_object()
        .and_then(|object| object.get("account"))
        .unwrap_or(value);
    let object = require_object(body, "account")?;
    Ok(CloudAccount {
        id: require_field_string(object, "id", "account")?.to_owned(),
        email: require_field_string(object, "email", "account")?.to_owned(),
        name: optional_field_string(object, "name", "account")?,
    })
}

fn parse_machines(value: &Value) -> Result<Vec<CloudMachine>, CloudError> {
    let items = match value {
        Value::Array(items) => items,
        Value::Object(object) => object
            .get("machines")
            .and_then(Value::as_array)
            .ok_or_else(|| CloudError::parse("machine list", "missing machines array"))?,
        _ => {
            return Err(CloudError::parse(
                "machine list",
                "expected an array or an object with machines",
            ));
        }
    };
    items
        .iter()
        .enumerate()
        .map(|(index, item)| parse_machine(item, index))
        .collect()
}

fn parse_machine(value: &Value, index: usize) -> Result<CloudMachine, CloudError> {
    let object = value.as_object().ok_or_else(|| {
        CloudError::parse("machine list", format!("machine {index} is not an object"))
    })?;
    let status = require_field_string(object, "status", "machine")?;
    let status = match status {
        "online" => MachinePresence::Online,
        "offline" => MachinePresence::Offline,
        "degraded" => MachinePresence::Degraded,
        "revoked" => MachinePresence::Revoked,
        _ => {
            return Err(CloudError::parse(
                "machine list",
                format!("machine {index} has an unknown presence"),
            ));
        }
    };
    let provisioning = match object.get("provisioning") {
        None | Some(Value::Null) => None,
        Some(value) => Some(parse_provisioning(value)?),
    };
    Ok(CloudMachine {
        id: require_field_string(object, "id", "machine")?.to_owned(),
        name: require_field_string(object, "name", "machine")?.to_owned(),
        status,
        project: optional_field_string(object, "project", "machine")?,
        last_seen_at: optional_timestamp(object, "last_seen_at", "machine")?,
        provisioning,
    })
}

fn parse_provisioning(value: &Value) -> Result<ProvisioningStatus, CloudError> {
    let object = require_object(value, "provisioning status")?;
    let phase = match require_field_string(object, "phase", "provisioning status")? {
        "queued" => ProvisioningPhase::Queued,
        "provisioning" => ProvisioningPhase::Provisioning,
        "online" => ProvisioningPhase::Online,
        "failed" => ProvisioningPhase::Failed,
        _ => {
            return Err(CloudError::parse(
                "provisioning status",
                "unknown provisioning phase",
            ));
        }
    };
    let progress_pct = optional_f64(object, "progress_pct", "provisioning status")?
        .map(|value| value.round().clamp(0.0, 100.0) as u8);
    let detail = optional_field_string(object, "detail", "provisioning status")?
        .map(|detail| safe_text(&detail));
    Ok(ProvisioningStatus {
        machine_id: require_field_string(object, "machine_id", "provisioning status")?.to_owned(),
        phase,
        step: optional_field_string(object, "step", "provisioning status")?,
        progress_pct,
        detail,
        updated_at: require_timestamp(object, "updated_at", "provisioning status")?,
        home_url: optional_field_string(object, "home_url", "provisioning status")?,
    })
}

fn parse_catalog(value: &Value) -> Result<ManagedPlanCatalog, CloudError> {
    let items = match value {
        Value::Array(items) => items,
        Value::Object(object) => object
            .get("plans")
            .and_then(Value::as_array)
            .ok_or_else(|| CloudError::parse("plan catalogue", "missing plans array"))?,
        _ => {
            return Err(CloudError::parse(
                "plan catalogue",
                "expected an array or an object with plans",
            ));
        }
    };
    let mut plans = items
        .iter()
        .enumerate()
        .map(|(index, item)| parse_plan(item, index))
        .collect::<Result<Vec<_>, _>>()?;
    plans.sort_by(|left, right| {
        left.monthly_eur
            .total_cmp(&right.monthly_eur)
            .then_with(|| left.ram_gb.cmp(&right.ram_gb))
    });
    let backups = match value.as_object().and_then(|object| object.get("backups")) {
        None | Some(Value::Null) => BackupPlan {
            monthly_eur: None,
            recommended: true,
            provider: Some("cloudflare-r2".to_owned()),
        },
        Some(value) => parse_backups(value)?,
    };
    Ok(ManagedPlanCatalog { plans, backups })
}

fn parse_plan(value: &Value, index: usize) -> Result<ManagedPlan, CloudError> {
    let object = value.as_object().ok_or_else(|| {
        CloudError::parse("plan catalogue", format!("plan {index} is not an object"))
    })?;
    let currency =
        optional_field_string(object, "currency", "plan")?.unwrap_or_else(|| "EUR".to_owned());
    if currency != "EUR" {
        return Err(CloudError::parse(
            "plan catalogue",
            "only euro plans are supported",
        ));
    }
    let vcpu = u32::try_from(require_u64(object, "vcpu", "plan")?).map_err(|_| {
        CloudError::parse(
            "plan catalogue",
            format!("plan {index} has an invalid vcpu"),
        )
    })?;
    let ram_gb = u32::try_from(require_u64(object, "ram_gb", "plan")?).map_err(|_| {
        CloudError::parse(
            "plan catalogue",
            format!("plan {index} has an invalid ram_gb"),
        )
    })?;
    let disk_gb = u32::try_from(require_u64(object, "disk_gb", "plan")?).map_err(|_| {
        CloudError::parse(
            "plan catalogue",
            format!("plan {index} has an invalid disk_gb"),
        )
    })?;
    let monthly_eur = require_f64(object, "monthly_eur", "plan")?.max(0.0);
    let setup_fee_eur = optional_f64(object, "setup_fee_eur", "plan")?.map(|fee| fee.max(0.0));
    Ok(ManagedPlan {
        id: require_field_string(object, "id", "plan")?.to_owned(),
        name: require_field_string(object, "name", "plan")?.to_owned(),
        vcpu,
        ram_gb,
        disk_gb,
        disk_kind: optional_field_string(object, "disk_kind", "plan")?,
        monthly_eur,
        setup_fee_eur,
        currency,
        provider_spec: optional_field_string(object, "provider_spec", "plan")?,
    })
}

fn parse_backups(value: &Value) -> Result<BackupPlan, CloudError> {
    let object = require_object(value, "backup add-on")?;
    let recommended = match object.get("recommended") {
        None => true,
        Some(value) => value
            .as_bool()
            .ok_or_else(|| CloudError::parse("backup add-on", "recommended is not a boolean"))?,
    };
    Ok(BackupPlan {
        monthly_eur: optional_f64(object, "monthly_eur", "backup add-on")?,
        recommended,
        provider: optional_field_string(object, "provider", "backup add-on")?,
    })
}

fn parse_relay_ticket(value: &Value) -> Result<RelayTicket, CloudError> {
    let object = require_object(value, "relay ticket")?;
    let ticket = require_field_string(object, "ticket", "relay ticket")?.to_owned();
    if ticket.is_empty() || ticket.chars().any(char::is_whitespace) {
        return Err(CloudError::parse(
            "relay ticket",
            "the ticket is empty or contains whitespace",
        ));
    }
    let relay_url = require_field_string(object, "relay_url", "relay ticket")?.to_owned();
    if !safe_relay_url(&relay_url) {
        return Err(CloudError::parse(
            "relay ticket",
            "the relay URL is not wss or a loopback ws URL",
        ));
    }
    let machine = object
        .get("machine")
        .ok_or_else(|| CloudError::parse("relay ticket", "missing machine"))?;
    let machine = require_object(machine, "relay ticket machine")?;
    Ok(RelayTicket {
        ticket: Zeroizing::new(ticket),
        expires_at: require_timestamp(object, "expires_at", "relay ticket")?,
        relay_url,
        machine: RelayTicketMachine {
            id: require_field_string(machine, "id", "relay ticket machine")?.to_owned(),
            name: require_field_string(machine, "name", "relay ticket machine")?.to_owned(),
            key_id: require_field_string(machine, "key_id", "relay ticket machine")?.to_owned(),
            public_key: require_field_string(machine, "public_key", "relay ticket machine")?
                .to_owned(),
            project: optional_field_string(machine, "project", "relay ticket machine")?,
        },
    })
}

fn require_object<'a>(value: &'a Value, what: &str) -> Result<&'a Map<String, Value>, CloudError> {
    value
        .as_object()
        .ok_or_else(|| CloudError::parse(what, "expected a JSON object"))
}

fn require_string(value: &Value, field: &str) -> Result<String, CloudError> {
    let object = value
        .as_object()
        .ok_or_else(|| CloudError::parse("response", "expected a JSON object"))?;
    require_field_string(object, field, "response").map(str::to_owned)
}

fn require_field_string<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    what: &str,
) -> Result<&'a str, CloudError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CloudError::parse(what, format!("missing {field}")))
}

fn optional_field_string(
    object: &Map<String, Value>,
    field: &str,
    what: &str,
) -> Result<Option<String>, CloudError> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(CloudError::parse(what, format!("{field} is not a string"))),
    }
}

fn optional_string(value: &Value, field: &str) -> Result<Option<String>, CloudError> {
    let Some(object) = value.as_object() else {
        return Err(CloudError::parse("response", "expected a JSON object"));
    };
    optional_field_string(object, field, "response")
}

fn require_u64(object: &Map<String, Value>, field: &str, what: &str) -> Result<u64, CloudError> {
    object
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| CloudError::parse(what, format!("missing or invalid {field}")))
}

fn optional_u64(value: &Value, field: &str) -> Result<Option<u64>, CloudError> {
    let Some(object) = value.as_object() else {
        return Err(CloudError::parse("response", "expected a JSON object"));
    };
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| CloudError::parse("response", format!("{field} is not an integer"))),
    }
}

fn require_f64(object: &Map<String, Value>, field: &str, what: &str) -> Result<f64, CloudError> {
    object
        .get(field)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .ok_or_else(|| CloudError::parse(what, format!("missing or invalid {field}")))
}

fn optional_f64(
    object: &Map<String, Value>,
    field: &str,
    what: &str,
) -> Result<Option<f64>, CloudError> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_f64()
            .filter(|value| value.is_finite())
            .map(Some)
            .ok_or_else(|| CloudError::parse(what, format!("{field} is not a number"))),
    }
}

fn require_timestamp(
    object: &Map<String, Value>,
    field: &str,
    what: &str,
) -> Result<Timestamp, CloudError> {
    optional_timestamp(object, field, what)?
        .ok_or_else(|| CloudError::parse(what, format!("missing {field}")))
}

fn optional_timestamp(
    object: &Map<String, Value>,
    field: &str,
    what: &str,
) -> Result<Option<Timestamp>, CloudError> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Timestamp::parse(text)
            .map(Some)
            .map_err(|_| CloudError::parse(what, format!("{field} is not an RFC 3339 timestamp"))),
        Some(_) => Err(CloudError::parse(what, format!("{field} is not a string"))),
    }
}

fn device_error_code(value: &Value) -> Option<String> {
    value
        .as_object()
        .and_then(|object| object.get("error"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn status_error(status: u16, value: &Value) -> CloudError {
    let message = api_message(value);
    let fallback = match status {
        400 => "The StackSpirit API rejected the request.",
        401 => "Sign in to StackSpirit again.",
        403 => "This account cannot use that machine.",
        404 => "The StackSpirit API could not find that item.",
        409 => "The machine changed; refresh and retry.",
        429 => "The StackSpirit API is rate limiting this account.",
        _ if status >= 500 => "The StackSpirit API failed on its side.",
        _ => "The StackSpirit API answered with an unexpected status.",
    };
    let message = if message.is_empty() {
        fallback.to_owned()
    } else {
        message
    };
    match status {
        400 => CloudError::new(CloudErrorCode::InvalidInput, message).with_status(status),
        401 => CloudError::new(CloudErrorCode::Auth, message)
            .with_hint("The account session expired or was revoked.")
            .with_status(status),
        403 => CloudError::new(CloudErrorCode::Forbidden, message).with_status(status),
        404 => CloudError::new(CloudErrorCode::NotFound, message).with_status(status),
        409 => CloudError::new(CloudErrorCode::Conflict, message).with_status(status),
        429 => CloudError::new(CloudErrorCode::RateLimited, message)
            .with_hint("Wait a moment and retry.")
            .retryable()
            .with_status(status),
        _ if status >= 500 => CloudError::new(CloudErrorCode::Server, message)
            .with_hint("Retry shortly.")
            .retryable()
            .with_status(status),
        _ => CloudError::new(CloudErrorCode::Http, message).with_status(status),
    }
}

fn api_message(value: &Value) -> String {
    let Some(object) = value.as_object() else {
        return String::new();
    };
    if let Some(message) = object.get("message").and_then(Value::as_str) {
        return safe_text(message);
    }
    if let Some(message) = object
        .get("error")
        .and_then(Value::as_object)
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
    {
        return safe_text(message);
    }
    String::new()
}

fn safe_text(value: &str) -> String {
    let scrubbed = scrub_paths(value);
    let mut clean = String::with_capacity(scrubbed.len());
    let mut previous_space = false;
    for character in scrubbed.chars() {
        let current = if character.is_control() || character.is_whitespace() {
            if previous_space {
                continue;
            }
            previous_space = true;
            ' '
        } else {
            previous_space = false;
            character
        };
        clean.push(current);
    }
    let trimmed = clean.trim();
    if trimmed.chars().count() > 300 {
        let mut truncated: String = trimmed.chars().take(300).collect();
        truncated.push('…');
        truncated
    } else {
        trimmed.to_owned()
    }
}

fn scrub_paths(value: &str) -> String {
    const PREFIXES: [&str; 8] = [
        "/home/", "/Users/", "/root/", "/tmp/", "/var/", "/etc/", "/opt/", "/srv/",
    ];
    let mut remaining = value;
    let mut clean = String::with_capacity(value.len());
    loop {
        let earliest = PREFIXES
            .iter()
            .filter_map(|prefix| remaining.find(prefix))
            .min();
        let Some(index) = earliest else {
            clean.push_str(remaining);
            return clean;
        };
        clean.push_str(&remaining[..index]);
        clean.push_str("[path]");
        let after = &remaining[index..];
        let end = after
            .find(|character: char| {
                character.is_whitespace()
                    || matches!(character, '"' | '\'' | '`' | ')' | ']' | '}' | ',' | ';')
            })
            .unwrap_or(after.len());
        remaining = &after[end..];
    }
}

fn safe_web_url(value: &str) -> bool {
    if let Some(rest) = value.strip_prefix("https://") {
        return rest
            .chars()
            .any(|character| character.is_ascii_alphanumeric());
    }
    is_loopback_url(value)
}

fn safe_relay_url(value: &str) -> bool {
    if let Some(rest) = value.strip_prefix("wss://") {
        return rest
            .chars()
            .any(|character| character.is_ascii_alphanumeric());
    }
    is_loopback_url_with_scheme(value, "ws")
}

fn is_loopback_url(value: &str) -> bool {
    is_loopback_url_with_scheme(value, "http")
}

fn is_loopback_url_with_scheme(value: &str, scheme: &str) -> bool {
    let Some(rest) = value.strip_prefix(scheme) else {
        return false;
    };
    let Some(rest) = rest.strip_prefix("://") else {
        return false;
    };
    for host in ["127.0.0.1", "localhost", "[::1]"] {
        if let Some(tail) = rest.strip_prefix(host) {
            return tail.is_empty() || tail.starts_with(':') || tail.starts_with('/');
        }
    }
    false
}
