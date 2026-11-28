//! Harness probing and the one-click connect plan.
//!
//! A probe asks two safe questions without reading any credential file or value:
//!
//! - does the harness executable answer `--version` on this Mac or the Home VPS?
//! - does the harness's own non-interactive status command report a stored sign-in?
//!
//! When a Home VPS harness needs a loopback OAuth callback, the connect plan carries a
//! temporary `ssh -N -L 127.0.0.1:<port>:127.0.0.1:<port>` forward. The desktop host
//! owns that process and tears it down when the sign-in finishes or the app exits.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{SpiritError, SpiritErrorCode};
use crate::harnesses::manifest::{HarnessManifest, HarnessProtocol, HarnessSurface};
use crate::ssh::command::{CommandOutput, CommandSpec};
use crate::ssh::{CommandRunner, SshOptions, SshRemote, SshTarget, ssh_command};

/// Deadline for one binary or auth probe.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
/// Upper bound for the managed sign-in command. The desktop host holds the child
/// process; this value is the process deadline if a runner ever executes the spec.
pub const AUTH_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// The managed forward is a long-lived process; it is stopped explicitly.
pub const FORWARD_TIMEOUT: Duration = Duration::from_secs(12 * 60 * 60);

/// Whether a probe ran on this Mac or on the saved Home VPS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessTargetKind {
    /// This Mac.
    Local,
    /// The saved Home VPS, over the app-owned SSH connection.
    HomeVps,
}

impl HarnessTargetKind {
    /// Returns the canonical wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::HomeVps => "home_vps",
        }
    }

    /// Returns a short human label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Local => "Local · this Mac",
            Self::HomeVps => "Home VPS",
        }
    }
}

impl std::fmt::Display for HarnessTargetKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A validated probe destination. The Home variant carries the same SSH target and
/// options the app uses for every other remote read.
#[derive(Debug, Clone)]
pub enum HarnessProbeTarget {
    /// This Mac.
    Local,
    /// The saved Home VPS.
    HomeVps {
        /// SSH destination.
        target: SshTarget,
        /// App-private known_hosts and identity options.
        options: SshOptions,
    },
}

impl HarnessProbeTarget {
    /// Returns the wire kind.
    #[must_use]
    pub fn kind(&self) -> HarnessTargetKind {
        match self {
            Self::Local => HarnessTargetKind::Local,
            Self::HomeVps { .. } => HarnessTargetKind::HomeVps,
        }
    }
}

/// The result of the binary-presence check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessBinaryState {
    /// The executable answered.
    Found,
    /// The executable is not installed on the target.
    Missing,
    /// The check could not prove either way.
    Unknown,
}

/// The result of the non-interactive auth check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessAuthState {
    /// The harness reported at least one stored sign-in.
    Configured,
    /// The harness reported no sign-in.
    Missing,
    /// The harness has no safe non-interactive check, or the check failed.
    Unknown,
}

/// The shortest honest summary of a probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessReadiness {
    /// Binary present and auth configured.
    Ready,
    /// The binary is missing.
    InstallRequired,
    /// The binary is present but no sign-in was found.
    AuthRequired,
    /// Neither state could be proven.
    Unknown,
}

/// A path-free probe result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessProbe {
    /// Manifest id.
    pub harness_id: String,
    /// Probe destination.
    pub target: HarnessTargetKind,
    /// Protocol copied from the manifest so a stale renderer cannot mislabel it.
    pub protocol: HarnessProtocol,
    /// Surface copied from the manifest so a stale renderer cannot mislabel it.
    pub surface: HarnessSurface,
    /// True when the harness can back embedded chat.
    pub supports_embedded_chat: bool,
    /// Binary presence.
    pub binary: HarnessBinaryState,
    /// First safe `--version` line, if the binary answered.
    pub version: Option<String>,
    /// Auth status.
    pub auth: HarnessAuthState,
    /// Fixed, safe explanation for the auth status.
    pub auth_detail: String,
    /// Computed readiness.
    pub readiness: HarnessReadiness,
    /// Loopback callback port declared by the manifest.
    pub oauth_callback_port: Option<u16>,
    /// True when connecting from the Home VPS uses an automatic loopback forward.
    pub requires_loopback_forward: bool,
    /// Safe warnings for the UI.
    pub warnings: Vec<String>,
}

impl HarnessProbe {
    /// True when the probe proved binary and auth.
    #[must_use]
    pub const fn ready(&self) -> bool {
        matches!(self.readiness, HarnessReadiness::Ready)
    }
}

/// Probes one harness on one target. The result never contains raw command output; the
/// version line is redacted and capped, and auth details are fixed copy.
pub async fn probe_harness(
    manifest: &HarnessManifest,
    target: &HarnessProbeTarget,
    runner: Arc<dyn CommandRunner>,
) -> HarnessProbe {
    let target_kind = target.kind();
    let binary_argv = vec![manifest.binary.clone(), "--version".to_owned()];
    let binary_result = run_argv(target, runner.clone(), &binary_argv, PROBE_TIMEOUT).await;

    let (binary, version, binary_detail, mut warnings) = match binary_result {
        Ok(output) => classify_binary(manifest, &output, target_kind),
        Err(error) => classify_binary_error(manifest, &error, target_kind),
    };

    let (auth, auth_detail) = if matches!(binary, HarnessBinaryState::Found) {
        probe_auth(manifest, target, runner).await
    } else {
        (
            HarnessAuthState::Unknown,
            "Skipped because the harness binary was not found.".to_owned(),
        )
    };

    let readiness = match (binary, auth) {
        (HarnessBinaryState::Missing, _) => HarnessReadiness::InstallRequired,
        (HarnessBinaryState::Found, HarnessAuthState::Missing) => HarnessReadiness::AuthRequired,
        (HarnessBinaryState::Found, HarnessAuthState::Configured) => HarnessReadiness::Ready,
        _ => HarnessReadiness::Unknown,
    };

    if let Some(detail) = binary_detail {
        warnings.push(detail);
    }
    if matches!(auth, HarnessAuthState::Unknown) && matches!(binary, HarnessBinaryState::Found) {
        warnings.push(
            "The sign-in state cannot be checked non-interactively; Connect starts the harness \
             sign-in."
                .to_owned(),
        );
    }
    if manifest.accepts_subscription() {
        warnings.push(
            "Subscription sign-in is governed by the vendor's consumer terms; read the note \
             before connecting."
                .to_owned(),
        );
    }
    let requires_loopback_forward = matches!(target, HarnessProbeTarget::HomeVps { .. })
        && manifest.oauth_callback_port.is_some()
        && !manifest.auth_cmd.trim().is_empty()
        && matches!(binary, HarnessBinaryState::Found);
    if requires_loopback_forward {
        if let Some(port) = manifest.oauth_callback_port {
            warnings.push(format!(
                "Connect opens a temporary loopback SSH forward for 127.0.0.1:{port} so the \
                 browser callback never needs a manual tunnel."
            ));
        }
    }

    HarnessProbe {
        harness_id: manifest.id.clone(),
        target: target.kind(),
        protocol: manifest.protocol,
        surface: manifest.surface,
        supports_embedded_chat: manifest.supports_embedded_chat,
        binary,
        version,
        auth,
        auth_detail,
        readiness,
        oauth_callback_port: manifest.oauth_callback_port,
        requires_loopback_forward,
        warnings,
    }
}

fn classify_binary(
    manifest: &HarnessManifest,
    output: &CommandOutput,
    target: HarnessTargetKind,
) -> (
    HarnessBinaryState,
    Option<String>,
    Option<String>,
    Vec<String>,
) {
    if target == HarnessTargetKind::HomeVps && output.status == 255 {
        return (
            HarnessBinaryState::Unknown,
            None,
            Some(
                "The SSH connection failed before the harness could answer; retry after \
                 reconnecting the Home VPS."
                    .to_owned(),
            ),
            Vec::new(),
        );
    }
    if output.status == 127 {
        let hint = if manifest.install_cmd.trim().is_empty() {
            format!("{} is not installed on the target.", manifest.binary)
        } else {
            format!(
                "{} is not installed on the target. Install it with: {}",
                manifest.binary, manifest.install_cmd
            )
        };
        return (HarnessBinaryState::Missing, None, Some(hint), Vec::new());
    }
    let version = first_safe_line(output);
    if output.status == 126 {
        return (
            HarnessBinaryState::Found,
            version,
            Some(format!(
                "{} exists but is not executable on the target.",
                manifest.binary
            )),
            Vec::new(),
        );
    }
    if output.status != 0 {
        return (
            HarnessBinaryState::Found,
            version,
            Some(format!(
                "{} answered --version with exit {}; the binary is present.",
                manifest.binary, output.status
            )),
            Vec::new(),
        );
    }
    (HarnessBinaryState::Found, version, None, Vec::new())
}

fn classify_binary_error(
    manifest: &HarnessManifest,
    error: &SpiritError,
    target: HarnessTargetKind,
) -> (
    HarnessBinaryState,
    Option<String>,
    Option<String>,
    Vec<String>,
) {
    let detail = match error.code {
        SpiritErrorCode::SshUnavailable if target == HarnessTargetKind::Local => {
            format!("{} is not installed or is not on PATH.", manifest.binary)
        }
        SpiritErrorCode::SshUnavailable => {
            "The system SSH client is not available for the Home VPS check.".to_owned()
        }
        SpiritErrorCode::RemoteTimeout => {
            format!("The {} binary check timed out.", manifest.binary)
        }
        SpiritErrorCode::SshAuth
        | SpiritErrorCode::HostKeyMismatch
        | SpiritErrorCode::HostNotFound => {
            "The Home VPS could not be reached for the binary check.".to_owned()
        }
        _ => format!(
            "The {} binary check failed: {}",
            manifest.binary, error.message
        ),
    };
    let state = if target == HarnessTargetKind::Local
        && matches!(error.code, SpiritErrorCode::SshUnavailable)
    {
        HarnessBinaryState::Missing
    } else {
        HarnessBinaryState::Unknown
    };
    (state, None, Some(detail), Vec::new())
}

async fn probe_auth(
    manifest: &HarnessManifest,
    target: &HarnessProbeTarget,
    runner: Arc<dyn CommandRunner>,
) -> (HarnessAuthState, String) {
    let Some(argv) = safe_auth_probe_argv(manifest) else {
        return (
            HarnessAuthState::Unknown,
            "This harness has no safe non-interactive auth check; Connect starts the harness \
             sign-in."
                .to_owned(),
        );
    };
    let output = match run_argv(target, runner, &argv, PROBE_TIMEOUT).await {
        Ok(output) => output,
        Err(error) => {
            let detail = match error.code {
                SpiritErrorCode::RemoteTimeout => {
                    "The sign-in status check timed out; Connect starts the harness sign-in."
                        .to_owned()
                }
                _ => "The sign-in status check could not run; Connect starts the harness sign-in."
                    .to_owned(),
            };
            return (HarnessAuthState::Unknown, detail);
        }
    };

    let text = format!(
        "{}\n{}",
        output.stdout_text().to_ascii_lowercase(),
        output.stderr_text().to_ascii_lowercase()
    );
    let missing_marker = text.contains("not logged in")
        || text.contains("not authenticated")
        || text.contains("no credentials")
        || text.contains("0 credentials")
        || text.contains("signed out");
    if missing_marker {
        return (
            HarnessAuthState::Missing,
            "No stored sign-in was reported; Connect starts the harness sign-in.".to_owned(),
        );
    }
    let configured = match manifest.binary.as_str() {
        // `opencode auth list` prints one bullet per stored provider.
        "opencode" => output.success() && text.contains('●'),
        // These status commands exit zero only for a stored sign-in and print a marker.
        "claude" | "codex" => output.success(),
        _ => false,
    };
    if configured {
        (
            HarnessAuthState::Configured,
            "A stored sign-in was reported. No key or token value was read.".to_owned(),
        )
    } else {
        (
            HarnessAuthState::Unknown,
            "The sign-in status could not be proven non-interactively; Connect starts the \
             harness sign-in."
                .to_owned(),
        )
    }
}

/// The only auth checks StackSpirit runs on its own. Every one prints provider names or
/// a logged-in marker; none prints a key, token or password.
fn safe_auth_probe_argv(manifest: &HarnessManifest) -> Option<Vec<String>> {
    match manifest.binary.as_str() {
        "opencode" => Some(vec![
            manifest.binary.clone(),
            "auth".to_owned(),
            "list".to_owned(),
        ]),
        "claude" => Some(vec![
            manifest.binary.clone(),
            "auth".to_owned(),
            "status".to_owned(),
        ]),
        "codex" => Some(vec![
            manifest.binary.clone(),
            "login".to_owned(),
            "status".to_owned(),
        ]),
        _ => None,
    }
}

fn first_safe_line(output: &CommandOutput) -> Option<String> {
    let text = if output.stdout.iter().any(|byte| !byte.is_ascii_whitespace()) {
        output.stdout_text()
    } else {
        output.stderr_text()
    };
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    let cleaned: String = line.chars().filter(|c| !c.is_control()).collect();
    let redacted = crate::error::redact(&cleaned);
    Some(redacted.chars().take(120).collect())
}

async fn run_argv(
    target: &HarnessProbeTarget,
    runner: Arc<dyn CommandRunner>,
    argv: &[String],
    timeout: Duration,
) -> Result<CommandOutput, SpiritError> {
    let Some(program) = argv.first() else {
        return Err(SpiritError::invalid_input("argv", "must not be empty"));
    };
    match target {
        HarnessProbeTarget::Local => {
            let spec = CommandSpec::new(program.clone(), argv[1..].to_vec()).with_timeout(timeout);
            runner.run(&spec).await
        }
        HarnessProbeTarget::HomeVps {
            target: destination,
            options,
        } => {
            SshRemote::new(runner, destination.clone(), options.clone())
                .run_with_timeout(argv, timeout)
                .await
        }
    }
}

/// One automatic temporary loopback forward.
#[derive(Debug, Clone)]
pub struct LoopbackForwardPlan {
    /// The forwarded port. The local and remote sides use the same number.
    pub port: u16,
    /// The exact local `ssh` argv to spawn.
    pub spec: CommandSpec,
}

impl LoopbackForwardPlan {
    /// Returns the local bind address string, for example `127.0.0.1:1455`.
    #[must_use]
    pub fn local_address(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    /// Returns the `-L` argument value.
    #[must_use]
    pub fn forward_argument(&self) -> String {
        format!("127.0.0.1:{port}:127.0.0.1:{port}", port = self.port)
    }
}

/// Builds `ssh -N -L 127.0.0.1:<port>:127.0.0.1:<port> ...` with the same strict host
/// key, identity and timeout options as every other remote read.
pub fn loopback_forward_plan(
    target: &SshTarget,
    options: &SshOptions,
    port: u16,
) -> Result<LoopbackForwardPlan, SpiritError> {
    target.validate()?;
    if port == 0 {
        return Err(SpiritError::invalid_input(
            "oauth_callback_port",
            "must be 1-65535",
        ));
    }
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
        "-o".to_owned(),
        "ExitOnForwardFailure=yes".to_owned(),
    ];
    if let Some(identity_file) = &options.identity_file {
        args.push("-i".to_owned());
        args.push(identity_file.display().to_string());
        args.push("-o".to_owned());
        args.push("IdentitiesOnly=yes".to_owned());
    }
    args.push("-N".to_owned());
    args.push("-L".to_owned());
    args.push(format!("127.0.0.1:{port}:127.0.0.1:{port}"));
    args.push("-p".to_owned());
    args.push(target.port.to_string());
    args.push("--".to_owned());
    args.push(target.destination());
    let spec = CommandSpec::new("ssh", args).with_timeout(FORWARD_TIMEOUT);
    Ok(LoopbackForwardPlan { port, spec })
}

/// A display-safe projection of a connect plan. It carries no local path, known_hosts
/// path, identity path or command line internals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessConnectPlanView {
    /// The install command to show when the binary is missing.
    pub install_command: Option<String>,
    /// The shell-free sign-in command to run, when one exists.
    pub auth_command: Option<String>,
    /// True when connecting from the Home VPS opens a loopback forward.
    pub requires_loopback_forward: bool,
    /// The forwarded OAuth callback port.
    pub forward_port: Option<u16>,
}

/// A probe plus the path-free connect plan shown in the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessProbeReport {
    /// Probe result.
    pub probe: HarnessProbe,
    /// Connect plan projection.
    pub plan: HarnessConnectPlanView,
}

/// The full host-side plan. The command specs are never serialized to the renderer.
#[derive(Debug, Clone)]
pub struct HarnessConnectPlan {
    /// Manifest being connected.
    pub manifest: HarnessManifest,
    /// Probe destination.
    pub target: HarnessTargetKind,
    /// Probe that preceded this plan.
    pub probe: HarnessProbe,
    /// Install hint when the binary is missing.
    pub install_command: Option<String>,
    /// Sign-in argv when one is runnable.
    pub auth_argv: Option<Vec<String>>,
    /// Sign-in command as displayed to the owner.
    pub auth_command: Option<String>,
    /// Exact local `ssh` or direct command to spawn for sign-in.
    pub auth_spec: Option<CommandSpec>,
    /// Automatic temporary loopback forward for a remote OAuth callback.
    pub forward: Option<LoopbackForwardPlan>,
}

impl HarnessConnectPlan {
    /// Returns the path-free view for the renderer.
    #[must_use]
    pub fn view(&self) -> HarnessConnectPlanView {
        HarnessConnectPlanView {
            install_command: self.install_command.clone(),
            auth_command: self.auth_command.clone(),
            requires_loopback_forward: self.forward.is_some(),
            forward_port: self.forward.as_ref().map(|forward| forward.port),
        }
    }
}

/// Builds the connect plan for the same target that produced `probe`.
///
/// A missing binary never produces a sign-in command. A Home VPS harness with a
/// declared OAuth callback port gets the temporary loopback forward; a local harness
/// never does.
pub fn build_connect_plan(
    manifest: &HarnessManifest,
    target: &HarnessProbeTarget,
    probe: &HarnessProbe,
) -> Result<HarnessConnectPlan, SpiritError> {
    if probe.harness_id != manifest.id {
        return Err(SpiritError::invalid_input(
            "probe",
            "does not belong to this harness",
        ));
    }
    if probe.target != target.kind() {
        return Err(SpiritError::invalid_input(
            "probe",
            "does not belong to this target",
        ));
    }
    let install_command = if matches!(probe.binary, HarnessBinaryState::Missing) {
        Some(manifest.install_cmd.clone()).filter(|command| !command.trim().is_empty())
    } else {
        None
    };

    let mut auth_argv = None;
    let mut auth_command = None;
    let mut auth_spec = None;
    if matches!(probe.binary, HarnessBinaryState::Found) {
        if let Some(argv) = parse_auth_argv(&manifest.auth_cmd)? {
            let spec = match target {
                HarnessProbeTarget::Local => {
                    CommandSpec::new(argv[0].clone(), argv[1..].to_vec()).with_timeout(AUTH_TIMEOUT)
                }
                HarnessProbeTarget::HomeVps {
                    target: destination,
                    options,
                } => ssh_command(destination, options, &argv, AUTH_TIMEOUT)?,
            };
            auth_command = Some(manifest.auth_cmd.trim().to_owned());
            auth_argv = Some(argv);
            auth_spec = Some(spec);
        }
    }

    let forward = match target {
        HarnessProbeTarget::HomeVps {
            target: destination,
            options,
        } if matches!(probe.binary, HarnessBinaryState::Found) && auth_spec.is_some() => manifest
            .oauth_callback_port
            .map(|port| loopback_forward_plan(destination, options, port))
            .transpose()?,
        _ => None,
    };

    Ok(HarnessConnectPlan {
        manifest: manifest.clone(),
        target: target.kind(),
        probe: probe.clone(),
        install_command,
        auth_argv,
        auth_command,
        auth_spec,
        forward,
    })
}

/// Parses a manifest `auth_cmd` into a shell-free argv.
///
/// Quotes and shell control operators are rejected rather than interpreted. An empty
/// command means the harness signs in inside its own terminal.
pub fn parse_auth_argv(auth_cmd: &str) -> Result<Option<Vec<String>>, SpiritError> {
    let trimmed = auth_cmd.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().any(|c| {
        c.is_control()
            || matches!(
                c,
                ';' | '|' | '&' | '$' | '`' | '>' | '<' | '\'' | '"' | '\\' | '(' | ')'
            )
    }) {
        return Err(SpiritError::invalid_input(
            "auth_cmd",
            "must be a shell-free command line without quotes or operators",
        ));
    }
    let argv: Vec<String> = trimmed.split_whitespace().map(str::to_owned).collect();
    if argv.is_empty() {
        return Ok(None);
    }
    let program = &argv[0];
    let first = program.chars().next().unwrap_or(' ');
    if first == '-'
        || !program
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-' | '/'))
    {
        return Err(SpiritError::invalid_input(
            "auth_cmd",
            "must start with an executable name or absolute path",
        ));
    }
    Ok(Some(argv))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_argv_is_shell_free() {
        assert_eq!(
            parse_auth_argv("claude auth login").unwrap(),
            Some(vec![
                "claude".to_owned(),
                "auth".to_owned(),
                "login".to_owned()
            ])
        );
        assert_eq!(parse_auth_argv("  ").unwrap(), None);
        for bad in [
            "claude auth login; rm -rf /",
            "claude auth login && echo done",
            "claude auth login | tee log",
            "claude \"auth\" login",
            "-bad",
        ] {
            assert!(parse_auth_argv(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn forward_argument_is_a_same_port_loopback() {
        let target = SshTarget::new("owner", "192.0.2.10", 22).unwrap();
        let options = SshOptions::new("/tmp/known_hosts");
        let plan = loopback_forward_plan(&target, &options, 1455).unwrap();
        assert_eq!(plan.forward_argument(), "127.0.0.1:1455:127.0.0.1:1455");
        assert!(plan.spec.args.iter().any(|arg| arg == "-N"));
        assert!(plan.spec.args.iter().any(|arg| arg == "-L"));
        assert!(
            plan.spec
                .args
                .iter()
                .any(|arg| arg == "127.0.0.1:1455:127.0.0.1:1455")
        );
        assert!(plan.spec.args.iter().any(|arg| arg == "owner@192.0.2.10"));
        assert!(
            plan.spec
                .args
                .iter()
                .any(|arg| arg == "ExitOnForwardFailure=yes")
        );
    }
}
