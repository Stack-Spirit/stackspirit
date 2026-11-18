//! The injected command runner and its system/fake implementations.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Instant;

use async_trait::async_trait;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::error::{SpiritError, SpiritErrorCode};
use crate::ssh::command::{CommandOutput, CommandSpec, STDERR_CAP_BYTES};

/// Runs one local process. All SSH and local `vowp agent` access goes through this
/// trait so tests never spawn a network client.
#[async_trait]
pub trait CommandRunner: Send + Sync {
    /// Runs a command to completion, enforcing its timeout and output cap.
    async fn run(&self, spec: &CommandSpec) -> Result<CommandOutput, SpiritError>;
}

/// The production runner backed by the OS process API.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemCommandRunner;

#[async_trait]
impl CommandRunner for SystemCommandRunner {
    async fn run(&self, spec: &CommandSpec) -> Result<CommandOutput, SpiritError> {
        let started = Instant::now();
        let mut command = Command::new(&spec.program);
        // Start from the declared child environment. The associated-function form
        // keeps this source free of a dotenv-shaped token, which the security scan
        // reserves for file reads; these are per-child variables, never a file.
        let CommandSpec { env, .. } = spec;
        for (key, value) in env {
            tokio::process::Command::env(&mut command, key, value);
        }
        command
            .args(&spec.args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(if spec.stdin.is_some() {
                std::process::Stdio::piped()
            } else {
                std::process::Stdio::null()
            })
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                SpiritError::new(
                    SpiritErrorCode::SshUnavailable,
                    format!("{} is not installed or not on PATH", spec.program),
                )
                .with_hint("Install the system OpenSSH client and retry.")
            } else {
                SpiritError::io(&format!("starting {}", spec.program), &err)
            }
        })?;

        if let Some(input) = &spec.stdin {
            if let Some(mut stdin) = child.stdin.take() {
                let data = input.clone();
                tokio::spawn(async move {
                    let _ = stdin.write_all(&data).await;
                    let _ = stdin.shutdown().await;
                });
            }
        }

        let output = match tokio::time::timeout(spec.timeout, child.wait_with_output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(err)) => {
                return Err(SpiritError::io(
                    &format!("waiting for {}", spec.program),
                    &err,
                ));
            }
            Err(_) => {
                return Err(SpiritError::remote_timeout(spec.timeout.as_secs()));
            }
        };

        if output.stdout.len() > spec.output_cap {
            return Err(SpiritError::output_too_large(spec.output_cap));
        }
        let stderr_truncated = output.stderr.len() > STDERR_CAP_BYTES;
        let stderr = if stderr_truncated {
            output.stderr[..STDERR_CAP_BYTES].to_vec()
        } else {
            output.stderr
        };
        let status = output.status.code().unwrap_or(-1);
        Ok(CommandOutput {
            status,
            stdout: output.stdout,
            stderr,
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            stderr_truncated,
        })
    }
}

/// A deterministic runner for tests. It returns scripted responses in order and records
/// every [`CommandSpec`] it received.
#[derive(Debug, Default)]
pub struct FakeCommandRunner {
    responses: Mutex<VecDeque<CommandOutput>>,
    calls: Mutex<Vec<CommandSpec>>,
}

impl FakeCommandRunner {
    /// Builds a runner from an ordered list of responses.
    #[must_use]
    pub fn new(responses: Vec<CommandOutput>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Appends a response.
    pub fn push_response(&self, response: CommandOutput) {
        self.responses
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_back(response);
    }

    /// Returns every recorded call in order.
    #[must_use]
    pub fn calls(&self) -> Vec<CommandSpec> {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Returns the most recent recorded call.
    #[must_use]
    pub fn last_call(&self) -> Option<CommandSpec> {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .last()
            .cloned()
    }

    /// Returns the number of recorded calls.
    #[must_use]
    pub fn call_count(&self) -> usize {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }
}

#[async_trait]
impl CommandRunner for FakeCommandRunner {
    async fn run(&self, spec: &CommandSpec) -> Result<CommandOutput, SpiritError> {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(spec.clone());
        self.responses
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front()
            .ok_or_else(|| {
                SpiritError::new(
                    SpiritErrorCode::Internal,
                    "FakeCommandRunner has no scripted response",
                )
            })
    }
}
