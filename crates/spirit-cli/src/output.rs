//! Output routing: one JSON document on stdout, or a short human summary.

use std::io::Write;

use serde::Serialize;
use spirit_core::error::SpiritError;

use crate::error::CliError;

/// A tiny writer pair shared by every command.
pub(crate) struct Output<'a> {
    json: bool,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

impl<'a> Output<'a> {
    pub(crate) fn new(json: bool, out: &'a mut dyn Write, err: &'a mut dyn Write) -> Self {
        Self { json, out, err }
    }

    pub(crate) const fn is_json(&self) -> bool {
        self.json
    }

    /// Writes one pretty JSON document followed by a newline.
    pub(crate) fn json<T>(&mut self, value: &T) -> Result<(), CliError>
    where
        T: Serialize + ?Sized,
    {
        serde_json::to_writer_pretty(&mut *self.out, value)
            .map_err(|error| CliError::new(SpiritError::parse("JSON output", error)))?;
        writeln!(self.out).map_err(write_error)
    }

    /// Writes one human line to stdout.
    pub(crate) fn text(&mut self, text: impl AsRef<str>) -> Result<(), CliError> {
        writeln!(self.out, "{}", text.as_ref()).map_err(write_error)
    }

    /// Writes one warning line to stderr.
    pub(crate) fn warning(&mut self, text: impl AsRef<str>) -> Result<(), CliError> {
        writeln!(self.err, "warning: {}", text.as_ref()).map_err(write_error)
    }

    /// Writes one unprefixed informational line to stderr. Used for the fixture banner
    /// and one-shot connection notes.
    pub(crate) fn note(&mut self, text: impl AsRef<str>) -> Result<(), CliError> {
        writeln!(self.err, "{}", text.as_ref()).map_err(write_error)
    }
}

fn write_error(error: std::io::Error) -> CliError {
    CliError::new(SpiritError::io("writing output", &error))
}

/// Removes control characters and caps human-facing text.
///
/// JSON output keeps the original value; this helper only protects terminal lines from
/// labels, remote details and paths containing escape sequences or newlines.
pub(crate) fn safe_text(input: &str, limit: usize) -> String {
    let mut out: String = input
        .chars()
        .filter(|character| !character.is_control())
        .take(limit)
        .collect();
    if input.chars().count() > limit {
        out.push('…');
    }
    out
}

/// Returns `value`, or `"none"` for an absent optional value in human output.
pub(crate) fn optional<T: ToString>(value: Option<T>) -> String {
    value.map_or_else(|| "none".to_owned(), |value| value.to_string())
}

/// Returns a lowercase debug label for a small enum in human output.
pub(crate) fn enum_label(value: impl std::fmt::Debug) -> String {
    format!("{value:?}").to_ascii_lowercase()
}
