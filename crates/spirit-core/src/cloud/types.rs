//! Typed StackSpirit Account, Cloud Relay and Managed Home contracts.
//!
//! The shapes mirror `runtime/docs/design/relay-and-packaging.md` §6.4 and the
//! TypeScript contracts already rendered by the shared Account UI. They are
//! deliberately separate from the local VOWP wire model: the cloud client talks
//! only to `api.stackspirit.io` / `relay.stackspirit.io`, while host, SSH and
//! provider-secret data stay on the owner's device.
//!
//! There is no field for an SSH private key, passphrase, password or provider
//! secret anywhere in a request. A relay ticket carries only public key material
//! and a scope list.

use std::fmt;

use serde::Serialize;
use zeroize::Zeroizing;

use crate::time::Timestamp;

/// One signed-in account profile. It never includes an access token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudAccount {
    /// Stable account id (`acct_...`).
    pub id: String,
    /// Account e-mail address.
    pub email: String,
    /// Optional display name.
    pub name: Option<String>,
}

/// The device flow's current polling answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevicePollState {
    /// The owner approved the code and the client now holds a session.
    Authorized,
    /// The owner has not approved the code yet.
    Pending,
    /// The API asked the client to poll less often.
    SlowDown,
}

/// `POST /v1/auth/device` response.
///
/// The `device_code` is a short-lived bearer-like value and is zeroized on drop;
/// it never appears in [`fmt::Debug`].
#[derive(Clone)]
pub struct DeviceAuthorization {
    /// Opaque polling code used only for `POST /v1/auth/device/token`.
    pub device_code: Zeroizing<String>,
    /// Short code the owner types on the verification page.
    pub user_code: String,
    /// The verification page.
    pub verification_uri: String,
    /// A complete verification page URL that embeds the code, when supplied.
    pub verification_uri_complete: Option<String>,
    /// Code lifetime in seconds as reported by the API.
    pub expires_in: u64,
    /// Minimum polling interval in seconds.
    pub interval: u64,
    /// Client-side expiry derived from `expires_in`.
    pub expires_at: Timestamp,
}

impl fmt::Debug for DeviceAuthorization {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceAuthorization")
            .field("device_code", &"[redacted]")
            .field("user_code", &self.user_code)
            .field("verification_uri", &self.verification_uri)
            .field("verification_uri_complete", &self.verification_uri_complete)
            .field("expires_in", &self.expires_in)
            .field("interval", &self.interval)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// The presence state of an enrolled Always-On Home.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachinePresence {
    /// The machine's outbound relay session is live.
    Online,
    /// The machine is enrolled but not connected.
    Offline,
    /// The machine connected but reported a degraded state.
    Degraded,
    /// The machine was revoked from the account.
    Revoked,
}

impl MachinePresence {
    /// Returns the canonical wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Offline => "offline",
            Self::Degraded => "degraded",
            Self::Revoked => "revoked",
        }
    }
}

/// The relay client kind carried by a ticket request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ClientKind {
    /// The hosted Web/PWA cockpit.
    Web,
    /// The StackSpirit command line.
    Cli,
    /// The StackSpirit desktop app.
    Desktop,
    /// A hosted or local MCP client.
    Mcp,
}

impl ClientKind {
    /// Returns the canonical wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Cli => "cli",
            Self::Desktop => "desktop",
            Self::Mcp => "mcp",
        }
    }
}

/// One scope a relay ticket may request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RelayScope {
    /// Read-only HTTP requests through the relayed daemon bridge.
    #[serde(rename = "http.read")]
    HttpRead,
    /// Mutating HTTP requests; the machine must grant this explicitly.
    #[serde(rename = "http.write")]
    HttpWrite,
    /// Read-only MCP requests.
    #[serde(rename = "mcp.read")]
    McpRead,
    /// Mutating MCP requests.
    #[serde(rename = "mcp.write")]
    McpWrite,
    /// The `ssh` relay scope for `stackspirit ssh`.
    #[serde(rename = "ssh")]
    Ssh,
    /// Command execution through the relayed agent.
    #[serde(rename = "exec")]
    Exec,
}

impl RelayScope {
    /// Returns the canonical wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HttpRead => "http.read",
            Self::HttpWrite => "http.write",
            Self::McpRead => "mcp.read",
            Self::McpWrite => "mcp.write",
            Self::Ssh => "ssh",
            Self::Exec => "exec",
        }
    }
}

/// The public half of an ephemeral P-256 ECDH key.
///
/// The form is the RFC 7517 JWK the API expects. There is intentionally no
/// private-key field: the relay's E2E channel is created in the webview and core
/// only ever sees the public coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayPublicJwk {
    /// Base64url-encoded X coordinate.
    pub x: String,
    /// Base64url-encoded Y coordinate.
    pub y: String,
}

impl RelayPublicJwk {
    /// Builds a P-256 public JWK from its two public coordinates.
    #[must_use]
    pub fn p256(x: impl Into<String>, y: impl Into<String>) -> Self {
        Self {
            x: x.into(),
            y: y.into(),
        }
    }
}

impl Serialize for RelayPublicJwk {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("RelayPublicJwk", 4)?;
        state.serialize_field("kty", "EC")?;
        state.serialize_field("crv", "P-256")?;
        state.serialize_field("x", &self.x)?;
        state.serialize_field("y", &self.y)?;
        state.end()
    }
}

/// `POST /v1/machines/:id/ticket` request body. Public values only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelayTicketRequest {
    /// The client kind requesting the ticket.
    pub kind: ClientKind,
    /// The requested scopes. The API clamps them to the account and machine grants.
    pub scopes: Vec<RelayScope>,
    /// The browser's ephemeral public P-256 key.
    pub client_key: RelayPublicJwk,
    /// A public random nonce that ties the ticket to one handshake.
    pub client_nonce: String,
    /// Requested lifetime in seconds; the client clamps this to 30..=300.
    pub ttl_secs: u32,
}

/// The machine card carried by a minted ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayTicketMachine {
    /// Stable machine id (`mch_...`).
    pub id: String,
    /// Human-readable machine name.
    pub name: String,
    /// Machine key id, for example `ed25519:<hex>`.
    pub key_id: String,
    /// Machine Ed25519 public key, lowercase hex.
    pub public_key: String,
    /// Project bound to the machine, when any.
    pub project: Option<String>,
}

/// A short-lived, single-use relay ticket.
///
/// The ticket itself is a credential. It is zeroized on drop and redacted from
/// [`fmt::Debug`].
#[derive(Clone)]
pub struct RelayTicket {
    /// The signed ticket JWT.
    pub ticket: Zeroizing<String>,
    /// Ticket expiry reported by the API.
    pub expires_at: Timestamp,
    /// The relay WebSocket URL accepted by the client.
    pub relay_url: String,
    /// The machine whose relay accepts this ticket.
    pub machine: RelayTicketMachine,
}

impl fmt::Debug for RelayTicket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RelayTicket")
            .field("ticket", &"[redacted]")
            .field("expires_at", &self.expires_at)
            .field("relay_url", &self.relay_url)
            .field("machine", &self.machine)
            .finish()
    }
}

/// Live provisioning phase for a Managed Always-On Home.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvisioningPhase {
    /// The request is accepted and waiting for a worker.
    Queued,
    /// The provider is creating the machine.
    Provisioning,
    /// The machine is online and ready.
    Online,
    /// Provisioning failed; the detail explains why.
    Failed,
}

impl ProvisioningPhase {
    /// Returns the canonical wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Provisioning => "provisioning",
            Self::Online => "online",
            Self::Failed => "failed",
        }
    }
}

/// Safe, path-scrubbed provisioning status for one Managed Home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisioningStatus {
    /// Machine the status belongs to.
    pub machine_id: String,
    /// Current phase.
    pub phase: ProvisioningPhase,
    /// Human-readable current step, when supplied.
    pub step: Option<String>,
    /// Progress from 0 to 100, when supplied.
    pub progress_pct: Option<u8>,
    /// Safe detail text with local absolute paths scrubbed.
    pub detail: Option<String>,
    /// When the API last updated the record.
    pub updated_at: Timestamp,
    /// The Home's public URL once it is online.
    pub home_url: Option<String>,
}

/// One enrolled Always-On Home. Public identity only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudMachine {
    /// Stable machine id (`mch_...`).
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Relay presence reported by the API.
    pub status: MachinePresence,
    /// Project bound to the machine, when any.
    pub project: Option<String>,
    /// Last successful relay connection, when the API reports one.
    pub last_seen_at: Option<Timestamp>,
    /// Inline provisioning state, never an SSH key.
    pub provisioning: Option<ProvisioningStatus>,
}

/// One published Managed Home tier.
///
/// Prices come from `GET /v1/plans`; the client never guesses a price.
#[derive(Debug, Clone, PartialEq)]
pub struct ManagedPlan {
    /// Stable plan id.
    pub id: String,
    /// Display name, for example `Home 8`.
    pub name: String,
    /// vCPU count.
    pub vcpu: u32,
    /// RAM in gigabytes.
    pub ram_gb: u32,
    /// Disk size in gigabytes.
    pub disk_gb: u32,
    /// Disk kind, when the provider reports one.
    pub disk_kind: Option<String>,
    /// Monthly price in euro, already including the StackSpirit markup.
    pub monthly_eur: f64,
    /// One-time setup fee in euro, when the provider charges one.
    pub setup_fee_eur: Option<f64>,
    /// Currency; this build accepts only euro.
    pub currency: String,
    /// The underlying provider spec, shown transparently when reported.
    pub provider_spec: Option<String>,
}

/// The optional off-site encrypted backup add-on.
#[derive(Debug, Clone, PartialEq)]
pub struct BackupPlan {
    /// Monthly price in euro, when published separately.
    pub monthly_eur: Option<f64>,
    /// Whether the UI recommends the add-on. It is still off by default.
    pub recommended: bool,
    /// Backup provider, for example `cloudflare-r2`.
    pub provider: Option<String>,
}

/// The public plan catalogue plus its backup add-on.
#[derive(Debug, Clone, PartialEq)]
pub struct ManagedPlanCatalog {
    /// Plans, sorted by monthly price and then RAM.
    pub plans: Vec<ManagedPlan>,
    /// The backup add-on. It is never enabled by the catalogue alone.
    pub backups: BackupPlan,
}
