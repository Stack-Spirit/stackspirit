//! Typed VOWP server-secret metadata and the validated `vowp secret set` request.
//!
//! VOWP owns the server vault. StackSpirit mirrors only names, the last four characters
//! and timestamps, so no list entry, outcome, error or debug rendering can carry a full
//! value. [`SecretSetRequest`] keeps its value in a zeroizing [`SecretString`] and has an
//! explicit `Debug` implementation that redacts it.
//!
//! VOWP facts this module relies on:
//! - VOWP `0.4.0`: `vowp --json secret ls` prints a bare array of
//!   `{name,last4,created_at,updated_at}` and reports no storage mode;
//! - VOWP `0.5.1`: `secret ls --json` prints `{"mode":"basic"|"vault_user"|"encrypted",
//!   "secrets":[…]}`; `mode` becomes [`SecretListView::storage`];
//! - `vowp --json secret rm NAME --yes` prints `{name,removed:true}`;
//! - `vowp --json secret set NAME` prompts on its stderr with `value for NAME: ` only
//!   after its `stty -echo` guard has run.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::engine::SecretString;
use crate::error::{SpiritError, SpiritErrorCode};
use crate::time::Timestamp;

/// Hard cap for one secret value in bytes.
///
/// This matches the one-time-password boundary: a value that does not fit on one
/// hidden-prompt line is rejected before any SSH command is attempted.
pub const SECRET_VALUE_MAX_BYTES: usize = 4096;

/// One stored secret's metadata. The value itself is never part of this type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretEntry {
    /// Validated secret name (`^[A-Z][A-Z0-9_]{0,63}$`).
    pub name: String,
    /// The last four characters of the value, at most four characters.
    pub last4: String,
    /// When VOWP first stored the secret.
    pub created_at: Timestamp,
    /// When VOWP last replaced the secret.
    pub updated_at: Timestamp,
}

/// The `vowp secret ls` read model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretListView {
    /// Secrets in the exact order VOWP returned them.
    pub secrets: Vec<SecretEntry>,
    /// Storage mode when VOWP reports one.
    ///
    /// VOWP `0.5.1` reports it as the document's `mode` (`basic`, `vault_user` or
    /// `encrypted`; other safe tokens are kept verbatim). `None` means "not reported"
    /// (the VOWP `0.4.0` bare array). It never means "basic".
    #[serde(default, alias = "mode")]
    pub storage: Option<String>,
}

/// The parsed result of `vowp --json secret rm NAME --yes`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretRemoveOutcome {
    /// The removed secret's name.
    pub name: String,
    /// Whether VOWP reports the secret as removed.
    pub removed: bool,
}

/// The safe summary of `vowp --json secret set NAME`. Never the value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretSetOutcome {
    /// The stored secret's name.
    pub name: String,
    /// The last four characters of the stored value.
    pub last4: String,
}

/// A validated request to store one secret on the server.
///
/// The name is validated against `^[A-Z][A-Z0-9_]{0,63}$`; the value must be non-empty,
/// at most [`SECRET_VALUE_MAX_BYTES`] bytes, and must not contain `\n`, `\r` or NUL. A
/// value that cannot be written as one hidden-prompt line is refused locally so it can
/// never reach an argv array or a terminal.
pub struct SecretSetRequest {
    name: String,
    value: SecretString,
}

impl SecretSetRequest {
    /// Validates and builds one `vowp secret set` request.
    pub fn new(name: impl Into<String>, value: SecretString) -> Result<Self, SpiritError> {
        let name = name.into();
        validate_secret_name(&name)?;
        validate_secret_value(value.expose())?;
        Ok(Self { name, value })
    }

    /// Returns the validated name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Borrows the value for the one stdin write. Callers must not log the result.
    #[must_use]
    pub(crate) fn expose_value(&self) -> &str {
        self.value.expose()
    }
}

impl fmt::Debug for SecretSetRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretSetRequest")
            .field("name", &self.name)
            .field("value", &"[redacted]")
            .finish()
    }
}

/// Validates a VOWP secret name: `^[A-Z][A-Z0-9_]{0,63}$`.
pub fn validate_secret_name(name: &str) -> Result<(), SpiritError> {
    let mut chars = name.chars();
    let valid_first = chars.next().is_some_and(|c| c.is_ascii_uppercase());
    let valid_rest = chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
    if name.len() <= 64 && valid_first && valid_rest {
        Ok(())
    } else {
        Err(SpiritError::invalid_input(
            "secret name",
            "use ^[A-Z][A-Z0-9_]{0,63}$ (uppercase letters, digits and '_' only)",
        ))
    }
}

/// Validates one secret value without ever naming it in the error.
fn validate_secret_value(value: &str) -> Result<(), SpiritError> {
    if value.is_empty() {
        return Err(SpiritError::invalid_input(
            "secret value",
            "must not be empty",
        ));
    }
    if value.len() > SECRET_VALUE_MAX_BYTES {
        return Err(SpiritError::invalid_input(
            "secret value",
            format!("must be at most {SECRET_VALUE_MAX_BYTES} bytes"),
        ));
    }
    if value.contains('\n') || value.contains('\r') || value.contains('\u{0}') {
        return Err(SpiritError::invalid_input(
            "secret value",
            "must not contain a newline, carriage return or NUL byte",
        ));
    }
    Ok(())
}

/// The two shapes a strict `secret ls` document may take.
#[derive(Deserialize)]
#[serde(untagged)]
enum SecretListWire {
    /// VOWP `0.4.0`: a bare array of metadata rows.
    Entries(Vec<SecretEntry>),
    /// VOWP `0.5.1`: a document that also reports the storage `mode`.
    Document(SecretListView),
}

/// Parses `vowp --json secret ls` strictly and validates every metadata row.
pub(crate) fn parse_secret_list(bytes: &[u8]) -> Result<SecretListView, SpiritError> {
    let wire: SecretListWire =
        serde_json::from_slice(bytes).map_err(|err| SpiritError::parse("secret list", err))?;
    let view = match wire {
        SecretListWire::Entries(secrets) => SecretListView {
            secrets,
            storage: None,
        },
        SecretListWire::Document(view) => view,
    };
    if let Some(mode) = &view.storage {
        if !is_safe_mode_token(mode) {
            return Err(SpiritError::vowp_incompatible(
                "secret list",
                "VOWP returned an unsafe vault storage mode",
            ));
        }
    }
    for entry in &view.secrets {
        if validate_secret_name(&entry.name).is_err() {
            return Err(SpiritError::vowp_incompatible(
                "secret list",
                format!("VOWP returned an invalid secret name {:?}", entry.name),
            ));
        }
        if entry.last4.chars().count() > 4 || entry.last4.chars().any(char::is_control) {
            return Err(SpiritError::vowp_incompatible(
                "secret list",
                format!("VOWP returned an unsafe last4 for {:?}", entry.name),
            ));
        }
    }
    Ok(view)
}

/// A storage mode must match `^[a-z][a-z0-9_]{0,31}$`.
fn is_safe_mode_token(mode: &str) -> bool {
    let mut chars = mode.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && mode.len() <= 32
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// The minimal object VOWP returns from `secret set`.
#[derive(Deserialize)]
struct SecretSetWire {
    name: String,
    last4: String,
}

/// Parses the final JSON object VOWP prints after the hidden prompt.
pub(crate) fn parse_secret_set(bytes: &[u8]) -> Result<SecretSetOutcome, SpiritError> {
    let text = strip_terminal_sequences(&String::from_utf8_lossy(bytes));
    let value = parse_final_json_object(&text)?;
    let wire: SecretSetWire =
        serde_json::from_value(value).map_err(|err| SpiritError::parse("secret set", err))?;
    validate_secret_name(&wire.name).map_err(|_| {
        SpiritError::vowp_incompatible(
            "secret set",
            format!("VOWP returned an invalid secret name {:?}", wire.name),
        )
    })?;
    if wire.last4.chars().count() > 4 || wire.last4.chars().any(char::is_control) {
        return Err(SpiritError::vowp_incompatible(
            "secret set",
            format!("VOWP returned an unsafe last4 for {:?}", wire.name),
        ));
    }
    Ok(SecretSetOutcome {
        name: wire.name,
        last4: wire.last4,
    })
}

/// Parses `vowp --json secret rm NAME --yes`.
pub(crate) fn parse_secret_remove(bytes: &[u8]) -> Result<SecretRemoveOutcome, SpiritError> {
    let wire: SecretRemoveOutcome =
        serde_json::from_slice(bytes).map_err(|err| SpiritError::parse("secret remove", err))?;
    validate_secret_name(&wire.name).map_err(|_| {
        SpiritError::vowp_incompatible(
            "secret remove",
            format!("VOWP returned an invalid secret name {:?}", wire.name),
        )
    })?;
    Ok(wire)
}

/// Returns the last complete JSON object in a pty transcript.
///
/// A remote pty can interleave the hidden prompt, CRLF translation and terminal control
/// sequences with the JSON result. The scan starts at the last `{` and walks backwards
/// until a suffix parses as one complete object, so a prefix never reaches the parser.
pub(crate) fn parse_final_json_object(text: &str) -> Result<serde_json::Value, SpiritError> {
    let starts: Vec<usize> = text.match_indices('{').map(|(index, _)| index).collect();
    for start in starts.into_iter().rev() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text[start..]) else {
            continue;
        };
        if value.is_object() {
            return Ok(value);
        }
    }
    Err(SpiritError::new(
        SpiritErrorCode::Parse,
        "secret set: the hidden prompt produced no final JSON object",
    ))
}

/// Removes terminal control sequences from a captured pty transcript.
///
/// CSI, OSC and two-character escapes are dropped, as are other C0/C1 controls except
/// tab and newline. Printable text, including the prompt and the JSON body, is kept, so
/// the parser still sees the exact object VOWP printed. Callers must never log the raw
/// transcript this function sanitises.
#[must_use]
pub(crate) fn strip_terminal_sequences(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    for next in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&next) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    while let Some(next) = chars.next() {
                        if next == '\u{7}' {
                            break;
                        }
                        if next == '\u{1b}' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                Some(_) => {
                    chars.next();
                }
                None => {}
            }
            continue;
        }
        if ch == '\n' || ch == '\t' || !ch.is_control() {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_names_match_the_vowp_grammar() {
        for valid in ["A", "TEST_KEY", "OPENAI_API_KEY_2", &"A".repeat(64)] {
            assert!(validate_secret_name(valid).is_ok(), "{valid} must be valid");
        }
        for invalid in ["", "a", "TEST-KEY", "1KEY", &"A".repeat(65), "TEST KEY"] {
            assert!(
                validate_secret_name(invalid).is_err(),
                "{invalid:?} must be invalid"
            );
        }
    }

    #[test]
    fn the_set_request_redacts_its_value() {
        let value = ["unit", "-test", "-value"].concat();
        let request = SecretSetRequest::new("TEST_KEY", SecretString::new(value.clone())).unwrap();
        let debug = format!("{request:?}");
        assert!(debug.contains("TEST_KEY"));
        assert!(!debug.contains(&value), "the value leaked through Debug");
        assert!(debug.contains("redacted"));
    }

    #[test]
    fn the_set_request_refuses_multiline_and_oversized_values() {
        for value in [
            String::new(),
            ["a\nb"].concat(),
            ["a\rb"].concat(),
            ["a", "\u{0}", "b"].concat(),
            "x".repeat(SECRET_VALUE_MAX_BYTES + 1),
        ] {
            assert!(
                SecretSetRequest::new("TEST_KEY", SecretString::new(value)).is_err(),
                "the request accepted an unsafe value"
            );
        }
        assert!(
            SecretSetRequest::new(
                "TEST_KEY",
                SecretString::new("x".repeat(SECRET_VALUE_MAX_BYTES))
            )
            .is_ok()
        );
    }

    #[test]
    fn control_sequences_are_stripped_but_the_json_is_kept() {
        let transcript =
            "\u{1b}[2Kvalue for TEST_KEY: \r\n\u{1b}]0;title\u{7}{\"name\":\"TEST_KEY\"}\r\n";
        let clean = strip_terminal_sequences(transcript);
        assert_eq!(clean, "value for TEST_KEY: \n{\"name\":\"TEST_KEY\"}\n");
        assert_eq!(
            parse_final_json_object(&clean).unwrap()["name"],
            serde_json::Value::String("TEST_KEY".to_owned())
        );
    }

    #[test]
    fn only_the_last_json_object_is_parsed() {
        let text =
            "value for TEST_KEY: {\"old\":true}\n{\"name\":\"TEST_KEY\",\"last4\":\"abcd\"}\n";
        let value = parse_final_json_object(text).unwrap();
        assert_eq!(value["name"], "TEST_KEY");
        assert!(value.get("old").is_none());
    }
}
