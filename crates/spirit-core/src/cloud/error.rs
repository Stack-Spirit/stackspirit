//! Typed, redaction-safe errors for the optional StackSpirit cloud surface.
//!
//! The cloud client never puts a bearer token, refresh token, device code, relay
//! ticket or SSH material in an error. Messages carry only the API's safe text
//! and a stable code; [`std::fmt::Debug`] shows the same public fields as
//! [`std::fmt::Display`].

use std::fmt;

use crate::error::{SpiritError, SpiritErrorCode};

/// Stable cloud error codes shared with the Account UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CloudErrorCode {
    /// The optional cloud surface is not available in this build.
    Unavailable,
    /// The API could not be reached.
    Unreachable,
    /// The API did not answer within the client's deadline.
    Timeout,
    /// Sign-in is missing, expired or was refused.
    Auth,
    /// The account may not use that machine or action.
    Forbidden,
    /// The account or machine does not exist.
    NotFound,
    /// The machine changed while the caller was using a stale view.
    Conflict,
    /// The API is rate limiting this account.
    RateLimited,
    /// The API failed on its side.
    Server,
    /// A response document could not be parsed.
    Parse,
    /// The API answered with another HTTP status.
    Http,
    /// The Always-On Home is not online, so a relay ticket cannot be used.
    MachineOffline,
    /// The relay transport is not available in this build.
    RelayUnavailable,
    /// The webview lacks a required cryptographic primitive.
    CryptoUnsupported,
    /// The caller supplied an invalid value before any request.
    InvalidInput,
    /// A local private file could not be read or written.
    Io,
}

impl CloudErrorCode {
    /// Returns the canonical snake_case wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unavailable => "cloud_unavailable",
            Self::Unreachable => "cloud_unreachable",
            Self::Timeout => "cloud_timeout",
            Self::Auth => "cloud_auth",
            Self::Forbidden => "cloud_forbidden",
            Self::NotFound => "cloud_not_found",
            Self::Conflict => "cloud_conflict",
            Self::RateLimited => "cloud_rate_limited",
            Self::Server => "cloud_server",
            Self::Parse => "cloud_parse",
            Self::Http => "cloud_http",
            Self::MachineOffline => "cloud_machine_offline",
            Self::RelayUnavailable => "cloud_relay_unavailable",
            Self::CryptoUnsupported => "cloud_crypto_unsupported",
            Self::InvalidInput => "invalid_input",
            Self::Io => "cloud_io",
        }
    }
}

impl fmt::Display for CloudErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One safe cloud error.
///
/// The type deliberately has no token, device-code or ticket field. The
/// optional `status` is the HTTP status when the API answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudError {
    /// Stable machine-readable code.
    pub code: CloudErrorCode,
    /// Safe, human-readable message.
    pub message: String,
    /// Optional next step for the owner.
    pub hint: Option<String>,
    /// Whether retrying the same operation may succeed.
    pub retryable: bool,
    /// HTTP status when the API answered.
    pub status: Option<u16>,
}

impl CloudError {
    /// Builds an error with a safe message and no hint.
    #[must_use]
    pub fn new(code: CloudErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: None,
            retryable: false,
            status: None,
        }
    }

    /// Adds a safe next step.
    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Marks the operation as safely retryable.
    #[must_use]
    pub fn retryable(mut self) -> Self {
        self.retryable = true;
        self
    }

    /// Records the HTTP status the API answered with.
    #[must_use]
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }

    /// Builds a parsing error that names the document or field, never the body.
    #[must_use]
    pub fn parse(what: &str, detail: impl fmt::Display) -> Self {
        Self::new(
            CloudErrorCode::Parse,
            format!("The StackSpirit API returned an unreadable {what}: {detail}."),
        )
    }

    /// Builds a local I/O error without including file contents.
    #[must_use]
    pub fn io(context: &str, error: &std::io::Error) -> Self {
        Self::new(CloudErrorCode::Io, format!("{context}: {error}"))
    }
}

impl fmt::Display for CloudError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CloudError {}

impl From<CloudError> for SpiritError {
    fn from(error: CloudError) -> Self {
        let code = match error.code {
            CloudErrorCode::InvalidInput => SpiritErrorCode::InvalidInput,
            CloudErrorCode::Auth => SpiritErrorCode::Auth,
            CloudErrorCode::Forbidden
            | CloudErrorCode::MachineOffline
            | CloudErrorCode::RelayUnavailable
            | CloudErrorCode::CryptoUnsupported => SpiritErrorCode::Refused,
            CloudErrorCode::NotFound => SpiritErrorCode::NotFound,
            CloudErrorCode::Conflict => SpiritErrorCode::Conflict,
            CloudErrorCode::RateLimited => SpiritErrorCode::Limit,
            CloudErrorCode::Parse => SpiritErrorCode::Parse,
            CloudErrorCode::Io => SpiritErrorCode::Io,
            CloudErrorCode::Unavailable
            | CloudErrorCode::Unreachable
            | CloudErrorCode::Timeout
            | CloudErrorCode::Server
            | CloudErrorCode::Http => SpiritErrorCode::Remote,
        };
        let mut mapped = SpiritError::new(code, error.message);
        mapped.hint = error.hint;
        mapped.retryable = error.retryable;
        mapped
    }
}
