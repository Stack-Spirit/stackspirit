//! `ssh-keyscan` / `ssh-keygen` helpers that produce SHA-256 fingerprints.

use serde::{Deserialize, Serialize};

use crate::error::SpiritError;
use crate::ssh::SshTarget;
use crate::ssh::command::{CommandSpec, READ_TIMEOUT};
use crate::ssh::runner::CommandRunner;
use crate::time::Timestamp;

/// One host key offered by a server, with the fingerprint the user must confirm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScannedHostKey {
    /// SSH key algorithm, for example `ssh-ed25519`.
    pub algorithm: String,
    /// `SHA256:...` exactly as `ssh-keygen -lf` prints it.
    pub sha256: String,
    /// The base64 public key body, written to the app-private `known_hosts`.
    pub public_key_base64: String,
    /// True for `ssh-ed25519`, the preferred algorithm.
    pub preferred: bool,
}

/// The result of scanning all offered host keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostKeyScan {
    /// Offered keys, preferred algorithm first.
    pub keys: Vec<ScannedHostKey>,
    /// When the scan ran.
    pub scanned_at: Timestamp,
}

/// Parses `ssh-keyscan` output into `(algorithm, base64)` pairs.
#[must_use]
pub fn parse_keyscan_output(input: &str) -> Vec<(String, String)> {
    let mut keys = Vec::new();
    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let _host = parts.next();
        let Some(algorithm) = parts.next() else {
            continue;
        };
        let Some(public_key_base64) = parts.next() else {
            continue;
        };
        keys.push((algorithm.to_owned(), public_key_base64.to_owned()));
    }
    keys
}

/// Extracts `(sha256, algorithm)` from one `ssh-keygen -lf` line.
#[must_use]
pub fn parse_fingerprint_output(input: &str) -> Option<(String, String)> {
    let line = input.lines().next()?.trim();
    let sha256 = line
        .split_whitespace()
        .find(|token| token.starts_with("SHA256:"))?
        .to_owned();
    let algorithm = line
        .rsplit_once('(')
        .and_then(|(_, rest)| rest.strip_suffix(')'))
        .map(|name| match name.to_ascii_lowercase().as_str() {
            "ed25519" => "ssh-ed25519".to_owned(),
            "rsa" => "ssh-rsa".to_owned(),
            "ecdsa" => "ecdsa-sha2-nistp256".to_owned(),
            other => other.to_owned(),
        })
        .unwrap_or_else(|| "unknown".to_owned());
    Some((sha256, algorithm))
}

/// Runs `ssh-keyscan` and then `ssh-keygen -lf` for every offered key.
pub async fn scan_host_keys<R: CommandRunner + ?Sized>(
    runner: &R,
    target: &SshTarget,
    scan_timeout: std::time::Duration,
) -> Result<HostKeyScan, SpiritError> {
    target.validate()?;
    let keyscan = CommandSpec::new(
        "ssh-keyscan",
        vec![
            "-T".to_owned(),
            scan_timeout.as_secs().max(1).to_string(),
            "-p".to_owned(),
            target.port.to_string(),
            target.host.clone(),
        ],
    )
    .with_timeout(scan_timeout);
    let output = runner.run(&keyscan).await?;
    if !output.success() {
        return Err(SpiritError::new(
            crate::error::SpiritErrorCode::Remote,
            format!("ssh-keyscan failed: {}", output.stderr_text()),
        )
        .retryable());
    }
    let parsed = parse_keyscan_output(&output.stdout_text());
    if parsed.is_empty() {
        return Err(SpiritError::new(
            crate::error::SpiritErrorCode::Remote,
            "ssh-keyscan returned no host keys",
        )
        .retryable()
        .with_hint("Check the host name and port, then retry."));
    }
    let mut keys = Vec::with_capacity(parsed.len());
    for (algorithm, public_key_base64) in parsed {
        let line = format!("{} {} {}\n", target.host, algorithm, public_key_base64);
        let keygen = CommandSpec::new("ssh-keygen", vec!["-lf".to_owned(), "-".to_owned()])
            .with_stdin(line.into_bytes())
            .with_timeout(READ_TIMEOUT);
        let fingerprint = runner.run(&keygen).await?;
        if !fingerprint.success() {
            continue;
        }
        let Some((sha256, _reported_algorithm)) =
            parse_fingerprint_output(&fingerprint.stdout_text())
        else {
            continue;
        };
        keys.push(ScannedHostKey {
            preferred: algorithm == "ssh-ed25519",
            algorithm,
            sha256,
            public_key_base64,
        });
    }
    if keys.is_empty() {
        return Err(SpiritError::new(
            crate::error::SpiritErrorCode::Remote,
            "ssh-keygen could not fingerprint any offered host key",
        )
        .retryable());
    }
    keys.sort_by(|left, right| {
        right
            .preferred
            .cmp(&left.preferred)
            .then_with(|| left.algorithm.cmp(&right.algorithm))
    });
    Ok(HostKeyScan {
        keys,
        scanned_at: Timestamp::now(),
    })
}
