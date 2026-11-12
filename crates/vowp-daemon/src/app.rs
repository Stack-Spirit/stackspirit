//! The axum application: state, routes and request handlers.
//!
//! Every request opens its own read-only [`vowp_store::Store`] against the served home.
//! The store handle is `Send` but not `Sync`, so a shared handle would serialize requests
//! or require a lock; opening per request also gives each request a fresh WAL view. The
//! blocking SQLite and JSONL work runs on `tokio::task::spawn_blocking`.

use std::future::Future;

use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use tokio::net::TcpListener;
use vowp_core::ledger::BoardSnapshot;
use vowp_protocol::api::{
    ApiError, ApiErrorBody, Board, BoardLimit, BoardRun, CostReport, CostRow as ApiCostRow, CostTotal, EventPage,
    Health, InboxPage, JobPage, SessionPage,
};
use vowp_protocol::{ErrorKind, InboxState, NodeName, ProjectName, SessionId, Timestamp, VowpConfig};
use vowp_store::session_log::SessionLog;
use vowp_store::{CostQuery, CostRow as StoreCostRow, JobFilter, NodePaths, SessionFilter, Store};

use crate::config::ProjectSelection;
use crate::filters::{self, CostQuerySpec, EventQuery};

/// State shared by every request handler.
#[derive(Debug, Clone)]
pub struct ApiState {
    paths: NodePaths,
    project: ProjectName,
    node: NodeName,
    schema_version: i64,
}

impl ApiState {
    /// Builds request state from the resolved home and project selection.
    pub fn new(paths: &NodePaths, selection: &ProjectSelection) -> Self {
        Self {
            paths: paths.clone(),
            project: selection.project.clone(),
            node: selection.node.clone(),
            schema_version: selection.schema_version,
        }
    }

    /// Opens the served project read-only, mapping failures onto a path-free 500.
    fn open(&self) -> Result<Store, ApiFailure> {
        Store::open_read_only(&self.paths, Some(&self.project)).map_err(|_| store_failure())
    }

    /// Returns the jobs matching `filter`.
    fn jobs(&self, filter: &JobFilter) -> Result<JobPage, ApiFailure> {
        let store = self.open()?;
        let jobs = store.list_jobs(filter).map_err(|_| store_failure())?;
        Ok(JobPage {
            total: usize_to_u64(jobs.len()),
            items: jobs,
            next: None,
        })
    }

    /// Returns the sessions matching `filter`.
    fn sessions(&self, filter: &SessionFilter) -> Result<SessionPage, ApiFailure> {
        let store = self.open()?;
        let sessions = store.list_sessions(filter).map_err(|_| store_failure())?;
        Ok(SessionPage {
            total: usize_to_u64(sessions.len()),
            items: sessions,
            next: None,
        })
    }

    /// Returns the served project's inbox items in `state`.
    fn inbox(&self, state: InboxState) -> Result<InboxPage, ApiFailure> {
        let store = self.open()?;
        let items = store.list_inbox(Some(state)).map_err(|_| store_failure())?;
        Ok(InboxPage {
            total: usize_to_u64(items.len()),
            items,
            next: None,
        })
    }

    /// Returns the served project's cost report grouped and bounded by `spec`.
    fn cost(&self, spec: CostQuerySpec) -> Result<CostReport, ApiFailure> {
        let store = self.open()?;
        let rows = store
            .cost_totals(
                &CostQuery {
                    since: spec.since,
                    until: spec.until,
                    project: Some(self.project.clone()),
                    job_id: None,
                },
                spec.group,
            )
            .map_err(|_| store_failure())?;
        Ok(build_cost_report(spec.by, rows))
    }

    /// Returns one page of the session's log read from disk.
    fn events(&self, id: &SessionId, query: EventQuery) -> Result<EventPage, ApiFailure> {
        let store = self.open()?;
        match store.session(id).map_err(|_| store_failure())? {
            Some(_) => {}
            None => {
                return Err(ApiFailure::not_found(format!("session {id} not found")));
            }
        }
        let reader = SessionLog::open_read(&self.paths, id).map_err(|_| {
            ApiFailure::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorKind::Io,
                "the session log could not be read",
            )
        })?;
        let events = reader
            .read_from(query.after_seq, Some(query.limit as usize))
            .map_err(|_| {
                ApiFailure::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorKind::Io,
                    "the session log could not be read",
                )
            })?;
        let next = if query.limit > 0 && events.len() == query.limit as usize {
            events.last().map(|event| event.seq)
        } else {
            None
        };
        Ok(EventPage {
            total: usize_to_u64(events.len()),
            items: events,
            next,
        })
    }

    /// Returns the served project's registered [`VowpConfig`], filled from its registry row.
    ///
    /// The registry JSON is written by `vowp init`; a project registered without `[project].path`
    /// (`vowp init` leaves it out) falls back to the registry path, matching the CLI and the
    /// supervisor so `last_checkpoint` reads the project's own checkout.
    fn config(&self, store: &Store) -> Result<VowpConfig, ApiFailure> {
        let project = store
            .project(&self.project)
            .map_err(|_| store_failure())?
            .ok_or_else(config_failure)?;
        let mut config: VowpConfig = serde_json::from_value(project.config).map_err(|_| config_failure())?;
        if config.project.name != self.project {
            return Err(config_failure());
        }
        config.validate().map_err(|_| config_failure())?;
        if config.project.path.is_none() {
            config.project.path = Some(project.path);
        }
        Ok(config)
    }

    /// Returns the board snapshot as the exact `vowp board --json` document.
    fn board(&self) -> Result<Board, ApiFailure> {
        let store = self.open()?;
        let config = self.config(&store)?;
        let snapshot = BoardSnapshot::collect(&store, &config, store.clock().now()).map_err(|_| board_failure())?;
        Ok(Board {
            at: snapshot.at,
            project: snapshot.project,
            jobs: snapshot.jobs_by_state,
            running: snapshot
                .running
                .iter()
                .map(|run| BoardRun {
                    job: run.job.clone(),
                    executor: run.executor.as_ref().map(ToString::to_string),
                    age_secs: run.age_secs,
                    last_event_age_secs: run.last_event_age_secs,
                })
                .collect(),
            slots_busy: snapshot.slots_busy,
            slots_total: snapshot.slots_total,
            planner: snapshot.planner_executor.as_ref().map(ToString::to_string),
            keeper: snapshot.keeper_executor.as_ref().map(ToString::to_string),
            limits: snapshot
                .limits
                .iter()
                .map(|limit| BoardLimit {
                    provider: limit.provider.clone(),
                    kind: limit.kind,
                    resets_at: limit.resets_at,
                })
                .collect(),
            cost_today_usd: snapshot.cost_today.usd,
            inbox_open: snapshot.inbox_open,
            last_checkpoint: snapshot.last_checkpoint,
        })
    }
}

/// Builds the router: the static board page at `/` and its phone alias `/board`, the static
/// session pages at `/sessions` and `/sessions/{id}`, plus the API under `/api/v1`.
pub fn router(state: ApiState) -> Router {
    Router::new()
        .route("/", get(board_page))
        .route("/board", get(board_page))
        .route("/sessions", get(sessions_page))
        .route("/sessions/{id}", get(sessions_page))
        .route("/api/v1/health", get(health))
        .route("/api/v1/board", get(board))
        .route("/api/v1/jobs", get(jobs))
        .route("/api/v1/sessions", get(sessions))
        .route("/api/v1/sessions/{id}/events", get(session_events))
        .route("/api/v1/inbox", get(inbox))
        .route("/api/v1/cost", get(cost))
        .fallback(unknown_endpoint)
        .with_state(state)
}

/// Serves the API on `listener` until the process stops.
pub async fn serve(listener: TcpListener, state: ApiState) -> std::io::Result<()> {
    axum::serve(listener, router(state)).await
}

/// Serves the API until `shutdown` resolves.
pub async fn serve_with_shutdown<F>(listener: TcpListener, state: ApiState, shutdown: F) -> std::io::Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown)
        .await
}

/// The embedded static board page, served read-only at `/` and `/board`.
const BOARD_PAGE: &str = include_str!("../web/index.html");

/// Serves the static board page; it fetches `/api/v1/board` and polls every five seconds.
///
/// The same shell is served at `/` and `/board`. Its card layout and `max-width: 360px`
/// rules make it usable on a phone without a separate page or a second API mapping.
async fn board_page() -> Html<&'static str> {
    Html(BOARD_PAGE)
}

/// The embedded static session shell, served read-only at `/sessions` and `/sessions/{id}`.
const SESSIONS_PAGE: &str = include_str!("../web/sessions.html");

/// Serves the static sessions shell.
///
/// The page reads its own path: `/sessions` renders the read-only session list and
/// `/sessions/{id}` renders one session's metadata and a polling event timeline. Both
/// routes serve the same shell; only the two read-only API endpoints are fetched.
async fn sessions_page() -> Html<&'static str> {
    Html(SESSIONS_PAGE)
}

/// Returns the health document for the served project.
async fn health(State(state): State<ApiState>) -> Json<Health> {
    Json(Health {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        node: state.node.clone(),
        project: state.project.clone(),
        schema_version: state.schema_version,
        heartbeat: read_heartbeat(&state.paths),
    })
}

/// Returns the served project's board as the `vowp board --json` document.
async fn board(State(state): State<ApiState>) -> Result<Json<Board>, ApiFailure> {
    let board = blocking(move || state.board()).await?;
    Ok(Json(board))
}

/// Lists jobs matching the query filters.
async fn jobs(State(state): State<ApiState>, RawQuery(query): RawQuery) -> Result<Json<JobPage>, ApiFailure> {
    let filter = filters::job_filter(query.as_deref()).map_err(ApiFailure::invalid)?;
    let page = blocking(move || state.jobs(&filter)).await?;
    Ok(Json(page))
}

/// Lists sessions matching the query filters.
async fn sessions(State(state): State<ApiState>, RawQuery(query): RawQuery) -> Result<Json<SessionPage>, ApiFailure> {
    let mut filter = filters::session_filter(query.as_deref()).map_err(ApiFailure::invalid)?;
    if filter.project.is_none() {
        filter.project = Some(state.project.clone());
    }
    let page = blocking(move || state.sessions(&filter)).await?;
    Ok(Json(page))
}

/// Lists the served project's inbox items in one state.
async fn inbox(State(state): State<ApiState>, RawQuery(query): RawQuery) -> Result<Json<InboxPage>, ApiFailure> {
    let filter = filters::inbox_state(query.as_deref()).map_err(ApiFailure::invalid)?;
    let page = blocking(move || state.inbox(filter)).await?;
    Ok(Json(page))
}

/// Returns the served project's cost report as the `vowp cost --json` document.
async fn cost(State(state): State<ApiState>, RawQuery(query): RawQuery) -> Result<Json<CostReport>, ApiFailure> {
    let spec = filters::cost_query(query.as_deref()).map_err(ApiFailure::invalid)?;
    let report = blocking(move || state.cost(spec)).await?;
    Ok(Json(report))
}

/// Returns one page of a session's exact stored events.
async fn session_events(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    RawQuery(query): RawQuery,
) -> Result<Json<EventPage>, ApiFailure> {
    let id = SessionId::parse(&id).map_err(|_| ApiFailure::invalid("invalid session id"))?;
    let query = filters::event_query(query.as_deref()).map_err(ApiFailure::invalid)?;
    let page = blocking(move || state.events(&id, query)).await?;
    Ok(Json(page))
}

/// Returns an `ApiError` for an unknown path.
async fn unknown_endpoint() -> ApiFailure {
    ApiFailure::not_found("unknown endpoint")
}

/// Runs blocking store or log work off the async reactor.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, ApiFailure> + Send + 'static,
) -> Result<T, ApiFailure> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| ApiFailure::new(StatusCode::INTERNAL_SERVER_ERROR, ErrorKind::Internal, "request failed"))?
}

/// The JSON error response for a failed request.
pub(crate) struct ApiFailure {
    status: StatusCode,
    body: ApiError,
}

impl ApiFailure {
    /// Builds a failure with no hint.
    fn new(status: StatusCode, kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            status,
            body: ApiError {
                error: ApiErrorBody {
                    kind,
                    message: message.into(),
                    hint: None,
                },
            },
        }
    }

    /// A `400` for an invalid filter, id or query value.
    fn invalid(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, ErrorKind::InvalidInput, message)
    }

    /// A `404` for an unknown session.
    fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, ErrorKind::NotFound, message)
    }
}

impl IntoResponse for ApiFailure {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

/// A path-free `500` for every store failure.
fn store_failure() -> ApiFailure {
    ApiFailure::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        ErrorKind::Db,
        "could not read the project database",
    )
}

/// A path-free `500` for an unreadable or invalid registered project config.
fn config_failure() -> ApiFailure {
    ApiFailure::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        ErrorKind::Config,
        "the project configuration could not be read",
    )
}

/// A path-free `500` for a board snapshot that could not be collected.
fn board_failure() -> ApiFailure {
    ApiFailure::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        ErrorKind::Db,
        "could not read the project board",
    )
}

/// Reduces store rows into the exact `vowp cost --json` document.
///
/// This mirrors `vowp_cli::cmd::cost::build_report`: tokens sum, known USD sums, unknown
/// entries stay separate and the source name derives from the entry counts.
fn build_cost_report(by: &'static str, rows: Vec<StoreCostRow>) -> CostReport {
    let mut total_tokens = vowp_protocol::Tokens::default();
    let mut total_usd = 0.0_f64;
    let mut total_runs = 0_u64;
    let mut receipt = 0_u64;
    let mut price_table = 0_u64;
    let mut unknown = 0_u64;

    let rows = rows
        .into_iter()
        .map(|row| {
            total_tokens.add(&row.tokens);
            total_usd += row.usd;
            total_runs += row.entries;
            receipt += row.receipt_entries;
            price_table += row.price_table_entries;
            unknown += row.unknown_entries;
            ApiCostRow {
                key: row.key,
                runs: row.entries,
                usd: known_usd(row.usd, row.entries, row.unknown_entries),
                tokens: row.tokens,
                source: source_name(row.receipt_entries, row.price_table_entries, row.unknown_entries).to_owned(),
                unknown_entries: row.unknown_entries,
            }
        })
        .collect();

    CostReport {
        by: by.to_owned(),
        rows,
        total: CostTotal {
            runs: total_runs,
            usd: known_usd(total_usd, total_runs, unknown),
            tokens: total_tokens,
            source: source_name(receipt, price_table, unknown).to_owned(),
            unknown_entries: unknown,
        },
    }
}

/// Returns the known USD amount, or `None` when every entry is unknown.
fn known_usd(usd: f64, entries: u64, unknown: u64) -> Option<f64> {
    if entries > 0 && unknown >= entries {
        None
    } else {
        Some(usd)
    }
}

/// Derives the source name from the receipt/price-table/unknown entry counts.
fn source_name(receipt: u64, price_table: u64, unknown: u64) -> &'static str {
    let present = [receipt > 0, price_table > 0, unknown > 0]
        .into_iter()
        .filter(|present| *present)
        .count();
    match (present, receipt > 0, price_table > 0, unknown > 0) {
        (0, _, _, _) => "unknown",
        (1, true, false, false) => "receipt",
        (1, false, true, false) => "price-table",
        (1, false, false, true) => "unknown",
        _ => "mixed",
    }
}

/// Reads `<home>/heartbeat` as Unix seconds, or `None` when absent or unreadable.
///
/// A file containing the legacy `stopped` marker is treated as no heartbeat, matching
/// `vowp status`: a deliberate stop must not look like a live supervisor.
fn read_heartbeat(paths: &NodePaths) -> Option<Timestamp> {
    let text = std::fs::read_to_string(paths.root().join("heartbeat")).ok()?;
    let mut lines = text.lines();
    let first = lines.next()?.trim();
    if lines.any(|line| line.trim() == "stopped") {
        return None;
    }
    first.parse::<i64>().ok().map(Timestamp::from_unix_secs)
}

/// Converts a collection length for the wire.
fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}
