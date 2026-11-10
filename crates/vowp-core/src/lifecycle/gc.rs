//! Garbage collection. H1 fills this module.

use vowp_protocol::{CatalogClass, GcReport, Timestamp};
use vowp_store::Store;

use crate::CoreError;
use crate::deps::Deps;

/// Options for `vowp gc`.
#[derive(Debug, Clone, Default)]
pub struct GcOptions {
    /// Delete eligible files instead of only reporting.
    pub apply: bool,
    /// Restrict to one class.
    pub class: Option<CatalogClass>,
    /// Clock override for tests.
    pub now: Option<Timestamp>,
}

/// Runs one GC pass.
pub fn gc(_deps: &Deps, _store: &Store, opts: &GcOptions) -> Result<GcReport, CoreError> {
    let now = opts.now.unwrap_or_else(Timestamp::now);
    Ok(super::empty_gc_report(now, !opts.apply))
}
