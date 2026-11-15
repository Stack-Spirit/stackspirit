//! Typed process failures and stable exit codes.
//!
//! Every command returns a [`CliError`]. `main` prints it (human text by default,
//! the core `SpiritError` JSON object under `--json`) and exits with the matching
//! code. Argument-parser failures exit 2 before a [`CliError`] exists.

use std::fmt;
use std::io::Write;

use spirit_core::error::{SpiritError, SpiritErrorCode};

/// The command succeeded.
pub const EXIT_SUCCESS: u8 = 0;
/// The command line or an input value is invalid.
pub const EXIT_USAGE: u8 = 2;
/// The operation conflicts with local state, a pin, or a capability gate.
pub const EXIT_CONFLICT: u8 = 3;
/// The requested host, project or item does not exist.
pub const EXIT_NOT_FOUND: u8 = 4;
/// A remote command, the transport, the local store, or output I/O failed.
pub const EXIT_FAILURE: u8 = 5;

/// Maps a core error code to the CLI's documented exit code.
#[must_use]
pub const fn exit_code_for(code: SpiritErrorCode) -> u8 {
    match code {
        SpiritErrorCode::InvalidInput => EXIT_USAGE,
        SpiritErrorCode::NotFound | SpiritErrorCode::HostNotFound => EXIT_NOT_FOUND,
        SpiritErrorCode::Conflict
        | SpiritErrorCode::Refused
        | SpiritErrorCode::Gate
        | SpiritErrorCode::HostKeyMismatch => EXIT_CONFLICT,
        SpiritErrorCode::Limit
        | SpiritErrorCode::Auth
        | SpiritErrorCode::SshAuth
        | SpiritErrorCode::SshUnavailable
        | SpiritErrorCode::Remote
        | SpiritErrorCode::RemoteTimeout
        | SpiritErrorCode::VowpMissing
        | SpiritErrorCode::VowpIncompatible
        | SpiritErrorCode::VowpCommand
        | SpiritErrorCode::Parse
        | SpiritErrorCode::Keychain
        | SpiritErrorCode::Engine
        | SpiritErrorCode::Io
        | SpiritErrorCode::Internal => EXIT_FAILURE,
    }
}

/// A command failure plus its process exit code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliError {
    /// Safe, typed core error. This is also the `--json` error body.
    pub error: SpiritError,
    /// The documented process exit code.
    pub exit: u8,
}

impl CliError {
    /// Wraps a core error and derives its exit code.
    #[must_use]
    pub fn new(error: SpiritError) -> Self {
        let exit = exit_code_for(error.code);
        Self { error, exit }
    }

    /// Builds a usage error (exit 2) without a field-prefixed core message.
    #[must_use]
    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(SpiritError::new(SpiritErrorCode::InvalidInput, message))
    }

    /// Builds a conflict error (exit 3).
    #[must_use]
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(SpiritError::new(SpiritErrorCode::Conflict, message))
    }

    /// Builds a host-not-found error (exit 4).
    #[must_use]
    pub fn host_not_found(message: impl Into<String>) -> Self {
        Self::new(SpiritError::new(SpiritErrorCode::HostNotFound, message))
    }

    /// Builds a remote/transport failure (exit 5).
    #[must_use]
    pub fn remote(message: impl Into<String>) -> Self {
        Self::new(SpiritError::new(SpiritErrorCode::Remote, message))
    }

    /// Adds a hint to the wrapped error.
    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.error = self.error.with_hint(hint);
        self
    }

    /// Writes the failure to `err`.
    ///
    /// JSON mode writes only the core `SpiritError` object (one line) so a caller can
    /// parse it. Human mode writes a short message and an optional hint.
    pub fn report(&self, json: bool, err: &mut dyn Write) -> std::io::Result<()> {
        if json {
            let body = serde_json::to_string(&self.error).unwrap_or_else(|_| {
                format!(
                    "{{\"code\":\"{}\",\"message\":\"{}\",\"hint\":null,\"retryable\":false}}",
                    self.error.code.as_str(),
                    crate::output::safe_text(&self.error.message, 500)
                        .replace('\\', "\\\\")
                        .replace('"', "\\\"")
                )
            });
            return writeln!(err, "{body}");
        }
        writeln!(err, "spirit: {}", self.error.message)?;
        if let Some(hint) = &self.error.hint {
            writeln!(err, "hint: {hint}")?;
        }
        Ok(())
    }
}

impl From<SpiritError> for CliError {
    fn from(error: SpiritError) -> Self {
        Self::new(error)
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.error.code, self.error.message)
    }
}

impl std::error::Error for CliError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_exit_codes_follow_the_brief() {
        assert_eq!(exit_code_for(SpiritErrorCode::InvalidInput), EXIT_USAGE);
        assert_eq!(exit_code_for(SpiritErrorCode::NotFound), EXIT_NOT_FOUND);
        assert_eq!(exit_code_for(SpiritErrorCode::HostNotFound), EXIT_NOT_FOUND);
        assert_eq!(exit_code_for(SpiritErrorCode::Conflict), EXIT_CONFLICT);
        assert_eq!(
            exit_code_for(SpiritErrorCode::HostKeyMismatch),
            EXIT_CONFLICT
        );
        assert_eq!(exit_code_for(SpiritErrorCode::SshAuth), EXIT_FAILURE);
        assert_eq!(exit_code_for(SpiritErrorCode::Remote), EXIT_FAILURE);
        assert_eq!(exit_code_for(SpiritErrorCode::Io), EXIT_FAILURE);
    }
}
