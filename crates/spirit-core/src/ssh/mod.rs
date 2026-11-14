//! Outbound SSH transport: target validation, argv construction and a runner wrapper.

pub mod command;
pub mod fingerprint;
pub mod known_hosts;
pub mod runner;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::SpiritError;
use crate::ssh::command::{
    CommandOutput, CommandSpec, OUTPUT_CAP_BYTES, READ_TIMEOUT, remote_command,
};
pub use command::{WRITE_TIMEOUT, quote_arg};
pub use fingerprint::{HostKeyScan, ScannedHostKey, scan_host_keys};
pub use runner::{CommandRunner, FakeCommandRunner, SystemCommandRunner};

/// The SSH destination for one host profile.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SshTarget {
    /// SSH user.
    pub user: String,
    /// DNS name or IP address. No scheme, `@`, or whitespace.
    pub host: String,
    /// SSH port.
    pub port: u16,
}

impl SshTarget {
    /// Validates and builds a target.
    pub fn new(
        user: impl Into<String>,
        host: impl Into<String>,
        port: u16,
    ) -> Result<Self, SpiritError> {
        let target = Self {
            user: user.into(),
            host: host.into(),
            port,
        };
        target.validate()?;
        Ok(target)
    }

    /// Validates the user, host and port without touching the network.
    pub fn validate(&self) -> Result<(), SpiritError> {
        if self.user.is_empty()
            || self.user.starts_with('-')
            || !self
                .user
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        {
            return Err(SpiritError::invalid_input(
                "ssh.user",
                "use letters, digits, '.', '_' and '-' only",
            ));
        }
        if self.host.is_empty()
            || self.host.starts_with('-')
            || self.host.len() > 253
            || !self.host.chars().all(|c| {
                c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '[' | ']' | '%')
            })
        {
            return Err(SpiritError::invalid_input(
                "ssh.host",
                "use a DNS name or IP address without a scheme, '@', or whitespace",
            ));
        }
        if self.port == 0 {
            return Err(SpiritError::invalid_input("ssh.port", "must be 1-65535"));
        }
        Ok(())
    }

    /// Returns `user@host`.
    #[must_use]
    pub fn destination(&self) -> String {
        format!("{}@{}", self.user, self.host)
    }

    /// Returns the `known_hosts` host pattern, including the non-default port.
    #[must_use]
    pub fn known_hosts_pattern(&self) -> String {
        if self.port == 22 {
            self.host.clone()
        } else {
            format!("[{}]:{}", self.host, self.port)
        }
    }
}

/// Options that shape the local `ssh` argv. They never contain key material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshOptions {
    /// App-private `known_hosts` path.
    pub known_hosts: PathBuf,
    /// Identity file path, when the user selected one. The file is never read.
    pub identity_file: Option<PathBuf>,
    /// SSH connect timeout.
    pub connect_timeout_secs: u64,
    /// Keepalive interval.
    pub server_alive_interval_secs: u64,
    /// Keepalive count.
    pub server_alive_count_max: u32,
}

impl SshOptions {
    /// Builds options with the documented defaults.
    #[must_use]
    pub fn new(known_hosts: impl Into<PathBuf>) -> Self {
        Self {
            known_hosts: known_hosts.into(),
            identity_file: None,
            connect_timeout_secs: 8,
            server_alive_interval_secs: 15,
            server_alive_count_max: 3,
        }
    }

    /// Sets an identity file path (path only; never read).
    #[must_use]
    pub fn with_identity_file(mut self, identity_file: Option<PathBuf>) -> Self {
        self.identity_file = identity_file;
        self
    }
}

/// Builds the local `ssh` command for one remote argv.
pub fn ssh_command(
    target: &SshTarget,
    options: &SshOptions,
    remote_argv: &[String],
    timeout: Duration,
) -> Result<CommandSpec, SpiritError> {
    target.validate()?;
    let mut args = vec![
        "-o".to_owned(),
        "BatchMode=yes".to_owned(),
        "-o".to_owned(),
        "StrictHostKeyChecking=yes".to_owned(),
        "-o".to_owned(),
        format!("UserKnownHostsFile={}", options.known_hosts.display()),
        "-o".to_owned(),
        format!("ConnectTimeout={}", options.connect_timeout_secs),
        "-o".to_owned(),
        format!("ServerAliveInterval={}", options.server_alive_interval_secs),
        "-o".to_owned(),
        format!("ServerAliveCountMax={}", options.server_alive_count_max),
    ];
    if let Some(identity_file) = &options.identity_file {
        args.push("-i".to_owned());
        args.push(identity_file.display().to_string());
        args.push("-o".to_owned());
        args.push("IdentitiesOnly=yes".to_owned());
    }
    args.push("-p".to_owned());
    args.push(target.port.to_string());
    args.push("--".to_owned());
    args.push(target.destination());
    args.push(remote_command(remote_argv)?);
    Ok(CommandSpec::new("ssh", args)
        .with_timeout(timeout)
        .with_output_cap(OUTPUT_CAP_BYTES))
}

/// A target plus the runner and options needed to execute remote argv arrays.
#[derive(Clone)]
pub struct SshRemote {
    /// Injected process runner.
    pub runner: Arc<dyn CommandRunner>,
    /// Destination.
    pub target: SshTarget,
    /// SSH options.
    pub options: SshOptions,
}

impl std::fmt::Debug for SshRemote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshRemote")
            .field("target", &self.target)
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl SshRemote {
    /// Builds a remote handle from an injected runner.
    #[must_use]
    pub fn new(runner: Arc<dyn CommandRunner>, target: SshTarget, options: SshOptions) -> Self {
        Self {
            runner,
            target,
            options,
        }
    }

    /// Runs a fixed remote argv array with the read timeout.
    pub async fn run(&self, argv: &[String]) -> Result<CommandOutput, SpiritError> {
        self.run_with_timeout(argv, READ_TIMEOUT).await
    }

    /// Runs a fixed remote argv array with an explicit timeout.
    pub async fn run_with_timeout(
        &self,
        argv: &[String],
        timeout: Duration,
    ) -> Result<CommandOutput, SpiritError> {
        let spec = ssh_command(&self.target, &self.options, argv, timeout)?;
        self.runner.run(&spec).await
    }

    /// Returns the known_hosts path.
    #[must_use]
    pub fn known_hosts_path(&self) -> &Path {
        &self.options.known_hosts
    }
}
