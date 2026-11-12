//! The VOWP daemon: a read-only loopback HTTP API over one existing VOWP home.
//!
//! The first v0.3 slice serves the static read-only board page at `GET /` and `GET /board`,
//! the static session list and detail pages at `GET /sessions` and `GET /sessions/{id}`,
//! the static cost and inbox pages at `GET /cost` and `GET /inbox`, and the API endpoints
//! `GET /api/v1/health`, `/api/v1/board`, `/api/v1/board/stream`, `/api/v1/jobs`,
//! `/api/v1/sessions`, `/api/v1/sessions/{id}`, `/api/v1/sessions/{id}/capsules/{capsuleId}`,
//! `/api/v1/sessions/{id}/events`, `/api/v1/sessions/{id}/events/stream`, `/api/v1/inbox` and
//! `/api/v1/cost` with the same `vowp-protocol` objects the CLI prints. The session detail
//! contains the exact `Session`, its latest capsule and at most 32 parent links, nearest
//! parent first; the capsule route answers only for a capsule belonging to that session.
//! The per-session stream sends stored `EventEnvelope` frames with periodic heartbeats and a
//! bounded per-client batch. The board stream sends the exact running `Board` document when
//! its content changes, plus heartbeats, and has no cursor or resume.
//!
//! The board page is also the phone board: its card layout is usable at a 360 px viewport.
//! It loads `/api/v1/board`, follows `/api/v1/board/stream`, and keeps a visible reconnect
//! state with bounded-backoff retry and a five-second polling fallback until the stream
//! opens. The session list page fetches `/api/v1/sessions`; the detail page loads its
//! metadata and a first event page, then follows the per-session SSE stream, with a visible
//! reconnect state, bounded-backoff retry and a five-second polling fallback.
//! The pages never write. The cost page renders grouped ledger rows and totals
//! with a `by` selector, and the inbox page renders open, answered and dismissed items with a
//! state selector; both only fetch the read-only cost and inbox endpoints.
//! It opens the home with [`vowp_store::Store::open_read_only`], never creates, migrates or
//! writes a home, and binds `127.0.0.1` only.
//!
//! **There is no auth token in this slice.** Nothing writes, and the listener is
//! loopback-only, so there is no privileged action to protect. A token must be added
//! before any write endpoint or a non-loopback listener; do not add a placeholder layer.
//! The daemon also never prints a home path, token or secret.
//!
//! Spec: `docs/design/v0.3/http-api.md`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod app;
mod config;
mod error;
mod filters;
mod stream;

pub use app::{ApiState, router, serve, serve_with_shutdown};
pub use config::{Invocation, Options, ProjectSelection, USAGE, parse_invocation, select_project};
pub use error::DaemonError;
