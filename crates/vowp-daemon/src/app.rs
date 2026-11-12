//! The axum application: state, routes and request handlers.
//!
//! Every request opens its own read-only [`vowp_store::Store`] against the served home.
//! The store handle is `Send` but not `Sync`, so a shared handle would serialize requests
//! or require a lock; opening per request also gives each request a fresh WAL view. The
//! blocking SQLite and JSONL work runs on `tokio::task::spawn_blocking`.

use std::future::Future;

use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use tokio::net::TcpListener;
use vowp_protocol::api::{ApiError, ApiErrorBody, EventPage, Health, JobPage, SessionPage};
use vowp_protocol::{ErrorKind, NodeName, ProjectName, SessionId, Timestamp};
use vowp_store::session_log::SessionLog;
use vowp_store::{JobFilter, NodePaths, SessionFilter, Store};

use crate::config::ProjectSelection;
use crate::filters::{self, EventQuery};

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
}

/// Builds the API router.
pub fn router(state: ApiState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/jobs", get(jobs))
        .route("/api/v1/sessions", get(sessions))
        .route("/api/v1/sessions/{id}/events", get(session_events))
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
