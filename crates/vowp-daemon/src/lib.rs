//! The VOWP daemon: a read-only loopback HTTP API over one existing VOWP home.
//!
//! The first v0.3 slice serves the static read-only board page at `GET /` and the API
//! endpoints `GET /api/v1/health`, `/api/v1/board`, `/api/v1/jobs`, `/api/v1/sessions` and
//! `/api/v1/sessions/{id}/events` with the same `vowp-protocol` objects the CLI prints.
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

pub use app::{ApiState, router, serve, serve_with_shutdown};
pub use config::{Invocation, Options, ProjectSelection, USAGE, parse_invocation, select_project};
pub use error::DaemonError;
