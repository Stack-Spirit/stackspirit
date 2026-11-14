//! Provider limit rows.

use serde::{Deserialize, Serialize};

use crate::time::Timestamp;
use crate::wire::enums::{HarnessKind, LimitRowKind};

/// One row of the provider limits table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Limit {
    /// Limits key: the provider name, or a CLI harness entry name.
    pub provider: String,
    /// Harness that reported the limit, when known.
    pub harness: Option<HarnessKind>,
    /// Limit class.
    pub kind: LimitRowKind,
    /// When the limit lifts; `None` means a human must resolve it.
    pub resets_at: Option<Timestamp>,
    /// Redacted human-readable detail.
    pub detail: String,
    /// When the limit was first observed.
    pub since: Timestamp,
    /// When the rung was last probed for recovery.
    pub last_probe_at: Option<Timestamp>,
    /// Consecutive failed probes.
    pub probe_fail_streak: u32,
    /// Temporary concurrency cap, when the provider reports one.
    pub concurrency_cap: Option<u32>,
    /// When the concurrency cap expires.
    pub cap_until: Option<Timestamp>,
}
