//! Read-only host probe used by the add wizard and the Host Manager.

use serde::{Deserialize, Serialize};

use crate::error::{SpiritError, SpiritErrorCode};
use crate::host::profile::HostKeyPin;
use crate::ssh::command::CommandOutput;
use crate::ssh::{SshOptions, SshRemote, SshTarget};
use crate::wire::parse_fleet_status;

/// One stage in the probe checklist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostProbeStage {
    /// Stable stage name: `host_key`, `ssh`, `vowp`, `snapshot` or `supervisor`.
    pub stage: String,
    /// Whether this stage passed.
    pub ok: bool,
    /// Safe detail line.
    pub detail: String,
}

/// The result of probing one host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostProbe {
    /// TCP/SSH handshake succeeded.
    pub reachable: bool,
    /// Authentication succeeded.
    pub auth_ok: bool,
    /// A pin exists and the connection used strict host-key checking.
    pub host_key_ok: bool,
    /// VOWP version, when it could be read.
    pub vowp_version: Option<String>,
    /// Round trip of the version probe.
    pub latency_ms: u64,
    /// Ordered checklist stages.
    pub stages: Vec<HostProbeStage>,
}

impl HostProbe {
    /// Returns true when checks 1-4 pass, so the profile may be saved.
    #[must_use]
    pub fn save_allowed(&self) -> bool {
        ["host_key", "ssh", "vowp", "snapshot"]
            .iter()
            .all(|name| self.stage(name).is_some_and(|stage| stage.ok))
    }

    /// Returns one stage by name.
    #[must_use]
    pub fn stage(&self, name: &str) -> Option<&HostProbeStage> {
        self.stages.iter().find(|stage| stage.stage == name)
    }
}

/// Everything the probe needs; no secret is accepted.
#[derive(Debug, Clone)]
pub struct HostProbeInput {
    /// SSH destination.
    pub target: SshTarget,
    /// SSH options with the app-private known_hosts path.
    pub options: SshOptions,
    /// Remote `--home`, when configured.
    pub vowp_home: Option<String>,
    /// The confirmed pin. Missing or changed pins block the probe.
    pub pin: Option<HostKeyPin>,
}

/// Probes a host with the injected runner.
///
/// Stages: pinned host key, SSH reachability/authentication, VOWP presence, snapshot
/// readability, and supervisor state (informational; not a save blocker).
pub async fn probe_host(
    runner: std::sync::Arc<dyn crate::ssh::CommandRunner>,
    input: &HostProbeInput,
) -> HostProbe {
    let remote = SshRemote::new(runner, input.target.clone(), input.options.clone());
    let mut stages = Vec::new();
    let mut reachable = false;
    let mut auth_ok = false;
    let mut host_key_ok = false;
    let mut vowp_version = None;
    let mut latency_ms = 0;

    match &input.pin {
        Some(pin) if pin.validate().is_ok() => {
            host_key_ok = true;
            stages.push(stage("host_key", true, format!("pinned {}", pin.sha256)));
        }
        Some(pin) => {
            stages.push(stage("host_key", false, pin_error(&pin.algorithm)));
        }
        None => {
            stages.push(stage(
                "host_key",
                false,
                "no pinned fingerprint; scan and confirm one first",
            ));
        }
    }

    let version_argv = version_argv(input.vowp_home.as_deref());
    let started = std::time::Instant::now();
    let destination = input.target.destination();
    let mut first = remote.run(&version_argv).await;
    // A host that has only the renamed `stackspirit` binary answers 127 to `vowp`. Retry
    // once with `stackspirit` and remember it for later commands to that host.
    if matches!(&first, Ok(output) if output.status == 127) {
        let mut alt = version_argv.clone();
        alt[0] = crate::ssh::command::ENGINE_BINARY.to_owned();
        if let Ok(output) = remote.run(&alt).await {
            if output.success() {
                crate::ssh::command::set_engine_binary_for(
                    &destination,
                    crate::ssh::command::ENGINE_BINARY,
                );
                first = Ok(output);
            }
        }
    }
    match first {
        Ok(output) => {
            latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            let (stage_ok, detail) = classify_version_output(&output);
            if output.status != 255 {
                reachable = true;
                auth_ok = true;
            }
            if output.status == 255 {
                if output
                    .stderr_text()
                    .contains("Host key verification failed")
                {
                    host_key_ok = false;
                } else {
                    reachable = true;
                }
                if output.stderr_text().contains("Permission denied") {
                    auth_ok = false;
                }
            }
            stages.push(stage("ssh", reachable && auth_ok, detail));
            if stage_ok {
                if let Some(version) = parse_version_line(&output.stdout_text()) {
                    vowp_version = Some(version.clone());
                    stages.push(stage("vowp", true, format!("vowp {version}")));
                } else {
                    stages.push(stage("vowp", false, "vowp --version returned no version"));
                }
            } else {
                stages.push(stage("vowp", false, "vowp --version failed"));
            }
        }
        Err(err) => {
            stages.push(stage("ssh", false, err.message.clone()));
            stages.push(stage("vowp", false, "not checked: SSH failed first"));
        }
    }

    if auth_ok {
        let mut argv = vec![crate::ssh::command::engine_binary_for(&destination).to_owned()];
        if let Some(home) = &input.vowp_home {
            argv.push("--home".to_owned());
            argv.push(home.clone());
        }
        argv.extend(["--json".to_owned(), "fleet".to_owned(), "status".to_owned()]);
        match remote.run(&argv).await {
            Ok(output) if output.success() => match parse_fleet_status(&output.stdout) {
                Ok(parsed) => {
                    stages.push(stage(
                        "snapshot",
                        true,
                        format!(
                            "snapshot v{} from {}",
                            parsed.value.snapshot.version,
                            parsed.value.source.as_str()
                        ),
                    ));
                    let supervisor = &parsed.value.snapshot.supervisor;
                    stages.push(stage(
                        "supervisor",
                        supervisor.running && !supervisor.paused,
                        if supervisor.running {
                            format!("supervisor up, tick {}s", supervisor.tick_secs)
                        } else {
                            "supervisor stopped".to_owned()
                        },
                    ));
                }
                Err(err) => stages.push(stage("snapshot", false, err.message)),
            },
            Ok(output) => stages.push(stage(
                "snapshot",
                false,
                format!(
                    "fleet status exited {}: {}",
                    output.status,
                    first_line(&output.stderr_text())
                ),
            )),
            Err(err) => stages.push(stage("snapshot", false, err.message)),
        }
    } else {
        stages.push(stage("snapshot", false, "not checked: SSH failed first"));
        stages.push(stage("supervisor", false, "not checked: SSH failed first"));
    }

    HostProbe {
        reachable,
        auth_ok,
        host_key_ok,
        vowp_version,
        latency_ms,
        stages,
    }
}

fn stage(name: &str, ok: bool, detail: impl Into<String>) -> HostProbeStage {
    HostProbeStage {
        stage: name.to_owned(),
        ok,
        detail: detail.into(),
    }
}

fn pin_error(algorithm: &str) -> String {
    format!("invalid pin ({algorithm}); re-scan and confirm the fingerprint")
}

fn version_argv(home: Option<&str>) -> Vec<String> {
    let mut argv = vec!["vowp".to_owned()];
    if let Some(home) = home {
        argv.push("--home".to_owned());
        argv.push(home.to_owned());
    }
    argv.push("--version".to_owned());
    argv
}

fn classify_version_output(output: &CommandOutput) -> (bool, String) {
    if output.success() {
        (true, "SSH reachable".to_owned())
    } else if output.status == 127 {
        (false, "SSH reachable; vowp is not installed".to_owned())
    } else if output
        .stderr_text()
        .contains("Host key verification failed")
    {
        (false, "host key verification failed".to_owned())
    } else if output.stderr_text().contains("Permission denied") {
        (false, "SSH authentication failed".to_owned())
    } else if output.status == 255 {
        (
            false,
            format!("SSH failed: {}", first_line(&output.stderr_text())),
        )
    } else {
        (false, format!("vowp --version exited {}", output.status))
    }
}

/// Parses the first non-empty line of `vowp --version` and keeps a safe token.
#[must_use]
pub fn parse_version_line(output: &str) -> Option<String> {
    let line = output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    let token = line
        .split_whitespace()
        .find(|token| token.chars().any(|c| c.is_ascii_digit()))
        .unwrap_or(line);
    let safe: String = token
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '_'))
        .take(80)
        .collect();
    if safe.is_empty() { None } else { Some(safe) }
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

/// Classifies a failed SSH command into a stable error code.
#[must_use]
pub fn classify_ssh_failure(output: &CommandOutput) -> SpiritError {
    let stderr = output.stderr_text();
    if stderr.contains("Host key verification failed") {
        return SpiritError::new(
            SpiritErrorCode::HostKeyMismatch,
            "host key verification failed",
        )
        .with_hint("Compare the fingerprint out of band and explicitly re-pin the host.");
    }
    if stderr.contains("Permission denied") || stderr.contains("Authentication failed") {
        return SpiritError::new(SpiritErrorCode::SshAuth, "SSH authentication failed")
            .with_hint("Check the SSH agent or the identity file path.");
    }
    SpiritError::new(
        SpiritErrorCode::Remote,
        format!(
            "remote command exited {}: {}",
            output.status,
            first_line(&stderr)
        ),
    )
    .retryable()
}
