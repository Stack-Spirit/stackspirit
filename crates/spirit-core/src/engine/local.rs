//! D6 backend guard and local-orchestrator state.
//!
//! Every local-orchestrator action re-checks the same prerequisites from the gate
//! snapshot and the latest Mac-side node/path-map state. A renderer is never the
//! authority: a stale "Ready" card cannot enable an action on its own. There is no
//! force switch and no hidden retry with an old credential.
//!
//! Read-only actions (surface list, agent status/paths, orchestrator list) remain
//! available while the gate is `Waiting` so the owner can see why D6 is disabled.
//! Explicit cleanup actions (surface revoke, orchestrator stop) also stay available;
//! the refusal in D6-READINESS §4 applies to *starting* a local surface.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::engine::gate::{
    GateSnapshot, LocalAgentState, LocalAgentStatus, LocalOrchestratorGate,
    local_orchestrator_enabled,
};
use crate::error::{SpiritError, SpiritErrorCode};
use crate::wire::{AGENT_PROTOCOL, AgentPathMap, NodeCard, NodePresence, NodeStatus, ProjectName};

/// The D6 action a guard is about to allow or refuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalOrchestratorAction {
    /// `vowp surface pair`.
    SurfacePair,
    /// `vowp surface ls`.
    SurfaceList,
    /// `vowp surface rotate`.
    SurfaceRotate,
    /// `vowp surface revoke`.
    SurfaceRevoke,
    /// `vowp agent status --json`.
    AgentStatus,
    /// `vowp agent pair`.
    AgentPair,
    /// `vowp agent paths ls` / `add` / `rm`.
    AgentPaths,
    /// `vowp orchestrator new`.
    OrchestratorNew,
    /// `vowp orchestrator ls`.
    OrchestratorList,
    /// `vowp orchestrator stop`.
    OrchestratorStop,
    /// `vowp orchestrator resume`.
    OrchestratorResume,
}

impl LocalOrchestratorAction {
    /// Returns the stable action label used in refusals.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SurfacePair => "surface_pair",
            Self::SurfaceList => "surface_list",
            Self::SurfaceRotate => "surface_rotate",
            Self::SurfaceRevoke => "surface_revoke",
            Self::AgentStatus => "agent_status",
            Self::AgentPair => "agent_pair",
            Self::AgentPaths => "agent_paths",
            Self::OrchestratorNew => "orchestrator_new",
            Self::OrchestratorList => "orchestrator_list",
            Self::OrchestratorStop => "orchestrator_stop",
            Self::OrchestratorResume => "orchestrator_resume",
        }
    }

    /// Returns true when this action starts a local surface or app host.
    ///
    /// Only these actions require a `Ready` gate. Listing and explicit cleanup remain
    /// available so a waiting, revoked or offline node can be diagnosed and stopped.
    #[must_use]
    pub const fn requires_ready(self) -> bool {
        matches!(
            self,
            Self::SurfacePair
                | Self::SurfaceRotate
                | Self::AgentPair
                | Self::OrchestratorNew
                | Self::OrchestratorResume
        )
    }

    /// Returns true when the action materializes a project-bound local host.
    #[must_use]
    pub const fn requires_path_map(self) -> bool {
        matches!(self, Self::OrchestratorNew | Self::OrchestratorResume)
    }
}

/// The exact prerequisite a guard refused, so callers and tests can assert it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalOrchestratorPrerequisite {
    /// The build's `local_orchestrator_actions` feature switch is off.
    FeatureFlag,
    /// `LocalOrchestratorGate` is not `Ready`.
    Gate,
    /// The local agent is not `connected`.
    AgentConnection,
    /// The negotiated agent protocol is not `vowp-agent/1`.
    AgentProtocol,
    /// The target node is not the active local-agent node.
    NodeName,
    /// The node's enrollment status is not `active`.
    NodeStatus,
    /// The node's computed presence is not `online`.
    NodePresence,
    /// The local path map is missing or invalid.
    PathMap,
}

impl LocalOrchestratorPrerequisite {
    /// Returns the exact prerequisite label used in a refusal.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FeatureFlag => "local_orchestrator_actions",
            Self::Gate => "local_orchestrator_gate",
            Self::AgentConnection => "local_agent_connection",
            Self::AgentProtocol => "agent_protocol",
            Self::NodeName => "node_name",
            Self::NodeStatus => "node_status",
            Self::NodePresence => "node_presence",
            Self::PathMap => "local_path_map",
        }
    }
}

/// The validity of one project's Mac-local path map.
///
/// The reason is generic and never contains an absolute local path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LocalPathMapStatus {
    /// The action does not materialize a project-bound host.
    NotRequired,
    /// No map for the project is known on this Mac.
    Missing,
    /// The map validated and can produce `PathRef` values.
    Valid,
    /// The map is present but rejected.
    Invalid {
        /// Generic, path-free reason.
        reason: String,
    },
}

impl LocalPathMapStatus {
    /// Builds a status from one Mac-local map.
    #[must_use]
    pub fn from_map(map: &AgentPathMap) -> Self {
        match map.validate() {
            Ok(()) => Self::Valid,
            Err(error) => Self::Invalid {
                reason: error.message,
            },
        }
    }
}

/// The latest local node state used by the guard. Only redacted fields are kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalNodeState {
    /// Node name.
    pub name: String,
    /// Enrollment state.
    pub status: String,
    /// Computed presence.
    pub presence: String,
    /// Agent protocol, when known.
    pub protocol: Option<String>,
    /// Agent binary version, when known.
    pub agent_version: Option<String>,
}

impl LocalNodeState {
    /// Builds the guard state from a `vowp node show --json` card.
    #[must_use]
    pub fn from_card(card: &NodeCard) -> Self {
        Self {
            name: card.name.to_string(),
            status: card.status.as_str().to_owned(),
            presence: card.presence.as_str().to_owned(),
            protocol: card.protocol.clone(),
            agent_version: card.agent_version.clone(),
        }
    }
}

/// D6 local state learned from the Mac. It never carries an absolute path or secret.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalOrchestratorState {
    /// Latest active node card, when one was read.
    pub node: Option<LocalNodeState>,
    /// Per-project path-map validity.
    pub path_maps: BTreeMap<String, LocalPathMapStatus>,
}

impl LocalOrchestratorState {
    /// Replaces the known path maps from a fresh `vowp agent paths ls --json`.
    pub fn set_path_maps(&mut self, maps: &[AgentPathMap]) {
        self.path_maps.clear();
        for map in maps {
            self.set_path_map(map);
        }
    }

    /// Records one project's path-map validity.
    pub fn set_path_map(&mut self, map: &AgentPathMap) {
        self.path_maps.insert(
            map.project.as_str().to_owned(),
            LocalPathMapStatus::from_map(map),
        );
    }

    /// Forgets one project's path-map validity.
    pub fn remove_path_map(&mut self, project: &str) {
        self.path_maps.remove(project);
    }

    /// Returns one project's path-map status.
    #[must_use]
    pub fn path_map(&self, project: Option<&str>) -> LocalPathMapStatus {
        match project {
            None => LocalPathMapStatus::NotRequired,
            Some(project) => self
                .path_maps
                .get(project)
                .cloned()
                .unwrap_or(LocalPathMapStatus::Missing),
        }
    }
}

/// The re-checked prerequisites for one D6 action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalOrchestratorGuard {
    /// Whether the build advertises the D6 action flag.
    pub feature_enabled: bool,
    /// Current gate snapshot.
    pub gate: LocalOrchestratorGate,
    /// Current Mac-side agent status.
    pub local_agent: LocalAgentStatus,
    /// Latest local node state, when read.
    pub node: Option<LocalNodeState>,
    /// Path-map validity for the project the action binds.
    pub path_map: LocalPathMapStatus,
}

impl LocalOrchestratorGuard {
    /// Builds a guard from the latest gate snapshot and local-orchestrator state.
    #[must_use]
    pub fn from_snapshot(snapshot: &GateSnapshot, state: &LocalOrchestratorState) -> Self {
        Self {
            feature_enabled: snapshot.features.local_orchestrator_actions,
            gate: snapshot.local_orchestrator.clone(),
            local_agent: snapshot.local_agent.clone(),
            node: state.node.clone(),
            path_map: LocalPathMapStatus::NotRequired,
        }
    }

    /// Binds the path-map status checked for a project-bound action.
    #[must_use]
    pub fn with_path_map(mut self, path_map: LocalPathMapStatus) -> Self {
        self.path_map = path_map;
        self
    }

    /// Binds one project's path-map status from the stored state.
    #[must_use]
    pub fn for_project(mut self, state: &LocalOrchestratorState, project: Option<&str>) -> Self {
        self.path_map = state.path_map(project);
        self
    }

    /// Re-checks the action and returns an exact refusal when a prerequisite is missing.
    ///
    /// Read-only and explicit cleanup actions return `Ok`; they exist to diagnose and
    /// stop a broken local surface.
    pub fn ensure(
        &self,
        action: LocalOrchestratorAction,
        target_node: Option<&str>,
    ) -> Result<(), SpiritError> {
        if !action.requires_ready() {
            return Ok(());
        }
        self.ensure_gate()?;
        self.ensure_agent_connection()?;
        self.ensure_feature_flag()?;
        self.ensure_protocol()?;
        self.ensure_node(target_node)?;
        if action.requires_path_map() {
            self.ensure_path_map()?;
        }
        Ok(())
    }

    fn ensure_gate(&self) -> Result<(), SpiritError> {
        match &self.gate {
            LocalOrchestratorGate::Ready { .. } => Ok(()),
            LocalOrchestratorGate::WaitingForVowpPhase3 { missing, .. } => Err(refusal(
                LocalOrchestratorPrerequisite::Gate,
                format!(
                    "local orchestrator gate is not ready; missing {}",
                    join_labels(missing)
                ),
            )),
            LocalOrchestratorGate::Incompatible {
                detected, required, ..
            } => Err(refusal(
                LocalOrchestratorPrerequisite::AgentProtocol,
                format!("protocol incompatible: detected {detected}, required {required}"),
            )),
            LocalOrchestratorGate::Unreachable { detail } => Err(refusal(
                LocalOrchestratorPrerequisite::Gate,
                format!("local orchestrator gate is unreachable: {detail}"),
            )),
            LocalOrchestratorGate::Unknown | LocalOrchestratorGate::Probing => Err(refusal(
                LocalOrchestratorPrerequisite::Gate,
                "local orchestrator gate has not completed a probe",
            )),
        }
    }

    fn ensure_agent_connection(&self) -> Result<(), SpiritError> {
        if self.local_agent.state == LocalAgentState::Connected {
            Ok(())
        } else {
            Err(refusal(
                LocalOrchestratorPrerequisite::AgentConnection,
                format!(
                    "local agent is not connected (state {})",
                    self.local_agent.state.as_str()
                ),
            ))
        }
    }

    fn ensure_feature_flag(&self) -> Result<(), SpiritError> {
        if self.feature_enabled {
            Ok(())
        } else {
            Err(refusal(
                LocalOrchestratorPrerequisite::FeatureFlag,
                "actions are disabled in this build; the gate must be Ready with a connected local agent",
            ))
        }
    }

    fn ensure_protocol(&self) -> Result<(), SpiritError> {
        if let LocalOrchestratorGate::Ready { agent_protocol, .. } = &self.gate {
            if agent_protocol != AGENT_PROTOCOL {
                return Err(refusal(
                    LocalOrchestratorPrerequisite::AgentProtocol,
                    format!(
                        "protocol incompatible: detected {agent_protocol}, required {AGENT_PROTOCOL}"
                    ),
                ));
            }
        }
        if let Some(protocol) = self.local_agent.protocol.as_deref() {
            if protocol != AGENT_PROTOCOL {
                return Err(refusal(
                    LocalOrchestratorPrerequisite::AgentProtocol,
                    format!(
                        "protocol incompatible: detected {protocol}, required {AGENT_PROTOCOL}"
                    ),
                ));
            }
        }
        Ok(())
    }

    fn ensure_node(&self, target_node: Option<&str>) -> Result<(), SpiritError> {
        let gate_node = match &self.gate {
            LocalOrchestratorGate::Ready { node, .. } => node.as_deref(),
            _ => None,
        };
        if let Some(node) = &self.node {
            // The stored card is evidence only when the gate validated the same node.
            // A stale active/online card from another connection must not authorize
            // an action the gate did not validate.
            match gate_node {
                Some(active) if node.name == active => {}
                Some(active) => {
                    return Err(refusal(
                        LocalOrchestratorPrerequisite::NodeName,
                        format!(
                            "node {} does not match the gate's active local-agent node {active}",
                            node.name
                        ),
                    ));
                }
                None => {
                    return Err(refusal(
                        LocalOrchestratorPrerequisite::NodeName,
                        format!(
                            "node {} was not validated by the gate's active local-agent node",
                            node.name
                        ),
                    ));
                }
            }
            if node.status != NodeStatus::Active.as_str() {
                return Err(refusal(
                    LocalOrchestratorPrerequisite::NodeStatus,
                    format!(
                        "node {} status is {}; D6 requires active",
                        node.name, node.status
                    ),
                ));
            }
            if node.presence != NodePresence::Online.as_str() {
                return Err(refusal(
                    LocalOrchestratorPrerequisite::NodePresence,
                    format!(
                        "node {} presence is {}; D6 requires online",
                        node.name, node.presence
                    ),
                ));
            }
            if let Some(protocol) = node.protocol.as_deref() {
                if protocol != AGENT_PROTOCOL {
                    return Err(refusal(
                        LocalOrchestratorPrerequisite::AgentProtocol,
                        format!(
                            "protocol incompatible: detected {protocol}, required {AGENT_PROTOCOL}"
                        ),
                    ));
                }
            }
            if let Some(target) = target_node {
                if target != node.name {
                    return Err(refusal(
                        LocalOrchestratorPrerequisite::NodeName,
                        format!(
                            "node {target} is not the active local-agent node {}",
                            node.name
                        ),
                    ));
                }
            }
            return Ok(());
        }
        if let Some(target) = target_node {
            match gate_node {
                Some(active) if target == active => {}
                Some(active) => {
                    return Err(refusal(
                        LocalOrchestratorPrerequisite::NodeName,
                        format!("node {target} is not the active local-agent node {active}"),
                    ));
                }
                None => {
                    return Err(refusal(
                        LocalOrchestratorPrerequisite::NodeName,
                        format!(
                            "node {target} was not validated by the gate's active local-agent node"
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    fn ensure_path_map(&self) -> Result<(), SpiritError> {
        match &self.path_map {
            LocalPathMapStatus::NotRequired | LocalPathMapStatus::Valid => Ok(()),
            LocalPathMapStatus::Missing => Err(refusal(
                LocalOrchestratorPrerequisite::PathMap,
                "local path map is missing for the project",
            )),
            LocalPathMapStatus::Invalid { reason } => Err(refusal(
                LocalOrchestratorPrerequisite::PathMap,
                format!("local path map is invalid: {reason}"),
            )),
        }
    }
}

/// Returns true only when a context can start a D6 action.
///
/// This is the free-function spelling from `D6-READINESS.md` §2.1. It re-checks the
/// gate and the Mac agent connection; callers that also hold path-map or node state
/// must use [`LocalOrchestratorGuard::ensure`].
#[must_use]
pub fn local_orchestrator_action_enabled(
    gate: &LocalOrchestratorGate,
    local_agent: LocalAgentState,
) -> bool {
    local_orchestrator_enabled(gate, local_agent == LocalAgentState::Connected)
}

/// Builds the D6 refusal error with the exact prerequisite label.
#[must_use]
pub fn refusal(
    prerequisite: LocalOrchestratorPrerequisite,
    detail: impl Into<String>,
) -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Gate,
        format!(
            "local orchestrator action refused: missing {}: {}",
            prerequisite.as_str(),
            detail.into()
        ),
    )
    .with_hint("Re-probe the local orchestrator gate and resolve the named prerequisite.")
}

fn join_labels(labels: &[String]) -> String {
    if labels.is_empty() {
        "the required capability".to_owned()
    } else {
        labels.join(", ")
    }
}

/// Returns the safe state label for one local-orchestrator action.
#[must_use]
pub fn action_label(action: LocalOrchestratorAction) -> &'static str {
    action.as_str()
}

/// Validates a project name for a D6 action.
pub fn ensure_project(project: &str) -> Result<ProjectName, SpiritError> {
    ProjectName::parse(project)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::gate::{GateFeatureFlags, GateSnapshot};
    use crate::time::Timestamp;

    fn ready_snapshot() -> GateSnapshot {
        GateSnapshot {
            probed_at: Timestamp::epoch(),
            local_orchestrator: LocalOrchestratorGate::Ready {
                agent_protocol: AGENT_PROTOCOL.to_owned(),
                node: Some("mac-studio".to_owned()),
                capabilities: vec!["surface_pair".to_owned()],
                evidence: vec!["test".to_owned()],
            },
            asif: crate::engine::gate::AsifGate::Unknown,
            local_agent: LocalAgentStatus {
                state: LocalAgentState::Connected,
                node: Some("mac-studio".to_owned()),
                protocol: Some(AGENT_PROTOCOL.to_owned()),
                last_heartbeat: None,
                last_error: None,
            },
            features: GateFeatureFlags {
                asif_actions: false,
                local_orchestrator_actions: true,
            },
        }
    }

    #[test]
    fn action_guard_allows_ready_connected_and_refuses_each_missing_prerequisite() {
        let snapshot = ready_snapshot();
        let state = LocalOrchestratorState::default();
        let guard = LocalOrchestratorGuard::from_snapshot(&snapshot, &state);
        assert!(
            guard
                .ensure(LocalOrchestratorAction::SurfacePair, Some("mac-studio"))
                .is_ok()
        );

        let mut stopped = snapshot.clone();
        stopped.local_agent.state = LocalAgentState::Stopped;
        let guard = LocalOrchestratorGuard::from_snapshot(&stopped, &state);
        let error = guard
            .ensure(LocalOrchestratorAction::SurfacePair, None)
            .unwrap_err();
        assert!(error.message.contains("local_agent_connection"), "{error}");
        assert!(error.message.contains("stopped"), "{error}");
    }

    #[test]
    fn cleanup_and_reads_remain_available_when_the_gate_is_waiting() {
        let mut snapshot = ready_snapshot();
        snapshot.local_orchestrator = LocalOrchestratorGate::WaitingForVowpPhase3 {
            missing: vec!["surface rotation".to_owned()],
            detected_version: Some("0.3.0".to_owned()),
            evidence: vec!["test".to_owned()],
        };
        snapshot.local_agent.state = LocalAgentState::NotEnrolled;
        snapshot.features.local_orchestrator_actions = false;
        let guard =
            LocalOrchestratorGuard::from_snapshot(&snapshot, &LocalOrchestratorState::default());
        assert!(
            guard
                .ensure(LocalOrchestratorAction::SurfaceList, None)
                .is_ok()
        );
        assert!(
            guard
                .ensure(LocalOrchestratorAction::SurfaceRevoke, None)
                .is_ok()
        );
        let error = guard
            .ensure(LocalOrchestratorAction::SurfacePair, None)
            .unwrap_err();
        assert!(error.message.contains("local_orchestrator_gate"), "{error}");
        assert!(error.message.contains("surface rotation"), "{error}");
    }

    #[test]
    fn node_and_path_map_refusals_name_the_exact_prerequisite() {
        let snapshot = ready_snapshot();
        let state = LocalOrchestratorState {
            node: Some(LocalNodeState {
                name: "mac-studio".to_owned(),
                status: "revoked".to_owned(),
                presence: "revoked".to_owned(),
                protocol: Some(AGENT_PROTOCOL.to_owned()),
                agent_version: Some("0.4.0".to_owned()),
            }),
            path_maps: BTreeMap::new(),
        };
        let guard = LocalOrchestratorGuard::from_snapshot(&snapshot, &state);
        let error = guard
            .ensure(LocalOrchestratorAction::OrchestratorNew, Some("mac-studio"))
            .unwrap_err();
        assert!(error.message.contains("node_status"), "{error}");
        assert!(error.message.contains("revoked"), "{error}");

        let state = LocalOrchestratorState::default();
        let guard = LocalOrchestratorGuard::from_snapshot(&snapshot, &state).with_path_map(
            LocalPathMapStatus::Invalid {
                reason: "allowed paths must be relative".to_owned(),
            },
        );
        let error = guard
            .ensure(LocalOrchestratorAction::OrchestratorNew, None)
            .unwrap_err();
        assert!(error.message.contains("local_path_map"), "{error}");
        assert!(!error.message.contains("/Users/"), "{error}");
    }
}
