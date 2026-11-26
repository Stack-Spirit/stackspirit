//! The StackSpirit cloud client shared by `login`, `machines`, `ssh` and `relay proxy`.
//!
//! Spec: `docs/design/relay-and-packaging.md` §6.4 and §6.7. The API is a JSON HTTP service;
//! the relay is a WebSocket service whose payloads are end-to-end encrypted. This module owns
//! the session file, the HTTP calls and the shared machine records; [`relay`] owns the
//! WebSocket client and the stdio bridge.
//!
//! Nothing here prints a token, a device code, a private key or a decrypted payload.

pub mod api;
pub mod credentials;
pub mod relay;

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use vowp_protocol::Timestamp;

use crate::CliError;

pub use api::{Api, DEFAULT_API_URL};
pub use credentials::Session;

/// Default relay client endpoint when a ticket response does not name one.
pub const DEFAULT_RELAY_URL: &str = "wss://relay.stackspirit.io/v1/client";

/// A signed-in API client bound to one home directory.
pub struct Client {
    api: Api,
    session: Session,
    home: PathBuf,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("api", &self.api)
            .field("session", &self.session)
            .finish()
    }
}

impl Client {
    /// Loads the stored session for `home` and builds its API client.
    pub fn load(home: &Path) -> Result<Self, CliError> {
        let session = Session::load(home)?.ok_or_else(not_signed_in)?;
        let api = Api::new(&session.api_url)?;
        Ok(Self {
            api,
            session,
            home: home.to_path_buf(),
        })
    }

    /// Returns the normalised API base.
    pub fn api_url(&self) -> &str {
        self.api.base()
    }

    /// Returns the stored session (tokens included; never render it).
    pub fn session(&self) -> &Session {
        &self.session
    }

    /// Sends an authenticated `GET` and returns the decoded JSON body.
    pub fn get(&mut self, path: &str) -> Result<Value, CliError> {
        self.request(Method::Get, path, &Value::Null)
    }

    /// Sends an authenticated `POST` and returns the decoded JSON body.
    pub fn post(&mut self, path: &str, body: &Value) -> Result<Value, CliError> {
        self.request(Method::Post, path, body)
    }

    fn request(&mut self, method: Method, path: &str, body: &Value) -> Result<Value, CliError> {
        let mut response = self.send(method, path, body)?;
        if response.status == 401 {
            self.refresh()?;
            response = self.send(method, path, body)?;
        }
        finish(response)
    }

    fn send(&self, method: Method, path: &str, body: &Value) -> Result<api::ApiResponse, CliError> {
        match method {
            Method::Get => self.api.get(path, Some(&self.session.access_token)),
            Method::Post => self.api.post(path, body, Some(&self.session.access_token)),
        }
    }

    /// Rotates the access token once with the stored refresh token and rewrites the session.
    fn refresh(&mut self) -> Result<(), CliError> {
        let refresh = self.session.refresh_token.clone().ok_or_else(session_expired)?;
        let response = self
            .api
            .post("/v1/auth/refresh", &json!({ "refresh_token": refresh }), None)?;
        if !response.success() {
            return Err(CliError::Auth(format!(
                "the cloud session expired ({}); run `{} login`",
                response.describe(),
                crate::cli::binary_brand()
            )));
        }
        let tokens = tokens_from_body(&response.body)?;
        self.session.access_token = tokens.access_token;
        if let Some(refresh) = tokens.refresh_token {
            self.session.refresh_token = Some(refresh);
        }
        if let Some(expires_at) = tokens.expires_at {
            self.session.expires_at = Some(expires_at);
        }
        if let Some(account_id) = tokens.account_id {
            self.session.account_id = Some(account_id);
        }
        if let Some(email) = tokens.email {
            self.session.email = Some(email);
        }
        self.session.save(&self.home)?;
        Ok(())
    }
}

/// The HTTP methods this client uses.
#[derive(Debug, Clone, Copy)]
enum Method {
    Get,
    Post,
}

/// Maps one non-2xx API response onto the CLI's error classes.
fn finish(response: api::ApiResponse) -> Result<Value, CliError> {
    if response.success() {
        return Ok(response.body);
    }
    let described = response.describe();
    match response.status {
        401 | 403 => Err(CliError::Auth(format!(
            "the cloud API refused the session: {described}"
        ))),
        404 => Err(CliError::NotFound(format!("the cloud API found nothing: {described}"))),
        409 => Err(CliError::Conflict(format!(
            "the cloud API reported a conflict: {described}"
        ))),
        429 => Err(CliError::Limited(format!(
            "the cloud API is rate limiting: {described}"
        ))),
        _ => Err(CliError::Remote(format!("the cloud API failed: {described}"))),
    }
}

fn not_signed_in() -> CliError {
    CliError::Auth(format!(
        "not signed in to the StackSpirit cloud; run `{} login`",
        crate::cli::binary_brand()
    ))
}

fn session_expired() -> CliError {
    CliError::Auth(format!(
        "the cloud session expired; run `{} login`",
        crate::cli::binary_brand()
    ))
}

/// One set of tokens returned by device login or refresh.
#[derive(Clone)]
pub struct Tokens {
    /// Current access token.
    pub access_token: String,
    /// Rotated refresh token, when the API returned one.
    pub refresh_token: Option<String>,
    /// RFC 3339 access-token expiry, when known.
    pub expires_at: Option<String>,
    /// Account id, when the API included it.
    pub account_id: Option<String>,
    /// Account email, when the API included it.
    pub email: Option<String>,
}

impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokens")
            .field("access_token", &"<redacted>")
            .field("refresh_token", &self.refresh_token.as_ref().map(|_| "<redacted>"))
            .field("expires_at", &self.expires_at)
            .field("account_id", &self.account_id)
            .field("email", &self.email)
            .finish()
    }
}

/// Parses a token response from `/v1/auth/device/token` or `/v1/auth/refresh`.
pub fn tokens_from_body(body: &Value) -> Result<Tokens, CliError> {
    let access_token = body
        .get("access_token")
        .and_then(Value::as_str)
        .or_else(|| body.get("token").and_then(Value::as_str))
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| CliError::Auth("the cloud API returned no access token".to_owned()))?;
    let expires_at = body
        .get("expires_at")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            body.get("expires_in")
                .and_then(Value::as_u64)
                .map(|secs| Timestamp::from_unix_secs(Timestamp::now().unix_secs() + secs as i64).as_rfc3339())
        });
    let account = body.get("account").unwrap_or(body);
    Ok(Tokens {
        access_token,
        refresh_token: body.get("refresh_token").and_then(Value::as_str).map(str::to_owned),
        expires_at,
        account_id: account
            .get("id")
            .or_else(|| account.get("account_id"))
            .or_else(|| body.get("account_id"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        email: account
            .get("email")
            .or_else(|| body.get("email"))
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

/// The start of a device authorisation grant.
#[derive(Clone)]
pub struct DeviceStart {
    /// Opaque device code; never printed.
    pub device_code: String,
    /// Human code to show next to the verification URL.
    pub user_code: String,
    /// URL where the user approves the code.
    pub verification_uri: String,
    /// Seconds until the device code expires.
    pub expires_in: u64,
    /// Polling interval in seconds.
    pub interval: u64,
}

impl std::fmt::Debug for DeviceStart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceStart")
            .field("device_code", &"<redacted>")
            .field("user_code", &self.user_code)
            .field("verification_uri", &self.verification_uri)
            .field("expires_in", &self.expires_in)
            .field("interval", &self.interval)
            .finish()
    }
}

/// Starts `POST /v1/auth/device`.
pub fn start_device_flow(api: &Api) -> Result<DeviceStart, CliError> {
    let response = api.post(
        "/v1/auth/device",
        &json!({
            "client_id": "stackspirit-cli",
            "scope": "account machines ssh",
        }),
        None,
    )?;
    if !response.success() {
        let message = format!("the cloud API refused the device login: {}", response.describe());
        return Err(if matches!(response.status, 401 | 403) {
            CliError::Auth(message)
        } else {
            CliError::Remote(message)
        });
    }
    let required = |field: &str| {
        response
            .body
            .get(field)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| CliError::Remote(format!("the device login response has no {field}")))
    };
    let device_code = required("device_code")?;
    let user_code = required("user_code")?;
    let verification_uri = response
        .body
        .get("verification_uri_complete")
        .and_then(Value::as_str)
        .or_else(|| response.body.get("verification_uri").and_then(Value::as_str))
        .or_else(|| response.body.get("verification_url").and_then(Value::as_str))
        .ok_or_else(|| CliError::Remote("the device login response has no verification URL".to_owned()))?
        .to_owned();
    Ok(DeviceStart {
        device_code,
        user_code,
        verification_uri,
        expires_in: response.body.get("expires_in").and_then(Value::as_u64).unwrap_or(600),
        interval: response.body.get("interval").and_then(Value::as_u64).unwrap_or(5),
    })
}

/// One device-flow poll result.
#[derive(Debug)]
pub enum DevicePoll {
    /// The user has not approved yet; poll again after the interval.
    Pending,
    /// The API asked for a slower poll.
    SlowDown,
    /// The grant completed.
    Tokens(Tokens),
}

/// Polls `POST /v1/auth/device/token` once.
pub fn poll_device_token(api: &Api, device_code: &str) -> Result<DevicePoll, CliError> {
    let response = api.post(
        "/v1/auth/device/token",
        &json!({
            "device_code": device_code,
            "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
        }),
        None,
    )?;
    if response.success() {
        return Ok(DevicePoll::Tokens(tokens_from_body(&response.body)?));
    }
    if api::device_pending(&response) {
        return Ok(DevicePoll::Pending);
    }
    if api::device_slow_down(&response) {
        return Ok(DevicePoll::SlowDown);
    }
    match response.error_code() {
        Some("expired_token") => Err(CliError::Auth(
            "the device login code expired; run the login command again".to_owned(),
        )),
        Some("access_denied") => Err(CliError::Auth("the device login was denied by the user".to_owned())),
        Some(_) => Err(CliError::Auth(format!(
            "the device login failed: {}",
            response.describe()
        ))),
        None => Err(CliError::Remote(format!(
            "the device login failed: {}",
            response.describe()
        ))),
    }
}

/// Sleeps for the current device-flow interval, capped so a bad response cannot hang forever.
pub fn wait_for_poll(interval: u64) {
    std::thread::sleep(Duration::from_secs(interval.min(60)));
}

/// An account profile from `GET /v1/me`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Account {
    /// Account id (`acct_...`), when reported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Account email, when reported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

/// Extracts an account from a `/v1/me` document.
pub fn account_from_body(body: &Value) -> Account {
    let account = body.get("account").unwrap_or(body);
    Account {
        id: account
            .get("id")
            .or_else(|| account.get("account_id"))
            .or_else(|| body.get("account_id"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        email: account
            .get("email")
            .or_else(|| body.get("email"))
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}

/// One machine registered with the cloud account.
#[derive(Debug, Clone, Serialize)]
pub struct Machine {
    /// `mch_...` id.
    pub id: String,
    /// Display name; falls back to the id.
    pub name: String,
    /// Canonical presence: `online`, `degraded`, `offline`, `revoked`, or the API's own value.
    pub state: String,
    /// True for `online` and `degraded`.
    pub online: bool,
    /// Last-seen timestamp as reported by the API.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<String>,
    /// Project name, when reported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// SSH user, when reported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_user: Option<String>,
}

impl Machine {
    /// Parses one machine object; a row without an id is ignored.
    fn from_value(value: &Value) -> Option<Self> {
        let id = value.get("id").and_then(Value::as_str)?.trim();
        if id.is_empty() {
            return None;
        }
        let presence = value.get("presence").filter(|presence| presence.is_object());
        let raw_state = presence
            .and_then(|presence| presence.get("state"))
            .or_else(|| presence.and_then(|presence| presence.get("status")))
            .or_else(|| value.get("state"))
            .or_else(|| value.get("status"))
            .and_then(Value::as_str);
        let reported_online = value.get("online").and_then(Value::as_bool).or_else(|| {
            presence
                .and_then(|presence| presence.get("online"))
                .and_then(Value::as_bool)
        });
        let (state, online) = match (reported_online, raw_state) {
            (Some(true), _) => ("online".to_owned(), true),
            (Some(false), Some(raw)) if !is_online_word(raw) => (canonical_state(raw), false),
            (Some(false), _) => ("offline".to_owned(), false),
            (None, Some(raw)) => {
                let state = canonical_state(raw);
                let online = matches!(state.as_str(), "online" | "degraded");
                (state, online)
            }
            (None, None) => ("offline".to_owned(), false),
        };
        let name = value
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(id)
            .to_owned();
        let last_seen_at = value
            .get("last_seen_at")
            .or_else(|| value.get("last_seen"))
            .or_else(|| presence.and_then(|presence| presence.get("last_seen_at")))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let project = value
            .get("project")
            .and_then(|project| project.as_str().or_else(|| project.get("name").and_then(Value::as_str)));
        Some(Self {
            id: id.to_owned(),
            name,
            state,
            online,
            last_seen_at,
            project: project.map(str::to_owned),
            ssh_user: value.get("ssh_user").and_then(Value::as_str).map(str::to_owned),
        })
    }
}

fn is_online_word(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "online" | "connected" | "active" | "degraded"
    )
}

fn canonical_state(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "online" | "connected" | "active" => "online".to_owned(),
        "degraded" => "degraded".to_owned(),
        "offline" | "disconnected" | "inactive" => "offline".to_owned(),
        "revoked" => "revoked".to_owned(),
        other => other.to_owned(),
    }
}

/// Parses `GET /v1/machines`, accepting an array or an object with a `machines`/`items` array.
pub fn machines_from_body(body: &Value) -> Result<Vec<Machine>, CliError> {
    let rows = if let Some(rows) = body.as_array() {
        rows
    } else {
        body.get("machines")
            .or_else(|| body.get("items"))
            .and_then(Value::as_array)
            .ok_or_else(|| CliError::Remote("the cloud API returned an unexpected machine list".to_owned()))?
    };
    Ok(rows.iter().filter_map(Machine::from_value).collect())
}

/// Resolves a machine by exact id first, then by exact name.
pub fn resolve_machine(machines: &[Machine], query: &str) -> Result<Machine, CliError> {
    if let Some(machine) = machines.iter().find(|machine| machine.id == query) {
        return Ok(machine.clone());
    }
    let matches: Vec<&Machine> = machines.iter().filter(|machine| machine.name == query).collect();
    match matches.as_slice() {
        [machine] => Ok((*machine).clone()),
        [] => Err(CliError::NotFound(format!(
            "machine {query}; run `{} machines ls`",
            crate::cli::binary_brand()
        ))),
        _ => Err(CliError::Conflict(format!(
            "machine name {query} is ambiguous; use one of: {}",
            matches
                .iter()
                .map(|machine| machine.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// Fetches and parses `GET /v1/machines`.
pub fn list_machines(client: &mut Client) -> Result<Vec<Machine>, CliError> {
    let body = client.get("/v1/machines")?;
    machines_from_body(&body)
}

/// Mints one relay ticket for `machine_id` bound to `client_key`.
pub fn mint_ticket(
    client: &mut Client,
    machine_id: &str,
    client_key: &vowp_protocol::Jwk,
    ttl_secs: u64,
) -> Result<Value, CliError> {
    if machine_id.is_empty()
        || !machine_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
    {
        return Err(CliError::Invalid(
            "the cloud API returned an unsafe machine id".to_owned(),
        ));
    }
    client.post(
        &format!("/v1/machines/{machine_id}/ticket"),
        &json!({
            "kind": "cli",
            "scopes": ["ssh"],
            "client_key": client_key,
            "ttl_secs": ttl_secs,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_presence_accepts_top_level_and_nested_fields() {
        let body = json!({
            "machines": [
                {"id": "mch_a", "name": "alpha", "online": true, "last_seen_at": "2026-01-01T00:00:00Z"},
                {"id": "mch_b", "name": "beta", "presence": {"state": "offline", "last_seen_at": "2026-01-02T00:00:00Z"}},
                {"id": "mch_c", "status": "revoked"}
            ]
        });
        let machines = machines_from_body(&body).expect("machines");
        assert_eq!(machines.len(), 3);
        assert_eq!(machines[0].state, "online");
        assert!(machines[0].online);
        assert_eq!(machines[1].state, "offline");
        assert!(!machines[1].online);
        assert_eq!(machines[2].state, "revoked");
        assert!(!machines[2].online);
    }

    #[test]
    fn resolve_machine_prefers_an_id_and_reports_ambiguity() {
        let machines = vec![
            Machine {
                id: "mch_a".to_owned(),
                name: "box".to_owned(),
                state: "online".to_owned(),
                online: true,
                last_seen_at: None,
                project: None,
                ssh_user: None,
            },
            Machine {
                id: "mch_b".to_owned(),
                name: "box".to_owned(),
                state: "offline".to_owned(),
                online: false,
                last_seen_at: None,
                project: None,
                ssh_user: None,
            },
        ];
        assert_eq!(resolve_machine(&machines, "mch_b").expect("id").id, "mch_b");
        assert!(matches!(resolve_machine(&machines, "box"), Err(CliError::Conflict(_))));
        assert!(matches!(
            resolve_machine(&machines, "missing"),
            Err(CliError::NotFound(_))
        ));
    }

    #[test]
    fn tokens_accept_the_documented_shapes() {
        let tokens = tokens_from_body(&json!({
            "access_token": "a",
            "refresh_token": "r",
            "expires_in": 900,
            "account": {"id": "acct_1", "email": "owner@example.test"}
        }))
        .expect("tokens");
        assert_eq!(tokens.access_token, "a");
        assert_eq!(tokens.refresh_token.as_deref(), Some("r"));
        assert_eq!(tokens.account_id.as_deref(), Some("acct_1"));
        assert_eq!(tokens.email.as_deref(), Some("owner@example.test"));
        assert!(tokens.expires_at.is_some());
        let debug = format!("{tokens:?}");
        assert!(!debug.contains("access_token: \"a\""), "{debug}");
        assert!(!debug.contains("refresh_token: Some(\"r\")"), "{debug}");
        assert!(debug.contains("<redacted>"), "{debug}");
    }
}
