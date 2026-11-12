//! Documents served by the v0.3 loopback HTTP API.
//!
//! Spec: `docs/design/v0.3/http-api.md`. The API serves the same `vowp-protocol`
//! objects the CLI prints; these are only the transport documents layered on top.
//!
//! The first slice is read-only: it has no token, no write endpoint and no SSE stream.
//! The listener is loopback-only, so there is no privileged action to authenticate. A
//! token must be added before this API grows a write endpoint or a non-loopback listener.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::ErrorKind;
use crate::event::EventEnvelope;
use crate::ids::{NodeName, ProjectName};
use crate::job::Job;
use crate::session::Session;
use crate::time::Timestamp;

/// The `/api/v1/health` document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Health {
    /// Version of the running `vowp-daemon` binary.
    pub version: String,
    /// Node whose home the daemon serves.
    pub node: NodeName,
    /// Project the daemon serves.
    pub project: ProjectName,
    /// Store schema version of the served project database.
    pub schema_version: i64,
    /// Unix time of the last supervisor heartbeat, or `null` when none has run.
    pub heartbeat: Option<Timestamp>,
}

/// One page of a list response.
///
/// `next` is a cursor for the caller's next request, and `total` counts the items in this
/// response (it is not a server-side total). The jobs and sessions endpoints in the first
/// slice are unpaginated and always return `next: null`; the per-session events endpoint
/// sets `next` to the last returned `seq` when the response filled the requested limit, so
/// the caller can pass it as the next `after_seq`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Page<T> {
    /// Items in this page.
    pub items: Vec<T>,
    /// Cursor for the next page, or `null` when this page is the end of the list.
    pub next: Option<u64>,
    /// Number of items in this response.
    pub total: u64,
}

/// One page of [`Job`] objects.
pub type JobPage = Page<Job>;

/// One page of [`Session`] objects.
pub type SessionPage = Page<Session>;

/// One page of [`EventEnvelope`] objects.
pub type EventPage = Page<EventEnvelope>;

/// The error document returned by every failed API request.
///
/// The shape matches `vowp --json` errors: one `error` object with a stable machine
/// `kind`, a human-readable `message` and an optional `hint`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ApiError {
    /// The error body.
    pub error: ApiErrorBody,
}

/// The body of an [`ApiError`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ApiErrorBody {
    /// Stable machine-readable error class.
    pub kind: ErrorKind,
    /// Human-readable message; never contains internal paths or SQL text.
    pub message: String,
    /// Optional suggestion for fixing the request.
    pub hint: Option<String>,
}
