//! Local process specs and POSIX remote-command quoting.
//!
//! Local programs are always spawned from an argv array; there is no local shell.
//! The remote command is one string built by single-quoting every argument. User text
//! can therefore contain spaces or quotes without splitting or injecting a command.

use std::time::Duration;

use crate::error::SpiritError;

/// Default cap for one JSON document.
pub const OUTPUT_CAP_BYTES: usize = 16 * 1024 * 1024;
/// Default cap for captured stderr.
pub const STDERR_CAP_BYTES: usize = 64 * 1024;
/// Timeout for read commands.
pub const READ_TIMEOUT: Duration = Duration::from_secs(20);
/// Timeout for write commands.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(30);
/// Timeout for the initial snapshot.
pub const INITIAL_TIMEOUT: Duration = Duration::from_secs(60);

/// One local process invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    /// Program to execute, resolved by the OS (never a shell string).
    pub program: String,
    /// Argument array.
    pub args: Vec<String>,
    /// Bytes written to stdin, when any.
    pub stdin: Option<Vec<u8>>,
    /// Deadline for the whole process.
    pub timeout: Duration,
    /// Maximum stdout bytes before the call fails with `output_too_large`.
    pub output_cap: usize,
}

impl CommandSpec {
    /// Builds a command with the default read timeout and output cap.
    #[must_use]
    pub fn new(program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
            stdin: None,
            timeout: READ_TIMEOUT,
            output_cap: OUTPUT_CAP_BYTES,
        }
    }

    /// Replaces the timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Replaces the stdout cap.
    #[must_use]
    pub fn with_output_cap(mut self, cap: usize) -> Self {
        self.output_cap = cap;
        self
    }

    /// Supplies stdin bytes.
    #[must_use]
    pub fn with_stdin(mut self, stdin: Vec<u8>) -> Self {
        self.stdin = Some(stdin);
        self
    }
}

/// The captured result of one local process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    /// Exit status. `-1` means the process was killed by a signal we did not decode.
    pub status: i32,
    /// Raw stdout bytes.
    pub stdout: Vec<u8>,
    /// Raw stderr bytes, bounded and never trusted.
    pub stderr: Vec<u8>,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
    /// True when stderr exceeded the capture cap and was cut.
    pub stderr_truncated: bool,
}

impl CommandOutput {
    /// Builds a successful result.
    #[must_use]
    pub fn ok(stdout: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 0,
            stdout: stdout.into(),
            stderr: Vec::new(),
            duration_ms: 0,
            stderr_truncated: false,
        }
    }

    /// Builds a failed result.
    #[must_use]
    pub fn fail(status: i32, stderr: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            stdout: Vec::new(),
            stderr: stderr.into(),
            duration_ms: 0,
            stderr_truncated: false,
        }
    }

    /// Returns true for exit status zero.
    #[must_use]
    pub const fn success(&self) -> bool {
        self.status == 0
    }

    /// Returns stdout as lossy UTF-8.
    #[must_use]
    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// Returns stderr as lossy UTF-8, redacted for display.
    #[must_use]
    pub fn stderr_text(&self) -> String {
        crate::error::redact(&String::from_utf8_lossy(&self.stderr))
    }
}

/// Quotes one argument for a POSIX shell: wraps it in single quotes and escapes every
/// embedded quote. NUL, newline and carriage return are rejected because they cannot be
/// represented safely in a single SSH remote-command string.
pub fn quote_arg(arg: &str) -> Result<String, SpiritError> {
    if arg.contains('\u{0}') || arg.contains('\n') || arg.contains('\r') {
        return Err(SpiritError::invalid_input(
            "remote argument",
            "must not contain NUL, newline or carriage return",
        ));
    }
    Ok(format!("'{}'", arg.replace('\'', "'\\''")))
}

/// Builds the remote command by single-quoting every argv element and joining with a
/// single space. There is no pipeline, `;`, `&&`, or redirection.
pub fn remote_command(argv: &[String]) -> Result<String, SpiritError> {
    let mut parts = Vec::with_capacity(argv.len());
    for arg in argv {
        parts.push(quote_arg(arg)?);
    }
    Ok(parts.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_argument_is_quoted() {
        assert_eq!(quote_arg("").unwrap(), "''");
    }

    #[test]
    fn embedded_quote_is_escaped() {
        assert_eq!(quote_arg("a'b").unwrap(), "'a'\\''b'");
    }

    #[test]
    fn newline_is_rejected() {
        assert!(quote_arg("a\nb").is_err());
    }

    #[test]
    fn remote_command_has_no_shell_operators() {
        let argv = vec!["vowp".to_owned(), "job".to_owned(), "retry".to_owned()];
        let command = remote_command(&argv).unwrap();
        assert_eq!(command, "'vowp' 'job' 'retry'");
        assert!(!command.contains(';'));
        assert!(!command.contains("&&"));
        assert!(!command.contains('|'));
    }
}
