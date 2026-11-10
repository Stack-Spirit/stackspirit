//! Lifecycle classification. H1 fills this module.

use vowp_protocol::Timestamp;
use vowp_store::Store;

use crate::CoreError;
use crate::deps::Deps;

use super::ClassifiedFile;

/// Classifies the node's files by lifecycle class.
pub fn classify(_deps: &Deps, _store: &Store, _now: Timestamp) -> Result<Vec<ClassifiedFile>, CoreError> {
    Ok(Vec::new())
}
