//! Stable, redaction-safe error type used across the core and the bridge.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Stable machine-readable error code.
///
/// Codes are part of the UI contract: every value maps to fixed copy in
/// `docs/PLAN.md` §5.5 and must not be renamed without a contract update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpiritErrorCode {
    /// The caller supplied a value that violates a local validation rule.
    InvalidInput,
    /// The requested item does not exist.
    NotFound,
    /// The operation conflicts with current state.
    Conflict,
    /// The operation was refused by a guard.
    Refused,
    /// A provider limit blocked the operation.
    Limit,
    /// Provider authentication failed or is missing.
    Auth,
    /// The host profile is not known locally.
    HostNotFound,
    /// The host key changed or no pin exists.
    HostKeyMismatch,
    /// SSH authentication failed.
    SshAuth,
    /// The system SSH client is missing or could not start.
    SshUnavailable,
    /// A remote command failed.
    Remote,
    /// A remote command exceeded its deadline.
    RemoteTimeout,
    /// The remote `vowp` binary is missing.
    VowpMissing,
    /// The VOWP document schema is not compatible with this build.
    VowpIncompatible,
    /// VOWP rejected or failed a command.
    VowpCommand,
    /// A document could not be parsed.
    Parse,
    /// The OS keychain failed.
    Keychain,
    /// The local engine failed.
    Engine,
    /// A capability gate refused the operation.
    Gate,
    /// Local I/O failed.
    Io,
    /// An unexpected internal failure.
    Internal,
}

impl SpiritErrorCode {
    /// Returns the canonical snake_case wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidInput => "invalid_input",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::Refused => "refused",
            Self::Limit => "limit",
            Self::Auth => "auth",
            Self::HostNotFound => "host_not_found",
            Self::HostKeyMismatch => "host_key_mismatch",
            Self::SshAuth => "ssh_auth",
            Self::SshUnavailable => "ssh_unavailable",
            Self::Remote => "remote",
            Self::RemoteTimeout => "remote_timeout",
            Self::VowpMissing => "vowp_missing",
            Self::VowpIncompatible => "vowp_incompatible",
            Self::VowpCommand => "vowp_command",
            Self::Parse => "parse",
            Self::Keychain => "keychain",
            Self::Engine => "engine",
            Self::Gate => "gate",
            Self::Io => "io",
            Self::Internal => "internal",
        }
    }
}

impl fmt::Display for SpiritErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A user-facing error with a stable code, a safe message and an optional hint.
///
/// The message and hint never contain provider keys, tokens, private keys, or raw
/// command lines. Use [`redact`] before embedding remote stderr.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpiritError {
    /// Stable machine-readable code.
    pub code: SpiritErrorCode,
    /// Safe, human-readable message.
    pub message: String,
    /// Optional next step for the user.
    pub hint: Option<String>,
    /// Whether retrying the same operation may succeed.
    pub retryable: bool,
}

impl SpiritError {
    /// Builds an error. The message is stored verbatim; callers are responsible for
    /// passing only safe text.
    #[must_use]
    pub fn new(code: SpiritErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: None,
            retryable: false,
        }
    }

    /// Adds a hint.
    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Marks the error retryable.
    #[must_use]
    pub fn retryable(mut self) -> Self {
        self.retryable = true;
        self
    }

    /// Builds a parse error naming the document.
    #[must_use]
    pub fn parse(document: &str, detail: impl fmt::Display) -> Self {
        Self::new(SpiritErrorCode::Parse, format!("{document}: {detail}"))
            .with_hint("The last good snapshot is kept. Refresh after VOWP is updated.")
    }

    /// Builds a schema incompatibility error.
    #[must_use]
    pub fn vowp_incompatible(document: &str, detail: impl fmt::Display) -> Self {
        Self::new(
            SpiritErrorCode::VowpIncompatible,
            format!("{document}: {detail}"),
        )
        .with_hint("Update VOWP or StackSpirit. StackSpirit does not guess at a changed schema.")
    }

    /// Builds a remote timeout error.
    #[must_use]
    pub fn remote_timeout(after_secs: u64) -> Self {
        Self::new(
            SpiritErrorCode::RemoteTimeout,
            format!("the remote command did not finish within {after_secs}s"),
        )
        .retryable()
        .with_hint("Check the SSH connection and retry.")
    }

    /// Builds the typed output-too-large guard.
    #[must_use]
    pub fn output_too_large(cap: usize) -> Self {
        Self::new(
            SpiritErrorCode::Parse,
            format!("output_too_large: the command produced more than {cap} bytes"),
        )
        .with_hint("StackSpirit never truncates a JSON document silently.")
    }

    /// Builds an invalid-input error.
    #[must_use]
    pub fn invalid_input(field: &str, detail: impl fmt::Display) -> Self {
        Self::new(SpiritErrorCode::InvalidInput, format!("{field}: {detail}"))
    }

    /// Builds a not-found error.
    #[must_use]
    pub fn not_found(what: &str) -> Self {
        Self::new(SpiritErrorCode::NotFound, format!("{what} not found"))
    }

    /// Builds a host-key mismatch error carrying only fingerprints, never key material.
    #[must_use]
    pub fn host_key_mismatch(old_sha256: &str, new_sha256: &str) -> Self {
        Self::new(
            SpiritErrorCode::HostKeyMismatch,
            format!("host key changed: pinned {old_sha256}, server offered {new_sha256}"),
        )
        .with_hint("Compare the fingerprint out of band and explicitly re-pin the host.")
    }

    /// Builds an I/O error with a redacted detail.
    #[must_use]
    pub fn io(context: &str, err: &std::io::Error) -> Self {
        Self::new(
            SpiritErrorCode::Io,
            format!("{context}: {}", redact(&err.to_string())),
        )
    }

    /// Returns the canonical code string.
    #[must_use]
    pub const fn code_str(&self) -> &'static str {
        self.code.as_str()
    }
}

impl fmt::Display for SpiritError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for SpiritError {}

impl From<std::io::Error> for SpiritError {
    fn from(value: std::io::Error) -> Self {
        Self::new(SpiritErrorCode::Io, redact(&value.to_string()))
    }
}

/// Returns a bounded, redacted copy of untrusted text.
///
/// The output is safe to show in a details pane: provider-looking tokens and
/// `key=value` secrets are replaced, and the result is capped at 2048 characters.
#[must_use]
pub fn redact(input: &str) -> String {
    const LIMIT: usize = 2048;
    let mut sanitized = input.to_owned();
    if let Some(start) = sanitized.find("-----BEGIN") {
        if let Some(key) = sanitized.find("PRIVATE KEY-----") {
            let end = key + "PRIVATE KEY-----".len();
            if start < end {
                sanitized.replace_range(start..end, "[redacted]");
            }
        }
    }
    let mut out = String::with_capacity(sanitized.len().min(LIMIT));
    let mut redact_next = false;
    for token in sanitized.split_inclusive(char::is_whitespace) {
        let bare = token.trim();
        let lower = bare.trim_matches(':').to_ascii_lowercase();
        let rendered = if redact_next || looks_like_secret(bare) {
            redact_next = false;
            "[redacted]"
        } else if lower == "bearer" || lower == "basic" || lower == "authorization" {
            redact_next = true;
            "[redacted]"
        } else {
            token
        };
        if out.len() + rendered.len() > LIMIT {
            out.push('…');
            break;
        }
        out.push_str(rendered);
    }
    out
}

fn looks_like_secret(token: &str) -> bool {
    let token = token.trim_matches(|c: char| {
        matches!(
            c,
            '"' | '\'' | ',' | ';' | ':' | ')' | '(' | '[' | ']' | '{' | '}'
        )
    });
    if token.is_empty() {
        return false;
    }
    let lower = token.to_ascii_lowercase();
    const PREFIXES: [&str; 8] = [
        concat!("s", "k-"),
        concat!("s", "k_"),
        concat!("g", "hp_"),
        concat!("git", "hub_pat_"),
        concat!("x", "oxb-"),
        concat!("x", "oxp-"),
        concat!("ak", "ia"),
        concat!("ai", "za"),
    ];
    if PREFIXES.iter().any(|prefix| lower.starts_with(prefix)) {
        return true;
    }
    if lower.contains("begin") && lower.contains("private key") {
        return true;
    }
    if let Some((name, _value)) = token.split_once('=') {
        let name = name.trim().to_ascii_lowercase();
        const SENSITIVE: [&str; 8] = [
            "api_key",
            "apikey",
            "token",
            "secret",
            "password",
            "passphrase",
            "authorization",
            "credential",
        ];
        if SENSITIVE.iter().any(|needle| name.contains(needle)) {
            return true;
        }
    }
    false
}

/// Maps a VOWP CLI exit code to a StackSpirit code, per `VOWP-CONTRACT.md` §8.2.
#[must_use]
pub const fn code_for_exit(exit: i32) -> SpiritErrorCode {
    match exit {
        2 => SpiritErrorCode::InvalidInput,
        3 => SpiritErrorCode::NotFound,
        4 => SpiritErrorCode::Conflict,
        5 => SpiritErrorCode::Refused,
        6 => SpiritErrorCode::Limit,
        7 => SpiritErrorCode::Remote,
        127 => SpiritErrorCode::VowpMissing,
        75 => SpiritErrorCode::Refused,
        _ => SpiritErrorCode::VowpCommand,
    }
}
