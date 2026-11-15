//! Durable outbox for system-initiated frames sent to one node's live agent.
//!
//! Spec: `docs/design/v0.4/local-orchestrator.md` §4.3. A home command that changes a
//! surface (today `vowp surface rotate`) cannot write to the agent connection itself. It
//! queues one [`AgentFrame`] below the home root instead:
//!
//! ```text
//! agent-frames/<node>/<request>.json    one AgentFrame
//! ```
//!
//! The gateway drains the directory of its authenticated node, sends each frame and
//! removes it after a successful write. A frame queued while the node is offline stays on
//! disk for the next authenticated connection. Files are 0600 in a 0700 directory and are
//! written through a private temporary file and a rename, so a reader never sees a partial
//! frame. A `SurfaceIssued` frame contains a plaintext token: the file is a secret and the
//! caller must never log or print it.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use vowp_protocol::{AgentFrame, NodeName};

use crate::CoreError;
use crate::surface::native::write_private_json;

/// Home directory holding one frame per node and request.
const FRAMES_DIR: &str = "agent-frames";

/// One frame waiting for a node's next authenticated connection.
#[derive(Clone)]
pub struct QueuedAgentFrame {
    /// Stable request id; also the file stem.
    pub request_id: String,
    /// Frame to send on the authenticated connection.
    pub frame: AgentFrame,
}

/// The per-node durable outbox of system-initiated frames.
///
/// Layout below the home root:
///
/// ```text
/// agent-frames/<node>/<request>.json
/// ```
///
/// The gateway for a node reads only that node's directory, so a frame queued for another
/// node is never sent on this connection.
#[derive(Debug, Clone)]
pub struct AgentFrameSpool {
    root: PathBuf,
}

impl AgentFrameSpool {
    /// Builds a spool below the home `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Returns the per-node frame directory.
    fn dir(&self, node: &NodeName) -> PathBuf {
        self.root.join(FRAMES_DIR).join(node.as_str())
    }

    /// Queues one frame and atomically replaces an earlier frame with the same request id.
    ///
    /// A request id is a plain file-name component: one to 128 ASCII alphanumerics, `-` or
    /// `_`. `rotate` uses the surface id, so a second rotation before delivery replaces the
    /// older token instead of leaving several plaintext tokens on disk.
    pub fn queue(&self, node: &NodeName, request_id: &str, frame: &AgentFrame) -> Result<(), CoreError> {
        validate_request_id(request_id)?;
        let dir = self.dir(node);
        create_private_dir(&self.root.join(FRAMES_DIR))?;
        create_private_dir(&dir)?;
        let path = dir.join(format!("{request_id}.json"));
        let temp = path.with_extension(format!("tmp-{}", std::process::id()));
        write_private_json(&temp, frame)?;
        fs::rename(&temp, &path).map_err(|source| {
            let _ = fs::remove_file(&temp);
            CoreError::Io {
                path: path.display().to_string(),
                source,
            }
        })
    }

    /// Returns the frames waiting for `node`, oldest request id first.
    ///
    /// A missing directory is an empty queue. A malformed frame is an error: the gateway
    /// closes the connection rather than silently dropping a system-initiated frame.
    pub fn pending(&self, node: &NodeName) -> Result<Vec<QueuedAgentFrame>, CoreError> {
        let dir = self.dir(node);
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(CoreError::Io {
                    path: dir.display().to_string(),
                    source,
                });
            }
        };
        let mut frames = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|source| CoreError::Io {
                path: dir.display().to_string(),
                source,
            })?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let Some(request_id) = path.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };
            validate_request_id(request_id)?;
            let bytes = fs::read(&path).map_err(|source| CoreError::Io {
                path: path.display().to_string(),
                source,
            })?;
            let frame: AgentFrame = serde_json::from_slice(&bytes)
                .map_err(|error| CoreError::Invalid(format!("{}: {error}", path.display())))?;
            frames.push(QueuedAgentFrame {
                request_id: request_id.to_owned(),
                frame,
            });
        }
        frames.sort_by(|left, right| left.request_id.cmp(&right.request_id));
        Ok(frames)
    }

    /// Removes one sent frame. A frame that is already gone is not an error.
    pub fn remove(&self, node: &NodeName, request_id: &str) -> Result<(), CoreError> {
        validate_request_id(request_id)?;
        let path = self.dir(node).join(format!("{request_id}.json"));
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(CoreError::Io {
                path: path.display().to_string(),
                source,
            }),
        }
    }
}

/// Returns whether a request id is a plain file-name component.
///
/// The same shape as the orchestrator command spool: no separator, no `..`, no empty stem.
fn validate_request_id(request_id: &str) -> Result<(), CoreError> {
    let safe = !request_id.is_empty()
        && request_id.len() <= 128
        && request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    if safe {
        Ok(())
    } else {
        Err(CoreError::Invalid(format!(
            "invalid agent frame request id {request_id:?}"
        )))
    }
}

/// Creates one directory below the home root with mode 0700.
fn create_private_dir(dir: &Path) -> Result<(), CoreError> {
    fs::create_dir_all(dir).map_err(|source| CoreError::Io {
        path: dir.display().to_string(),
        source,
    })?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).map_err(|source| CoreError::Io {
        path: dir.display().to_string(),
        source,
    })
}
