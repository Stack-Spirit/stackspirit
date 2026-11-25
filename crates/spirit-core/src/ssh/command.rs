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
    /// Extra environment variables for the child only.
    ///
    /// Values are never rendered in an error and must never hold secret material
    /// that is not already required by the child. The SSH askpass flow uses this
    /// for the helper *path* only.
    pub env: Vec<(String, String)>,
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
            env: Vec::new(),
            timeout: READ_TIMEOUT,
            output_cap: OUTPUT_CAP_BYTES,
        }
    }

    /// Adds one environment variable for the child process.
    #[must_use]
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let Self { env, .. } = &mut self;
        env.push((key.into(), value.into()));
        self
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

/// Preferred engine binary name.
pub const ENGINE_BINARY: &str = "stackspirit";
/// Legacy engine binary name that existing homes keep using.
pub const LEGACY_ENGINE_BINARY: &str = "vowp";
/// Preferred project config file name.
pub const ENGINE_CONFIG_FILE: &str = "stackspirit.toml";
/// Legacy project config file name.
pub const LEGACY_ENGINE_CONFIG_FILE: &str = "vowp.toml";
/// Preferred home environment variable.
pub const ENGINE_HOME_ENV: &str = "STACKSPIRIT_HOME";
/// Legacy home environment variable.
pub const LEGACY_ENGINE_HOME_ENV: &str = "VOWP_HOME";

/// Returns the engine home from an environment lookup: `STACKSPIRIT_HOME` first, then
/// `VOWP_HOME`. Empty values are ignored.
pub fn engine_home_from_env(get: impl Fn(&str) -> Option<String>) -> Option<String> {
    [ENGINE_HOME_ENV, LEGACY_ENGINE_HOME_ENV]
        .into_iter()
        .find_map(|key| get(key).filter(|value| !value.is_empty()))
}

/// Picks the project config file name: `stackspirit.toml` when `has_file` reports it,
/// otherwise the legacy `vowp.toml`.
pub fn engine_config_file(has_file: impl Fn(&str) -> bool) -> &'static str {
    if has_file(ENGINE_CONFIG_FILE) {
        ENGINE_CONFIG_FILE
    } else {
        LEGACY_ENGINE_CONFIG_FILE
    }
}

/// Resolves the local engine program: `stackspirit` when an executable of that name is on
/// the given `PATH` value, otherwise `vowp`.
#[must_use]
pub fn resolve_local_engine_binary(path_env: Option<&str>) -> &'static str {
    let found = path_env.is_some_and(|path| {
        std::env::split_paths(path).any(|dir| {
            let candidate = dir.join(ENGINE_BINARY);
            std::fs::metadata(&candidate).is_ok_and(|meta| meta.is_file() && is_executable(&meta))
        })
    });
    if found {
        ENGINE_BINARY
    } else {
        LEGACY_ENGINE_BINARY
    }
}

#[cfg(unix)]
fn is_executable(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_meta: &std::fs::Metadata) -> bool {
    true
}

/// Resolves the local engine program from the process `PATH`.
#[must_use]
pub fn local_engine_binary() -> &'static str {
    let path = std::env::var_os("PATH").map(|path| path.to_string_lossy().into_owned());
    resolve_local_engine_binary(path.as_deref())
}

/// Resolves the engine home directory from the process environment
/// (`STACKSPIRIT_HOME`, then `VOWP_HOME`).
#[must_use]
pub fn engine_home_from_process_env() -> Option<String> {
    engine_home_from_env(|key| std::env::var(key).ok())
}

/// Process-wide record of which engine binary each SSH destination answered to.
///
/// The remote command stays a plain single-quoted argv (no shell operators), so the
/// binary is chosen up front: `stackspirit` until a command saw that only `vowp` exists.
static REMOTE_ENGINE_BINARIES: std::sync::Mutex<Vec<(String, &'static str)>> =
    std::sync::Mutex::new(Vec::new());

/// Returns the engine program to run on `destination`: `stackspirit` unless a command
/// found that only the legacy `vowp` exists there.
#[must_use]
pub fn engine_binary_for(destination: &str) -> &'static str {
    REMOTE_ENGINE_BINARIES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .find(|(host, _)| host == destination)
        .map_or(ENGINE_BINARY, |(_, binary)| *binary)
}

/// Records the engine program that works on `destination`.
pub fn set_engine_binary_for(destination: &str, binary: &'static str) {
    let mut guard = REMOTE_ENGINE_BINARIES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(entry) = guard.iter_mut().find(|(host, _)| host == destination) {
        entry.1 = binary;
    } else {
        guard.push((destination.to_owned(), binary));
    }
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

    #[test]
    fn remote_binary_defaults_to_stackspirit_until_vowp_is_recorded() {
        assert_eq!(engine_binary_for("ss129-host"), "stackspirit");
        set_engine_binary_for("ss129-host", LEGACY_ENGINE_BINARY);
        assert_eq!(engine_binary_for("ss129-host"), "vowp");
        set_engine_binary_for("ss129-host", ENGINE_BINARY);
        assert_eq!(engine_binary_for("ss129-host"), "stackspirit");
        assert_eq!(engine_binary_for("ss129-other"), "stackspirit");
    }

    #[test]
    fn local_binary_prefers_stackspirit_when_on_path() {
        let dir = std::env::temp_dir().join(format!("ss129-bin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.display().to_string();
        assert_eq!(resolve_local_engine_binary(Some(&path)), "vowp");
        std::fs::write(dir.join("stackspirit"), b"").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file = dir.join("stackspirit");
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert_eq!(resolve_local_engine_binary(Some(&path)), "vowp");
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert_eq!(resolve_local_engine_binary(Some(&path)), "stackspirit");
        assert_eq!(resolve_local_engine_binary(None), "vowp");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn config_file_prefers_stackspirit_toml() {
        assert_eq!(engine_config_file(|_| true), "stackspirit.toml");
        assert_eq!(engine_config_file(|name| name == "vowp.toml"), "vowp.toml");
        assert_eq!(engine_config_file(|_| false), "vowp.toml");
    }

    #[test]
    fn home_env_prefers_stackspirit_home() {
        let both = |key: &str| Some(format!("/{key}"));
        assert_eq!(
            engine_home_from_env(both).as_deref(),
            Some("/STACKSPIRIT_HOME")
        );
        let legacy = |key: &str| (key == "VOWP_HOME").then(|| "/old".to_owned());
        assert_eq!(engine_home_from_env(legacy).as_deref(), Some("/old"));
        let empty = |key: &str| (key == "STACKSPIRIT_HOME").then(String::new);
        assert_eq!(engine_home_from_env(empty), None);
    }
}
