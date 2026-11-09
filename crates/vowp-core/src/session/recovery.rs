//! Startup recovery: finish or abort switch rows left in `started` and close runs that
//! outlived their supervisor. Spec: `docs/design/sessions.md` §5 (failure table) and
//! `docs/design/crates/vowp-core.md` §3.
//!
//! Crash recovery never rewrites the log: it either appends the missing `switch` event
//! and marks the row completed, or marks it aborted and recomputes the session state
//! (`blocked` when the last turn ended with a limit, else `idle`). Exactly one of the
//! two outcomes is chosen per row.

use vowp_store::Store;

use crate::deps::Deps;
use crate::error::CoreError;
pub use crate::session::service::RecoveryReport;
use crate::session::service::SessionService;

/// Runs startup recovery against `store`.
///
/// The algorithm lives on [`SessionService::recover`] because the supervisor owns one
/// service for the process; this module is the stable `session::recovery` entry point
/// used by loops and by the crash tests.
pub fn recover(deps: &Deps, store: &Store) -> Result<RecoveryReport, CoreError> {
    SessionService::new(deps.clone()).recover(store)
}
