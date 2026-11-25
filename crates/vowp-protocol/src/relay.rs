//! StackSpirit relay control protocol: frames, scopes, codes and the binary data-frame header.
//!
//! Spec: `docs/design/relay-and-packaging.md` §6.3 and §6.5. Everything here is pure data: no
//! I/O, no cryptography and no clock. The P-256/HKDF/AES-GCM channel that fills the encrypted
//! [`RelayDataFrame`] lives in `vowp-agent`, so this crate does not acquire AES or elliptic-curve
//! dependencies.
//!
//! Readers ignore unknown fields (a future optional field is additive), reject an unknown
//! `type` and an unsupported `v`, and treat a missing required field as a protocol error. Use
//! [`RelayFrame::from_json`] to get exactly that behaviour.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::ProtocolError;
use crate::time::Timestamp;

/// The only control-frame version this build speaks.
pub const RELAY_FRAME_VERSION: u8 = 1;
/// WebSocket subprotocol offered on both relay endpoints.
pub const RELAY_SUBPROTOCOL: &str = "stackspirit-relay/1";
/// `MachineCapabilities::protocol` for the encrypted remote-frame protocol.
pub const REMOTE_PROTOCOL: &str = "stackspirit-remote/1";
/// Largest WebSocket message of any kind (control or data), in bytes.
pub const MAX_RELAY_FRAME_BYTES: usize = 1_048_576;
/// Largest JSON control frame, in bytes.
pub const MAX_CONTROL_FRAME_BYTES: usize = 65_536;
/// Largest encrypted inner `RemoteFrame` plaintext (768 KiB).
pub const MAX_REMOTE_PLAINTEXT_BYTES: usize = 786_432;
/// Handshake timeout in seconds.
pub const RELAY_HANDSHAKE_SECS: u64 = 5;
/// Seconds a ticket `jti` is remembered for replay detection.
pub const TICKET_REPLAY_SECS: u64 = 600;
/// Maximum ticket lifetime in seconds.
pub const MAX_TICKET_TTL_SECS: u64 = 300;
/// Canonical-string prefix of the machine challenge signature.
pub const RELAY_AUTH_DOMAIN: &str = "STACKSPIRIT-RELAY-AUTH-V1";
/// Canonical-string prefix of the credential-refresh signature.
pub const CREDENTIAL_DOMAIN: &str = "STACKSPIRIT-CREDENTIAL-V1";
/// First line of the end-to-end handshake transcript.
pub const E2E_TRANSCRIPT_DOMAIN: &str = "stackspirit-e2e-v1";
/// The only end-to-end suite in v1.
pub const E2E_SUITE: &str = "p256-hkdf-sha256-aes256gcm";

/// Data-frame magic bytes, `SSR1`.
pub const DATA_MAGIC: [u8; 4] = *b"SSR1";
/// Data-frame wire version.
pub const DATA_VERSION: u8 = 0x01;
/// Data-frame type for an encrypted payload.
pub const DATA_TYPE_DATA: u8 = 0x01;
/// Data-frame type reserved for rekey; rejected in v1.
pub const DATA_TYPE_REKEY: u8 = 0x02;
/// Size of the authenticated data-frame header (bytes 0..32).
pub const DATA_HEADER_BYTES: usize = 32;
/// Size of the full data-frame prefix: the header plus the 4-byte ciphertext length.
pub const DATA_PREFIX_BYTES: usize = 36;

/// WebSocket close code: normal.
pub const CLOSE_NORMAL: u16 = 1000;
/// WebSocket close code: a frame was over the cap.
pub const CLOSE_TOO_BIG: u16 = 1009;
/// WebSocket close code: backpressure or temporary overload.
pub const CLOSE_OVERLOADED: u16 = 1013;
/// WebSocket close code: authentication failed.
pub const CLOSE_UNAUTHENTICATED: u16 = 4001;
/// WebSocket close code: the machine is offline.
pub const CLOSE_MACHINE_OFFLINE: u16 = 4004;
/// WebSocket close code: superseded by another connection.
pub const CLOSE_SUPERSEDED: u16 = 4008;
/// WebSocket close code: rate limited.
pub const CLOSE_RATE_LIMITED: u16 = 4009;
/// WebSocket close code: the ticket expired.
pub const CLOSE_TICKET_EXPIRED: u16 = 4010;
/// WebSocket close code: a scope was denied.
pub const CLOSE_SCOPE_DENIED: u16 = 4011;
/// WebSocket close code: the end-to-end handshake or crypto failed.
pub const CLOSE_CRYPTO: u16 = 4012;

/// A permission a ticket can grant and a machine can enable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
pub enum Scope {
    /// Read the loopback daemon's `/api/v1` surface.
    #[serde(rename = "http.read")]
    HttpRead,
    /// POST to the explicit daemon write routes.
    #[serde(rename = "http.write")]
    HttpWrite,
    /// Read-only MCP tools.
    #[serde(rename = "mcp.read")]
    McpRead,
    /// MCP tools that change state.
    #[serde(rename = "mcp.write")]
    McpWrite,
    /// The TCP/SSH channel.
    #[serde(rename = "ssh")]
    Ssh,
    /// The exec channel.
    #[serde(rename = "exec")]
    Exec,
}

impl Scope {
    /// Every scope, in wire order.
    pub const ALL: [Scope; 6] = [
        Scope::HttpRead,
        Scope::HttpWrite,
        Scope::McpRead,
        Scope::McpWrite,
        Scope::Ssh,
        Scope::Exec,
    ];

    /// The wire string, for example `http.read`.
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::HttpRead => "http.read",
            Scope::HttpWrite => "http.write",
            Scope::McpRead => "mcp.read",
            Scope::McpWrite => "mcp.write",
            Scope::Ssh => "ssh",
            Scope::Exec => "exec",
        }
    }

    /// Parses a wire string.
    pub fn parse(text: &str) -> Option<Scope> {
        Scope::ALL.into_iter().find(|scope| scope.as_str() == text)
    }
}

/// The kind of client behind a ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClientKind {
    /// The browser PWA.
    Web,
    /// The `stackspirit` CLI.
    Cli,
    /// The desktop app.
    Desktop,
    /// The hosted MCP server.
    Mcp,
}

/// A machine's presence as the relay reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MachineState {
    /// Connected and serving.
    Online,
    /// Connected but unable to serve some channels.
    Degraded,
    /// Not connected.
    Offline,
    /// Revoked by the owner; the connector must stop.
    Revoked,
}

/// What a machine advertises in `machine.hello`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MachineCapabilities {
    /// Remote-frame protocol, [`REMOTE_PROTOCOL`].
    #[serde(default = "default_remote_protocol")]
    pub protocol: String,
    /// HTTP channel enabled.
    #[serde(default)]
    pub http: bool,
    /// MCP channel enabled.
    #[serde(default)]
    pub mcp: bool,
    /// TCP/SSH channel enabled.
    #[serde(default)]
    pub ssh: bool,
    /// Exec channel enabled.
    #[serde(default)]
    pub exec: bool,
    /// Channel limit.
    #[serde(default = "default_max_channels")]
    pub max_channels: u32,
    /// Project names this machine serves.
    #[serde(default)]
    pub projects: Vec<String>,
}

fn default_remote_protocol() -> String {
    REMOTE_PROTOCOL.to_owned()
}

fn default_max_channels() -> u32 {
    64
}

impl Default for MachineCapabilities {
    fn default() -> Self {
        Self {
            protocol: default_remote_protocol(),
            http: false,
            mcp: false,
            ssh: false,
            exec: false,
            max_channels: default_max_channels(),
            projects: Vec::new(),
        }
    }
}

/// An EC P-256 public key in JWK form (RFC 7517). Private members are not representable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Jwk {
    /// Key type; always `EC`.
    pub kty: String,
    /// Curve; always `P-256`.
    pub crv: String,
    /// Base64url (no padding) x coordinate, 32 bytes.
    pub x: String,
    /// Base64url (no padding) y coordinate, 32 bytes.
    pub y: String,
}

impl Jwk {
    /// The RFC 7638 canonical JSON: lexicographic keys, no whitespace.
    pub fn canonical(&self) -> String {
        // The four members are fixed and sorted: crv, kty, x, y. Values are escaped by serde_json
        // so a hostile member cannot break out of its string.
        format!(
            "{{\"crv\":{},\"kty\":{},\"x\":{},\"y\":{}}}",
            json_string(&self.crv),
            json_string(&self.kty),
            json_string(&self.x),
            json_string(&self.y)
        )
    }
}

fn json_string(text: &str) -> String {
    serde_json::Value::String(text.to_owned()).to_string()
}

/// One relay control frame. One WebSocket text message carries exactly one frame.
///
/// Unknown fields are ignored; use [`RelayFrame::from_json`] to also enforce the version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RelayFrame {
    /// Machine to relay: first frame on the machine endpoint.
    #[serde(rename = "machine.hello")]
    MachineHello {
        /// Frame version.
        v: u8,
        /// `mch_` machine id.
        machine_id: String,
        /// `acct_` account id.
        account_id: String,
        /// `ed25519:<hex>` key id.
        key_id: String,
        /// Lowercase hex Ed25519 public key.
        public_key: String,
        /// The ES256 machine credential.
        credential: String,
        /// Agent version string.
        agent_version: String,
        /// Advertised capabilities.
        #[serde(default)]
        capabilities: MachineCapabilities,
        /// Send time.
        sent_at: Timestamp,
    },
    /// Relay to machine: the signing challenge.
    #[serde(rename = "relay.challenge")]
    RelayChallenge {
        /// Frame version.
        v: u8,
        /// Relay instance id.
        relay_id: String,
        /// Session id.
        session_id: String,
        /// Single-use nonce.
        nonce: String,
        /// Exact RFC 3339 millisecond server time.
        server_time: Timestamp,
        /// Heartbeat interval the relay wants.
        heartbeat_secs: u64,
        /// Largest data frame the relay accepts.
        max_frame_bytes: u64,
        /// Relay feature strings.
        #[serde(default)]
        features: Vec<String>,
    },
    /// Machine to relay: signature over the challenge.
    #[serde(rename = "machine.auth")]
    MachineAuth {
        /// Frame version.
        v: u8,
        /// Lowercase hex Ed25519 signature.
        signature: String,
    },
    /// Relay to machine: authentication succeeded.
    #[serde(rename = "relay.welcome")]
    RelayWelcome {
        /// Frame version.
        v: u8,
        /// Relay instance id.
        relay_id: String,
        /// Session id.
        session_id: String,
        /// Machine id.
        machine_id: String,
        /// Account id.
        account_id: String,
        /// Heartbeat interval.
        heartbeat_secs: u64,
        /// Largest data frame.
        max_frame_bytes: u64,
        /// Relay feature strings.
        #[serde(default)]
        features: Vec<String>,
    },
    /// Machine to relay: presence update.
    #[serde(rename = "machine.status")]
    MachineStatus {
        /// Frame version.
        v: u8,
        /// Presence state.
        state: MachineState,
        /// Open channels.
        channels: u32,
        /// Send time.
        sent_at: Timestamp,
    },
    /// Client to relay: first frame on the client endpoint.
    #[serde(rename = "client.hello")]
    ClientHello {
        /// Frame version.
        v: u8,
        /// Target machine id.
        machine_id: String,
        /// The relay ticket.
        ticket: String,
        /// Client id.
        client_id: String,
        /// Client kind.
        kind: ClientKind,
        /// Protocol, [`REMOTE_PROTOCOL`].
        protocol: String,
        /// Client capability strings.
        #[serde(default)]
        capabilities: Vec<String>,
        /// Ephemeral P-256 client key.
        client_key: Jwk,
        /// Base64url 32-byte client nonce.
        client_nonce: String,
        /// Send time.
        sent_at: Timestamp,
    },
    /// Relay to client: the channel exists.
    #[serde(rename = "client.welcome")]
    ClientWelcome {
        /// Frame version.
        v: u8,
        /// Relay instance id.
        relay_id: String,
        /// Session id.
        session_id: String,
        /// Allocated ULID channel id.
        channel_id: String,
        /// Machine id.
        machine_id: String,
        /// Account id.
        account_id: String,
        /// Ticket expiry.
        expires_at: Timestamp,
        /// Relay feature strings.
        #[serde(default)]
        features: Vec<String>,
    },
    /// Relay to machine: a client wants a channel.
    #[serde(rename = "channel.open")]
    ChannelOpen {
        /// Frame version.
        v: u8,
        /// ULID channel id.
        channel_id: String,
        /// Machine id.
        machine_id: String,
        /// Client id.
        client_id: String,
        /// Client kind.
        kind: ClientKind,
        /// The ticket to re-verify.
        ticket: String,
        /// Scopes the client requested.
        #[serde(default)]
        scopes: Vec<Scope>,
        /// Ephemeral client key.
        client_key: Jwk,
        /// Client nonce.
        client_nonce: String,
    },
    /// Machine to relay: the machine's half of the handshake.
    #[serde(rename = "channel.accept")]
    ChannelAccept {
        /// Frame version.
        v: u8,
        /// ULID channel id.
        channel_id: String,
        /// Ephemeral P-256 machine key.
        machine_key: Jwk,
        /// Base64url 32-byte machine nonce.
        machine_nonce: String,
        /// Long-lived `ed25519:<hex>` identity id.
        machine_key_id: String,
        /// Lowercase hex Ed25519 signature over the transcript.
        machine_signature: String,
        /// Crypto suite, [`E2E_SUITE`].
        suite: String,
    },
    /// Machine to relay: the machine refuses a channel.
    #[serde(rename = "channel.reject")]
    ChannelReject {
        /// Frame version.
        v: u8,
        /// ULID channel id.
        channel_id: String,
        /// Stable error code.
        code: String,
        /// Path-free message.
        message: String,
        /// Whether retrying can help.
        #[serde(default)]
        retryable: bool,
    },
    /// Either side: operational channel close.
    #[serde(rename = "channel.close")]
    ChannelClose {
        /// Frame version.
        v: u8,
        /// ULID channel id.
        channel_id: String,
        /// Stable code.
        code: String,
        /// Path-free message.
        message: String,
    },
    /// Relay to machine: another machine connection replaced this one.
    #[serde(rename = "superseded")]
    Superseded {
        /// Frame version.
        v: u8,
        /// Reason text.
        reason: String,
    },
    /// Heartbeat request.
    #[serde(rename = "ping")]
    Ping {
        /// Frame version.
        v: u8,
        /// Echoed nonce.
        nonce: String,
    },
    /// Heartbeat answer.
    #[serde(rename = "pong")]
    Pong {
        /// Frame version.
        v: u8,
        /// The echoed nonce.
        nonce: String,
    },
    /// Either side: a protocol or authentication error.
    #[serde(rename = "error")]
    Error {
        /// Frame version.
        v: u8,
        /// Stable error code.
        code: String,
        /// Path-free message.
        message: String,
        /// Whether retrying can help.
        #[serde(default)]
        retryable: bool,
    },
}

impl RelayFrame {
    /// Parses one control frame: unknown fields are ignored, an unknown `type`, a missing
    /// required field or an unsupported `v` is an error, and a frame over
    /// [`MAX_CONTROL_FRAME_BYTES`] is refused before parsing.
    pub fn from_json(text: &str) -> Result<Self, ProtocolError> {
        if text.len() > MAX_CONTROL_FRAME_BYTES {
            return Err(ProtocolError::Config(format!(
                "relay control frame is {} bytes; the cap is {MAX_CONTROL_FRAME_BYTES}",
                text.len()
            )));
        }
        let frame: RelayFrame =
            serde_json::from_str(text).map_err(|error| ProtocolError::Config(format!("relay frame: {error}")))?;
        if frame.version() != RELAY_FRAME_VERSION {
            return Err(ProtocolError::Config(format!(
                "unsupported relay frame version {}",
                frame.version()
            )));
        }
        Ok(frame)
    }

    /// Serialises the frame as one JSON text message.
    pub fn to_json(&self) -> Result<String, ProtocolError> {
        serde_json::to_string(self).map_err(|error| ProtocolError::Config(format!("relay frame: {error}")))
    }

    /// The frame's `v`.
    pub fn version(&self) -> u8 {
        match self {
            RelayFrame::MachineHello { v, .. }
            | RelayFrame::RelayChallenge { v, .. }
            | RelayFrame::MachineAuth { v, .. }
            | RelayFrame::RelayWelcome { v, .. }
            | RelayFrame::MachineStatus { v, .. }
            | RelayFrame::ClientHello { v, .. }
            | RelayFrame::ClientWelcome { v, .. }
            | RelayFrame::ChannelOpen { v, .. }
            | RelayFrame::ChannelAccept { v, .. }
            | RelayFrame::ChannelReject { v, .. }
            | RelayFrame::ChannelClose { v, .. }
            | RelayFrame::Superseded { v, .. }
            | RelayFrame::Ping { v, .. }
            | RelayFrame::Pong { v, .. }
            | RelayFrame::Error { v, .. } => *v,
        }
    }

    /// The wire `type` tag.
    pub fn type_name(&self) -> &'static str {
        match self {
            RelayFrame::MachineHello { .. } => "machine.hello",
            RelayFrame::RelayChallenge { .. } => "relay.challenge",
            RelayFrame::MachineAuth { .. } => "machine.auth",
            RelayFrame::RelayWelcome { .. } => "relay.welcome",
            RelayFrame::MachineStatus { .. } => "machine.status",
            RelayFrame::ClientHello { .. } => "client.hello",
            RelayFrame::ClientWelcome { .. } => "client.welcome",
            RelayFrame::ChannelOpen { .. } => "channel.open",
            RelayFrame::ChannelAccept { .. } => "channel.accept",
            RelayFrame::ChannelReject { .. } => "channel.reject",
            RelayFrame::ChannelClose { .. } => "channel.close",
            RelayFrame::Superseded { .. } => "superseded",
            RelayFrame::Ping { .. } => "ping",
            RelayFrame::Pong { .. } => "pong",
            RelayFrame::Error { .. } => "error",
        }
    }
}

/// Stable string codes carried in `error`, `channel.reject`, `channel.close` and remote frames.
pub mod codes {
    /// Authentication failed.
    pub const UNAUTHENTICATED: &str = "unauthenticated";
    /// The ticket was already used.
    pub const TICKET_REPLAYED: &str = "ticket_replayed";
    /// The ticket expired.
    pub const TICKET_EXPIRED: &str = "ticket_expired";
    /// The ticket is not valid for this machine or key.
    pub const TICKET_INVALID: &str = "ticket_invalid";
    /// A scope is not enabled locally or not in the ticket.
    pub const SCOPE_DENIED: &str = "scope_denied";
    /// The end-to-end handshake or a data frame failed.
    pub const CRYPTO_ERROR: &str = "crypto_error";
    /// The machine is at its channel limit.
    pub const OVERLOADED: &str = "overloaded";
    /// The channel kind is recognised but not wired yet.
    pub const NOT_IMPLEMENTED: &str = "not_implemented";
    /// The request is malformed or not allowed.
    pub const BAD_REQUEST: &str = "bad_request";
    /// The local daemon cannot be reached.
    pub const DAEMON_UNAVAILABLE: &str = "daemon_unavailable";
    /// The machine was revoked.
    pub const REVOKED: &str = "revoked";
    /// A protocol violation.
    pub const PROTOCOL: &str = "protocol";
    /// A relay-side or local fault.
    pub const INTERNAL: &str = "internal";
    /// The relay was superseded by another connection.
    pub const SUPERSEDED: &str = "superseded";
}

/// The binary data frame. One WebSocket binary message carries exactly one.
///
/// ```text
/// 0..4 SSR1 | 4 version | 5 type | 6..8 flags | 8..24 channel id | 24..32 sequence | 32..36 len | ciphertext
/// ```
///
/// The first 32 bytes are the AES-GCM additional authenticated data, see
/// [`RelayDataFrame::aad`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayDataFrame {
    /// Raw 16-byte ULID channel id.
    pub channel_id: [u8; 16],
    /// Sequence number, starting at 1 per direction.
    pub sequence: u64,
    /// AES-256-GCM ciphertext including the 16-byte tag.
    pub ciphertext: Vec<u8>,
}

/// Why a binary data frame was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DataFrameError {
    /// Shorter than the fixed prefix.
    #[error("data frame is shorter than its {DATA_PREFIX_BYTES}-byte prefix")]
    Truncated,
    /// Bad magic.
    #[error("data frame magic is not SSR1")]
    BadMagic,
    /// Unsupported wire version.
    #[error("unsupported data frame version {0}")]
    BadVersion(u8),
    /// Unknown or reserved type (rekey is rejected in v1).
    #[error("unsupported data frame type {0}")]
    BadType(u8),
    /// Non-zero flags in v1.
    #[error("data frame flags must be zero in v1")]
    BadFlags,
    /// The declared ciphertext length disagrees with the message size.
    #[error("data frame length field disagrees with the message size")]
    BadLength,
    /// Over [`MAX_RELAY_FRAME_BYTES`].
    #[error("data frame is over {MAX_RELAY_FRAME_BYTES} bytes")]
    TooLarge,
}

impl RelayDataFrame {
    /// Parses a channel id string (26-character ULID) into its 16 raw bytes.
    pub fn channel_bytes(channel_id: &str) -> Option<[u8; 16]> {
        ulid::Ulid::from_string(channel_id).ok().map(|id| id.to_bytes())
    }

    /// The 26-character ULID string for a raw channel id.
    pub fn channel_string(channel_id: [u8; 16]) -> String {
        ulid::Ulid::from_bytes(channel_id).to_string()
    }

    /// The header bytes 0..32, used as the AES-GCM additional authenticated data.
    pub fn aad(channel_id: [u8; 16], sequence: u64) -> [u8; DATA_HEADER_BYTES] {
        let mut header = [0u8; DATA_HEADER_BYTES];
        header[0..4].copy_from_slice(&DATA_MAGIC);
        header[4] = DATA_VERSION;
        header[5] = DATA_TYPE_DATA;
        // header[6..8] are the zero flags.
        header[8..24].copy_from_slice(&channel_id);
        header[24..32].copy_from_slice(&sequence.to_be_bytes());
        header
    }

    /// Serialises the frame; fails if the result would exceed [`MAX_RELAY_FRAME_BYTES`].
    pub fn to_bytes(&self) -> Result<Vec<u8>, DataFrameError> {
        let total = DATA_PREFIX_BYTES + self.ciphertext.len();
        if total > MAX_RELAY_FRAME_BYTES {
            return Err(DataFrameError::TooLarge);
        }
        let length = u32::try_from(self.ciphertext.len()).map_err(|_| DataFrameError::TooLarge)?;
        let mut bytes = Vec::with_capacity(total);
        bytes.extend_from_slice(&Self::aad(self.channel_id, self.sequence));
        bytes.extend_from_slice(&length.to_be_bytes());
        bytes.extend_from_slice(&self.ciphertext);
        Ok(bytes)
    }

    /// Parses one binary message strictly.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DataFrameError> {
        if bytes.len() > MAX_RELAY_FRAME_BYTES {
            return Err(DataFrameError::TooLarge);
        }
        if bytes.len() < DATA_PREFIX_BYTES {
            return Err(DataFrameError::Truncated);
        }
        if bytes[0..4] != DATA_MAGIC {
            return Err(DataFrameError::BadMagic);
        }
        if bytes[4] != DATA_VERSION {
            return Err(DataFrameError::BadVersion(bytes[4]));
        }
        if bytes[5] != DATA_TYPE_DATA {
            return Err(DataFrameError::BadType(bytes[5]));
        }
        if bytes[6..8] != [0, 0] {
            return Err(DataFrameError::BadFlags);
        }
        let mut channel_id = [0u8; 16];
        channel_id.copy_from_slice(&bytes[8..24]);
        let mut sequence = [0u8; 8];
        sequence.copy_from_slice(&bytes[24..32]);
        let mut length = [0u8; 4];
        length.copy_from_slice(&bytes[32..36]);
        let length = u32::from_be_bytes(length) as usize;
        if bytes.len() - DATA_PREFIX_BYTES != length {
            return Err(DataFrameError::BadLength);
        }
        Ok(Self {
            channel_id,
            sequence: u64::from_be_bytes(sequence),
            ciphertext: bytes[DATA_PREFIX_BYTES..].to_vec(),
        })
    }
}
