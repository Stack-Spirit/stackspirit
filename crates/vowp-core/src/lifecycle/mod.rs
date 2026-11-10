//! Lifecycle classes, garbage collection, compaction and backup.
//!
//! Spec: `docs/design/v0.2/home-and-surfaces.md` §10 and §13.2. C0 creates the module and
//! its compiling stubs; H1/H2 fill the bodies.

use std::path::PathBuf;

use vowp_protocol::{CatalogClass, GcReport, ProjectName, Timestamp};
use vowp_store::Store;

use crate::CoreError;
use crate::deps::Deps;

pub mod backup;
pub mod classes;
pub mod compact;
pub mod gc;

pub use backup::{BackupOptions, backup, restore};
pub use classes::classify;
pub use compact::{CompactReport, compact_session};
pub use gc::{GcOptions, gc};

/// One file with its lifecycle class.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassifiedFile {
    /// File path.
    pub path: PathBuf,
    /// Lifecycle class.
    pub class: CatalogClass,
    /// Project the file belongs to, when bound.
    pub project: Option<ProjectName>,
    /// Size in bytes.
    pub bytes: u64,
    /// Why the file got this class.
    pub reason: String,
}

/// One node-level lifecycle pass. C0 does nothing.
pub fn tick(_deps: &Deps, _store: &Store, _now: Timestamp) -> Result<(), CoreError> {
    Ok(())
}

/// Returns an empty GC report for the node.
pub(crate) fn empty_gc_report(now: Timestamp, dry_run: bool) -> GcReport {
    GcReport {
        generated_at: now,
        dry_run,
        classes: Vec::new(),
        deleted_files: 0,
        freed_bytes: 0,
        compacted_sessions: 0,
        errors: Vec::new(),
    }
}
