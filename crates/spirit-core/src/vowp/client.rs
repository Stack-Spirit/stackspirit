//! The `VowpClient` boundary and its SSH, fixture and mock implementations.
//!
//! The trait is the only way core reads VOWP. It performs no caching and no optimistic
//! mutation: every write returns VOWP's own object and the caller refreshes afterwards.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use zeroize::Zeroizing;

use crate::error::{SpiritError, SpiritErrorCode, code_for_exit};
use crate::ssh::command::{CommandOutput, INITIAL_TIMEOUT, READ_TIMEOUT, WRITE_TIMEOUT};
use crate::ssh::{CommandRunner, SshOptions, SshRemote, SshTarget};
use crate::vowp::asif::{AsifExport, AsifExportOptions, AsifImportOptions, AsifPackage};
use crate::vowp::local::SurfacePairing;
use crate::vowp::secret::{
    SecretListView, SecretRemoveOutcome, SecretSetOutcome, SecretSetRequest, parse_secret_list,
    parse_secret_remove, parse_secret_set, validate_secret_name,
};
use crate::wire::enums::{OrchestratorApp, TargetMode};
use crate::wire::ids::{InboxId, JobId, NodeName, ProjectName, SessionId};
use crate::wire::parse_error_document;
use crate::wire::{
    AsifExportSummary, AsifImportResponse, CapacityReportDoc, CapacityTarget, FleetQueueRow,
    FleetStatus, InboxItem, JobAction, NodeCard, OrchestratorCard, OrchestratorNewRequest,
    SetTargetInput, SurfaceEntry, SurfacePairOutput, SurfacePairRequest, SurfaceRevokeResult,
    SurfaceRotateRequest, SurfaceRotateResult, WireWarning, ensure_surface_id,
    parse_capacity_report, parse_fleet_inbox, parse_fleet_queue, parse_fleet_status,
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
    /// Reads and parses `vowp --json fleet status` with the normal read timeout.
    ///
    /// This is the method every poll uses. The initial connect read uses
    /// [`Self::fleet_status_initial`] instead.
    async fn fleet_status(&self) -> Result<FleetStatus, SpiritError>;
    /// Reads `vowp --json fleet status` with the longer initial-connect timeout.
    ///
    /// The default delegates to [`Self::fleet_status`] so clients that do not
    /// distinguish the two calls keep working; the live SSH client allows 60 s only
    /// here and 20 s for every later poll.
    async fn fleet_status_initial(&self) -> Result<FleetStatus, SpiritError> {
        self.fleet_status().await
    }
    /// Reads and parses `vowp --json capacity report`.
    async fn capacity_report(&self) -> Result<CapacityReportDoc, SpiritError>;
    /// Reads and parses `vowp --json fleet queue`.
    async fn fleet_queue(&self) -> Result<Vec<FleetQueueRow>, SpiritError>;
    /// Reads and parses `vowp --json fleet inbox [--all]`.
    async fn fleet_inbox(&self, all: bool) -> Result<Vec<InboxItem>, SpiritError>;
    /// Runs `vowp --project <p> --json job <action> <job>`; park requires a reason.
    async fn job_action(
        &self,
        project: &str,
        job: &str,
        action: JobAction,
        reason: Option<&str>,
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

    /// Exports one session as a capped `.asif.zip` through the two-step remote temp path.
    ///
    /// The default refuses because a client that does not implement ASIF must not
    /// pretend to export. The live SSH client and the mock implement it.
    async fn asif_export(
        &self,
        _project: &str,
        _session: &str,
        _options: AsifExportOptions,
    ) -> Result<AsifExport, SpiritError> {
        Err(SpiritError::new(
            SpiritErrorCode::Gate,
            "ASIF export is unavailable for this client",
        )
        .with_hint("Connect with a VOWP client that implements the ASIF contract."))
    }

    /// Imports one local package by streaming it to `session import - --format asif`.
    ///
    /// The default refuses for the same reason as [`Self::asif_export`].
    async fn asif_import(
        &self,
        _project: &str,
        _package: &[u8],
        _options: AsifImportOptions,
    ) -> Result<AsifImportResponse, SpiritError> {
        Err(SpiritError::new(
            SpiritErrorCode::Gate,
            "ASIF import is unavailable for this client",
        )
        .with_hint("Connect with a VOWP client that implements the ASIF contract."))
    }

    /// Creates a pending home surface through `vowp surface pair`.
    ///
    /// The default refuses: a client that does not implement D6 must not pretend to
    /// mint a surface or a pairing code.
    async fn surface_pair(
        &self,
        _request: SurfacePairRequest,
    ) -> Result<SurfacePairing, SpiritError> {
        Err(SpiritError::new(
            SpiritErrorCode::Gate,
            "surface pairing is unavailable for this client",
        )
        .with_hint("Connect with a VOWP client that implements the D6 surface contract."))
    }

    /// Reads `vowp surface ls [--node <node>] --json`.
    async fn surface_list(&self, _node: Option<&str>) -> Result<Vec<SurfaceEntry>, SpiritError> {
        Err(SpiritError::new(
            SpiritErrorCode::Gate,
            "the surface read model is unavailable for this client",
        )
        .with_hint("Connect with a VOWP client that implements the D6 surface contract."))
    }

    /// Rotates a surface token through `vowp surface rotate`.
    async fn surface_rotate(
        &self,
        _request: SurfaceRotateRequest,
    ) -> Result<SurfaceRotateResult, SpiritError> {
        Err(SpiritError::new(
            SpiritErrorCode::Gate,
            "surface rotation is unavailable for this client",
        )
        .with_hint("Connect with a VOWP client that implements the D6 surface contract."))
    }

    /// Revokes every token of a surface through `vowp surface revoke`.
    async fn surface_revoke(&self, _surface: &str) -> Result<SurfaceRevokeResult, SpiritError> {
        Err(SpiritError::new(
            SpiritErrorCode::Gate,
            "surface revocation is unavailable for this client",
        )
        .with_hint("Connect with a VOWP client that implements the D6 surface contract."))
    }

    /// Takes a session over for this surface through the hidden
    /// `vowp --json session takeover` command.
    ///
    /// The default refuses so a client that cannot take over never fakes ownership.
    async fn session_takeover(
        &self,
        _request: SessionTakeoverRequest,
    ) -> Result<SessionTakeoverResult, SpiritError> {
        Err(not_supported_takeover(
            "session takeover is unavailable for this client",
        ))
    }

    /// Reads `vowp orchestrator ls --json`.
    async fn orchestrator_list(&self) -> Result<Vec<OrchestratorCard>, SpiritError> {
        Err(SpiritError::new(
            SpiritErrorCode::Gate,
            "the orchestrator read model is unavailable for this client",
        )
        .with_hint("Connect with a VOWP client that implements the D6 orchestrator contract."))
    }

    /// Materializes a local app host through `vowp orchestrator new --node ...`.
    async fn orchestrator_new(
        &self,
        _request: OrchestratorNewRequest,
    ) -> Result<OrchestratorCard, SpiritError> {
        Err(SpiritError::new(
            SpiritErrorCode::Gate,
            "local orchestrator creation is unavailable for this client",
        )
        .with_hint("Connect with a VOWP client that implements the D6 orchestrator contract."))
    }

    /// Stops a local app host through `vowp orchestrator stop`.
    async fn orchestrator_stop(&self, _name: &str) -> Result<OrchestratorCard, SpiritError> {
        Err(SpiritError::new(
            SpiritErrorCode::Gate,
            "local orchestrator stop is unavailable for this client",
        )
        .with_hint("Connect with a VOWP client that implements the D6 orchestrator contract."))
    }

    /// Resumes a local app host through `vowp orchestrator resume`.
    async fn orchestrator_resume(&self, _name: &str) -> Result<OrchestratorCard, SpiritError> {
        Err(SpiritError::new(
            SpiritErrorCode::Gate,
            "local orchestrator resume is unavailable for this client",
        )
        .with_hint("Connect with a VOWP client that implements the D6 orchestrator contract."))
    }

    /// Reads one redacted `vowp node show --json` card.
    async fn node_card(
        &self,
        _project: Option<&str>,
        _node: &str,
    ) -> Result<NodeCard, SpiritError> {
        Err(SpiritError::new(
            SpiritErrorCode::Gate,
            "the node read model is unavailable for this client",
        )
        .with_hint("Connect with a VOWP client that implements the node contract."))
    }

    /// Reads `vowp --json secret ls`.
    ///
    /// The read model contains names, the last four characters and timestamps only.
    /// The default refuses because a client without a server vault must not pretend to
    /// list one.
    async fn secret_list(&self) -> Result<SecretListView, SpiritError> {
        Err(SpiritError::new(
            SpiritErrorCode::Gate,
            "the server secret store is unavailable for this client",
        )
        .with_hint("Connect with a VOWP client that implements the server secret store."))
    }

    /// Stores one secret through `vowp secret set NAME` and the hidden prompt.
    ///
    /// The default refuses before it touches the value, so a client that cannot drive
    /// the prompt can never echo it. The live SSH client implements the pty path.
    async fn secret_set(
        &self,
        _request: SecretSetRequest,
    ) -> Result<SecretSetOutcome, SpiritError> {
        Err(SpiritError::new(
            SpiritErrorCode::Gate,
            "the server secret store is unavailable for this client",
        )
        .with_hint("Connect with a VOWP client that implements the server secret store."))
    }

    /// Removes one secret through `vowp --json secret rm NAME --yes`.
    ///
    /// The default refuses for the same reason as [`Self::secret_list`].
    async fn secret_remove(&self, _name: &str) -> Result<SecretRemoveOutcome, SpiritError> {
        Err(SpiritError::new(
            SpiritErrorCode::Gate,
            "the server secret store is unavailable for this client",
        )
        .with_hint("Connect with a VOWP client that implements the server secret store."))
    }

    /// Returns the non-fatal [`WireWarning`]s collected since the last drain.
    ///
    /// The live client records them so the raw-document drawer can show parse
    /// warnings even when tracing is off. Clients that do not parse wire documents
    /// return an empty list.
    fn take_warnings(&self) -> Vec<WireWarning> {
        Vec::new()
    }
}

/// The live client: system SSH running `vowp ... --json`.
#[derive(Debug, Clone)]
pub struct SshVowpClient {
    remote: SshRemote,
    home: Option<String>,
    warnings: Arc<Mutex<Vec<WireWarning>>>,
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
            warnings: Arc::new(Mutex::new(Vec::new())),
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

    fn record_warnings(&self, warnings: Vec<WireWarning>) {
        let mut sink = self
            .warnings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sink.extend(warnings);
    }

    fn check_output(&self, output: CommandOutput) -> Result<CommandOutput, SpiritError> {
        check_command_output(output)
    }

    async fn read_fleet_status(&self, timeout: Duration) -> Result<FleetStatus, SpiritError> {
        let mut argv = self.base_argv();
        push_json(&mut argv);
        argv.extend(["fleet".to_owned(), "status".to_owned()]);
        let output = self.run(&argv, timeout).await?;
        let output = self.check_output(output)?;
        let parsed = parse_fleet_status(&output.stdout)?;
        self.record_warnings(parsed.warnings);
        Ok(parsed.value)
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

/// Maps one finished VOWP/SSH command to a typed error without retrying a conflict.
///
/// Exit 4 is authoritative: StackSpirit refreshes and lets the user decide, and never
/// mutates an import package to get past a conflict.
pub(crate) fn check_command_output(output: CommandOutput) -> Result<CommandOutput, SpiritError> {
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
            return Err(
                SpiritError::new(SpiritErrorCode::SshAuth, "SSH authentication failed")
                    .with_hint("Check the SSH agent or the identity file path."),
            );
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
    let error = SpiritError::new(code, detail);
    let error = if matches!(
        code,
        SpiritErrorCode::InvalidInput | SpiritErrorCode::Refused | SpiritErrorCode::Conflict
    ) {
        error
    } else {
        error.retryable()
    };
    if code == SpiritErrorCode::VowpMissing {
        Err(error.with_hint("Install VOWP on the host, then re-probe."))
    } else {
        Err(error)
    }
}

/// Maps one finished secret command without ever embedding raw stderr.
///
/// `check_command_output` includes the first stderr line for general commands. A secret
/// command's stderr can carry a pty transcript, so this variant classifies the failure
/// and keeps only a typed message. A missing secret maps to the typed not-found code.
fn check_secret_output(
    output: CommandOutput,
    name: Option<&str>,
) -> Result<CommandOutput, SpiritError> {
    if output.success() {
        return Ok(output);
    }
    if output.status == 255 {
        let stderr = output.stderr_text();
        if stderr.contains("Host key verification failed") {
            return Err(SpiritError::new(
                SpiritErrorCode::HostKeyMismatch,
                "host key verification failed",
            )
            .with_hint("Compare the fingerprint out of band and explicitly re-pin the host."));
        }
        if stderr.contains("Permission denied") || stderr.contains("Authentication failed") {
            return Err(
                SpiritError::new(SpiritErrorCode::SshAuth, "SSH authentication failed")
                    .with_hint("Check the SSH agent or the identity file path."),
            );
        }
        return Err(
            SpiritError::new(SpiritErrorCode::Remote, "SSH transport failed")
                .retryable()
                .with_hint("Check the SSH connection and retry."),
        );
    }
    let code = if output.status == 127 {
        SpiritErrorCode::VowpMissing
    } else {
        code_for_exit(output.status)
    };
    let error = match code {
        SpiritErrorCode::NotFound => match name {
            Some(name) => SpiritError::not_found(&format!("secret {name}")),
            None => SpiritError::not_found("secret"),
        },
        SpiritErrorCode::InvalidInput => SpiritError::new(
            code,
            "VOWP rejected the secret request",
        )
        .with_hint("Use an uppercase name such as OPENAI_API_KEY."),
        SpiritErrorCode::Refused => SpiritError::new(
            code,
            "VOWP refused the secret operation",
        )
        .with_hint(
            "The remote VOWP needs a terminal for the hidden prompt; check the server's SSH pty policy.",
        ),
        SpiritErrorCode::VowpMissing => SpiritError::new(
            code,
            "vowp is not installed on the host",
        )
        .with_hint("Install VOWP on the host, then re-probe."),
        _ => SpiritError::new(code, format!("vowp secret exited {}", output.status)),
    };
    if matches!(
        code,
        SpiritErrorCode::InvalidInput
            | SpiritErrorCode::NotFound
            | SpiritErrorCode::Refused
            | SpiritErrorCode::Conflict
            | SpiritErrorCode::VowpMissing
    ) {
        Err(error)
    } else {
        Err(error.retryable())
    }
}

/// True when VOWP rejected the command itself rather than the takeover.
fn takeover_unsupported(output: &CommandOutput) -> bool {
    if output.status == 127 {
        return true;
    }
    let text = format!(
        "{}\n{}",
        output.stderr_text(),
        String::from_utf8_lossy(&output.stdout)
    )
    .to_lowercase();
    text.contains("unrecognized subcommand")
        || text.contains("unknown command")
        || text.contains("unknown subcommand")
        || text.contains("unexpected argument")
        || text.contains("unsupported_command")
        || text.contains("unsupported command")
}

fn not_supported_takeover(detail: &str) -> SpiritError {
    SpiritError::new(SpiritErrorCode::Gate, format!("not_supported: {detail}"))
        .with_hint("Update VOWP to 0.5.1 or newer. The banner stays read-only.")
}

/// One takeover request. The surface kind and holder are fixed by StackSpirit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTakeoverRequest {
    /// Session id or unique prefix.
    pub session: String,
    /// Node this StackSpirit surface runs on, resolved by the host, never the renderer.
    pub node: String,
    /// Friendly surface label, for example `StackSpirit · macbook`.
    pub label: Option<String>,
}

impl SessionTakeoverRequest {
    /// Validates every value that reaches argv.
    pub fn validate(&self) -> Result<(), SpiritError> {
        SessionId::parse(&self.session)?;
        NodeName::parse(&self.node)?;
        if let Some(label) = &self.label {
            if label.is_empty() || label.len() > 80 || label.chars().any(char::is_control) {
                return Err(SpiritError::new(
                    SpiritErrorCode::InvalidInput,
                    "the surface label must be 1-80 printable characters",
                ));
            }
        }
        Ok(())
    }
}

/// The path-free result of a takeover.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SessionTakeoverResult {
    /// Session now owned by StackSpirit.
    pub session_id: String,
    /// The previous owner as VOWP reported it, when there was one.
    pub previous: Option<String>,
}

fn push_json(argv: &mut Vec<String>) {
    argv.push("--json".to_owned());
}

fn orchestrator_app_arg(app: &OrchestratorApp) -> Result<&'static str, SpiritError> {
    match app {
        OrchestratorApp::Opencode => Ok("opencode"),
        OrchestratorApp::ClaudeCode => Ok("claude-code"),
        OrchestratorApp::Chat => Ok("chat"),
        OrchestratorApp::Unknown(raw) => Err(SpiritError::invalid_input(
            "orchestrator app",
            format!("unknown app {raw:?}"),
        )),
    }
}

/// Validates the user-visible park reason before it reaches `vowp job park --reason`.
fn validated_park_reason(reason: Option<&str>) -> Result<&str, SpiritError> {
    let trimmed = reason
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| SpiritError::invalid_input("reason", "is required when parking a job"))?;
    if trimmed.chars().count() > 200 {
        return Err(SpiritError::invalid_input(
            "reason",
            "must be at most 200 characters",
        ));
    }
    if trimmed.contains('\u{0}') {
        return Err(SpiritError::invalid_input(
            "reason",
            "must not contain a NUL byte",
        ));
    }
    Ok(trimmed)
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
        self.read_fleet_status(READ_TIMEOUT).await
    }

    async fn fleet_status_initial(&self) -> Result<FleetStatus, SpiritError> {
        self.read_fleet_status(INITIAL_TIMEOUT).await
    }

    async fn capacity_report(&self) -> Result<CapacityReportDoc, SpiritError> {
        let mut argv = self.base_argv();
        push_json(&mut argv);
        argv.extend(["capacity".to_owned(), "report".to_owned()]);
        let output = self.run(&argv, READ_TIMEOUT).await?;
        let output = self.check_output(output)?;
        let parsed = parse_capacity_report(&output.stdout)?;
        self.record_warnings(parsed.warnings);
        Ok(parsed.value)
    }

    async fn fleet_queue(&self) -> Result<Vec<FleetQueueRow>, SpiritError> {
        let mut argv = self.base_argv();
        push_json(&mut argv);
        argv.extend(["fleet".to_owned(), "queue".to_owned()]);
        let output = self.run(&argv, READ_TIMEOUT).await?;
        let output = self.check_output(output)?;
        let parsed = parse_fleet_queue(&output.stdout)?;
        self.record_warnings(parsed.warnings);
        Ok(parsed.value)
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
        let parsed = parse_fleet_inbox(&output.stdout)?;
        self.record_warnings(parsed.warnings);
        Ok(parsed.value)
    }

    async fn job_action(
        &self,
        project: &str,
        job: &str,
        action: JobAction,
        reason: Option<&str>,
    ) -> Result<(), SpiritError> {
        ProjectName::parse(project)?;
        JobId::parse(job)?;
        let park_reason = if action == JobAction::Park {
            Some(validated_park_reason(reason)?)
        } else {
            None
        };
        let mut argv = self.base_argv();
        argv.extend(["--project".to_owned(), project.to_owned()]);
        push_json(&mut argv);
        argv.extend(["job".to_owned(), action.as_str().to_owned(), job.to_owned()]);
        if let Some(reason) = park_reason {
            argv.extend(["--reason".to_owned(), reason.to_owned()]);
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

    async fn surface_pair(
        &self,
        request: SurfacePairRequest,
    ) -> Result<SurfacePairing, SpiritError> {
        request.validate()?;
        let mut argv = self.base_argv();
        push_json(&mut argv);
        argv.extend([
            "surface".to_owned(),
            "pair".to_owned(),
            "--session".to_owned(),
            request.session,
            "--node".to_owned(),
            request.node,
            "--kind".to_owned(),
            request.kind.as_str().to_owned(),
        ]);
        if let Some(app) = request.app {
            argv.extend(["--app".to_owned(), app]);
        }
        if let Some(replace) = request.replace {
            argv.extend(["--replace".to_owned(), replace]);
        }
        let output = self.run(&argv, WRITE_TIMEOUT).await?;
        let output = self.check_output(output)?;
        let parsed: SurfacePairOutput = serde_json::from_slice(&output.stdout)
            .map_err(|err| SpiritError::parse("surface pair", err))?;
        Ok(SurfacePairing::from_output(parsed))
    }

    async fn surface_list(&self, node: Option<&str>) -> Result<Vec<SurfaceEntry>, SpiritError> {
        let mut argv = self.base_argv();
        push_json(&mut argv);
        argv.extend(["surface".to_owned(), "ls".to_owned()]);
        if let Some(node) = node {
            NodeName::parse(node)?;
            argv.extend(["--node".to_owned(), node.to_owned()]);
        }
        let output = self.run(&argv, READ_TIMEOUT).await?;
        let output = self.check_output(output)?;
        serde_json::from_slice(&output.stdout)
            .map_err(|err| SpiritError::parse("surface list", err))
    }

    async fn surface_rotate(
        &self,
        request: SurfaceRotateRequest,
    ) -> Result<SurfaceRotateResult, SpiritError> {
        request.validate()?;
        let mut argv = self.base_argv();
        push_json(&mut argv);
        argv.extend(["surface".to_owned(), "rotate".to_owned(), request.surface]);
        if let Some(node) = request.node {
            argv.extend(["--node".to_owned(), node]);
        }
        if let Some(wait) = request.wait {
            argv.extend(["--wait".to_owned(), wait]);
        }
        if let Some(grace) = request.grace {
            argv.extend(["--grace".to_owned(), grace]);
        }
        let output = self.run(&argv, WRITE_TIMEOUT).await?;
        let output = self.check_output(output)?;
        serde_json::from_slice(&output.stdout)
            .map_err(|err| SpiritError::parse("surface rotate", err))
    }

    async fn surface_revoke(&self, surface: &str) -> Result<SurfaceRevokeResult, SpiritError> {
        ensure_surface_id(surface)?;
        let mut argv = self.base_argv();
        push_json(&mut argv);
        argv.extend([
            "surface".to_owned(),
            "revoke".to_owned(),
            surface.to_owned(),
            "--yes".to_owned(),
        ]);
        let output = self.run(&argv, WRITE_TIMEOUT).await?;
        let output = self.check_output(output)?;
        serde_json::from_slice(&output.stdout)
            .map_err(|err| SpiritError::parse("surface revoke", err))
    }

    async fn session_takeover(
        &self,
        request: SessionTakeoverRequest,
    ) -> Result<SessionTakeoverResult, SpiritError> {
        request.validate()?;
        let mut argv = self.base_argv();
        push_json(&mut argv);
        argv.extend([
            "session".to_owned(),
            "takeover".to_owned(),
            "--kind".to_owned(),
            "chat".to_owned(),
            "--holder".to_owned(),
            "stackspirit".to_owned(),
            "--node".to_owned(),
            request.node.clone(),
        ]);
        if let Some(label) = &request.label {
            argv.extend(["--label".to_owned(), label.clone()]);
        }
        argv.push(request.session.clone());
        let output = self.run(&argv, WRITE_TIMEOUT).await?;
        if !output.success() && takeover_unsupported(&output) {
            return Err(not_supported_takeover(
                "the installed VOWP does not provide `session takeover`",
            ));
        }
        let output = self.check_output(output)?;
        let value: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|err| SpiritError::parse("session takeover", err))?;
        Ok(SessionTakeoverResult {
            session_id: request.session,
            previous: value
                .get("previous")
                .filter(|previous| !previous.is_null())
                .map(|previous| match previous.as_str() {
                    Some(text) => text.to_owned(),
                    None => previous.to_string(),
                }),
        })
    }

    async fn orchestrator_list(&self) -> Result<Vec<OrchestratorCard>, SpiritError> {
        let mut argv = self.base_argv();
        push_json(&mut argv);
        argv.extend(["orchestrator".to_owned(), "ls".to_owned()]);
        let output = self.run(&argv, READ_TIMEOUT).await?;
        let output = self.check_output(output)?;
        serde_json::from_slice(&output.stdout)
            .map_err(|err| SpiritError::parse("orchestrator list", err))
    }

    async fn orchestrator_new(
        &self,
        request: OrchestratorNewRequest,
    ) -> Result<OrchestratorCard, SpiritError> {
        request.validate()?;
        let app = orchestrator_app_arg(&request.app)?;
        let mut argv = self.base_argv();
        push_json(&mut argv);
        argv.extend([
            "orchestrator".to_owned(),
            "new".to_owned(),
            "--node".to_owned(),
            request.node,
            "--app".to_owned(),
            app.to_owned(),
        ]);
        if let Some(project) = request.project {
            argv.extend(["--project".to_owned(), project]);
        }
        if let Some(name) = request.name {
            argv.extend(["--name".to_owned(), name]);
        }
        if request.force {
            argv.push("--force".to_owned());
        }
        let output = self.run(&argv, WRITE_TIMEOUT).await?;
        let output = self.check_output(output)?;
        serde_json::from_slice(&output.stdout)
            .map_err(|err| SpiritError::parse("orchestrator new", err))
    }

    async fn orchestrator_stop(&self, name: &str) -> Result<OrchestratorCard, SpiritError> {
        let mut argv = self.base_argv();
        push_json(&mut argv);
        argv.extend([
            "orchestrator".to_owned(),
            "stop".to_owned(),
            name.to_owned(),
        ]);
        let output = self.run(&argv, WRITE_TIMEOUT).await?;
        let output = self.check_output(output)?;
        serde_json::from_slice(&output.stdout)
            .map_err(|err| SpiritError::parse("orchestrator stop", err))
    }

    async fn orchestrator_resume(&self, name: &str) -> Result<OrchestratorCard, SpiritError> {
        let mut argv = self.base_argv();
        push_json(&mut argv);
        argv.extend([
            "orchestrator".to_owned(),
            "resume".to_owned(),
            name.to_owned(),
        ]);
        let output = self.run(&argv, WRITE_TIMEOUT).await?;
        let output = self.check_output(output)?;
        serde_json::from_slice(&output.stdout)
            .map_err(|err| SpiritError::parse("orchestrator resume", err))
    }

    async fn node_card(&self, project: Option<&str>, node: &str) -> Result<NodeCard, SpiritError> {
        NodeName::parse(node)?;
        let mut argv = self.base_argv();
        push_json(&mut argv);
        if let Some(project) = project {
            ProjectName::parse(project)?;
            argv.extend(["--project".to_owned(), project.to_owned()]);
        }
        argv.extend(["node".to_owned(), "show".to_owned(), node.to_owned()]);
        let output = self.run(&argv, READ_TIMEOUT).await?;
        let output = self.check_output(output)?;
        serde_json::from_slice(&output.stdout).map_err(|err| SpiritError::parse("node show", err))
    }

    async fn secret_list(&self) -> Result<SecretListView, SpiritError> {
        let mut argv = self.base_argv();
        push_json(&mut argv);
        argv.extend(["secret".to_owned(), "ls".to_owned()]);
        let output = self.run(&argv, READ_TIMEOUT).await?;
        let output = check_secret_output(output, None)?;
        parse_secret_list(&output.stdout)
    }

    async fn secret_set(&self, request: SecretSetRequest) -> Result<SecretSetOutcome, SpiritError> {
        let mut argv = self.base_argv();
        push_json(&mut argv);
        argv.extend([
            "secret".to_owned(),
            "set".to_owned(),
            request.name().to_owned(),
        ]);
        let prompt = format!("value for {}: ", request.name());
        let mut input = Zeroizing::new(Vec::with_capacity(request.expose_value().len() + 1));
        input.extend_from_slice(request.expose_value().as_bytes());
        input.push(b'\n');
        let output = self
            .remote
            .run_with_prompt(&argv, &prompt, &input, WRITE_TIMEOUT)
            .await?;
        let output = check_secret_output(output, Some(request.name()))?;
        let outcome = parse_secret_set(&output.stdout)?;
        if outcome.name != request.name() {
            return Err(SpiritError::vowp_incompatible(
                "secret set",
                format!(
                    "VOWP stored {:?} instead of {:?}",
                    outcome.name,
                    request.name()
                ),
            ));
        }
        Ok(outcome)
    }

    async fn secret_remove(&self, name: &str) -> Result<SecretRemoveOutcome, SpiritError> {
        validate_secret_name(name)?;
        let mut argv = self.base_argv();
        push_json(&mut argv);
        argv.extend([
            "secret".to_owned(),
            "rm".to_owned(),
            name.to_owned(),
            "--yes".to_owned(),
        ]);
        let output = self.run(&argv, WRITE_TIMEOUT).await?;
        let output = check_secret_output(output, Some(name))?;
        parse_secret_remove(&output.stdout)
    }

    async fn asif_export(
        &self,
        project: &str,
        session: &str,
        options: AsifExportOptions,
    ) -> Result<AsifExport, SpiritError> {
        let (export, warnings) = crate::vowp::asif::export_asif(
            &self.remote,
            self.home.as_deref(),
            project,
            session,
            options,
        )
        .await?;
        self.record_warnings(warnings);
        Ok(export)
    }

    async fn asif_import(
        &self,
        project: &str,
        package: &[u8],
        options: AsifImportOptions,
    ) -> Result<AsifImportResponse, SpiritError> {
        let (response, warnings) = crate::vowp::asif::import_asif(
            &self.remote,
            self.home.as_deref(),
            project,
            package,
            options,
        )
        .await?;
        self.record_warnings(warnings);
        Ok(response)
    }

    fn take_warnings(&self) -> Vec<WireWarning> {
        let mut sink = self
            .warnings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::mem::take(&mut *sink)
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
    actions: Vec<(String, String, JobAction, Option<String>)>,
    targets: Vec<SetTargetInput>,
    asif_export: Option<(AsifExportSummary, Vec<u8>)>,
    asif_import: Option<AsifImportResponse>,
    asif_exports: Vec<(String, String, AsifExportOptions)>,
    asif_imports: Vec<(String, usize, AsifImportOptions)>,
    warnings: Vec<WireWarning>,
    surfaces: Vec<SurfaceEntry>,
    surface_pairing: Option<SurfacePairOutput>,
    surface_rotation: Option<SurfaceRotateResult>,
    surface_revocation: Option<SurfaceRevokeResult>,
    surface_pair_calls: Vec<SurfacePairRequest>,
    surface_rotate_calls: Vec<SurfaceRotateRequest>,
    surface_revoke_calls: Vec<String>,
    orchestrators: Vec<OrchestratorCard>,
    orchestrator_new_result: Option<OrchestratorCard>,
    orchestrator_new_calls: Vec<OrchestratorNewRequest>,
    orchestrator_stop_calls: Vec<String>,
    orchestrator_resume_calls: Vec<String>,
    node_cards: Vec<NodeCard>,
    secrets: SecretListView,
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
                asif_export: None,
                asif_import: None,
                asif_exports: Vec::new(),
                asif_imports: Vec::new(),
                warnings: Vec::new(),
                surfaces: Vec::new(),
                surface_pairing: None,
                surface_rotation: None,
                surface_revocation: None,
                surface_pair_calls: Vec::new(),
                surface_rotate_calls: Vec::new(),
                surface_revoke_calls: Vec::new(),
                orchestrators: Vec::new(),
                orchestrator_new_result: None,
                orchestrator_new_calls: Vec::new(),
                orchestrator_stop_calls: Vec::new(),
                orchestrator_resume_calls: Vec::new(),
                node_cards: Vec::new(),
                secrets: SecretListView {
                    secrets: Vec::new(),
                    storage: None,
                },
            })),
        }
    }

    /// Sets the warnings the next `take_warnings` calls return.
    #[must_use]
    pub fn with_warnings(self, warnings: Vec<WireWarning>) -> Self {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .warnings = warnings;
        self
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

    /// Configures the summary and package bytes returned by `asif_export`.
    #[must_use]
    pub fn with_asif_export(self, summary: AsifExportSummary, package: Vec<u8>) -> Self {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .asif_export = Some((summary, package));
        self
    }

    /// Configures the response returned by `asif_import`.
    #[must_use]
    pub fn with_asif_import(self, response: AsifImportResponse) -> Self {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .asif_import = Some(response);
        self
    }

    /// Returns recorded ASIF export calls.
    #[must_use]
    pub fn asif_exports(&self) -> Vec<(String, String, AsifExportOptions)> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .asif_exports
            .clone()
    }

    /// Returns recorded ASIF import calls.
    #[must_use]
    pub fn asif_imports(&self) -> Vec<(String, usize, AsifImportOptions)> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .asif_imports
            .clone()
    }

    /// Replaces the `vowp surface ls` rows.
    #[must_use]
    pub fn with_surfaces(self, surfaces: Vec<SurfaceEntry>) -> Self {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .surfaces = surfaces;
        self
    }

    /// Configures the `vowp surface pair` result.
    #[must_use]
    pub fn with_surface_pairing(self, pairing: SurfacePairOutput) -> Self {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .surface_pairing = Some(pairing);
        self
    }

    /// Configures the `vowp surface rotate` result.
    #[must_use]
    pub fn with_surface_rotation(self, rotation: SurfaceRotateResult) -> Self {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .surface_rotation = Some(rotation);
        self
    }

    /// Configures the `vowp surface revoke` result.
    #[must_use]
    pub fn with_surface_revocation(self, revocation: SurfaceRevokeResult) -> Self {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .surface_revocation = Some(revocation);
        self
    }

    /// Replaces the `vowp orchestrator ls` cards.
    #[must_use]
    pub fn with_orchestrators(self, orchestrators: Vec<OrchestratorCard>) -> Self {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .orchestrators = orchestrators;
        self
    }

    /// Configures the card returned by `vowp orchestrator new`.
    #[must_use]
    pub fn with_orchestrator_new_result(self, card: OrchestratorCard) -> Self {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .orchestrator_new_result = Some(card);
        self
    }

    /// Replaces the `vowp node show` cards.
    #[must_use]
    pub fn with_node_cards(self, cards: Vec<NodeCard>) -> Self {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .node_cards = cards;
        self
    }

    /// Replaces the `vowp secret ls` read model.
    #[must_use]
    pub fn with_secrets(self, secrets: SecretListView) -> Self {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .secrets = secrets;
        self
    }

    /// Returns recorded surface-pair requests.
    #[must_use]
    pub fn surface_pair_calls(&self) -> Vec<SurfacePairRequest> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .surface_pair_calls
            .clone()
    }

    /// Returns recorded surface-rotate requests.
    #[must_use]
    pub fn surface_rotate_calls(&self) -> Vec<SurfaceRotateRequest> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .surface_rotate_calls
            .clone()
    }

    /// Returns recorded surface-revoke ids.
    #[must_use]
    pub fn surface_revoke_calls(&self) -> Vec<String> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .surface_revoke_calls
            .clone()
    }

    /// Returns recorded orchestrator-new requests.
    #[must_use]
    pub fn orchestrator_new_calls(&self) -> Vec<OrchestratorNewRequest> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .orchestrator_new_calls
            .clone()
    }

    /// Returns recorded orchestrator-stop names.
    #[must_use]
    pub fn orchestrator_stop_calls(&self) -> Vec<String> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .orchestrator_stop_calls
            .clone()
    }

    /// Returns recorded orchestrator-resume names.
    #[must_use]
    pub fn orchestrator_resume_calls(&self) -> Vec<String> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .orchestrator_resume_calls
            .clone()
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
    pub fn actions(&self) -> Vec<(String, String, JobAction, Option<String>)> {
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
        reason: Option<&str>,
    ) -> Result<(), SpiritError> {
        let park_reason = if action == JobAction::Park {
            Some(validated_park_reason(reason)?.to_owned())
        } else {
            None
        };
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .actions
            .push((project.to_owned(), job.to_owned(), action, park_reason));
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

    async fn surface_pair(
        &self,
        request: SurfacePairRequest,
    ) -> Result<SurfacePairing, SpiritError> {
        request.validate()?;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.surface_pair_calls.push(request);
        state
            .surface_pairing
            .clone()
            .map(SurfacePairing::from_output)
            .ok_or_else(|| {
                SpiritError::new(
                    SpiritErrorCode::Internal,
                    "the mock VOWP client has no configured surface pairing",
                )
            })
    }

    async fn surface_list(&self, _node: Option<&str>) -> Result<Vec<SurfaceEntry>, SpiritError> {
        Ok(self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .surfaces
            .clone())
    }

    async fn surface_rotate(
        &self,
        request: SurfaceRotateRequest,
    ) -> Result<SurfaceRotateResult, SpiritError> {
        request.validate()?;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.surface_rotate_calls.push(request);
        state.surface_rotation.clone().ok_or_else(|| {
            SpiritError::new(
                SpiritErrorCode::Internal,
                "the mock VOWP client has no configured surface rotation",
            )
        })
    }

    async fn surface_revoke(&self, surface: &str) -> Result<SurfaceRevokeResult, SpiritError> {
        ensure_surface_id(surface)?;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.surface_revoke_calls.push(surface.to_owned());
        state.surface_revocation.clone().ok_or_else(|| {
            SpiritError::new(
                SpiritErrorCode::Internal,
                "the mock VOWP client has no configured surface revocation",
            )
        })
    }

    async fn orchestrator_list(&self) -> Result<Vec<OrchestratorCard>, SpiritError> {
        Ok(self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .orchestrators
            .clone())
    }

    async fn orchestrator_new(
        &self,
        request: OrchestratorNewRequest,
    ) -> Result<OrchestratorCard, SpiritError> {
        request.validate()?;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.orchestrator_new_calls.push(request);
        state.orchestrator_new_result.clone().ok_or_else(|| {
            SpiritError::new(
                SpiritErrorCode::Internal,
                "the mock VOWP client has no configured orchestrator",
            )
        })
    }

    async fn orchestrator_stop(&self, name: &str) -> Result<OrchestratorCard, SpiritError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.orchestrator_stop_calls.push(name.to_owned());
        state
            .orchestrators
            .iter()
            .find(|card| card.name == name)
            .cloned()
            .ok_or_else(|| SpiritError::not_found(&format!("orchestrator {name}")))
    }

    async fn orchestrator_resume(&self, name: &str) -> Result<OrchestratorCard, SpiritError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.orchestrator_resume_calls.push(name.to_owned());
        state
            .orchestrators
            .iter()
            .find(|card| card.name == name)
            .cloned()
            .ok_or_else(|| SpiritError::not_found(&format!("orchestrator {name}")))
    }

    async fn node_card(&self, _project: Option<&str>, node: &str) -> Result<NodeCard, SpiritError> {
        NodeName::parse(node)?;
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .node_cards
            .iter()
            .find(|card| card.name.as_str() == node)
            .cloned()
            .ok_or_else(|| SpiritError::not_found(&format!("node {node}")))
    }

    async fn secret_list(&self) -> Result<SecretListView, SpiritError> {
        Ok(self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .secrets
            .clone())
    }

    async fn secret_set(
        &self,
        _request: SecretSetRequest,
    ) -> Result<SecretSetOutcome, SpiritError> {
        Err(SpiritError::new(
            SpiritErrorCode::Gate,
            "the mock VOWP client has no server secret store",
        )
        .with_hint("Connect to a host to store secrets with `vowp secret set`."))
    }

    async fn secret_remove(&self, _name: &str) -> Result<SecretRemoveOutcome, SpiritError> {
        Err(SpiritError::new(
            SpiritErrorCode::Gate,
            "the mock VOWP client has no server secret store",
        )
        .with_hint("Connect to a host to remove secrets with `vowp secret rm`."))
    }

    async fn asif_export(
        &self,
        project: &str,
        session: &str,
        options: AsifExportOptions,
    ) -> Result<AsifExport, SpiritError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .asif_exports
            .push((project.to_owned(), session.to_owned(), options));
        let Some((summary, package)) = state.asif_export.clone() else {
            return Err(SpiritError::new(
                SpiritErrorCode::Internal,
                "the mock ASIF export has no configured package",
            ));
        };
        let requested = SessionId::parse(session)?;
        if !summary.is_for_session(&requested) {
            return Err(SpiritError::not_found(&format!("session {session}")));
        }
        Ok(AsifExport {
            summary,
            package: AsifPackage::new(package),
        })
    }

    async fn asif_import(
        &self,
        project: &str,
        package: &[u8],
        options: AsifImportOptions,
    ) -> Result<AsifImportResponse, SpiritError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .asif_imports
            .push((project.to_owned(), package.len(), options));
        state.asif_import.clone().ok_or_else(|| {
            SpiritError::new(
                SpiritErrorCode::Internal,
                "the mock ASIF import has no configured response",
            )
        })
    }

    fn take_warnings(&self) -> Vec<WireWarning> {
        std::mem::take(
            &mut self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .warnings,
        )
    }
}
