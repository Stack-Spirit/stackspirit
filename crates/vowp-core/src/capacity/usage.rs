//! Per-run usage rows. E1 fills this module.

use vowp_protocol::RunUsage;
use vowp_store::Store;

use crate::CoreError;

/// Stores one finished run's measured usage.
pub fn record(_store: &Store, _usage: &RunUsage) -> Result<(), CoreError> {
    Ok(())
}
