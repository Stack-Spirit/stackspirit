//! Read-only capability gates for ASIF (D5) and local orchestrators (D6).
//!
//! A gate is `Ready` only from a successful capability probe, never from a version
//! string alone. Evidence strings contain command labels, exit codes, versions and
//! capability names; never paths, bodies, tokens or secrets.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::SpiritError;
use crate::ssh::command::CommandOutput;
use crate::ssh::{CommandRunner, SshOptions, SshTarget, ssh_command};
use crate::time::Timestamp;

const GATE_CACHE_TTL_SECS: u64 = 10 * 60;
const NIL_ULID: &str = "00000000000000000000000000";

/// The complete Phase 3 capability set named by `VOWP-CONTRACT.md` §11.3.
pub const PHASE3_MILESTONES: [&str; 6] = [
    "local opencode app host",
    "surface bridge",
    "MCP proxy",
    "orchestrator materialization",
    "backfill",
    "local cache",
];

/// The VOWP milestones the ASIF gate needs.
pub const ASIF_MILESTONES: [&str; 2] = ["v4-asif-core", "v4-asif-cli"];

/// Returns the complete Phase 3 capability set for a details drawer.
#[must_use]
pub fn phase3_milestones() -> Vec<String> {
    PHASE3_MILESTONES
        .iter()
        .map(|item| (*item).to_owned())
        .collect()
}

/// Local-orchestrator capability state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LocalOrchestratorGate {
    /// The probe has not run, or there is no connection.
    Unknown,
    /// The probe is running.
    Probing,
    /// Every required capability is proven.
    Ready {
        /// Negotiated agent protocol.
        agent_protocol: String,
        /// Active local agent node.
        node: Option<String>,
        /// Capability labels.
        capabilities: Vec<String>,
        /// Probe evidence.
        evidence: Vec<String>,
    },
    /// The current VOWP home lacks Phase 2/3 capabilities.
    WaitingForVowpPhase3 {
        /// Exact missing capability/milestone labels.
        missing: Vec<String>,
        /// Detected VOWP version, when known.
        detected_version: Option<String>,
        /// Probe evidence.
        evidence: Vec<String>,
    },
    /// A present document contradicts the contract.
    Incompatible {
        /// What was detected.
        detected: String,
        /// What is required.
        required: String,
        /// Probe evidence.
        evidence: Vec<String>,
    },
    /// The transport probe failed.
    Unreachable {
        /// Safe detail.
        detail: String,
    },
}

/// ASIF capability state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AsifGate {
    /// The probe has not run, or there is no active project.
    Unknown,
    /// The probe is running.
    Probing,
    /// Export and/or import is proven. A partial ready state is honest and useful.
    Ready {
        /// Whether export is proven.
        export: bool,
        /// Whether import is proven.
        import: bool,
        /// Detected VOWP version.
        detected_version: Option<String>,
        /// Probe evidence.
        evidence: Vec<String>,
    },
    /// The current VOWP lacks `v4-asif-*` capabilities.
    WaitingForVowpAsif {
        /// Exact missing milestone names.
        missing: Vec<String>,
        /// Detected VOWP version.
        detected_version: Option<String>,
        /// Probe evidence.
        evidence: Vec<String>,
    },
    /// A present document contradicts the probe.
    Incompatible {
        /// What was detected.
        detected: String,
        /// What is required.
        required: String,
        /// Probe evidence.
        evidence: Vec<String>,
    },
    /// The transport probe failed.
    Unreachable {
        /// Safe detail.
        detail: String,
    },
}

/// The Mac-side local agent state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalAgentState {
    /// Not probed or unrecognised.
    Unknown,
    /// No local `vowp` binary.
    NotInstalled,
    /// `vowp agent status` exited 3.
    NotEnrolled,
    /// Connecting.
    Connecting,
    /// Connected.
    Connected,
    /// Reconnecting.
    Reconnecting,
    /// Revoked.
    Revoked,
    /// Superseded.
    Superseded,
    /// Stopped.
    Stopped,
    /// Protocol mismatch.
    Incompatible,
}

/// The Mac-side local agent status. It is displayed separately from the home gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalAgentStatus {
    /// Connection state.
    pub state: LocalAgentState,
    /// Node name, when known.
    pub node: Option<String>,
    /// Agent protocol, when known.
    pub protocol: Option<String>,
    /// Last heartbeat.
    pub last_heartbeat: Option<Timestamp>,
    /// Last safe error.
    pub last_error: Option<String>,
}

impl Default for LocalAgentStatus {
    fn default() -> Self {
        Self {
            state: LocalAgentState::Unknown,
            node: None,
            protocol: None,
            last_heartbeat: None,
            last_error: None,
        }
    }
}

/// D4/D5/D6 feature switches. Both actions stay false until their phase ships.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateFeatureFlags {
    /// Enable ASIF export/import only in D5.
    pub asif_actions: bool,
    /// Enable local orchestrator actions only in D6.
    pub local_orchestrator_actions: bool,
}

/// One gate snapshot for the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateSnapshot {
    /// When the probe ran.
    pub probed_at: Timestamp,
    /// Home capability gate.
    pub local_orchestrator: LocalOrchestratorGate,
    /// ASIF gate.
    pub asif: AsifGate,
    /// Mac-side agent status.
    pub local_agent: LocalAgentStatus,
    /// Feature switches.
    pub features: GateFeatureFlags,
}

impl Default for GateSnapshot {
    fn default() -> Self {
        Self {
            probed_at: Timestamp::epoch(),
            local_orchestrator: LocalOrchestratorGate::Unknown,
            asif: AsifGate::Unknown,
            local_agent: LocalAgentStatus::default(),
            features: GateFeatureFlags::default(),
        }
    }
}

impl GateSnapshot {
    /// Returns true only when ASIF export is proven and the D5 feature flag is on.
    #[must_use]
    pub fn asif_export_enabled(&self) -> bool {
        self.features.asif_actions && matches!(self.asif, AsifGate::Ready { export: true, .. })
    }

    /// Returns true only when ASIF import is proven and the D5 feature flag is on.
    #[must_use]
    pub fn asif_import_enabled(&self) -> bool {
        self.features.asif_actions && matches!(self.asif, AsifGate::Ready { import: true, .. })
    }

    /// Returns true only when the local-orchestrator gate is ready, the D6 flag is on,
    /// and the local agent is connected.
    #[must_use]
    pub fn local_orchestrator_enabled(&self) -> bool {
        self.features.local_orchestrator_actions
            && matches!(self.local_orchestrator, LocalOrchestratorGate::Ready { .. })
            && self.local_agent.state == LocalAgentState::Connected
    }
}

/// A per-host gate cache, refreshed on connect, version change or explicit re-probe.
#[derive(Debug, Default)]
pub struct GateCache {
    inner: Mutex<HashMap<String, GateSnapshot>>,
}

impl GateCache {
    /// Builds an empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns one host's snapshot while it is younger than ten minutes.
    #[must_use]
    pub fn get(&self, host_id: &str, now: Timestamp) -> Option<GateSnapshot> {
        let guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let snapshot = guard.get(host_id)?;
        if now.since(snapshot.probed_at) > Duration::from_secs(GATE_CACHE_TTL_SECS) {
            None
        } else {
            Some(snapshot.clone())
        }
    }

    /// Stores a host's new snapshot, replacing any older state immediately.
    pub fn put(&self, host_id: impl Into<String>, snapshot: GateSnapshot) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(host_id.into(), snapshot);
    }

    /// Drops one host's cache. A later failure cannot leave a sticky `Ready`.
    pub fn invalidate(&self, host_id: &str) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(host_id);
    }

    /// Drops every cached host. Used when no host is connected.
    pub fn clear(&self) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }
}

/// Connection details the probes need. The runner is passed separately so tests can
/// route commands without a network.
#[derive(Debug, Clone)]
pub struct GateProbeOptions {
    /// SSH destination.
    pub target: SshTarget,
    /// SSH options with the app-private known_hosts path.
    pub ssh: SshOptions,
    /// Remote `--home`, when configured.
    pub home: Option<String>,
    /// First active project from the last snapshot, required by the ASIF export probe.
    pub active_project: Option<String>,
}

fn base_argv(options: &GateProbeOptions, json: bool) -> Vec<String> {
    let mut argv = vec!["vowp".to_owned()];
    if let Some(home) = &options.home {
        argv.extend(["--home".to_owned(), home.clone()]);
    }
    if json {
        argv.push("--json".to_owned());
    }
    argv
}

async fn run_remote(
    runner: &dyn CommandRunner,
    options: &GateProbeOptions,
    argv: &[String],
) -> Result<CommandOutput, SpiritError> {
    let spec = ssh_command(&options.target, &options.ssh, argv, Duration::from_secs(20))?;
    runner.run(&spec).await
}

fn version_from_output(output: &CommandOutput) -> Option<String> {
    if !output.success() {
        return None;
    }
    crate::host::parse_version_line(&output.stdout_text())
}

fn safe_detail(error: &SpiritError) -> String {
    let redacted = crate::error::redact(&error.message);
    let scrubbed: String = redacted
        .split_whitespace()
        .map(|word| {
            if word.starts_with('/')
                || word.starts_with("~/")
                || word.contains("known_hosts")
                || word.contains("id_rsa")
                || word.contains("id_ed25519")
            {
                "[path]"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    if scrubbed.is_empty() {
        error.code.as_str().to_owned()
    } else {
        scrubbed.chars().take(200).collect()
    }
}

fn safe_token(input: &str, limit: usize) -> String {
    input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .take(limit)
        .collect()
}

fn safe_protocol(input: &str) -> String {
    input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '/'))
        .take(64)
        .collect()
}

fn evidence_exit(label: &str, output: &CommandOutput) -> String {
    format!("{label}: exit {}", output.status)
}

/// Probes ASIF export and import support. Read-only: the export probe uses the nil ULID
/// and writes nothing.
pub async fn probe_asif(runner: &dyn CommandRunner, options: &GateProbeOptions) -> AsifGate {
    let mut evidence = Vec::new();
    let version_argv = {
        let mut argv = base_argv(options, false);
        argv.push("--version".to_owned());
        argv
    };
    let version = match run_remote(runner, options, &version_argv).await {
        Ok(output) => match version_from_output(&output) {
            Some(version) => version,
            None => {
                return AsifGate::Unreachable {
                    detail: format!("vowp --version failed: {}", output.status),
                };
            }
        },
        Err(error) => {
            return AsifGate::Unreachable {
                detail: safe_detail(&error),
            };
        }
    };
    evidence.push(format!("vowp --version: {version}"));

    let Some(project) = options
        .active_project
        .as_deref()
        .filter(|project| !project.is_empty())
    else {
        return AsifGate::Unknown;
    };
    let project = safe_token(project, 64);
    if project.is_empty() {
        return AsifGate::Unknown;
    }

    let export_argv = {
        let mut argv = base_argv(options, false);
        argv.extend([
            "--project".to_owned(),
            project.clone(),
            "session".to_owned(),
            "export".to_owned(),
            NIL_ULID.to_owned(),
            "--format".to_owned(),
            "asif".to_owned(),
            "--out".to_owned(),
            "-".to_owned(),
        ]);
        argv
    };
    let export = match run_remote(runner, options, &export_argv).await {
        Ok(output) => {
            evidence.push(evidence_exit("session export --format asif", &output));
            match output.status {
                3 => true,
                2 => {
                    let text = output.stderr_text().to_ascii_lowercase();
                    if text.contains("invalid value")
                        || text.contains("unexpected argument")
                        || text.contains("possible values")
                    {
                        evidence.push(
                            "session export --format asif: unsupported flag value".to_owned(),
                        );
                        false
                    } else {
                        evidence.push("session export --format asif: ambiguous exit 2".to_owned());
                        false
                    }
                }
                _ => {
                    evidence.push("session export --format asif: ambiguous result".to_owned());
                    false
                }
            }
        }
        Err(error) => {
            return AsifGate::Unreachable {
                detail: safe_detail(&error),
            };
        }
    };

    let import_help_argv = {
        let mut argv = base_argv(options, false);
        argv.extend([
            "session".to_owned(),
            "import".to_owned(),
            "--help".to_owned(),
        ]);
        argv
    };
    let import = match run_remote(runner, options, &import_help_argv).await {
        Ok(output) if output.success() => {
            let text = output.stdout_text().to_ascii_lowercase();
            if text.contains("--format") && text.contains("asif") {
                evidence.push("session import --format asif: help lists asif".to_owned());
                true
            } else {
                evidence.push("session import --format asif: help is ambiguous".to_owned());
                false
            }
        }
        Ok(output) => {
            evidence.push(evidence_exit("session import --help", &output));
            false
        }
        Err(error) => {
            return AsifGate::Unreachable {
                detail: safe_detail(&error),
            };
        }
    };

    if export && import {
        return AsifGate::Ready {
            export: true,
            import: true,
            detected_version: Some(version),
            evidence,
        };
    }
    if export {
        return AsifGate::Ready {
            export: true,
            import: false,
            detected_version: Some(version),
            evidence,
        };
    }
    if import {
        return AsifGate::Incompatible {
            detected: "import help advertises asif while export is unsupported".to_owned(),
            required: "a consistent ASIF CLI surface".to_owned(),
            evidence,
        };
    }
    AsifGate::WaitingForVowpAsif {
        missing: vec!["v4-asif-core".to_owned(), "v4-asif-cli".to_owned()],
        detected_version: Some(version),
        evidence,
    }
}

#[derive(Debug, Deserialize)]
struct NodeListRow {
    name: String,
}

#[derive(Debug, Deserialize)]
struct NodeCardProbe {
    name: Option<String>,
    kind: Option<String>,
    status: Option<String>,
    presence: Option<String>,
    #[serde(default)]
    protocol: Option<String>,
    #[serde(default)]
    agent_version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AgentStatusDoc {
    #[serde(default)]
    connection: Option<String>,
    #[serde(default)]
    node: Option<String>,
    #[serde(default)]
    protocol: Option<String>,
    #[serde(default)]
    last_heartbeat: Option<Timestamp>,
    #[serde(default)]
    last_error: Option<String>,
}

/// Probes the home capability and the Mac-side local agent separately.
pub async fn probe_local_orchestrator(
    runner: &dyn CommandRunner,
    local_runner: Option<&dyn CommandRunner>,
    options: &GateProbeOptions,
) -> (LocalOrchestratorGate, LocalAgentStatus) {
    let local_agent = probe_local_agent(local_runner).await;
    let mut evidence = Vec::new();
    let version_argv = {
        let mut argv = base_argv(options, false);
        argv.push("--version".to_owned());
        argv
    };
    let version = match run_remote(runner, options, &version_argv).await {
        Ok(output) => match version_from_output(&output) {
            Some(version) => version,
            None => {
                return (
                    LocalOrchestratorGate::Unreachable {
                        detail: format!("vowp --version failed: {}", output.status),
                    },
                    local_agent,
                );
            }
        },
        Err(error) => {
            return (
                LocalOrchestratorGate::Unreachable {
                    detail: safe_detail(&error),
                },
                local_agent,
            );
        }
    };
    evidence.push(format!("vowp --version: {version}"));

    let mut missing: Vec<String> = Vec::new();
    let node_list_argv = {
        let mut argv = base_argv(options, true);
        argv.extend(["node".to_owned(), "ls".to_owned()]);
        argv
    };
    let node_list = run_remote(runner, options, &node_list_argv).await;
    let names: Vec<String> = match node_list {
        Ok(output) if output.success() => {
            evidence.push("vowp node ls --json: exit 0".to_owned());
            serde_json::from_slice::<Vec<NodeListRow>>(&output.stdout)
                .map(|rows| rows.into_iter().map(|row| row.name).collect())
                .unwrap_or_default()
        }
        Ok(output) => {
            evidence.push(evidence_exit("vowp node ls --json", &output));
            Vec::new()
        }
        Err(error) => {
            return (
                LocalOrchestratorGate::Unreachable {
                    detail: safe_detail(&error),
                },
                local_agent,
            );
        }
    };
    if names.is_empty() {
        missing.push("node model".to_owned());
        return (
            LocalOrchestratorGate::WaitingForVowpPhase3 {
                missing,
                detected_version: Some(version),
                evidence,
            },
            local_agent,
        );
    }

    let mut active_agent: Option<NodeCardProbe> = None;
    for name in names.iter().take(16) {
        let argv = {
            let mut argv = base_argv(options, true);
            argv.extend(["node".to_owned(), "show".to_owned(), safe_token(name, 64)]);
            argv
        };
        let output = match run_remote(runner, options, &argv).await {
            Ok(output) => output,
            Err(error) => {
                return (
                    LocalOrchestratorGate::Unreachable {
                        detail: safe_detail(&error),
                    },
                    local_agent,
                );
            }
        };
        if !output.success() {
            evidence.push(evidence_exit("vowp node show --json", &output));
            continue;
        }
        let Ok(card) = serde_json::from_slice::<NodeCardProbe>(&output.stdout) else {
            evidence.push("vowp node show --json: parse failed".to_owned());
            continue;
        };
        if card.kind.is_none() || card.status.is_none() || card.presence.is_none() {
            return (
                LocalOrchestratorGate::Incompatible {
                    detected: format!(
                        "node card for {} is missing kind/status/presence",
                        safe_token(name, 64)
                    ),
                    required: "NodeCard with kind, status and presence".to_owned(),
                    evidence,
                },
                local_agent,
            );
        }
        let kind = card.kind.as_deref().unwrap_or_default();
        let status = card.status.as_deref().unwrap_or_default();
        if kind == "agent" && status == "active" {
            evidence.push(format!(
                "vowp node show {} --json: kind=agent status=active presence={}",
                safe_token(name, 64),
                safe_token(card.presence.as_deref().unwrap_or_default(), 32)
            ));
            active_agent = Some(card);
            break;
        }
    }
    let Some(agent_card) = active_agent else {
        missing.push("no active local agent node".to_owned());
        return (
            LocalOrchestratorGate::WaitingForVowpPhase3 {
                missing,
                detected_version: Some(version),
                evidence,
            },
            local_agent,
        );
    };
    if let Some(agent_version) = &agent_card.agent_version {
        evidence.push(format!(
            "node agent version: {}",
            safe_token(agent_version, 40)
        ));
    }
    let agent_protocol = agent_card.protocol.clone();
    match agent_protocol.as_deref() {
        Some("vowp-agent/1") => {}
        Some(other) => {
            return (
                LocalOrchestratorGate::Incompatible {
                    detected: safe_protocol(other),
                    required: "vowp-agent/1".to_owned(),
                    evidence,
                },
                local_agent,
            );
        }
        None => {
            missing.push("agent protocol not reported".to_owned());
        }
    }

    let offload_argv = {
        let mut argv = base_argv(options, true);
        argv.extend(["offload".to_owned(), "status".to_owned()]);
        argv
    };
    match run_remote(runner, options, &offload_argv).await {
        Ok(output)
            if output.success()
                && serde_json::from_slice::<serde_json::Value>(&output.stdout).is_ok() =>
        {
            evidence.push("vowp offload status --json: exit 0".to_owned());
        }
        Ok(output) => {
            evidence.push(evidence_exit("vowp offload status --json", &output));
            missing.push("offload executor (Phase 2)".to_owned());
        }
        Err(error) => {
            return (
                LocalOrchestratorGate::Unreachable {
                    detail: safe_detail(&error),
                },
                local_agent,
            );
        }
    }

    for (label, missing_label, args, needle) in [
        (
            "vowp agent surface start --help",
            "local opencode app host",
            vec!["agent", "surface", "start", "--help"],
            None,
        ),
        (
            "vowp agent surface attach --help",
            "surface bridge",
            vec!["agent", "surface", "attach", "--help"],
            None,
        ),
        (
            "vowp mcp --help",
            "MCP proxy",
            vec!["mcp", "--help"],
            Some("--agent-socket"),
        ),
        (
            "vowp orchestrator new --help",
            "orchestrator materialization",
            vec!["orchestrator", "new", "--help"],
            Some("--node"),
        ),
    ] {
        let argv = {
            let mut argv = base_argv(options, false);
            argv.extend(args.into_iter().map(str::to_owned));
            argv
        };
        match run_remote(runner, options, &argv).await {
            Ok(output) if output.success() => {
                let text = output.stdout_text();
                if needle.is_none_or(|needle| text.contains(needle)) {
                    evidence.push(format!("{label}: exit 0"));
                } else {
                    evidence.push(format!("{label}: flag not advertised"));
                    missing.push(missing_label.to_owned());
                }
            }
            Ok(output) => {
                evidence.push(evidence_exit(label, &output));
                missing.push(missing_label.to_owned());
            }
            Err(error) => {
                return (
                    LocalOrchestratorGate::Unreachable {
                        detail: safe_detail(&error),
                    },
                    local_agent,
                );
            }
        }
    }

    let node_paths_argv = {
        let mut argv = base_argv(options, true);
        argv.extend([
            "node".to_owned(),
            "paths".to_owned(),
            safe_token(agent_card.name.as_deref().unwrap_or_default(), 64),
        ]);
        argv
    };
    match run_remote(runner, options, &node_paths_argv).await {
        Ok(output) if output.success() => {
            evidence.push("vowp node paths --json: exit 0".to_owned());
        }
        Ok(output) => {
            evidence.push(evidence_exit("vowp node paths --json", &output));
            missing.push("local cache".to_owned());
        }
        Err(error) => {
            return (
                LocalOrchestratorGate::Unreachable {
                    detail: safe_detail(&error),
                },
                local_agent,
            );
        }
    }

    let orchestrator_ls_argv = {
        let mut argv = base_argv(options, true);
        argv.extend(["orchestrator".to_owned(), "ls".to_owned()]);
        argv
    };
    match run_remote(runner, options, &orchestrator_ls_argv).await {
        Ok(output) if output.success() => {
            evidence.push("vowp orchestrator ls --json: exit 0".to_owned());
        }
        Ok(output) => {
            evidence.push(evidence_exit("vowp orchestrator ls --json", &output));
            missing.push("orchestrator read model".to_owned());
        }
        Err(error) => {
            return (
                LocalOrchestratorGate::Unreachable {
                    detail: safe_detail(&error),
                },
                local_agent,
            );
        }
    }

    missing.sort();
    missing.dedup();
    if !missing.is_empty() {
        return (
            LocalOrchestratorGate::WaitingForVowpPhase3 {
                missing,
                detected_version: Some(version),
                evidence,
            },
            local_agent,
        );
    }
    (
        LocalOrchestratorGate::Ready {
            agent_protocol: "vowp-agent/1".to_owned(),
            node: agent_card.name.map(|name| safe_token(&name, 64)),
            capabilities: vec![
                "node_card".to_owned(),
                "offload_status".to_owned(),
                "surface_start".to_owned(),
                "surface_attach".to_owned(),
                "mcp_agent_socket".to_owned(),
                "orchestrator_new_node".to_owned(),
                "orchestrator_read".to_owned(),
            ],
            evidence,
        },
        local_agent,
    )
}

async fn probe_local_agent(local_runner: Option<&dyn CommandRunner>) -> LocalAgentStatus {
    let Some(runner) = local_runner else {
        return LocalAgentStatus {
            state: LocalAgentState::NotInstalled,
            last_error: Some("local vowp is not available".to_owned()),
            ..LocalAgentStatus::default()
        };
    };
    let argv = vec!["--json".to_owned(), "agent".to_owned(), "status".to_owned()];
    let spec =
        crate::ssh::command::CommandSpec::new("vowp", argv).with_timeout(Duration::from_secs(10));
    match runner.run(&spec).await {
        Ok(output) if output.status == 3 => LocalAgentStatus {
            state: LocalAgentState::NotEnrolled,
            ..LocalAgentStatus::default()
        },
        Ok(output) if !output.success() => LocalAgentStatus {
            state: LocalAgentState::Unknown,
            last_error: Some(format!("vowp agent status exited {}", output.status)),
            ..LocalAgentStatus::default()
        },
        Ok(output) => match serde_json::from_slice::<AgentStatusDoc>(&output.stdout) {
            Ok(doc) => {
                let state = match doc.connection.as_deref().unwrap_or("unknown") {
                    "connected" => LocalAgentState::Connected,
                    "connecting" => LocalAgentState::Connecting,
                    "reconnecting" => LocalAgentState::Reconnecting,
                    "revoked" => LocalAgentState::Revoked,
                    "superseded" => LocalAgentState::Superseded,
                    "stopped" => LocalAgentState::Stopped,
                    _ => LocalAgentState::Unknown,
                };
                let incompatible = doc
                    .protocol
                    .as_deref()
                    .is_some_and(|protocol| protocol != "vowp-agent/1");
                LocalAgentStatus {
                    state: if incompatible {
                        LocalAgentState::Incompatible
                    } else {
                        state
                    },
                    node: doc.node.map(|node| safe_token(&node, 64)),
                    protocol: doc.protocol.map(|protocol| safe_protocol(&protocol)),
                    last_heartbeat: doc.last_heartbeat,
                    last_error: doc
                        .last_error
                        .map(|error| error.chars().take(200).collect()),
                }
            }
            Err(_) => LocalAgentStatus {
                state: LocalAgentState::Unknown,
                last_error: Some("vowp agent status output is not valid JSON".to_owned()),
                ..LocalAgentStatus::default()
            },
        },
        Err(error) => LocalAgentStatus {
            state: LocalAgentState::NotInstalled,
            last_error: Some(safe_detail(&error)),
            ..LocalAgentStatus::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(features: GateFeatureFlags) -> GateSnapshot {
        GateSnapshot {
            probed_at: Timestamp::epoch(),
            local_orchestrator: LocalOrchestratorGate::Ready {
                agent_protocol: "vowp-agent/1".to_owned(),
                node: Some("mac-studio".to_owned()),
                capabilities: vec!["surface_start".to_owned()],
                evidence: vec!["test".to_owned()],
            },
            asif: AsifGate::Ready {
                export: true,
                import: false,
                detected_version: Some("0.4.0".to_owned()),
                evidence: vec!["test".to_owned()],
            },
            local_agent: LocalAgentStatus {
                state: LocalAgentState::Connected,
                node: Some("mac-studio".to_owned()),
                protocol: Some("vowp-agent/1".to_owned()),
                last_heartbeat: None,
                last_error: None,
            },
            features,
        }
    }

    #[test]
    fn engine_gate_ready_fixture_still_disables_unimplemented_actions() {
        let snapshot = snapshot(GateFeatureFlags::default());
        assert!(matches!(
            snapshot.local_orchestrator,
            LocalOrchestratorGate::Ready { .. }
        ));
        assert!(!snapshot.local_orchestrator_enabled());
        assert!(!snapshot.asif_export_enabled());
        assert!(!snapshot.asif_import_enabled());
    }

    #[test]
    fn engine_asif_export_and_import_enable_independently_after_d5_flag() {
        let mut snapshot = snapshot(GateFeatureFlags {
            asif_actions: true,
            local_orchestrator_actions: false,
        });
        assert!(snapshot.asif_export_enabled());
        assert!(!snapshot.asif_import_enabled());
        assert!(!snapshot.local_orchestrator_enabled());

        snapshot.asif = AsifGate::Ready {
            export: true,
            import: true,
            detected_version: Some("0.4.0".to_owned()),
            evidence: vec!["test".to_owned()],
        };
        assert!(snapshot.asif_export_enabled());
        assert!(snapshot.asif_import_enabled());
    }

    #[test]
    fn engine_local_orchestrator_action_needs_the_feature_flag_and_connected_agent() {
        let mut snapshot = snapshot(GateFeatureFlags {
            asif_actions: false,
            local_orchestrator_actions: true,
        });
        assert!(snapshot.local_orchestrator_enabled());
        snapshot.local_agent.state = LocalAgentState::NotInstalled;
        assert!(!snapshot.local_orchestrator_enabled());
    }

    #[test]
    fn engine_gate_details_name_every_phase3_milestone() {
        assert_eq!(phase3_milestones().len(), 6);
        assert!(phase3_milestones().contains(&"backfill".to_owned()));
        assert!(phase3_milestones().contains(&"local cache".to_owned()));
        assert_eq!(
            ASIF_MILESTONES.map(str::to_owned).to_vec(),
            vec!["v4-asif-core".to_owned(), "v4-asif-cli".to_owned()]
        );
    }
}
