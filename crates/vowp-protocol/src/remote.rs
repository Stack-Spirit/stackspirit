//! StackSpirit remote frames: the JSON plaintext inside an encrypted relay data frame.
//!
//! Spec: `docs/design/relay-and-packaging.md` §6.6. Pure data: no I/O and no cryptography.
//! `data` and `body` fields are standard base64 with padding. Unknown fields are ignored and an
//! unknown `type` is an error, like the control frames in [`crate::relay`].

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::ProtocolError;
use crate::relay::{MAX_REMOTE_PLAINTEXT_BYTES, Scope};

/// Remote-frame protocol version carried in `Hello`.
pub const REMOTE_FRAME_VERSION: u8 = 1;
/// Largest valid client-allocated stream id.
pub const MAX_STREAM_ID: u32 = 2_147_483_647;
/// Largest base64 `data`/`body` chunk, in encoded bytes (512 KiB).
pub const MAX_CHUNK_ENCODED_BYTES: usize = 524_288;

/// Which stream an `ExecData` frame belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecChannel {
    /// Process standard output.
    Stdout,
    /// Process standard error.
    Stderr,
    /// Data for the process standard input.
    Stdin,
}

/// One end-to-end plaintext frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RemoteFrame {
    /// First client frame: the scopes it wants.
    Hello {
        /// Frame version.
        v: u8,
        /// Client id.
        client: String,
        /// Machine id the client believes it reached.
        machine: String,
        /// Requested scopes.
        #[serde(default)]
        scopes: Vec<Scope>,
        /// Client capability strings.
        #[serde(default)]
        capabilities: Vec<String>,
    },
    /// The machine's authoritative grant.
    HelloAck {
        /// Frame version.
        v: u8,
        /// Machine id.
        machine: String,
        /// Granted scopes.
        #[serde(default)]
        scopes: Vec<Scope>,
        /// Machine capability strings.
        #[serde(default)]
        capabilities: Vec<String>,
    },
    /// Start an HTTP request against the loopback daemon.
    HttpRequest {
        /// Client-allocated stream id.
        stream: u32,
        /// `GET` or `POST`.
        method: String,
        /// Absolute path under `/api/v1/`.
        path: String,
        /// Allowed request headers.
        #[serde(default)]
        headers: BTreeMap<String, String>,
        /// Base64 request body.
        #[serde(default)]
        body: String,
    },
    /// Start of an HTTP response.
    HttpResponse {
        /// Stream id.
        stream: u32,
        /// HTTP status.
        status: u16,
        /// Allowed response headers.
        #[serde(default)]
        headers: BTreeMap<String, String>,
    },
    /// A chunk of an HTTP body.
    HttpBody {
        /// Stream id.
        stream: u32,
        /// Base64 chunk.
        #[serde(default)]
        data: String,
        /// True on the last chunk.
        #[serde(default)]
        end: bool,
    },
    /// Cancel an HTTP request.
    HttpCancel {
        /// Stream id.
        stream: u32,
    },
    /// One complete JSON-RPC 2.0 request.
    McpRequest {
        /// Stream id.
        stream: u32,
        /// Optional project name.
        #[serde(default)]
        project: Option<String>,
        /// Optional session id.
        #[serde(default)]
        session: Option<String>,
        /// The JSON-RPC object.
        request: serde_json::Value,
    },
    /// The JSON-RPC answer.
    McpResponse {
        /// Stream id.
        stream: u32,
        /// The JSON-RPC object.
        response: serde_json::Value,
    },
    /// Cancel an MCP request.
    McpCancel {
        /// Stream id.
        stream: u32,
    },
    /// Open the TCP/SSH stream; `target` is the literal `ssh`.
    TcpOpen {
        /// Stream id.
        stream: u32,
        /// Always `ssh` in v1.
        target: String,
    },
    /// Raw TCP bytes.
    TcpData {
        /// Stream id.
        stream: u32,
        /// Base64 bytes.
        #[serde(default)]
        data: String,
    },
    /// Close the TCP stream.
    TcpClose {
        /// Stream id.
        stream: u32,
    },
    /// Start an exec process (disabled by default).
    ExecStart {
        /// Stream id.
        stream: u32,
        /// Command and arguments.
        argv: Vec<String>,
        /// Working directory.
        #[serde(default)]
        cwd: Option<String>,
        /// Allocate a pty.
        #[serde(default)]
        pty: bool,
        /// Pty columns.
        #[serde(default)]
        cols: u16,
        /// Pty rows.
        #[serde(default)]
        rows: u16,
        /// Environment variables.
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    /// Exec data.
    ExecData {
        /// Stream id.
        stream: u32,
        /// Base64 bytes.
        #[serde(default)]
        data: String,
        /// Which stream the bytes belong to.
        channel: ExecChannel,
    },
    /// Resize an exec pty.
    ExecResize {
        /// Stream id.
        stream: u32,
        /// Columns.
        cols: u16,
        /// Rows.
        rows: u16,
    },
    /// An exec process ended.
    ExecExit {
        /// Stream id.
        stream: u32,
        /// Exit code.
        code: i32,
        /// Terminating signal, if any.
        #[serde(default)]
        signal: Option<String>,
    },
    /// One stream failed. The message is path-free and redacted.
    StreamError {
        /// Stream id.
        stream: u32,
        /// Stable code, see [`crate::relay::codes`].
        code: String,
        /// Path-free message.
        message: String,
        /// Whether retrying can help.
        #[serde(default)]
        retryable: bool,
    },
    /// Application heartbeat.
    Ping {
        /// Echoed nonce.
        nonce: String,
    },
    /// Application heartbeat answer.
    Pong {
        /// The echoed nonce.
        nonce: String,
    },
    /// Authenticated application close.
    Close {
        /// Stable code.
        code: String,
        /// Path-free message.
        message: String,
    },
}

impl RemoteFrame {
    /// Parses one plaintext frame; refuses one over [`MAX_REMOTE_PLAINTEXT_BYTES`].
    pub fn from_json(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() > MAX_REMOTE_PLAINTEXT_BYTES {
            return Err(ProtocolError::Config(format!(
                "remote frame is {} bytes; the cap is {MAX_REMOTE_PLAINTEXT_BYTES}",
                bytes.len()
            )));
        }
        serde_json::from_slice(bytes).map_err(|error| ProtocolError::Config(format!("remote frame: {error}")))
    }

    /// Serialises the frame; refuses one over [`MAX_REMOTE_PLAINTEXT_BYTES`].
    pub fn to_json(&self) -> Result<Vec<u8>, ProtocolError> {
        let bytes =
            serde_json::to_vec(self).map_err(|error| ProtocolError::Config(format!("remote frame: {error}")))?;
        if bytes.len() > MAX_REMOTE_PLAINTEXT_BYTES {
            return Err(ProtocolError::Config(format!(
                "remote frame is {} bytes; the cap is {MAX_REMOTE_PLAINTEXT_BYTES}",
                bytes.len()
            )));
        }
        Ok(bytes)
    }

    /// The stream id the frame belongs to, or `None` for connection-level frames.
    pub fn stream(&self) -> Option<u32> {
        match self {
            RemoteFrame::HttpRequest { stream, .. }
            | RemoteFrame::HttpResponse { stream, .. }
            | RemoteFrame::HttpBody { stream, .. }
            | RemoteFrame::HttpCancel { stream }
            | RemoteFrame::McpRequest { stream, .. }
            | RemoteFrame::McpResponse { stream, .. }
            | RemoteFrame::McpCancel { stream }
            | RemoteFrame::TcpOpen { stream, .. }
            | RemoteFrame::TcpData { stream, .. }
            | RemoteFrame::TcpClose { stream }
            | RemoteFrame::ExecStart { stream, .. }
            | RemoteFrame::ExecData { stream, .. }
            | RemoteFrame::ExecResize { stream, .. }
            | RemoteFrame::ExecExit { stream, .. }
            | RemoteFrame::StreamError { stream, .. } => Some(*stream),
            RemoteFrame::Hello { .. }
            | RemoteFrame::HelloAck { .. }
            | RemoteFrame::Ping { .. }
            | RemoteFrame::Pong { .. }
            | RemoteFrame::Close { .. } => None,
        }
    }
}

/// True when `stream` is a valid client-allocated id (`1..=2_147_483_647`).
pub fn valid_stream_id(stream: u32) -> bool {
    (1..=MAX_STREAM_ID).contains(&stream)
}
