//! Phase 0 node enrollment: create a pending node and redeem its one-time code.
//!
//! The home creates an `agent`-kind `pending` node and stores only the code hash. Redemption
//! consumes the code atomically ([`vowp_store::Store::consume_enrollment_code`]), binds the
//! node's ed25519 public key, marks the node active and records its version, protocol,
//! capabilities and initial path map. A code issued before an existing enrollment is stale
//! and cannot re-enroll an active or revoked node.

use vowp_protocol::{AGENT_PROTOCOL, AgentCapabilities, AgentPathMap, NodeName, NodeStatus, Timestamp};
use vowp_store::{EnrollmentCode, EnrollmentKind, NodeCredential, NodePath, Store};

use super::auth;
use super::code::{self, NewEnrollmentCode};
use crate::CoreError;

/// A pending node plus the plaintext code shown once by `vowp node enroll`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingEnrollment {
    /// Node the code enrolls.
    pub node: NodeName,
    /// Plaintext code; callers print it once and never store it.
    pub code: String,
    /// Stored hash of [`Self::code`].
    pub code_hash: String,
    /// Instant at which the code stops redeeming.
    pub expires_at: Timestamp,
}

/// The fields of one `Enroll` frame that the home consumes.
#[derive(Debug, Clone, PartialEq)]
pub struct EnrollRequest {
    /// One-time code read from stdin by the agent.
    pub code: String,
    /// Lowercase hex ed25519 public key.
    pub public_key: String,
    /// Agent binary version.
    pub agent_version: String,
    /// Names-only capabilities.
    pub capabilities: AgentCapabilities,
    /// Initial project path map.
    pub paths: Vec<AgentPathMap>,
}

/// A successfully enrolled node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enrolled {
    /// Enrolled node.
    pub node: NodeName,
}

/// Creates a pending `agent` node and a one-time enrollment code.
///
/// A node that already exists in any enrollment state is a conflict: Phase 0 has no
/// `--force` re-enrollment. The code is returned in plaintext exactly once; only its hash is
/// written to the store.
pub fn create_pending(
    store: &Store,
    node: &NodeName,
    labels: &[String],
    now: Timestamp,
) -> Result<PendingEnrollment, CoreError> {
    if let Some(card) = store.node_card(node, now)? {
        return Err(vowp_store::StoreError::Conflict(format!(
            "node {node} already exists ({:?}); revoke or remove it before enrolling again",
            card.status
        ))
        .into());
    }
    store.insert_pending_node(node, labels, now)?;
    let generated = code::new_enrollment_code(now);
    let NewEnrollmentCode {
        code,
        code_hash,
        expires_at,
    } = generated;
    store.insert_enrollment_code(&EnrollmentCode {
        code_hash: code_hash.clone(),
        node: node.clone(),
        kind: EnrollmentKind::Node,
        surface_id: None,
        expires_at,
        used_at: None,
        created_by: "owner".to_owned(),
        created_at: now,
    })?;
    Ok(PendingEnrollment {
        node: node.clone(),
        code,
        code_hash,
        expires_at,
    })
}

/// Redeems one enrollment code and binds the node's public key.
///
/// The store consumes the code in one immediate transaction, so two racing gateways cannot
/// both redeem it. An expired, unknown or already-used code is refused with a message that
/// never echoes the code.
pub fn redeem(store: &Store, request: &EnrollRequest, now: Timestamp) -> Result<Enrolled, CoreError> {
    if !code::has_code_shape(&request.code) {
        return Err(CoreError::Invalid("enrollment code must be 52 characters".to_owned()));
    }
    if !auth::is_public_key_hex(&request.public_key) {
        return Err(CoreError::Invalid(
            "node public key must be 32 bytes in lowercase hex".to_owned(),
        ));
    }
    validate_paths(&request.paths)?;

    let code_hash = code::hash_code(&request.code);
    let Some(stored) = store.consume_enrollment_code(&code_hash, now)? else {
        return Err(CoreError::Refused(
            "enrollment code is invalid, expired or already used".to_owned(),
        ));
    };
    if stored.kind != EnrollmentKind::Node {
        return Err(CoreError::Invalid("enrollment code is not a node code".to_owned()));
    }
    let Some(card) = store.node_card(&stored.node, now)? else {
        return Err(CoreError::NotFound(format!("node {}", stored.node)));
    };
    match card.status {
        NodeStatus::Pending => {}
        NodeStatus::Active | NodeStatus::Revoked => {
            // A fresh code may rotate an existing node's key, but a code created before the
            // last successful contact is stale: re-enrollment must be an explicit new action.
            let node = store.require_node(&stored.node)?;
            let fresh = node.last_seen_at.is_none_or(|seen| stored.created_at > seen);
            if !fresh {
                return Err(CoreError::Refused(
                    "enrollment code predates the node's current enrollment".to_owned(),
                ));
            }
        }
    }

    // Rotate every previous key; only the key in this frame stays active.
    store.revoke_node_credentials(&stored.node, now)?;
    store.upsert_node_credential(&NodeCredential {
        node: stored.node.clone(),
        key_id: request.public_key.clone(),
        public_key: request.public_key.clone(),
        created_at: now,
        last_used_at: None,
        revoked_at: None,
    })?;
    store.set_node_status(&stored.node, NodeStatus::Active, "", now)?;
    store.touch_node(
        &stored.node,
        now,
        Some(&request.agent_version),
        Some(AGENT_PROTOCOL),
        &capabilities_value(&request.capabilities)?,
    )?;
    for path in &request.paths {
        store.upsert_node_path(&NodePath {
            node: stored.node.clone(),
            project: path.project.clone(),
            local_root: path.local_root.clone(),
            worktree_root: path.worktree_root.clone().unwrap_or_else(|| path.local_root.clone()),
            allow: path.allow.clone(),
            updated_at: now,
        })?;
    }
    Ok(Enrolled { node: stored.node })
}

/// Revokes a node and every active credential, returning how many credentials changed.
pub fn revoke(store: &Store, node: &NodeName, now: Timestamp) -> Result<u64, CoreError> {
    if store.node_card(node, now)?.is_none() {
        return Err(CoreError::NotFound(format!("node {node}")));
    }
    let revoked = store.revoke_node_credentials(node, now)?;
    store.set_node_status(node, NodeStatus::Revoked, "", now)?;
    Ok(revoked)
}

/// Serialises names-only capabilities for the `nodes.capabilities` column.
pub(crate) fn capabilities_value(capabilities: &AgentCapabilities) -> Result<serde_json::Value, CoreError> {
    serde_json::to_value(capabilities)
        .map_err(vowp_protocol::ProtocolError::from)
        .map_err(CoreError::from)
}

/// Refuses a path map entry the home cannot store safely.
fn validate_paths(paths: &[AgentPathMap]) -> Result<(), CoreError> {
    for path in paths {
        if path.local_root.is_empty() || !path.local_root.starts_with('/') || path.local_root.contains('\0') {
            return Err(CoreError::Invalid(format!(
                "project {} local root must be an absolute path",
                path.project
            )));
        }
        if let Some(root) = &path.worktree_root {
            if root.is_empty() || !root.starts_with('/') || root.contains('\0') {
                return Err(CoreError::Invalid(format!(
                    "project {} worktree root must be an absolute path",
                    path.project
                )));
            }
        }
    }
    Ok(())
}
