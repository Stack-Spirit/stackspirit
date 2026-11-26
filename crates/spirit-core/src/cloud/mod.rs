//! Optional StackSpirit Account, Cloud Relay and Managed Home client.
//!
//! Account-free local and SSH mode is the default: importing this module makes
//! no request and starts no timer or socket. A host constructs a [`CloudClient`]
//! only when the owner opens the optional Account panel, injects its HTTPS
//! transport, and supplies a [`CloudSessionStore`] when it wants the session to
//! survive a restart.
//!
//! Security boundaries encoded here:
//!
//! - the bearer session lives in memory and in the host's private `0600` file,
//!   never in a URL, a log, an error or a view model;
//! - no request type has a field for an SSH private key, passphrase, password or
//!   provider secret;
//! - a relay ticket carries only a public P-256 JWK, a public nonce and scopes;
//! - [`CloudRequest`] and [`CloudResponse`] redact their bodies from `Debug`.
//!
//! The relay WebSocket/crypto channel is a renderer transport (`CloudRelayClient`
//! in the Web app); this crate stops at the typed ticket boundary and remains
//! headless.

pub mod client;
pub mod error;
pub mod session;
pub mod types;

pub use client::{
    CloudClient, CloudMethod, CloudRequest, CloudResponse, CloudTransport, DEFAULT_API_BASE_URL,
    DEFAULT_DEVICE_CLIENT_ID, DEFAULT_REQUEST_TIMEOUT,
};
pub use error::{CloudError, CloudErrorCode};
pub use session::{
    CLOUD_SESSION_FILE, CLOUD_SESSION_SCHEMA_VERSION, CloudSession, CloudSessionStore,
    JsonCloudSessionStore,
};
pub use types::{
    BackupPlan, ClientKind, CloudAccount, CloudMachine, DeviceAuthorization, DevicePollState,
    MachinePresence, ManagedPlan, ManagedPlanCatalog, ProvisioningPhase, ProvisioningStatus,
    RelayPublicJwk, RelayScope, RelayTicket, RelayTicketMachine, RelayTicketRequest,
};
