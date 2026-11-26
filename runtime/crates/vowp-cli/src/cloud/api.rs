//! A minimal blocking client for `https://api.stackspirit.io/v1`.
//!
//! Spec: `docs/design/relay-and-packaging.md` §6.4. Every request is one JSON document; a token
//! is sent only in the `Authorization: Bearer` header and is never placed in a URL, an error
//! message or a log line.

use std::time::Duration;

use serde_json::Value;

use crate::CliError;

/// Default API base when neither the flag nor the session says otherwise.
pub const DEFAULT_API_URL: &str = "https://api.stackspirit.io";

/// One API response, including non-2xx ones (status errors are classified by the caller).
#[derive(Clone)]
pub struct ApiResponse {
    /// HTTP status.
    pub status: u16,
    /// Decoded JSON body; `null` for an empty body and a JSON string for a non-JSON body.
    pub body: Value,
}

impl std::fmt::Debug for ApiResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // A token response body must never reach a log through `Debug`.
        f.debug_struct("ApiResponse")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

impl ApiResponse {
    /// True for a 2xx status.
    pub fn success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// The API's stable error code, when the body carries one.
    pub fn error_code(&self) -> Option<&str> {
        let error = self.body.get("error")?;
        error
            .get("code")
            .and_then(Value::as_str)
            .or_else(|| error.as_str())
            .or_else(|| self.body.get("code").and_then(Value::as_str))
    }

    /// The API's path-free error message, when the body carries one.
    pub fn error_message(&self) -> Option<&str> {
        self.body
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .or_else(|| self.body.get("message").and_then(Value::as_str))
    }

    /// A short diagnostic that names the code and status, never the body.
    pub fn describe(&self) -> String {
        match (self.error_code(), self.error_message()) {
            (Some(code), Some(message)) => format!("{code}: {message}"),
            (Some(code), None) => code.to_owned(),
            (None, _) => format!("HTTP {}", self.status),
        }
    }
}

/// The unauthenticated HTTP transport: a base URL and a small blocking agent.
pub struct Api {
    agent: ureq::Agent,
    base: String,
}

impl std::fmt::Debug for Api {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Api").field("base", &self.base).finish_non_exhaustive()
    }
}

impl Api {
    /// Builds a client for `base`, normalising away trailing slashes.
    pub fn new(base: &str) -> Result<Self, CliError> {
        let base = base.trim().trim_end_matches('/').to_owned();
        if base.is_empty() {
            return Err(CliError::Invalid(
                "the API URL is empty; pass --api-url <URL>".to_owned(),
            ));
        }
        if !(base.starts_with("https://") || base.starts_with("http://")) {
            return Err(CliError::Invalid(format!(
                "the API URL must start with https:// (or http:// for a local test server), got {base}"
            )));
        }
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(30)))
            .build();
        Ok(Self {
            agent: ureq::Agent::new_with_config(config),
            base,
        })
    }

    /// The normalised base URL.
    pub fn base(&self) -> &str {
        &self.base
    }

    /// Sends one `GET` and decodes the JSON body.
    pub fn get(&self, path: &str, token: Option<&str>) -> Result<ApiResponse, CliError> {
        let mut request = self.agent.get(self.url(path));
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let mut response = request
            .call()
            .map_err(|error| CliError::Remote(format!("the StackSpirit API is unreachable: {error}")))?;
        decode(&mut response)
    }

    /// Sends one `POST` with a JSON body and decodes the JSON response.
    pub fn post(&self, path: &str, body: &Value, token: Option<&str>) -> Result<ApiResponse, CliError> {
        let mut request = self
            .agent
            .post(self.url(path))
            .header("content-type", "application/json");
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let text = serde_json::to_string(body)
            .map_err(|error| CliError::Invalid(format!("cannot serialise the API request: {error}")))?;
        let mut response = request
            .send(text)
            .map_err(|error| CliError::Remote(format!("the StackSpirit API is unreachable: {error}")))?;
        decode(&mut response)
    }

    fn url(&self, path: &str) -> String {
        if path.starts_with('/') {
            format!("{}{path}", self.base)
        } else {
            format!("{}/{}", self.base, path)
        }
    }
}

/// Reads and decodes one response body, leaving non-JSON bodies as a JSON string.
fn decode(response: &mut ureq::http::Response<ureq::Body>) -> Result<ApiResponse, CliError> {
    let status = response.status().as_u16();
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|error| CliError::Remote(format!("cannot read the StackSpirit API response: {error}")))?;
    let body = if text.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap_or(Value::String(text))
    };
    Ok(ApiResponse { status, body })
}

/// Returns true when `response` is the device-flow "keep polling" answer.
///
/// RFC 8628 servers answer a pending poll with HTTP 400 and `error: authorization_pending`;
/// some deployments use 428 or a body-only marker, so both are accepted without including the
/// body in any message.
pub fn device_pending(response: &ApiResponse) -> bool {
    if matches!(response.status, 400 | 428) {
        if let Some(code) = response.error_code() {
            return code == "authorization_pending";
        }
        if let Value::String(text) = &response.body {
            return text.contains("authorization_pending");
        }
    }
    false
}

/// Returns true when the API asks the client to slow its device-flow polling.
pub fn device_slow_down(response: &ApiResponse) -> bool {
    response.error_code() == Some("slow_down")
}
