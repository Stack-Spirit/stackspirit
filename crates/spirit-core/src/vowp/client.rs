//! The `VowpClient` boundary and its SSH, fixture and mock implementations.
//!
//! The trait is the only way core reads VOWP. It performs no caching and no optimistic
//! mutation: every write returns VOWP's own object and the caller refreshes afterwards.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::error::{SpiritError, SpiritErrorCode, code_for_exit};
use crate::ssh::command::{CommandOutput, INITIAL_TIMEOUT, READ_TIMEOUT, WRITE_TIMEOUT};
use crate::ssh::{CommandRunner, SshOptions, SshRemote, SshTarget};
use crate::wire::enums::TargetMode;
use crate::wire::ids::{InboxId, JobId, ProjectName};
use crate::wire::parse_error_document;
use crate::wire::{
    CapacityReportDoc, CapacityTarget, FleetQueueRow, FleetStatus, InboxItem, JobAction,
    SetTargetInput, parse_capacity_report, parse_fleet_inbox, parse_fleet_queue,
    parse_fleet_status,
};

/// A successful `vowp --version` probe.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VowpProbe {
    /// Version string, safe token only.
    pub version: String,
    /// Round-trip latency in milliseconds.
    pub latency_ms: u64,
    /// Whether this build understands the version's JSON contract. This is not gate
    /// evidence; gates use capability probes.
    pub protocol_compatible: bool,
}

/// The read/write boundary used by the connection manager, service and UI adapters.
#[async_trait]
pub trait VowpClient: Send + Sync {
    /// Reads `vowp --version`.
    async fn probe(&self) -> Result<VowpProbe, SpiritError>;
    /// Reads and parses `vowp --json fleet status`.
    async fn fleet_status(&self) -> Result<FleetStatus, SpiritError>;
    /// Reads and parses `vowp --json capacity report`.
    async fn capacity_report(&self) -> Result<CapacityReportDoc, SpiritError>;
    /// Reads and parses `vowp --json fleet queue`.
    async fn fleet_queue(&self) -> Result<Vec<FleetQueueRow>, SpiritError>;
    /// Reads and parses `vowp --json fleet inbox [--all]`.
    async fn fleet_inbox(&self, all: bool) -> Result<Vec<InboxItem>, SpiritError>;
    /// Runs `vowp --project <p> --json job <action> <job>`.
    async fn job_action(
        &self,
        project: &str,
        job: &str,
        action: JobAction,
    ) -> Result<(), SpiritError>;
    /// Runs `vowp [--project <p>] --json inbox answer <id> <answer>`.
    async fn answer_inbox(
        &self,
        project: Option<&str>,
        id: &str,
        answer: &str,
    ) -> Result<InboxItem, SpiritError>;
    /// Runs `vowp --json capacity set ...`.
    async fn set_target(
        &self,
        target: u32,
        mode: TargetMode,
        override_: bool,
        reason: &str,
    ) -> Result<CapacityTarget, SpiritError>;
}

/// The live client: system SSH running `vowp ... --json`.
#[derive(Debug, Clone)]
pub struct SshVowpClient {
    remote: SshRemote,
    home: Option<String>,
}

impl SshVowpClient {
    /// Builds a client for one host.
    #[must_use]
    pub fn new(
        runner: Arc<dyn CommandRunner>,
        target: SshTarget,
        options: SshOptions,
        home: Option<String>,
    ) -> Self {
        Self {
            remote: SshRemote::new(runner, target, options),
            home,
        }
    }

    /// Returns the remote target.
    #[must_use]
    pub fn target(&self) -> &SshTarget {
        &self.remote.target
    }

    fn base_argv(&self) -> Vec<String> {
        let mut argv = vec!["vowp".to_owned()];
        if let Some(home) = &self.home {
            argv.push("--home".to_owned());
            argv.push(home.clone());
        }
        argv
    }

    async fn run(&self, argv: &[String], timeout: Duration) -> Result<CommandOutput, SpiritError> {
        self.remote.run_with_timeout(argv, timeout).await
    }

    fn check_output(&self, output: CommandOutput) -> Result<CommandOutput, SpiritError> {
        if output.success() {
            return Ok(output);
        }
        if let Ok(document) = parse_error_document(&output.stdout) {
            return Err(document.to_spirit_error(output.status));
        }
        let stderr = output.stderr_text();
        if output.status == 255 {
            if stderr.contains("Host key verification failed") {
                return Err(SpiritError::new(
                    SpiritErrorCode::HostKeyMismatch,
                    "host key verification failed",
                )
                .with_hint("Compare the fingerprint out of band and explicitly re-pin the host."));
            }
            if stderr.contains("Permission denied") || stderr.contains("Authentication failed") {
                return Err(SpiritError::new(
                    SpiritErrorCode::SshAuth,
                    "SSH authentication failed",
                )
                .with_hint("Check the SSH agent or the identity file path."));
            }
            return Err(SpiritError::new(
                SpiritErrorCode::Remote,
                format!("SSH transport failed: {}", first_line(&stderr)),
            )
            .retryable());
        }
        let code = if output.status == 127 {
            SpiritErrorCode::VowpMissing
        } else {
            code_for_exit(output.status)
        };
        let detail = if stderr.is_empty() {
            format!("vowp exited {}", output.status)
        } else {
            format!("vowp exited {}: {}", output.status, first_line(&stderr))
        };
        let error = SpiritError::new(code, detail).retryable();
        if code == SpiritErrorCode::VowpMissing {
            Err(error.with_hint("Install VOWP on the host, then re-probe."))
        } else {
            Err(error)
        }
    }
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

fn push_json(argv: &mut Vec<String>) {
    argv.push("--json".to_owned());
}

#[async_trait]
impl VowpClient for SshVowpClient {
    async fn probe(&self) -> Result<VowpProbe, SpiritError> {
        let mut argv = self.base_argv();
        argv.push("--version".to_owned());
        let started = Instant::now();
        let output = self.run(&argv, READ_TIMEOUT).await?;
        let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        if output.status == 127 {
            return Err(SpiritError::new(
                SpiritErrorCode::VowpMissing,
                "vowp is not installed on the host",
            )
            .with_hint("Install VOWP on the host, then re-probe."));
        }
        let output = self.check_output(output)?;
        let version = crate::host::parse_version_line(&output.stdout_text()).ok_or_else(|| {
            SpiritError::new(
                SpiritErrorCode::VowpCommand,
                "vowp --version returned no version",
            )
        })?;
        Ok(VowpProbe {
            version,
            latency_ms,
            protocol_compatible: true,
        })
    }

    async fn fleet_status(&self) -> Result<FleetStatus, SpiritError> {
        let mut argv = self.base_argv();
        push_json(&mut argv);
        argv.extend(["fleet".to_owned(), "status".to_owned()]);
        let output = self.run(&argv, INITIAL_TIMEOUT).await?;
        let output = self.check_output(output)?;
        let parsed = parse_fleet_status(&output.stdout)?;
        for warning in &parsed.warnings {
            tracing::debug!(path = %warning.path, detail = %warning.detail, "fleet status wire warning");
        }
        Ok(parsed.value)
    }

    async fn capacity_report(&self) -> Result<CapacityReportDoc, SpiritError> {
        let mut argv = self.base_argv();
        push_json(&mut argv);
        argv.extend(["capacity".to_owned(), "report".to_owned()]);
        let output = self.run(&argv, READ_TIMEOUT).await?;
        let output = self.check_output(output)?;
        let parsed = parse_capacity_report(&output.stdout)?;
        for warning in &parsed.warnings {
            tracing::debug!(path = %warning.path, detail = %warning.detail, "capacity wire warning");
        }
        Ok(parsed.value)
    }

    async fn fleet_queue(&self) -> Result<Vec<FleetQueueRow>, SpiritError> {
        let mut argv = self.base_argv();
        push_json(&mut argv);
        argv.extend(["fleet".to_owned(), "queue".to_owned()]);
        let output = self.run(&argv, READ_TIMEOUT).await?;
        let output = self.check_output(output)?;
        Ok(parse_fleet_queue(&output.stdout)?.value)
    }

    async fn fleet_inbox(&self, all: bool) -> Result<Vec<InboxItem>, SpiritError> {
        let mut argv = self.base_argv();
        push_json(&mut argv);
        argv.extend(["fleet".to_owned(), "inbox".to_owned()]);
        if all {
            argv.push("--all".to_owned());
        }
        let output = self.run(&argv, READ_TIMEOUT).await?;
        let output = self.check_output(output)?;
        Ok(parse_fleet_inbox(&output.stdout)?.value)
    }

    async fn job_action(
        &self,
        project: &str,
        job: &str,
        action: JobAction,
    ) -> Result<(), SpiritError> {
        ProjectName::parse(project)?;
        JobId::parse(job)?;
        let mut argv = self.base_argv();
        argv.extend(["--project".to_owned(), project.to_owned()]);
        push_json(&mut argv);
        argv.extend(["job".to_owned(), action.as_str().to_owned(), job.to_owned()]);
        if action == JobAction::Park {
            argv.extend(["--reason".to_owned(), "parked from StackSpirit".to_owned()]);
        }
        let output = self.run(&argv, WRITE_TIMEOUT).await?;
        self.check_output(output)?;
        Ok(())
    }

    async fn answer_inbox(
        &self,
        project: Option<&str>,
        id: &str,
        answer: &str,
    ) -> Result<InboxItem, SpiritError> {
        let trimmed = answer.trim();
        if trimmed.is_empty() || answer.len() > 10_000 || answer.contains('\u{0}') {
            return Err(SpiritError::invalid_input(
                "answer",
                "must be 1-10,000 bytes and contain no NUL",
            ));
        }
        InboxId::parse(id)?;
        let mut argv = self.base_argv();
        if let Some(project) = project {
            ProjectName::parse(project)?;
            argv.extend(["--project".to_owned(), project.to_owned()]);
        }
        push_json(&mut argv);
        argv.extend([
            "inbox".to_owned(),
            "answer".to_owned(),
            id.to_owned(),
            answer.to_owned(),
            "--by".to_owned(),
            "stackspirit".to_owned(),
        ]);
        let output = self.run(&argv, WRITE_TIMEOUT).await?;
        let output = self.check_output(output)?;
        serde_json::from_slice(&output.stdout)
            .map_err(|err| SpiritError::parse("inbox answer", err))
    }

    async fn set_target(
        &self,
        target: u32,
        mode: TargetMode,
        override_: bool,
        reason: &str,
    ) -> Result<CapacityTarget, SpiritError> {
        let input = SetTargetInput {
            target,
            mode: mode.clone(),
            override_,
            reason: reason.trim().to_owned(),
        };
        input.validate()?;
        let mut argv = self.base_argv();
        push_json(&mut argv);
        argv.extend([
            "capacity".to_owned(),
            "set".to_owned(),
            "--target".to_owned(),
            target.to_string(),
            "--mode".to_owned(),
            mode.as_str().to_owned(),
        ]);
        if override_ {
            argv.push("--override".to_owned());
        }
        argv.extend([
            "--reason".to_owned(),
            input.reason,
            "--by".to_owned(),
            "stackspirit".to_owned(),
        ]);
        let output = self.run(&argv, WRITE_TIMEOUT).await?;
        let output = self.check_output(output)?;
        serde_json::from_slice(&output.stdout)
            .map_err(|err| SpiritError::parse("capacity target", err))
    }
}

#[derive(Debug)]
struct MockState {
    probe: VowpProbe,
    fleet_status: FleetStatus,
    capacity: CapacityReportDoc,
    queue: Vec<FleetQueueRow>,
    inbox: Vec<InboxItem>,
    fail_probe: Option<SpiritError>,
    fail_status: Option<SpiritError>,
    status_calls: u32,
    actions: Vec<(String, String, JobAction)>,
    targets: Vec<SetTargetInput>,
}

/// A programmable in-memory client for tests and previews.
#[derive(Debug, Clone)]
pub struct MockVowpClient {
    state: Arc<Mutex<MockState>>,
}

impl MockVowpClient {
    /// Builds a mock from a fleet status and capacity report.
    #[must_use]
    pub fn new(fleet_status: FleetStatus, capacity: CapacityReportDoc) -> Self {
        Self {
            state: Arc::new(Mutex::new(MockState {
                probe: VowpProbe {
                    version: "0.4.0".to_owned(),
                    latency_ms: 42,
                    protocol_compatible: true,
                },
                fleet_status,
                capacity,
                queue: Vec::new(),
                inbox: Vec::new(),
                fail_probe: None,
                fail_status: None,
                status_calls: 0,
                actions: Vec::new(),
                targets: Vec::new(),
            })),
        }
    }

    /// Replaces the probe result.
    #[must_use]
    pub fn with_probe(self, probe: VowpProbe) -> Self {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .probe = probe;
        self
    }

    /// Replaces the queue rows.
    #[must_use]
    pub fn with_queue(self, queue: Vec<FleetQueueRow>) -> Self {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .queue = queue;
        self
    }

    /// Replaces the inbox items.
    #[must_use]
    pub fn with_inbox(self, inbox: Vec<InboxItem>) -> Self {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .inbox = inbox;
        self
    }

    /// Makes `probe` fail.
    #[must_use]
    pub fn failing_probe(self, error: SpiritError) -> Self {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .fail_probe = Some(error);
        self
    }

    /// Makes `fleet_status` fail.
    #[must_use]
    pub fn failing_status(self, error: SpiritError) -> Self {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .fail_status = Some(error);
        self
    }

    /// Restores a successful `fleet_status`.
    pub fn clear_status_failure(&self) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .fail_status = None;
    }

    /// Returns the number of `fleet_status` calls.
    #[must_use]
    pub fn status_calls(&self) -> u32 {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .status_calls
    }

    /// Returns recorded job actions.
    #[must_use]
    pub fn actions(&self) -> Vec<(String, String, JobAction)> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .actions
            .clone()
    }

    /// Returns recorded target writes.
    #[must_use]
    pub fn targets(&self) -> Vec<SetTargetInput> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .targets
            .clone()
    }
}

#[async_trait]
impl VowpClient for MockVowpClient {
    async fn probe(&self) -> Result<VowpProbe, SpiritError> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(error) = &state.fail_probe {
            return Err(error.clone());
        }
        Ok(state.probe.clone())
    }

    async fn fleet_status(&self) -> Result<FleetStatus, SpiritError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.status_calls = state.status_calls.saturating_add(1);
        if let Some(error) = &state.fail_status {
            return Err(error.clone());
        }
        Ok(state.fleet_status.clone())
    }

    async fn capacity_report(&self) -> Result<CapacityReportDoc, SpiritError> {
        Ok(self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .capacity
            .clone())
    }

    async fn fleet_queue(&self) -> Result<Vec<FleetQueueRow>, SpiritError> {
        Ok(self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .queue
            .clone())
    }

    async fn fleet_inbox(&self, _all: bool) -> Result<Vec<InboxItem>, SpiritError> {
        Ok(self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .inbox
            .clone())
    }

    async fn job_action(
        &self,
        project: &str,
        job: &str,
        action: JobAction,
    ) -> Result<(), SpiritError> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .actions
            .push((project.to_owned(), job.to_owned(), action));
        Ok(())
    }

    async fn answer_inbox(
        &self,
        _project: Option<&str>,
        id: &str,
        answer: &str,
    ) -> Result<InboxItem, SpiritError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(index) = state.inbox.iter().position(|item| item.id.as_str() == id) else {
            return Err(SpiritError::not_found(&format!("inbox item {id}")));
        };
        state.inbox[index].answer = Some(answer.to_owned());
        state.inbox[index].state = crate::wire::enums::InboxState::Answered;
        Ok(state.inbox[index].clone())
    }

    async fn set_target(
        &self,
        target: u32,
        mode: TargetMode,
        override_: bool,
        reason: &str,
    ) -> Result<CapacityTarget, SpiritError> {
        let input = SetTargetInput {
            target,
            mode: mode.clone(),
            override_,
            reason: reason.to_owned(),
        };
        input.validate()?;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.targets.push(input);
        let mut updated = state.capacity.report.target.clone();
        updated.target_workers = target;
        updated.mode = mode;
        updated.is_override = override_;
        updated.set_by = "stackspirit".to_owned();
        updated.set_at = crate::time::Timestamp::now();
        updated.reason = reason.to_owned();
        Ok(updated)
    }
}
