//! `status.json`: the last local view of the agent connection.
//!
//! The file is written 0600 on each heartbeat and is safe to display: it holds no private key,
//! enrollment code, surface token or other secret value.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use vowp_protocol::{NodeName, Timestamp};

use crate::cache::Cache;
use crate::config::{STATUS_FILE, write_private};
use crate::error::AgentError;
use crate::surface_state::{SurfaceStatus, state_of_dir, surface_statuses};

/// The connection state shown by `vowp agent status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    /// No status has been written yet.
    #[default]
    Unknown,
    /// Opening the transport and completing the handshake.
    Connecting,
    /// The handshake succeeded and heartbeats are flowing.
    Connected,
    /// The connection dropped and a retry is scheduled.
    Reconnecting,
    /// The home revoked this node.
    Revoked,
    /// Another connection took over.
    Superseded,
    /// The agent is not running.
    Stopped,
}

/// One local status document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentStatus {
    /// Current connection state.
    #[serde(default)]
    pub connection: ConnectionState,
    /// Node name assigned by the home.
    pub node: Option<NodeName>,
    /// Home identity.
    pub home: Option<String>,
    /// Negotiated protocol (`vowp-agent/1`).
    pub protocol: Option<String>,
    /// When the last heartbeat was sent.
    pub last_heartbeat: Option<Timestamp>,
    /// Last connection, transport or policy error.
    pub last_error: Option<String>,
    /// Bytes waiting in the local spool.
    pub pending_spool_bytes: u64,
    /// Short fingerprint of the node key.
    pub key_fingerprint: Option<String>,
    /// Earliest local surface-token expiry, when a token is paired.
    pub surface_token_expires_at: Option<Timestamp>,
    /// Number of entries in the local read cache.
    #[serde(default)]
    pub cache_entries: u64,
    /// Bytes on disk used by the local read cache.
    #[serde(default)]
    pub cache_bytes: u64,
    /// When the oldest local cache entry was stored.
    #[serde(default)]
    pub cache_oldest_at: Option<Timestamp>,
}

impl Default for AgentStatus {
    fn default() -> Self {
        Self {
            connection: ConnectionState::Unknown,
            node: None,
            home: None,
            protocol: None,
            last_heartbeat: None,
            last_error: None,
            pending_spool_bytes: 0,
            key_fingerprint: None,
            surface_token_expires_at: None,
            cache_entries: 0,
            cache_bytes: 0,
            cache_oldest_at: None,
        }
    }
}

impl AgentStatus {
    /// Returns `state_dir/status.json`.
    pub fn file_path(state_dir: &Path) -> PathBuf {
        state_dir.join(STATUS_FILE)
    }

    /// Loads `status.json`; `Ok(None)` means it has never been written.
    pub fn load(state_dir: &Path) -> Result<Option<Self>, AgentError> {
        let path = Self::file_path(state_dir);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(AgentError::Io(error)),
        };
        Ok(Some(serde_json::from_str(&text)?))
    }

    /// Writes `status.json` mode 0600.
    ///
    /// The document also carries the local `surfaces` array, computed from the state
    /// directory at write time. The array has ids, prefixes, expiries and states, never a
    /// token; a caller reading [`AgentStatus`] back gets the same fields without it.
    pub fn write(&self, state_dir: &Path) -> Result<(), AgentError> {
        let document = StatusDocument {
            status: self,
            surfaces: surface_statuses(state_dir),
        };
        let bytes = serde_json::to_vec_pretty(&document)?;
        write_private(&Self::file_path(state_dir), &bytes)
    }

    /// Returns the local surface array for the status document.
    pub fn surfaces(&self, state_dir: &Path) -> Vec<SurfaceStatus> {
        surface_statuses(state_dir)
    }

    /// Fills the cache fields from a directory walk below `state_dir`.
    ///
    /// Only counts and the oldest timestamp are read; no cached value, argument or path is
    /// copied into the status document.
    pub fn refresh_cache(&mut self, state_dir: &Path) {
        let stats = Cache::new(state_dir).stats();
        self.cache_entries = stats.entries;
        self.cache_bytes = stats.bytes;
        self.cache_oldest_at = stats.oldest_stored_at;
    }
}

/// The `status.json` document: the [`AgentStatus`] fields plus the local surface array.
#[derive(Serialize)]
struct StatusDocument<'a> {
    #[serde(flatten)]
    status: &'a AgentStatus,
    /// Local surface id, prefix, expiry and state; never a token.
    surfaces: Vec<SurfaceStatus>,
}

/// Returns the number of bytes below `state_dir/spool`.
pub fn pending_spool_bytes(state_dir: &Path) -> u64 {
    let mut total = 0u64;
    walk_bytes(&state_dir.join("spool"), &mut total);
    total
}

fn walk_bytes(dir: &Path, total: &mut u64) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            walk_bytes(&entry.path(), total);
        } else if file_type.is_file() {
            if let Ok(metadata) = entry.metadata() {
                *total = total.saturating_add(metadata.len());
            }
        }
    }
}

/// Returns the earliest active `surfaces/<id>/expires_at` value, when one exists.
///
/// Phase 0 never pairs a surface, so this is normally `None`. The surface-pairing job writes an
/// RFC 3339 expiry next to each token; `status` reports the earliest one. A surface marked
/// `needs_pair` or `revoked` is not in the active set even though its dead expiry file is kept
/// for inspection.
pub fn surface_token_expiry(state_dir: &Path) -> Option<Timestamp> {
    let entries = fs::read_dir(state_dir.join("surfaces")).ok()?;
    let mut earliest: Option<Timestamp> = None;
    for entry in entries.flatten() {
        if !state_of_dir(&entry.path()).is_active() {
            continue;
        }
        let Ok(text) = fs::read_to_string(entry.path().join("expires_at")) else {
            continue;
        };
        let Ok(expiry) = Timestamp::parse(text.trim()) else {
            continue;
        };
        earliest = Some(match earliest {
            Some(current) => current.min(expiry),
            None => expiry,
        });
    }
    earliest
}
