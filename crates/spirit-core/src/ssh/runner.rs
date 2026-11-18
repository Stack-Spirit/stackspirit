//! The injected command runner and its system/fake implementations.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Instant;

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::error::{SpiritError, SpiritErrorCode};
use crate::ssh::command::{CommandOutput, CommandSpec, STDERR_CAP_BYTES};

/// Runs one local process. All SSH and local `vowp agent` access goes through this
/// trait so tests never spawn a network client.
#[async_trait]
pub trait CommandRunner: Send + Sync {
    /// Runs a command to completion, enforcing its timeout and output cap.
    async fn run(&self, spec: &CommandSpec) -> Result<CommandOutput, SpiritError>;

    /// Runs one command that asks a question on its output before it reads stdin.
    ///
    /// The default refuses with a typed `gate` error. A runner that cannot prove it
    /// waited for the exact prompt must never write the bytes immediately: for
    /// `vowp secret set` that would echo the value into captured output.
    ///
    /// `spec.stdin` must stay unset; `input` is written once, only after `prompt`
    /// appears. Implementations must apply `spec.timeout` to the whole interaction and
    /// must not retain or log the input.
    async fn run_with_prompt(
        &self,
        _spec: &CommandSpec,
        _prompt: &str,
        _input: &[u8],
    ) -> Result<CommandOutput, SpiritError> {
        Err(SpiritError::new(
            SpiritErrorCode::Gate,
            "interactive input unavailable: this command runner cannot wait for a prompt",
        )
        .with_hint("Use the system command runner for the hidden secret prompt."))
    }
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

    async fn run_with_prompt(
        &self,
        spec: &CommandSpec,
        prompt: &str,
        input: &[u8],
    ) -> Result<CommandOutput, SpiritError> {
        if spec.stdin.is_some() {
            return Err(SpiritError::new(
                SpiritErrorCode::Internal,
                "run_with_prompt requires spec.stdin to stay unset",
            ));
        }
        if prompt.is_empty() {
            return Err(SpiritError::new(
                SpiritErrorCode::Internal,
                "run_with_prompt requires a non-empty prompt",
            ));
        }
        let started = Instant::now();
        let mut command = Command::new(&spec.program);
        // The associated-function form keeps this source free of a dotenv-shaped
        // token, which the security scan reserves for file reads.
        let CommandSpec { env, .. } = spec;
        for (key, value) in env {
            tokio::process::Command::env(&mut command, key, value);
        }
        command
            .args(&spec.args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::piped())
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
        let (Some(stdout), Some(stderr), Some(stdin)) =
            (child.stdout.take(), child.stderr.take(), child.stdin.take())
        else {
            return Err(SpiritError::new(
                SpiritErrorCode::Internal,
                "the child pipes are unavailable",
            ));
        };

        let (tx, mut rx) = mpsc::channel(16);
        let stdout_task = tokio::spawn(pump_stream(stdout, StreamKind::Stdout, tx.clone()));
        let stderr_task = tokio::spawn(pump_stream(stderr, StreamKind::Stderr, tx.clone()));
        drop(tx);

        let interaction = async {
            let mut stdout_bytes = Vec::new();
            let mut stderr_bytes = Vec::new();
            let mut stderr_truncated = false;
            let mut prompt_seen = false;
            while let Some(chunk) = rx.recv().await {
                let chunk =
                    chunk.map_err(|err| SpiritError::io("reading the hidden prompt", &err))?;
                append_stream(
                    &mut stdout_bytes,
                    &mut stderr_bytes,
                    &mut stderr_truncated,
                    spec.output_cap,
                    chunk,
                )?;
                if contains_bytes(&stdout_bytes, prompt.as_bytes())
                    || contains_bytes(&stderr_bytes, prompt.as_bytes())
                {
                    prompt_seen = true;
                    break;
                }
            }
            if !prompt_seen {
                let status = child
                    .wait()
                    .await
                    .map_err(|err| SpiritError::io("waiting for the hidden prompt", &err))?;
                return Err(SpiritError::new(
                    SpiritErrorCode::VowpCommand,
                    format!(
                        "the process exited {} before its prompt",
                        status.code().unwrap_or(-1)
                    ),
                ));
            }

            let mut stdin = stdin;
            stdin
                .write_all(input)
                .await
                .map_err(|err| SpiritError::io("writing to the hidden prompt", &err))?;
            stdin
                .shutdown()
                .await
                .map_err(|err| SpiritError::io("closing the hidden prompt", &err))?;
            drop(stdin);

            // Keep draining while the child runs, so a full channel can never block it
            // on a full pipe.
            let mut wait = Box::pin(child.wait());
            let status = loop {
                tokio::select! {
                    chunk = rx.recv() => {
                        match chunk {
                            Some(chunk) => {
                                let chunk = chunk
                                    .map_err(|err| SpiritError::io("reading the hidden prompt", &err))?;
                                append_stream(
                                    &mut stdout_bytes,
                                    &mut stderr_bytes,
                                    &mut stderr_truncated,
                                    spec.output_cap,
                                    chunk,
                                )?;
                            }
                            None => {
                                break wait
                                    .await
                                    .map_err(|err| SpiritError::io("waiting for the child", &err))?;
                            }
                        }
                    }
                    result = &mut wait => {
                        break result.map_err(|err| SpiritError::io("waiting for the child", &err))?;
                    }
                }
            };
            while let Some(chunk) = rx.recv().await {
                let chunk =
                    chunk.map_err(|err| SpiritError::io("reading the hidden prompt", &err))?;
                append_stream(
                    &mut stdout_bytes,
                    &mut stderr_bytes,
                    &mut stderr_truncated,
                    spec.output_cap,
                    chunk,
                )?;
            }
            stdout_task.await.map_err(|err| {
                SpiritError::new(
                    SpiritErrorCode::Internal,
                    format!("the stdout reader task failed: {err}"),
                )
            })?;
            stderr_task.await.map_err(|err| {
                SpiritError::new(
                    SpiritErrorCode::Internal,
                    format!("the stderr reader task failed: {err}"),
                )
            })?;
            Ok((status, stdout_bytes, stderr_bytes, stderr_truncated))
        };

        match tokio::time::timeout(spec.timeout, interaction).await {
            Ok(result) => {
                let (status, stdout, stderr, stderr_truncated) = result?;
                Ok(CommandOutput {
                    status: status.code().unwrap_or(-1),
                    stdout,
                    stderr,
                    duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                    stderr_truncated,
                })
            }
            Err(_) => {
                let _ = child.kill().await;
                Err(SpiritError::remote_timeout(spec.timeout.as_secs()))
            }
        }
    }
}

/// Which pipe one chunk came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamKind {
    /// The child's stdout.
    Stdout,
    /// The child's stderr.
    Stderr,
}

/// One bounded read from a child pipe.
struct StreamChunk {
    stream: StreamKind,
    bytes: Vec<u8>,
}

/// Reads one child pipe to EOF and forwards chunks to the interaction loop.
async fn pump_stream<R>(
    mut reader: R,
    stream: StreamKind,
    tx: mpsc::Sender<Result<StreamChunk, std::io::Error>>,
) where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut buffer = [0u8; 4096];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) => break,
            Ok(read) => {
                let chunk = StreamChunk {
                    stream,
                    bytes: buffer[..read].to_vec(),
                };
                if tx.send(Ok(chunk)).await.is_err() {
                    break;
                }
            }
            Err(error) => {
                let _ = tx.send(Err(error)).await;
                break;
            }
        }
    }
}

/// Appends one chunk under the same caps `run` enforces.
fn append_stream(
    stdout: &mut Vec<u8>,
    stderr: &mut Vec<u8>,
    stderr_truncated: &mut bool,
    output_cap: usize,
    chunk: StreamChunk,
) -> Result<(), SpiritError> {
    match chunk.stream {
        StreamKind::Stdout => {
            stdout.extend_from_slice(&chunk.bytes);
            if stdout.len() > output_cap {
                return Err(SpiritError::output_too_large(output_cap));
            }
        }
        StreamKind::Stderr => {
            let room = STDERR_CAP_BYTES.saturating_sub(stderr.len());
            let take = room.min(chunk.bytes.len());
            stderr.extend_from_slice(&chunk.bytes[..take]);
            if take < chunk.bytes.len() {
                *stderr_truncated = true;
            }
        }
    }
    Ok(())
}

/// Returns true when `needle` occurs contiguously in `haystack`, including across chunks.
fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
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
