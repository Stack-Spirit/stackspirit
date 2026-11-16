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
use crate::ssh::command::{CommandOutput, CommandSpec};
use crate::ssh::{CommandRunner, SshOptions, SshTarget, ssh_command};
use crate::time::Timestamp;

const GATE_CACHE_TTL_SECS: u64 = 10 * 60;
const NIL_ULID: &str = "00000000000000000000000000";

/// The `orchestrator new` after-help sentence that proves the local-node meaning.
/// The global SSH selector says only "Run the command on that node over ssh"; that string
/// is never accepted as proof (D6-READINESS.md §2.2.2).
const ORCHESTRATOR_NEW_LOCAL_NODE_TEXT: [&str; 2] =
    ["names the node that runs the app", "not an SSH redirect"];

/// The `surface pair` after-help sentence that proves `--node` is the surface's node.
/// The top-level `vowp surface --help` sentence is not accepted; the probe reads
/// `vowp surface pair --help`, where the merged CLI repeats the local meaning.
const SURFACE_PAIR_LOCAL_NODE_TEXT: [&str; 2] =
    ["names the node the surface runs on", "not an SSH redirect"];

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

/// A cached gate snapshot, refreshed on connect, version change or explicit re-probe.
#[derive(Debug, Default)]
pub struct GateCache {
    inner: Mutex<Option<GateSnapshot>>,
}

impl GateCache {
    /// Builds an empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the cached snapshot while it is younger than ten minutes.
    #[must_use]
    pub fn get(&self, now: Timestamp) -> Option<GateSnapshot> {
        let guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let snapshot = guard.as_ref()?;
        if now.since(snapshot.probed_at) > Duration::from_secs(GATE_CACHE_TTL_SECS) {
            None
        } else {
            Some(snapshot.clone())
        }
    }

    /// Stores a new snapshot, replacing any older state immediately.
    pub fn put(&self, snapshot: GateSnapshot) {
        *self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(snapshot);
    }

    /// Drops the cache. A later failure cannot leave a sticky `Ready`.
    pub fn invalidate(&self) {
        *self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }
}

/// A per-host gate cache, refreshed on connect, version change or explicit re-probe.
///
/// The desktop keeps one entry per host connection so a `Ready` snapshot from host A
/// is never shown while host B is connected, or while disconnected.
#[derive(Debug, Default)]
pub struct HostGateCache {
    inner: Mutex<HashMap<String, GateSnapshot>>,
}

impl HostGateCache {
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

/// Returns the active project as a safe token, or `None` when no usable project is known.
///
/// Node reads on a multi-project home need `--project`; a bare `vowp node ls` exits 2
/// there. The token is never rewritten silently: an unsafe value is treated as unknown.
fn project_token(options: &GateProbeOptions) -> Option<String> {
    let project = options
        .active_project
        .as_deref()
        .filter(|project| !project.is_empty())?;
    let safe = safe_token(project, 64);
    (safe == project).then_some(safe)
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
    let scrubbed = safe_text(&error.message);
    if scrubbed.is_empty() {
        error.code.as_str().to_owned()
    } else {
        scrubbed
    }
}

/// Redacts secrets and path-like tokens from a short display string.
///
/// Evidence and `LocalAgentStatus.last_error` may quote VOWP text; no home path, state
/// directory, key file, or token may survive this filter.
fn safe_text(input: &str) -> String {
    let redacted = crate::error::redact(input);
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
    scrubbed.chars().take(200).collect()
}

fn local_agent_state_label(state: LocalAgentState) -> &'static str {
    match state {
        LocalAgentState::Unknown => "unknown",
        LocalAgentState::NotInstalled => "not_installed",
        LocalAgentState::NotEnrolled => "not_enrolled",
        LocalAgentState::Connecting => "connecting",
        LocalAgentState::Connected => "connected",
        LocalAgentState::Reconnecting => "reconnecting",
        LocalAgentState::Revoked => "revoked",
        LocalAgentState::Superseded => "superseded",
        LocalAgentState::Stopped => "stopped",
        LocalAgentState::Incompatible => "incompatible",
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

/// Runs one home-side `--help` probe and records whether the merged CLI advertises the
/// exact flags and local-node meaning D6 needs. A missing or ambiguous help stays
/// `Waiting`; it is never accepted as evidence for a different command.
async fn probe_home_help(
    runner: &dyn CommandRunner,
    options: &GateProbeOptions,
    label: &str,
    args: &[&str],
    required: &[&str],
    evidence: &mut Vec<String>,
) -> Result<bool, SpiritError> {
    let argv = {
        let mut argv = base_argv(options, false);
        argv.extend(args.iter().map(|arg| (*arg).to_owned()));
        argv
    };
    match run_remote(runner, options, &argv).await {
        Ok(output) if output.success() => {
            let text = output.stdout_text();
            if required.iter().all(|needle| text.contains(needle)) {
                evidence.push(format!("{label}: exit 0"));
                Ok(true)
            } else {
                evidence.push(format!("{label}: required flags/text not advertised"));
                Ok(false)
            }
        }
        Ok(output) => {
            evidence.push(evidence_exit(label, &output));
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

/// Runs the Mac-side help probes that prove the local agent can pair a surface, run the
/// daemon, and read its path map. Failures are missing capabilities, not `Unreachable`:
/// the home gate is still a valid read when the local binary is old or absent.
async fn probe_local_agent_commands(
    runner: &dyn CommandRunner,
    evidence: &mut Vec<String>,
) -> Vec<String> {
    let mut missing = Vec::new();
    let checks: [(&str, &[&str], &[&str], &str); 3] = [
        (
            "vowp agent pair --help",
            &["agent", "pair", "--help"],
            &["--code-file", "--surface", "--session"],
            "local surface pairing",
        ),
        (
            "vowp agent run --help",
            &["agent", "run", "--help"],
            &["--foreground", "--role"],
            "local agent run",
        ),
        (
            "vowp agent paths --help",
            &["agent", "paths", "--help"],
            &["add", "rm", "ls"],
            "local path map",
        ),
    ];
    for (label, args, required, missing_label) in checks {
        let spec = CommandSpec::new("vowp", args.iter().map(|arg| (*arg).to_owned()).collect())
            .with_timeout(Duration::from_secs(10));
        match runner.run(&spec).await {
            Ok(output) if output.success() => {
                let text = output.stdout_text();
                if required.iter().all(|needle| text.contains(needle)) {
                    evidence.push(format!("{label}: exit 0"));
                } else {
                    evidence.push(format!("{label}: required flags/text not advertised"));
                    missing.push(missing_label.to_owned());
                }
            }
            Ok(output) => {
                evidence.push(evidence_exit(label, &output));
                missing.push(missing_label.to_owned());
            }
            Err(_) => {
                evidence.push(format!("{label}: unavailable"));
                missing.push(missing_label.to_owned());
            }
        }
    }

    // Service and doctor are optional local prerequisites, not Ready gates. Record what
    // the installed local binary advertises so the details drawer can explain a gap.
    for (label, args) in [
        (
            "vowp agent service --help",
            &["agent", "service", "--help"][..],
        ),
        (
            "vowp agent doctor --help",
            &["agent", "doctor", "--help"][..],
        ),
    ] {
        let spec = CommandSpec::new("vowp", args.iter().map(|arg| (*arg).to_owned()).collect())
            .with_timeout(Duration::from_secs(10));
        match runner.run(&spec).await {
            Ok(output) if output.success() => evidence.push(format!("{label}: exit 0 (optional)")),
            Ok(output) => evidence.push(format!("{} (optional)", evidence_exit(label, &output))),
            Err(_) => evidence.push(format!("{label}: unavailable (optional)")),
        }
    }
    missing
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
    /// Present in the merged Phase 3/4 `agent status` document. Older builds omit it;
    /// absence keeps the `local cache` capability `Waiting`.
    #[serde(default)]
    cache_entries: Option<u64>,
    #[serde(default)]
    cache_bytes: Option<u64>,
}

/// Probes the home capability and the Mac-side local agent separately.
pub async fn probe_local_orchestrator(
    runner: &dyn CommandRunner,
    local_runner: Option<&dyn CommandRunner>,
    options: &GateProbeOptions,
) -> (LocalOrchestratorGate, LocalAgentStatus) {
    let (local_agent, local_cache_reported) = probe_local_agent(local_runner).await;
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
    let Some(project) = project_token(options) else {
        evidence.push(
            "vowp node ls --json: no usable active project; node reads not attempted".to_owned(),
        );
        return (
            LocalOrchestratorGate::WaitingForVowpPhase3 {
                missing: vec!["active project".to_owned()],
                detected_version: Some(version),
                evidence,
            },
            local_agent,
        );
    };
    let node_list_argv = {
        let mut argv = base_argv(options, true);
        argv.extend([
            "--project".to_owned(),
            project.clone(),
            "node".to_owned(),
            "ls".to_owned(),
        ]);
        argv
    };
    let node_list = run_remote(runner, options, &node_list_argv).await;
    // The v0.3 `Node` inventory row has no `kind`/`status`; only its name is used here.
    // An active local agent is proven by `node show`, never inferred from this shape.
    let names: Vec<String> = match node_list {
        Ok(output) if output.success() => {
            match serde_json::from_slice::<Vec<NodeListRow>>(&output.stdout) {
                Ok(rows) => {
                    evidence.push("vowp node ls --json: exit 0".to_owned());
                    rows.into_iter().map(|row| row.name).collect()
                }
                Err(_) => {
                    evidence.push("vowp node ls --json: not a Node[] document".to_owned());
                    return (
                        LocalOrchestratorGate::Incompatible {
                            detected: "vowp node ls --json is not the documented array".to_owned(),
                            required: "a JSON array of node rows".to_owned(),
                            evidence,
                        },
                        local_agent,
                    );
                }
            }
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
    let mut active_agent_not_online = false;
    let mut malformed_card = false;
    for name in names.iter().take(16) {
        let argv = {
            let mut argv = base_argv(options, true);
            argv.extend([
                "--project".to_owned(),
                project.clone(),
                "node".to_owned(),
                "show".to_owned(),
                safe_token(name, 64),
            ]);
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
            malformed_card = true;
            continue;
        };
        // A card without kind/status/presence cannot prove an active local agent.
        // Absence is not an agent; the contract asks for `Incompatible` here.
        let (Some(kind), Some(status), Some(presence)) = (
            card.kind.as_deref(),
            card.status.as_deref(),
            card.presence.as_deref(),
        ) else {
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
        };
        if card.name.as_deref().is_none_or(str::is_empty) {
            return (
                LocalOrchestratorGate::Incompatible {
                    detected: format!("node card for {} is missing its name", safe_token(name, 64)),
                    required: "NodeCard with a name".to_owned(),
                    evidence,
                },
                local_agent,
            );
        }
        evidence.push(format!(
            "vowp node show --json: kind={} status={} presence={}",
            safe_token(kind, 16),
            safe_token(status, 16),
            safe_token(presence, 16)
        ));
        if kind == "agent" && status == "active" {
            if presence == "online" {
                evidence.push(format!(
                    "vowp node show {} --json: kind=agent status=active presence=online",
                    safe_token(name, 64)
                ));
                active_agent = Some(card);
                break;
            }
            active_agent_not_online = true;
        }
    }
    let Some(agent_card) = active_agent else {
        if malformed_card {
            return (
                LocalOrchestratorGate::Incompatible {
                    detected: "a node card is not a NodeCard".to_owned(),
                    required: "NodeCard with kind, status and presence".to_owned(),
                    evidence,
                },
                local_agent,
            );
        }
        if active_agent_not_online {
            evidence.push("local agent node is not online".to_owned());
        }
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
    match agent_card
        .agent_version
        .as_deref()
        .map(str::trim)
        .filter(|version| !version.is_empty())
    {
        Some(agent_version) => evidence.push(format!(
            "node agent version: {}",
            safe_token(agent_version, 40)
        )),
        None => {
            evidence.push("node agent version: not reported".to_owned());
            missing.push("agent version not reported".to_owned());
        }
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
        Ok(output) if output.success() => {
            let documented = serde_json::from_slice::<serde_json::Value>(&output.stdout)
                .ok()
                .is_some_and(|doc| {
                    ["nodes", "assignments", "ready"]
                        .iter()
                        .all(|key| doc.get(*key).is_some_and(serde_json::Value::is_array))
                });
            if documented {
                evidence.push("vowp offload status --json: exit 0".to_owned());
            } else {
                evidence.push("vowp offload status --json: not the documented shape".to_owned());
                missing.push("offload executor (Phase 2)".to_owned());
            }
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

    // The merged v0.4 CLI surface. `surface pair`/`rotate` repeat the local-node meaning
    // in their after-help; the top-level `surface --help` sentence and the global SSH
    // selector are never accepted on their own. `agent surface start|attach|detach` is a
    // design sketch, not a command in the merged source, so it is not probed.
    let home_checks: [(&str, &[&str], &[&str], &str); 8] = [
        (
            "vowp surface pair --help",
            &["surface", "pair", "--help"],
            &[
                "--session",
                "--kind",
                "--app",
                "--replace",
                SURFACE_PAIR_LOCAL_NODE_TEXT[0],
                SURFACE_PAIR_LOCAL_NODE_TEXT[1],
            ],
            "surface pairing",
        ),
        (
            "vowp surface ls --help",
            &["surface", "ls", "--help"],
            &[],
            "surface read model",
        ),
        (
            "vowp surface revoke --help",
            &["surface", "revoke", "--help"],
            &["--yes"],
            "surface revocation",
        ),
        (
            "vowp surface rotate --help",
            &["surface", "rotate", "--help"],
            &[
                "--wait",
                "--grace",
                SURFACE_PAIR_LOCAL_NODE_TEXT[0],
                SURFACE_PAIR_LOCAL_NODE_TEXT[1],
            ],
            "surface rotation",
        ),
        (
            "vowp mcp --help",
            &["mcp", "--help"],
            &["--agent-socket", "--surface"],
            "MCP proxy",
        ),
        (
            "vowp orchestrator new --help",
            &["orchestrator", "new", "--help"],
            &[
                "--app",
                "--force",
                ORCHESTRATOR_NEW_LOCAL_NODE_TEXT[0],
                ORCHESTRATOR_NEW_LOCAL_NODE_TEXT[1],
            ],
            "orchestrator materialization",
        ),
        (
            "vowp orchestrator stop --help",
            &["orchestrator", "stop", "--help"],
            &[],
            "orchestrator lifecycle",
        ),
        (
            "vowp orchestrator resume --help",
            &["orchestrator", "resume", "--help"],
            &[],
            "orchestrator lifecycle",
        ),
    ];
    for (label, args, required, missing_label) in home_checks {
        match probe_home_help(runner, options, label, args, required, &mut evidence).await {
            Ok(true) => {}
            Ok(false) => missing.push(missing_label.to_owned()),
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

    let orchestrator_ls_argv = {
        let mut argv = base_argv(options, true);
        argv.extend(["orchestrator".to_owned(), "ls".to_owned()]);
        argv
    };
    match run_remote(runner, options, &orchestrator_ls_argv).await {
        Ok(output) if output.success() => {
            if serde_json::from_slice::<Vec<serde_json::Value>>(&output.stdout).is_ok() {
                evidence.push("vowp orchestrator ls --json: exit 0".to_owned());
            } else {
                evidence.push("vowp orchestrator ls --json: not an array".to_owned());
                missing.push("orchestrator read model".to_owned());
            }
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

    // The Mac-side half is a required part of Ready. A local agent that is not installed,
    // not enrolled, stopped, revoked, or reconnecting keeps the gate `Waiting`; it can
    // never be `Ready` on the home probe alone.
    if let Some(local) = local_runner {
        if local_agent.state != LocalAgentState::NotInstalled {
            missing.extend(probe_local_agent_commands(local, &mut evidence).await);
        }
    }
    if local_agent.state != LocalAgentState::Connected {
        evidence.push(format!(
            "vowp agent status --json: {}",
            local_agent_state_label(local_agent.state)
        ));
        missing.push("local agent connection".to_owned());
    } else if local_agent.protocol.as_deref() != Some("vowp-agent/1") {
        evidence.push("vowp agent status --json: local protocol not reported".to_owned());
        missing.push("local agent protocol".to_owned());
    }
    if local_runner.is_some() && local_agent.state != LocalAgentState::NotInstalled {
        if local_cache_reported {
            evidence.push("vowp agent status --json: cache stats reported".to_owned());
        } else {
            evidence.push("vowp agent status --json: cache stats not reported".to_owned());
            missing.push("local cache".to_owned());
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
                "surface_pair".to_owned(),
                "surface_list".to_owned(),
                "surface_revoke".to_owned(),
                "surface_rotate".to_owned(),
                "mcp_agent_socket".to_owned(),
                "orchestrator_new_node".to_owned(),
                "orchestrator_lifecycle".to_owned(),
                "orchestrator_read".to_owned(),
                "local_agent_connected".to_owned(),
                "local_surface_pair".to_owned(),
                "local_agent_run".to_owned(),
                "backfill".to_owned(),
                "local_path_map".to_owned(),
                "local_cache".to_owned(),
            ],
            evidence,
        },
        local_agent,
    )
}

async fn probe_local_agent(local_runner: Option<&dyn CommandRunner>) -> (LocalAgentStatus, bool) {
    let Some(runner) = local_runner else {
        return (
            LocalAgentStatus {
                state: LocalAgentState::NotInstalled,
                last_error: Some("local vowp is not available".to_owned()),
                ..LocalAgentStatus::default()
            },
            false,
        );
    };
    let argv = vec!["--json".to_owned(), "agent".to_owned(), "status".to_owned()];
    let spec =
        crate::ssh::command::CommandSpec::new("vowp", argv).with_timeout(Duration::from_secs(10));
    match runner.run(&spec).await {
        Ok(output) if output.status == 3 => (
            LocalAgentStatus {
                state: LocalAgentState::NotEnrolled,
                ..LocalAgentStatus::default()
            },
            false,
        ),
        Ok(output) if !output.success() => (
            LocalAgentStatus {
                state: LocalAgentState::Unknown,
                last_error: Some(format!("vowp agent status exited {}", output.status)),
                ..LocalAgentStatus::default()
            },
            false,
        ),
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
                let cache_reported = doc.cache_entries.is_some() && doc.cache_bytes.is_some();
                (
                    LocalAgentStatus {
                        state: if incompatible {
                            LocalAgentState::Incompatible
                        } else {
                            state
                        },
                        node: doc.node.map(|node| safe_token(&node, 64)),
                        protocol: doc.protocol.map(|protocol| safe_protocol(&protocol)),
                        last_heartbeat: doc.last_heartbeat,
                        last_error: doc.last_error.map(|error| safe_text(&error)),
                    },
                    cache_reported,
                )
            }
            Err(_) => (
                LocalAgentStatus {
                    state: LocalAgentState::Unknown,
                    last_error: Some("vowp agent status output is not valid JSON".to_owned()),
                    ..LocalAgentStatus::default()
                },
                false,
            ),
        },
        Err(error) => (
            LocalAgentStatus {
                state: LocalAgentState::NotInstalled,
                last_error: Some(safe_detail(&error)),
                ..LocalAgentStatus::default()
            },
            false,
        ),
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
                capabilities: vec!["surface_pair".to_owned()],
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

    #[test]
    fn engine_host_gate_cache_isolates_hosts_and_invalidates() {
        let cache = HostGateCache::new();
        let probed = Timestamp::parse("2026-11-14T12:00:00.000Z").unwrap();
        let mut snapshot = snapshot(GateFeatureFlags::default());
        snapshot.probed_at = probed;
        cache.put("host-a", snapshot.clone());

        assert!(cache.get("host-a", probed.plus_secs(600)).is_some());
        assert!(cache.get("host-a", probed.plus_secs(601)).is_none());
        assert!(cache.get("host-b", probed).is_none());

        cache.invalidate("host-a");
        assert!(cache.get("host-a", probed).is_none());

        cache.put("host-a", snapshot);
        cache.clear();
        assert!(cache.get("host-a", probed).is_none());
    }
}
