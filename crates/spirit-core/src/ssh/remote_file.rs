//! A narrow remote-file helper for the two-step ASIF package transfer.
//!
//! The helper creates one private remote temp directory with `mktemp -d` and `chmod
//! 700`, reads one bounded child file, and removes the directory. Every dynamic value
//! is a separate argv element and therefore POSIX-quoted by [`crate::ssh::command`];
//! no pipeline, redirection, `;` or `&&` is ever used.
//!
//! This module is crate-private on purpose. It is not part of the read contract, it is
//! never exposed to the renderer, and none of its errors carry the remote path. VOWP's
//! export summary is parsed separately so the path never enters a view type.

use std::fmt;
use std::time::Duration;

use crate::error::{SpiritError, SpiritErrorCode};
use crate::ssh::SshRemote;
use crate::ssh::command::{CommandOutput, READ_TIMEOUT};

/// The longest portable remote path this build accepts.
const MAX_REMOTE_PATH: usize = 4096;

/// A remote temp directory created by this process and owned by one transfer.
pub(crate) struct RemoteTempDir {
    remote: SshRemote,
    path: String,
}

impl fmt::Debug for RemoteTempDir {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteTempDir")
            .field("path", &"<private>")
            .finish_non_exhaustive()
    }
}

impl RemoteTempDir {
    /// Creates a private (0700) remote temp directory.
    ///
    /// The returned path must be an absolute single-line path whose final component
    /// starts with `prefix`; anything else is a typed error and is never used or
    /// removed. The directory is removed again on every later failure path by the
    /// caller, and by this function when `chmod` fails.
    pub(crate) async fn create(remote: SshRemote, prefix: &str) -> Result<Self, SpiritError> {
        validate_prefix(prefix)?;
        let template = format!("{prefix}.XXXXXXXXXX");
        let argv = vec![
            "mktemp".to_owned(),
            "-d".to_owned(),
            "-t".to_owned(),
            template,
        ];
        let output = run_checked(&remote, &argv, READ_TIMEOUT, "mktemp").await?;
        let path = parse_single_line(&output.stdout)?;
        validate_temp_path(&path, prefix)?;

        let dir = Self { remote, path };
        if let Err(error) = dir.make_private().await {
            let _ = dir.remove().await;
            return Err(error);
        }
        Ok(dir)
    }

    /// Returns the remote directory path. Crate-private and never rendered.
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    /// Returns one direct child path after validating the file name.
    ///
    /// The caller still passes the returned path through every remote command as one
    /// quoted argv element.
    pub(crate) fn child(&self, name: &str) -> Result<String, SpiritError> {
        validate_child_name(name)?;
        Ok(format!("{}/{}", self.path, name))
    }

    /// Reads one bounded direct child file and verifies its byte count against `stat`.
    ///
    /// The size is checked before the read, so an oversized file is refused without a
    /// 272 MiB buffer. The read itself keeps the same hard cap. No error carries the
    /// remote path or the file bytes.
    pub(crate) async fn read_file(
        &self,
        path: &str,
        cap: u64,
        timeout: Duration,
    ) -> Result<Vec<u8>, SpiritError> {
        self.ensure_within(path)?;
        let stat_argv = vec![
            "stat".to_owned(),
            "-c".to_owned(),
            "%s".to_owned(),
            path.to_owned(),
        ];
        let output = run_checked(&self.remote, &stat_argv, READ_TIMEOUT, "stat").await?;
        let size = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<u64>()
            .map_err(|_| {
                SpiritError::new(
                    SpiritErrorCode::Parse,
                    "the remote ASIF file size is unreadable",
                )
            })?;
        if size > cap {
            return Err(SpiritError::transfer_too_large("the remote ASIF file", cap));
        }
        let cap_usize = usize::try_from(cap)
            .map_err(|_| SpiritError::transfer_too_large("the remote ASIF file", cap))?;
        let cat_argv = vec!["cat".to_owned(), path.to_owned()];
        let output = self
            .remote
            .run_with_cap(&cat_argv, timeout, cap_usize)
            .await?;
        if !output.success() {
            return Err(helper_failure("cat", output.status));
        }
        let actual = u64::try_from(output.stdout.len()).unwrap_or(u64::MAX);
        if actual != size {
            return Err(SpiritError::new(
                SpiritErrorCode::Remote,
                "the remote ASIF file changed size during the read",
            )
            .retryable()
            .with_hint("Retry the transfer; StackSpirit never keeps a truncated package."));
        }
        Ok(output.stdout)
    }

    /// Removes the private temp directory. The path was validated at creation time.
    pub(crate) async fn remove(self) -> Result<(), SpiritError> {
        let argv = vec![
            "rm".to_owned(),
            "-rf".to_owned(),
            "--".to_owned(),
            self.path.clone(),
        ];
        match self.remote.run_with_timeout(&argv, READ_TIMEOUT).await {
            Ok(output) if output.success() => Ok(()),
            Ok(output) => Err(cleanup_failure(Some(output.status))),
            Err(_) => Err(cleanup_failure(None)),
        }
    }

    async fn make_private(&self) -> Result<(), SpiritError> {
        let argv = vec!["chmod".to_owned(), "700".to_owned(), self.path.clone()];
        run_checked(&self.remote, &argv, READ_TIMEOUT, "chmod").await?;
        Ok(())
    }

    fn ensure_within(&self, path: &str) -> Result<(), SpiritError> {
        let prefix = format!("{}/", self.path);
        let Some(name) = path.strip_prefix(&prefix) else {
            return Err(SpiritError::new(
                SpiritErrorCode::Internal,
                "the remote file is outside the private temp directory",
            ));
        };
        validate_child_name(name)
    }
}

async fn run_checked(
    remote: &SshRemote,
    argv: &[String],
    timeout: Duration,
    label: &str,
) -> Result<CommandOutput, SpiritError> {
    let output = remote.run_with_timeout(argv, timeout).await?;
    if output.success() {
        Ok(output)
    } else {
        Err(helper_failure(label, output.status))
    }
}

fn helper_failure(label: &str, status: i32) -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Remote,
        format!("the remote ASIF helper step {label} exited {status}"),
    )
    .with_hint("Check the SSH connection and the host's VOWP installation, then retry.")
}

fn cleanup_failure(status: Option<i32>) -> SpiritError {
    let detail = status.map_or_else(
        || "the cleanup command did not finish".to_owned(),
        |status| format!("the cleanup command exited {status}"),
    );
    SpiritError::new(
        SpiritErrorCode::Remote,
        format!("the remote ASIF temp directory could not be removed: {detail}"),
    )
    .with_hint("Remove the leftover private temp directory on the VOWP host.")
}

fn parse_single_line(stdout: &[u8]) -> Result<String, SpiritError> {
    let text = String::from_utf8_lossy(stdout);
    let path = text.trim_end_matches(['\n', '\r']);
    if path.is_empty() || path.contains('\n') || path.contains('\r') {
        return Err(SpiritError::invalid_input(
            "remote temp directory",
            "did not return one safe path line",
        ));
    }
    Ok(path.to_owned())
}

fn validate_prefix(prefix: &str) -> Result<(), SpiritError> {
    if prefix.is_empty()
        || prefix.len() > 64
        || !prefix
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return Err(SpiritError::invalid_input(
            "remote temp prefix",
            "must be 1-64 ASCII letters, digits, '-' or '_'",
        ));
    }
    Ok(())
}

/// Validates the path `mktemp` returned before any command uses or removes it.
fn validate_temp_path(path: &str, prefix: &str) -> Result<(), SpiritError> {
    let invalid = || {
        SpiritError::invalid_input(
            "remote temp directory",
            "did not return a safe private temp path",
        )
    };
    if path.is_empty()
        || path.len() > MAX_REMOTE_PATH
        || !path.starts_with('/')
        || path.ends_with('/')
        || path.contains("//")
        || path.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(invalid());
    }
    let mut components = path.split('/');
    // A leading '/' produces an empty first component.
    if components.next() != Some("") {
        return Err(invalid());
    }
    let mut last = None;
    for component in components {
        if component.is_empty() || component == "." || component == ".." {
            return Err(invalid());
        }
        last = Some(component);
    }
    let Some(name) = last else {
        return Err(invalid());
    };
    if !name.starts_with(prefix)
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        return Err(invalid());
    }
    Ok(())
}

fn validate_child_name(name: &str) -> Result<(), SpiritError> {
    if name.is_empty()
        || name.len() > 255
        || name == "."
        || name == ".."
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        return Err(SpiritError::invalid_input(
            "remote file name",
            "must be one safe path component",
        ));
    }
    Ok(())
}

/// Replaces a known remote path in an error message and hint.
///
/// This is the final guard that keeps the remote temp path out of every typed error,
/// even when VOWP echoes the `--out` argument in its own error message.
pub(crate) fn scrub_paths(mut error: SpiritError, paths: &[&str]) -> SpiritError {
    for path in paths {
        // A single-character path such as `/` would mangle every separator in the
        // message; there is no useful remote temp path that short.
        if path.len() < 2 {
            continue;
        }
        error.message = error.message.replace(path, "[remote-path]");
        if let Some(hint) = &mut error.hint {
            *hint = hint.replace(path, "[remote-path]");
        }
    }
    error
}
