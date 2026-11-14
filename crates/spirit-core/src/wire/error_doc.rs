//! The `--json` error document and exit-code mapping.

use serde::{Deserialize, Serialize};

use crate::error::{SpiritError, SpiritErrorCode, code_for_exit};
use crate::wire::enums::ErrorKind;

/// The body of a VOWP `--json` error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorBody {
    /// Stable machine class.
    pub kind: ErrorKind,
    /// Human message; VOWP guarantees no secret.
    pub message: String,
    /// Optional next step.
    pub hint: Option<String>,
}

/// The whole error document printed on stdout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorDocument {
    /// The error body.
    pub error: ErrorBody,
}

impl ErrorDocument {
    /// Maps the document to a [`SpiritError`], using the exit code for the stable code
    /// but the document message for display.
    #[must_use]
    pub fn to_spirit_error(&self, exit: i32) -> SpiritError {
        let code = self.error.kind.to_spirit_code();
        let mut error = SpiritError::new(code, crate::error::redact(&self.error.message));
        error.hint = self.error.hint.as_deref().map(crate::error::redact);
        error.retryable = matches!(
            self.error.kind,
            ErrorKind::Remote | ErrorKind::Timeout | ErrorKind::Unknown(_)
        );
        // A documented kind wins over the generic exit mapping, but an exit code that
        // says "not found" or "conflict" is still authoritative for retry policy.
        if matches!(
            code_for_exit(exit),
            SpiritErrorCode::NotFound | SpiritErrorCode::Conflict | SpiritErrorCode::Refused
        ) {
            error.retryable = code_for_exit(exit) == SpiritErrorCode::NotFound;
        }
        error
    }
}
