//! Documents served by the v0.3 loopback HTTP API.
//!
//! Spec: `docs/design/v0.3/http-api.md`. The API serves the same `vowp-protocol`
//! objects the CLI prints; these are only the transport documents layered on top.
//!
//! The first slice is read-only: it has no token, no write endpoint and no SSE stream.
//! The listener is loopback-only, so there is no privileged action to authenticate. A
//! token must be added before this API grows a write endpoint or a non-loopback listener.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cost::Tokens;
use crate::error::ErrorKind;
use crate::event::EventEnvelope;
use crate::ids::{JobId, NodeName, ProjectName};
use crate::inbox::InboxItem;
use crate::job::Job;
use crate::limit::LimitRowKind;
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

/// The `/api/v1/board` document.
///
/// This is exactly the JSON shape `vowp board --json` prints for one project, built from
/// `vowp_core::ledger::BoardSnapshot` so the web window and the phone board share one board
/// contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Board {
    /// Instant the snapshot was taken.
    pub at: Timestamp,
    /// Project the snapshot covers.
    pub project: ProjectName,
    /// Job counts keyed by canonical snake_case state name.
    pub jobs: BTreeMap<String, u32>,
    /// Jobs currently holding a run or worktree lock.
    pub running: Vec<BoardRun>,
    /// Worker and verifier slots currently busy.
    pub slots_busy: u32,
    /// Worker and verifier slots configured.
    pub slots_total: u32,
    /// First rung of the `planner` profile, or `null` when it is not configured.
    pub planner: Option<String>,
    /// First rung of the `keeper` profile, or `null` when it is not configured.
    pub keeper: Option<String>,
    /// Provider limits still in force, reduced to the board columns.
    pub limits: Vec<BoardLimit>,
    /// Cost recorded today (UTC), or `null` when no USD amount is known.
    pub cost_today_usd: Option<f64>,
    /// Open inbox items.
    pub inbox_open: u32,
    /// Newest checkpoint time, or `null` when none exists.
    pub last_checkpoint: Option<Timestamp>,
}

/// One running job in [`Board::running`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BoardRun {
    /// Job id.
    pub job: JobId,
    /// Executor of the run, or `null` when the job holds a lock without a live run.
    pub executor: Option<String>,
    /// Seconds since the run (or the state change) started.
    pub age_secs: u64,
    /// Seconds since the session's last recorded event, when a session exists.
    pub last_event_age_secs: Option<u64>,
}

/// One reduced limit row in [`Board::limits`].
///
/// `kind` is [`LimitRowKind`], not the event-only [`crate::limit::LimitKind`], because a
/// provider marked `unavailable` after repeated unknown failures is still an active limit
/// and `vowp board --json` prints that class.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BoardLimit {
    /// Limits key: the provider name, or a CLI harness entry name.
    pub provider: String,
    /// Canonical limit class.
    pub kind: LimitRowKind,
    /// When the limit lifts, or `null` when a human must resolve it.
    pub resets_at: Option<Timestamp>,
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

/// One page of [`InboxItem`] objects.
pub type InboxPage = Page<InboxItem>;

/// The `/api/v1/cost` document.
///
/// This is exactly the JSON shape `vowp cost --json` prints: one row per grouped key and
/// a totals row. An unknown USD amount stays `null`; it is never rendered as zero.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CostReport {
    /// Dimension the rows are grouped by: `job`, `role`, `provider`, `model` or `day`.
    pub by: String,
    /// Grouped cost rows, ordered by known USD descending then key ascending.
    pub rows: Vec<CostRow>,
    /// Totals across every row.
    pub total: CostTotal,
}

/// One grouped row in a [`CostReport`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CostRow {
    /// Job id, role, provider, model or `YYYY-MM-DD` day, depending on the grouping.
    pub key: String,
    /// Cost entries in this group (receipt, price-table and unknown combined).
    pub runs: u64,
    /// Sum of known USD amounts; `null` when every entry is unknown.
    pub usd: Option<f64>,
    /// Token usage across the grouped entries.
    pub tokens: Tokens,
    /// `receipt`, `price-table`, `unknown` or `mixed`, derived from the entry sources.
    pub source: String,
    /// Entries whose USD amount is unknown (never counted as free).
    pub unknown_entries: u64,
}

/// The totals row in a [`CostReport`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CostTotal {
    /// Cost entries across every group.
    pub runs: u64,
    /// Sum of known USD amounts; `null` when every entry is unknown.
    pub usd: Option<f64>,
    /// Token usage across every group.
    pub tokens: Tokens,
    /// `receipt`, `price-table`, `unknown` or `mixed`, derived from the entry sources.
    pub source: String,
    /// Entries whose USD amount is unknown (never counted as free).
    pub unknown_entries: u64,
}

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
