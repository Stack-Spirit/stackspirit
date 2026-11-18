//! One-time-password onboarding: install one local public key with a single password
//! login, verify the key, then forget the password.
//!
//! The password never enters argv or the process environment. A generated
//! `SSH_ASKPASS` helper reads it from a `0600` file inside a private `0700` temp
//! directory; only the helper *path* is exported. The directory is removed on every
//! exit path, including cancellation and early errors.
//!
//! The first hop relaxes `BatchMode` only to let the askpass helper answer the
//! password prompt. It still uses the app-private `known_hosts`, strict host-key
//! checking, and a fingerprint scan against the confirmed [`HostKeyPin`] before any
//! password byte is written to disk. Every later hop is key-only and `BatchMode=yes`.
//!
//! Optional `PasswordAuthentication no` is applied only with proven passwordless
//! sudo, validated by `sshd -t` **and** the effective `sshd -T` output, and reverted
//! when anything fails. A sudo password is never requested or accepted.

use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::engine::keychain::SecretString;
use crate::error::{SpiritError, SpiritErrorCode};
use crate::host::profile::HostKeyPin;
use crate::ssh::command::{
    CommandOutput, CommandSpec, READ_TIMEOUT, WRITE_TIMEOUT, quote_arg, remote_command,
};
use crate::ssh::fingerprint::{parse_fingerprint_output, scan_host_keys};
use crate::ssh::known_hosts::upsert_pinned_key;
use crate::ssh::{CommandRunner, SshOptions, SshTarget};

/// Longest one-time password this build accepts.
const MAX_PASSWORD_BYTES: usize = 4096;
/// Largest `.pub` file or public-key line this build will read.
const MAX_PUBLIC_KEY_LINE_BYTES: usize = 16 * 1024;
/// [`MAX_PUBLIC_KEY_LINE_BYTES`] as the file metadata width.
const MAX_PUBLIC_KEY_BYTES: u64 = 16 * 1024;
/// Bare public-key algorithms accepted for install.
const PUBLIC_KEY_ALGORITHMS: [&str; 7] = [
    "ssh-ed25519",
    "ssh-rsa",
    "ecdsa-sha2-nistp256",
    "ecdsa-sha2-nistp384",
    "ecdsa-sha2-nistp521",
    "sk-ssh-ed25519@openssh.com",
    "sk-ecdsa-sha2-nistp256@openssh.com",
];
/// App-owned drop-in file. StackSpirit writes and removes exactly this path.
const DROP_IN_PATH: &str = "/etc/ssh/sshd_config.d/99-stackspirit-password-auth.conf";
/// Contents of the drop-in. Fixed; never built from user input.
const DROP_IN_CONTENT: &str = "PasswordAuthentication no\n";
/// Fixed remote install script. The one key line arrives on stdin.
///
/// It is the shell form of [`merge_authorized_keys`]: ensure `~/.ssh` is `700` and
/// `authorized_keys` is `600`, then append the exact line only when `grep -qxF`
/// says it is absent. No user text is interpolated into the script; the key line
/// is read from stdin and passed to `grep` and `printf` as one quoted argument.
const INSTALL_AUTHORIZED_KEY: &str = "set -eu; umask 077; mkdir -p \"$HOME/.ssh\"; chmod 700 \"$HOME/.ssh\"; file=\"$HOME/.ssh/authorized_keys\"; touch -- \"$file\"; chmod 600 -- \"$file\"; key=$(cat); [ -n \"$key\" ] || exit 1; if grep -qxF -- \"$key\" \"$file\"; then exit 0; fi; if [ -s \"$file\" ] && [ -n \"$(tail -c 1 \"$file\")\" ]; then printf '\\n' >> \"$file\"; fi; printf '%s\\n' \"$key\" >> \"$file\"";

/// A request to install one existing local public key with a one-time password.
pub struct PasswordInstallRequest {
    /// SSH destination for the server being onboarded.
    pub ssh: SshTarget,
    /// The host-key pin the user already confirmed out of band.
    pub host_key: HostKeyPin,
    /// One-time login password. Zeroized on drop; never written outside the askpass
    /// temp directory, never logged and never serialized.
    pub password: SecretString,
    /// Local private-key path. The public half is read from `<path>.pub`.
    pub private_key_path: PathBuf,
    /// Ask for `PasswordAuthentication no` when the server supports a safe apply.
    pub disable_password_auth: bool,
}

impl PasswordInstallRequest {
    /// Validates local fields without touching the network or reading a key.
    pub fn validate(&self) -> Result<(), SpiritError> {
        self.ssh.validate()?;
        self.host_key.validate()?;
        if self.password.is_empty() {
            return Err(SpiritError::invalid_input(
                "password",
                "the one-time password must not be empty",
            ));
        }
        if self.password.expose().len() > MAX_PASSWORD_BYTES {
            return Err(SpiritError::invalid_input(
                "password",
                "the one-time password is unexpectedly long",
            ));
        }
        if self.password.expose().contains(['\n', '\r', '\u{0}']) {
            return Err(SpiritError::invalid_input(
                "password",
                "the one-time password must not contain a line break or NUL",
            ));
        }
        if self.private_key_path.as_os_str().is_empty() {
            return Err(SpiritError::invalid_input(
                "private_key_path",
                "must name the local private key",
            ));
        }
        Ok(())
    }
}

impl fmt::Debug for PasswordInstallRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PasswordInstallRequest")
            .field("ssh", &self.ssh)
            .field("host_key", &self.host_key)
            .field("password", &"<redacted>")
            .field("private_key_path", &self.private_key_path)
            .field("disable_password_auth", &self.disable_password_auth)
            .finish()
    }
}

/// What happened to the optional password-login step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PasswordAuthOutcome {
    /// The caller did not ask to disable password login.
    NotRequested,
    /// The drop-in is effective (`sshd -T`) and sshd reloaded successfully.
    Disabled,
    /// The server lacks a safe way to apply the change. Nothing was left changed.
    NotSupported {
        /// Why the step was skipped.
        reason: PasswordAuthUnsupported,
    },
    /// The change was attempted and failed. The drop-in was removed again when
    /// the revert itself succeeded; `detail` says when it did not.
    Failed {
        /// Safe exit-status detail; never command output or a secret.
        detail: String,
    },
}

/// A typed reason why `PasswordAuthentication no` was not applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PasswordAuthUnsupported {
    /// `sudo` is not installed or could not start.
    SudoUnavailable,
    /// `sudo -n true` failed; the account has no proven passwordless sudo.
    NoPasswordlessSudo,
    /// `sshd` is not installed or not on the root `PATH`.
    SshdUnavailable,
    /// `/etc/ssh/sshd_config.d` is missing, so a drop-in has no safe home.
    DropInUnsupported,
    /// `sshd -T` does not show `PasswordAuthentication no`; the drop-in is ignored.
    DropInIgnored,
    /// No supported unit could reload sshd.
    ReloadUnavailable,
}

/// The result of one successful onboarding. The password is already gone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasswordInstallOutcome {
    /// True only after a fresh `BatchMode=yes`, key-only login ran `true`.
    pub installed: bool,
    /// `SHA256:...` fingerprint of the installed public key.
    pub fingerprint: String,
    /// True only when the optional drop-in is effective and sshd reloaded.
    pub password_login_disabled: bool,
    /// Detail of the optional password-login step.
    pub password_auth: PasswordAuthOutcome,
}

/// The merged `authorized_keys` content and whether anything was added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedKeysMerge {
    /// Merged content. Every existing byte is preserved exactly.
    pub content: String,
    /// True when the key line was appended.
    pub changed: bool,
}

/// Installs one existing local public key with a one-time password.
///
/// The install runs in this order:
/// 1. read and validate `<private_key_path>.pub` (no network yet);
/// 2. scan the host and require the confirmed [`HostKeyPin`] before any password
///    material is written to disk;
/// 3. pin exactly the confirmed key in the app-private `known_hosts`;
/// 4. log in once with a generated askpass helper and merge the key into
///    `~/.ssh/authorized_keys` with `700`/`600` permissions;
/// 5. verify a fresh key-only `BatchMode=yes` login;
/// 6. optionally apply and validate the password-login drop-in.
///
/// A failure in step 4 or 5 is returned as a typed error and never as
/// `installed: true`. Some stderr text may be included, so the final error is
/// scrubbed for the password before it is returned.
pub async fn install_public_key_with_password(
    runner: Arc<dyn CommandRunner>,
    request: &PasswordInstallRequest,
    options: &SshOptions,
) -> Result<PasswordInstallOutcome, SpiritError> {
    install_inner(runner.as_ref(), request, options)
        .await
        .map_err(|error| scrub_password(error, request.password.expose()))
}

async fn install_inner(
    runner: &dyn CommandRunner,
    request: &PasswordInstallRequest,
    options: &SshOptions,
) -> Result<PasswordInstallOutcome, SpiritError> {
    request.validate()?;
    let private_key = fs::metadata(&request.private_key_path).map_err(|error| {
        SpiritError::io("reading the private key path", &error)
            .with_hint("Keep the private half next to the .pub file; StackSpirit never copies it.")
    })?;
    if !private_key.is_file() {
        return Err(SpiritError::invalid_input(
            "private_key_path",
            "must name the local private key file",
        ));
    }
    let public_key = read_public_key(&request.private_key_path)?;
    let fingerprint = fingerprint_public_key(runner, &public_key.line).await?;
    pin_host_key(runner, request, options).await?;

    // The askpass directory lives only for the one password hop; dropping it before
    // verification also means a slow sudo step cannot keep the secret on disk.
    {
        let session = AskpassSession::create(request.password.expose())?;
        let bootstrap = RemoteSsh {
            runner,
            target: &request.ssh,
            options,
            auth: SshAuth::Password {
                askpass: &session.helper,
            },
        };
        install_authorized_key(&bootstrap, &public_key.line).await?;
    }

    let key_auth = RemoteSsh {
        runner,
        target: &request.ssh,
        options,
        auth: SshAuth::PublicKey {
            identity: &request.private_key_path,
        },
    };
    verify_key_login(&key_auth).await?;

    let password_auth = if request.disable_password_auth {
        disable_password_auth(&key_auth).await
    } else {
        PasswordAuthOutcome::NotRequested
    };

    Ok(PasswordInstallOutcome {
        installed: true,
        fingerprint,
        password_login_disabled: password_auth == PasswordAuthOutcome::Disabled,
        password_auth,
    })
}

/// A validated public-key line.
struct PublicKeyLine {
    line: String,
}

fn read_public_key(private_key_path: &Path) -> Result<PublicKeyLine, SpiritError> {
    let mut public_key_path = private_key_path.as_os_str().to_owned();
    public_key_path.push(".pub");
    let public_key_path = PathBuf::from(public_key_path);
    let metadata = fs::metadata(&public_key_path).map_err(|error| {
        SpiritError::io("reading the public key next to the private key", &error)
            .with_hint("Create the pair with ssh-keygen first; StackSpirit reads only <path>.pub.")
    })?;
    if !metadata.is_file() {
        return Err(SpiritError::invalid_input(
            "public_key",
            "the .pub path must be a regular file",
        ));
    }
    if metadata.len() > MAX_PUBLIC_KEY_BYTES {
        return Err(SpiritError::invalid_input(
            "public_key",
            "the public key file is unexpectedly large",
        ));
    }
    let bytes = fs::read(&public_key_path)
        .map_err(|error| SpiritError::io("reading the public key", &error))?;
    let text = std::str::from_utf8(&bytes).map_err(|_| {
        SpiritError::invalid_input("public_key", "the public key file is not UTF-8 text")
    })?;
    let line = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .or_else(|| text.strip_suffix('\r'))
        .unwrap_or(text);
    validate_public_key_line(line)?;
    Ok(PublicKeyLine {
        line: line.to_owned(),
    })
}

/// Refuses anything that is not one bare OpenSSH public-key line.
///
/// `authorized_keys` options such as `from="..."` are refused on purpose: this API
/// installs exactly the local key, not a policy line.
fn validate_public_key_line(line: &str) -> Result<(), SpiritError> {
    let invalid = |detail: &str| SpiritError::invalid_input("public_key", detail);
    if line.is_empty() {
        return Err(invalid("the public key line is empty"));
    }
    if line.len() > MAX_PUBLIC_KEY_LINE_BYTES {
        return Err(invalid("the public key line is unexpectedly long"));
    }
    if line.chars().next().is_some_and(char::is_whitespace) {
        return Err(invalid(
            "the public key line must not start with whitespace",
        ));
    }
    if line.contains(['\n', '\r', '\u{0}']) {
        return Err(invalid(
            "the public key line must be exactly one line without NUL",
        ));
    }
    if line.chars().any(|c| c.is_control() && c != '\t') {
        return Err(invalid("the public key line contains a control character"));
    }
    let mut parts = line.split_whitespace();
    let Some(algorithm) = parts.next() else {
        return Err(invalid("the public key line is empty"));
    };
    let Some(body) = parts.next() else {
        return Err(invalid("the public key line has no key body"));
    };
    if !PUBLIC_KEY_ALGORITHMS.contains(&algorithm) {
        return Err(invalid(
            "only a bare ssh-ed25519, ssh-rsa, ecdsa-* or sk-* key is accepted",
        ));
    }
    if !looks_like_base64(body) {
        return Err(invalid("the public key body is not valid base64"));
    }
    Ok(())
}

fn looks_like_base64(body: &str) -> bool {
    if body.is_empty() || body.len() > 8192 || body.len() % 4 != 0 {
        return false;
    }
    let unpadded = body.trim_end_matches('=');
    let padding = body.len() - unpadded.len();
    if padding > 2 || unpadded.contains('=') {
        return false;
    }
    unpadded
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/'))
}

async fn fingerprint_public_key(
    runner: &dyn CommandRunner,
    line: &str,
) -> Result<String, SpiritError> {
    let spec = CommandSpec::new("ssh-keygen", vec!["-lf".to_owned(), "-".to_owned()])
        .with_stdin(format!("{line}\n").into_bytes());
    let output = runner.run(&spec).await?;
    if !output.success() {
        return Err(SpiritError::invalid_input(
            "public_key",
            "ssh-keygen could not parse the public key",
        ));
    }
    parse_fingerprint_output(&output.stdout_text())
        .map(|(sha256, _algorithm)| sha256)
        .ok_or_else(|| {
            SpiritError::invalid_input("public_key", "ssh-keygen returned no SHA256 fingerprint")
        })
}

async fn pin_host_key(
    runner: &dyn CommandRunner,
    request: &PasswordInstallRequest,
    options: &SshOptions,
) -> Result<(), SpiritError> {
    let scan_timeout = Duration::from_secs(options.connect_timeout_secs.max(1));
    let scan = scan_host_keys(runner, &request.ssh, scan_timeout).await?;
    crate::host::profile::verify_pin_against_scan(&request.host_key, &scan.keys)?;
    let confirmed = scan
        .keys
        .iter()
        .find(|key| key.sha256 == request.host_key.sha256)
        .ok_or_else(|| {
            SpiritError::new(
                SpiritErrorCode::Internal,
                "the confirmed host key disappeared from the scan",
            )
        })?;
    upsert_pinned_key(&options.known_hosts, &request.ssh, confirmed)
}

async fn install_authorized_key(remote: &RemoteSsh<'_>, key_line: &str) -> Result<(), SpiritError> {
    // The empty-file merge is exactly the unit the remote guard receives: one
    // validated line plus one trailing newline. The guard does the real merge
    // against whatever the file already holds.
    let unit = merge_authorized_keys("", key_line)?;
    let output = remote
        .run_with_stdin(
            &shell_argv(INSTALL_AUTHORIZED_KEY),
            unit.content.into_bytes(),
            WRITE_TIMEOUT,
        )
        .await?;
    if !output.success() {
        return Err(bootstrap_failure(&output, "installing the public key"));
    }
    Ok(())
}

/// The pure reference for the guarded `authorized_keys` append.
///
/// The remote bootstrap script implements the same rule with `grep -qxF`, so a
/// retry never adds a duplicate and the file is never rewritten. Existing bytes
/// are preserved, including comments, blank lines and a missing final newline.
/// The comparison is exact: a changed comment or spacing is a different line.
/// A malformed key line is refused here, before any connection.
pub fn merge_authorized_keys(
    existing: &str,
    key_line: &str,
) -> Result<AuthorizedKeysMerge, SpiritError> {
    validate_public_key_line(key_line)?;
    let present = existing
        .lines()
        .any(|line| line.strip_suffix('\r').unwrap_or(line) == key_line);
    if present {
        return Ok(AuthorizedKeysMerge {
            content: existing.to_owned(),
            changed: false,
        });
    }
    let mut content = String::with_capacity(existing.len() + key_line.len() + 2);
    content.push_str(existing);
    if !existing.is_empty() && !existing.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(key_line);
    content.push('\n');
    Ok(AuthorizedKeysMerge {
        content,
        changed: true,
    })
}

async fn verify_key_login(remote: &RemoteSsh<'_>) -> Result<(), SpiritError> {
    let output = remote.run(&["true".to_owned()], READ_TIMEOUT).await?;
    if output.success() {
        return Ok(());
    }
    if output
        .stderr_text()
        .contains("Host key verification failed")
    {
        return Err(SpiritError::new(
            SpiritErrorCode::HostKeyMismatch,
            "host key verification failed on the key-only verification hop",
        )
        .with_hint("Compare the fingerprint out of band and explicitly re-pin the host."));
    }
    Err(SpiritError::new(
        SpiritErrorCode::SshAuth,
        "the public key was installed but the key-only login did not verify; it is not reported as installed",
    )
    .with_hint("Check the local key pair and the server's AuthorizedKeysFile setting, then retry."))
}

fn bootstrap_failure(output: &CommandOutput, action: &str) -> SpiritError {
    let stderr = output.stderr_text();
    if stderr.contains("Host key verification failed") {
        return SpiritError::new(
            SpiritErrorCode::HostKeyMismatch,
            "host key verification failed",
        )
        .with_hint("Compare the fingerprint out of band and explicitly re-pin the host.");
    }
    if stderr.contains("Permission denied") || stderr.contains("Authentication failed") {
        return SpiritError::new(
            SpiritErrorCode::SshAuth,
            "SSH password authentication failed",
        )
        .with_hint("Check the one-time password, then retry. It is never saved.");
    }
    SpiritError::new(
        SpiritErrorCode::Remote,
        format!(
            "{action} failed with exit {}: {}",
            output.status,
            first_line(&stderr)
        ),
    )
    .retryable()
}

fn first_line(input: &str) -> String {
    input
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(200)
        .collect()
}

fn shell_argv(script: &str) -> Vec<String> {
    vec!["sh".to_owned(), "-c".to_owned(), script.to_owned()]
}

/// The two credential modes this module is allowed to use.
#[derive(Clone, Copy)]
enum SshAuth<'a> {
    Password { askpass: &'a Path },
    PublicKey { identity: &'a Path },
}

struct RemoteSsh<'a> {
    runner: &'a dyn CommandRunner,
    target: &'a SshTarget,
    options: &'a SshOptions,
    auth: SshAuth<'a>,
}

impl RemoteSsh<'_> {
    async fn run(
        &self,
        remote_argv: &[String],
        timeout: Duration,
    ) -> Result<CommandOutput, SpiritError> {
        let spec = self.spec(remote_argv, timeout)?;
        self.runner.run(&spec).await
    }

    async fn run_with_stdin(
        &self,
        remote_argv: &[String],
        stdin: Vec<u8>,
        timeout: Duration,
    ) -> Result<CommandOutput, SpiritError> {
        let spec = self.spec(remote_argv, timeout)?.with_stdin(stdin);
        self.runner.run(&spec).await
    }

    fn spec(&self, remote_argv: &[String], timeout: Duration) -> Result<CommandSpec, SpiritError> {
        self.target.validate()?;
        let mut args = vec![
            "-o".to_owned(),
            "StrictHostKeyChecking=yes".to_owned(),
            "-o".to_owned(),
            format!("UserKnownHostsFile={}", self.options.known_hosts.display()),
            "-o".to_owned(),
            format!("ConnectTimeout={}", self.options.connect_timeout_secs),
            "-o".to_owned(),
            format!(
                "ServerAliveInterval={}",
                self.options.server_alive_interval_secs
            ),
            "-o".to_owned(),
            format!(
                "ServerAliveCountMax={}",
                self.options.server_alive_count_max
            ),
        ];
        let mut env = Vec::new();
        match self.auth {
            SshAuth::Password { askpass } => {
                let askpass = askpass.to_str().ok_or_else(|| {
                    SpiritError::invalid_input("askpass helper", "the path must be UTF-8")
                })?;
                args.extend([
                    "-o".to_owned(),
                    "BatchMode=no".to_owned(),
                    "-o".to_owned(),
                    "PreferredAuthentications=password,keyboard-interactive".to_owned(),
                    "-o".to_owned(),
                    "PubkeyAuthentication=no".to_owned(),
                    "-o".to_owned(),
                    "NumberOfPasswordPrompts=1".to_owned(),
                ]);
                env.push(("SSH_ASKPASS".to_owned(), askpass.to_owned()));
                env.push(("SSH_ASKPASS_REQUIRE".to_owned(), "force".to_owned()));
            }
            SshAuth::PublicKey { identity } => {
                args.extend([
                    "-o".to_owned(),
                    "BatchMode=yes".to_owned(),
                    "-o".to_owned(),
                    "PreferredAuthentications=publickey".to_owned(),
                    "-i".to_owned(),
                    identity.display().to_string(),
                    "-o".to_owned(),
                    "IdentitiesOnly=yes".to_owned(),
                ]);
            }
        }
        args.push("-p".to_owned());
        args.push(self.target.port.to_string());
        args.push("--".to_owned());
        args.push(self.target.destination());
        args.push(remote_command(remote_argv)?);
        let mut spec = CommandSpec::new("ssh", args).with_timeout(timeout);
        for (key, value) in env {
            spec = spec.with_env(key, value);
        }
        Ok(spec)
    }
}

struct AskpassSession {
    dir: PathBuf,
    helper: PathBuf,
    secret: PathBuf,
}

impl AskpassSession {
    fn create(password: &str) -> Result<Self, SpiritError> {
        let dir = create_private_temp_dir()?;
        let session = Self {
            helper: dir.join("askpass.sh"),
            secret: dir.join("password"),
            dir,
        };
        session.write_secret(password)?;
        session.write_helper()?;
        Ok(session)
    }

    fn write_secret(&self, password: &str) -> Result<(), SpiritError> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&self.secret)
            .map_err(|error| SpiritError::io("creating the askpass secret", &error))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|error| SpiritError::io("securing the askpass secret", &error))?;
        }
        file.write_all(password.as_bytes())
            .map_err(|error| SpiritError::io("writing the askpass secret", &error))?;
        file.sync_all()
            .map_err(|error| SpiritError::io("syncing the askpass secret", &error))?;
        Ok(())
    }

    fn write_helper(&self) -> Result<(), SpiritError> {
        let secret = self.secret.to_str().ok_or_else(|| {
            SpiritError::invalid_input("askpass helper", "the path must be UTF-8")
        })?;
        // The trailing newline is the askpass line terminator; the password itself
        // is read from the exact bytes in the secret file.
        let script = format!("#!/bin/sh\ncat -- {}\nprintf '\\n'\n", quote_arg(secret)?);
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o700);
        }
        let mut file = options
            .open(&self.helper)
            .map_err(|error| SpiritError::io("creating the askpass helper", &error))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o700))
                .map_err(|error| SpiritError::io("securing the askpass helper", &error))?;
        }
        file.write_all(script.as_bytes())
            .map_err(|error| SpiritError::io("writing the askpass helper", &error))?;
        file.sync_all()
            .map_err(|error| SpiritError::io("syncing the askpass helper", &error))?;
        Ok(())
    }
}

impl Drop for AskpassSession {
    fn drop(&mut self) {
        zeroize_file(&self.secret);
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[cfg(unix)]
fn create_private_temp_dir() -> Result<PathBuf, SpiritError> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    let base = std::env::temp_dir();
    for _ in 0..8 {
        let dir = base.join(format!("stackspirit-ssh-{}", ulid::Ulid::generate()));
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        match builder.create(&dir) {
            Ok(()) => {
                if let Err(error) = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)) {
                    let _ = fs::remove_dir(&dir);
                    return Err(SpiritError::io("securing the askpass directory", &error));
                }
                return Ok(dir);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(SpiritError::io("creating the askpass directory", &error));
            }
        }
    }
    Err(SpiritError::new(
        SpiritErrorCode::Internal,
        "could not create a unique askpass directory",
    ))
}

#[cfg(not(unix))]
fn create_private_temp_dir() -> Result<PathBuf, SpiritError> {
    Err(SpiritError::new(
        SpiritErrorCode::SshUnavailable,
        "one-time-password SSH onboarding needs a POSIX OpenSSH client",
    )
    .with_hint("Use a key-based host profile on this platform."))
}

fn zeroize_file(path: &Path) {
    let Ok(mut file) = fs::OpenOptions::new().write(true).open(path) else {
        return;
    };
    let Ok(metadata) = file.metadata() else {
        return;
    };
    let Ok(len) = usize::try_from(metadata.len()) else {
        return;
    };
    if len > 64 * 1024 {
        return;
    }
    let zeros = vec![0_u8; len];
    if file.write_all(&zeros).is_ok() {
        let _ = file.sync_all();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PasswordAuthFailure {
    NotSupported(PasswordAuthUnsupported),
    Failed(&'static str),
}

async fn disable_password_auth(remote: &RemoteSsh<'_>) -> PasswordAuthOutcome {
    match apply_password_auth_drop_in(remote).await {
        Ok(()) => PasswordAuthOutcome::Disabled,
        Err(PasswordAuthFailure::NotSupported(reason)) => {
            PasswordAuthOutcome::NotSupported { reason }
        }
        Err(PasswordAuthFailure::Failed(detail)) => PasswordAuthOutcome::Failed {
            detail: detail.to_owned(),
        },
    }
}

async fn apply_password_auth_drop_in(remote: &RemoteSsh<'_>) -> Result<(), PasswordAuthFailure> {
    let sudo = sudo_run(remote, &["true"]).await?;
    if !sudo.success() {
        return Err(if sudo.status == 127 {
            PasswordAuthFailure::NotSupported(PasswordAuthUnsupported::SudoUnavailable)
        } else {
            PasswordAuthFailure::NotSupported(PasswordAuthUnsupported::NoPasswordlessSudo)
        });
    }

    let baseline = sudo_run(remote, &["sshd", "-t"]).await?;
    if !baseline.success() {
        return Err(if baseline.status == 127 {
            PasswordAuthFailure::NotSupported(PasswordAuthUnsupported::SshdUnavailable)
        } else {
            PasswordAuthFailure::Failed("the existing sshd configuration does not pass sshd -t")
        });
    }

    let drop_in_dir = sudo_run(remote, &["test", "-d", "/etc/ssh/sshd_config.d"]).await?;
    if !drop_in_dir.success() {
        return Err(PasswordAuthFailure::NotSupported(
            PasswordAuthUnsupported::DropInUnsupported,
        ));
    }

    let write =
        sudo_run_with_stdin(remote, &["tee", DROP_IN_PATH], DROP_IN_CONTENT.as_bytes()).await?;
    if !write.success() {
        let reverted = revert_drop_in(remote).await;
        return Err(PasswordAuthFailure::Failed(if reverted {
            "writing the sshd drop-in failed"
        } else {
            "writing the sshd drop-in failed and the partial file could not be reverted"
        }));
    }

    match validate_and_reload(remote).await {
        Ok(()) => Ok(()),
        Err(failure) => {
            let reverted = revert_drop_in(remote).await;
            if !reverted {
                return Err(PasswordAuthFailure::Failed(match failure {
                    PasswordAuthFailure::NotSupported(_) => {
                        "the sshd change was not applied and the drop-in could not be reverted cleanly"
                    }
                    PasswordAuthFailure::Failed(_) => {
                        "the sshd change failed and the drop-in could not be reverted cleanly"
                    }
                }));
            }
            Err(failure)
        }
    }
}

async fn validate_and_reload(remote: &RemoteSsh<'_>) -> Result<(), PasswordAuthFailure> {
    let checked = sudo_run(remote, &["sshd", "-t"]).await?;
    if !checked.success() {
        return Err(if checked.status == 127 {
            PasswordAuthFailure::NotSupported(PasswordAuthUnsupported::SshdUnavailable)
        } else {
            PasswordAuthFailure::Failed("sshd -t rejected the drop-in")
        });
    }
    let effective = sudo_run(remote, &["sshd", "-T"]).await?;
    if !effective.success() {
        return Err(PasswordAuthFailure::NotSupported(
            PasswordAuthUnsupported::SshdUnavailable,
        ));
    }
    if !password_auth_disabled_in_effective_config(&effective.stdout_text()) {
        return Err(PasswordAuthFailure::NotSupported(
            PasswordAuthUnsupported::DropInIgnored,
        ));
    }
    match reload_sshd(remote).await {
        ReloadOutcome::Reloaded => Ok(()),
        ReloadOutcome::Unavailable => Err(PasswordAuthFailure::NotSupported(
            PasswordAuthUnsupported::ReloadUnavailable,
        )),
        ReloadOutcome::Failed(detail) => Err(PasswordAuthFailure::Failed(detail)),
    }
}

fn password_auth_disabled_in_effective_config(text: &str) -> bool {
    text.lines().any(|line| {
        let mut parts = line.split_whitespace();
        matches!(
            parts.next().map(str::to_ascii_lowercase).as_deref(),
            Some("passwordauthentication")
        ) && matches!(
            parts.next().map(str::to_ascii_lowercase).as_deref(),
            Some("no")
        )
    })
}

enum ReloadOutcome {
    Reloaded,
    Unavailable,
    Failed(&'static str),
}

async fn reload_sshd(remote: &RemoteSsh<'_>) -> ReloadOutcome {
    let first = match sudo_run(remote, &["systemctl", "reload", "sshd"]).await {
        Ok(output) => output,
        Err(_) => return ReloadOutcome::Failed("the sshd reload command did not run"),
    };
    if first.success() {
        return ReloadOutcome::Reloaded;
    }
    let second = match sudo_run(remote, &["systemctl", "reload", "ssh"]).await {
        Ok(output) => output,
        Err(_) => return ReloadOutcome::Failed("the sshd reload command did not run"),
    };
    if second.success() {
        return ReloadOutcome::Reloaded;
    }
    if first.status == 127 && second.status == 127 {
        ReloadOutcome::Unavailable
    } else {
        ReloadOutcome::Failed("neither ssh systemd unit could be reloaded")
    }
}

async fn revert_drop_in(remote: &RemoteSsh<'_>) -> bool {
    let removed = sudo_run(remote, &["rm", "-f", DROP_IN_PATH])
        .await
        .is_ok_and(|output| output.success());
    if !removed {
        return false;
    }
    let validated = sudo_run(remote, &["sshd", "-t"])
        .await
        .is_ok_and(|output| output.success());
    if validated {
        // A reload that never happened in the failed apply cannot have loaded the
        // removed file; this attempt only helps a daemon that did load it.
        let _ = reload_sshd(remote).await;
    }
    validated
}

async fn sudo_run(
    remote: &RemoteSsh<'_>,
    args: &[&str],
) -> Result<CommandOutput, PasswordAuthFailure> {
    let argv = sudo_argv(args);
    remote
        .run(&argv, WRITE_TIMEOUT)
        .await
        .map_err(|_| PasswordAuthFailure::Failed("a sudo step could not start"))
}

async fn sudo_run_with_stdin(
    remote: &RemoteSsh<'_>,
    args: &[&str],
    stdin: &[u8],
) -> Result<CommandOutput, PasswordAuthFailure> {
    let argv = sudo_argv(args);
    remote
        .run_with_stdin(&argv, stdin.to_vec(), WRITE_TIMEOUT)
        .await
        .map_err(|_| PasswordAuthFailure::Failed("a sudo step could not start"))
}

fn sudo_argv(args: &[&str]) -> Vec<String> {
    let mut argv = vec!["sudo".to_owned(), "-n".to_owned()];
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    argv
}

fn scrub_password(mut error: SpiritError, secret: &str) -> SpiritError {
    if secret.is_empty() {
        return error;
    }
    error.message = error.message.replace(secret, "[redacted]");
    if let Some(hint) = &mut error.hint {
        *hint = hint.replace(secret, "[redacted]");
    }
    error
}
