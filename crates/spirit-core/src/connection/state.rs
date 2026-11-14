//! Connection and poll state.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::error::SpiritError;
use crate::host::profile::HostId;
use crate::time::Timestamp;

/// The stage of an in-progress connect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectStage {
    /// Verifying the pinned host key and `known_hosts`.
    ResolvingHostKey,
    /// Opening the SSH transport.
    OpeningSsh,
    /// Running `vowp --version`.
    CheckingVowp,
    /// Reading and parsing `fleet status`.
    ReadingSnapshot,
}

impl ConnectStage {
    /// Returns a short UI label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ResolvingHostKey => "Resolving host key",
            Self::OpeningSsh => "Opening SSH",
            Self::CheckingVowp => "Checking VOWP",
            Self::ReadingSnapshot => "Reading snapshot",
        }
    }
}

/// The state machine of the single active host connection.
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    /// No host is connected.
    Disconnected,
    /// A connect is in progress.
    Connecting {
        /// Host being connected.
        host_id: HostId,
        /// Current stage.
        stage: ConnectStage,
    },
    /// The host is connected and the last poll succeeded.
    Connected {
        /// Connected host.
        host_id: HostId,
        /// When the connection became healthy.
        since: Timestamp,
        /// Round-trip latency statistics.
        latency: LatencyStats,
    },
    /// Three consecutive polls failed; live data is dimmed and writes are disabled.
    Stale {
        /// Previously connected host.
        host_id: HostId,
        /// Last successful poll.
        last_ok_at: Timestamp,
        /// Safe error summary.
        error: String,
    },
    /// A disconnect is in progress.
    Disconnecting {
        /// Host being disconnected.
        host_id: HostId,
    },
    /// The last connect attempt failed.
    Failed {
        /// Host that failed.
        host_id: HostId,
        /// Safe error.
        error: SpiritError,
        /// Whether retrying may succeed.
        retryable: bool,
    },
}

impl ConnectionState {
    /// Returns the stable state string used by the bridge.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Disconnected => "disconnected",
            Self::Connecting { .. } => "connecting",
            Self::Connected { .. } => "connected",
            Self::Stale { .. } => "stale",
            Self::Disconnecting { .. } => "disconnecting",
            Self::Failed { .. } => "failed",
        }
    }

    /// Returns the active host, if any.
    #[must_use]
    pub fn host_id(&self) -> Option<&HostId> {
        match self {
            Self::Disconnected => None,
            Self::Connecting { host_id, .. }
            | Self::Connected { host_id, .. }
            | Self::Stale { host_id, .. }
            | Self::Disconnecting { host_id }
            | Self::Failed { host_id, .. } => Some(host_id),
        }
    }

    /// Returns true only when writes are allowed.
    ///
    /// A stale snapshot, a failed connection, or an in-progress transition always
    /// disables mutations until a refresh succeeds.
    #[must_use]
    pub const fn allows_mutations(&self) -> bool {
        matches!(self, Self::Connected { .. })
    }

    /// Returns the current latency stats, when connected.
    #[must_use]
    pub fn latency(&self) -> Option<&LatencyStats> {
        match self {
            Self::Connected { latency, .. } => Some(latency),
            _ => None,
        }
    }

    /// Projects the state into the flat bridge shape.
    #[must_use]
    pub fn snapshot(&self) -> ConnectionSnapshot {
        match self {
            Self::Disconnected => ConnectionSnapshot {
                state: self.label().to_owned(),
                host_id: None,
                since: None,
                latency: None,
                last_ok_at: None,
                error: None,
            },
            Self::Connecting { host_id, .. } => ConnectionSnapshot {
                state: self.label().to_owned(),
                host_id: Some(host_id.clone()),
                since: None,
                latency: None,
                last_ok_at: None,
                error: None,
            },
            Self::Connected {
                host_id,
                since,
                latency,
            } => ConnectionSnapshot {
                state: self.label().to_owned(),
                host_id: Some(host_id.clone()),
                since: Some(*since),
                latency: Some(latency.clone()),
                last_ok_at: Some(*since),
                error: None,
            },
            Self::Stale {
                host_id,
                last_ok_at,
                error,
            } => ConnectionSnapshot {
                state: self.label().to_owned(),
                host_id: Some(host_id.clone()),
                since: None,
                latency: None,
                last_ok_at: Some(*last_ok_at),
                error: Some(SpiritError::new(
                    crate::error::SpiritErrorCode::RemoteTimeout,
                    error.clone(),
                )),
            },
            Self::Disconnecting { host_id } => ConnectionSnapshot {
                state: self.label().to_owned(),
                host_id: Some(host_id.clone()),
                since: None,
                latency: None,
                last_ok_at: None,
                error: None,
            },
            Self::Failed {
                host_id,
                error,
                retryable,
            } => ConnectionSnapshot {
                state: self.label().to_owned(),
                host_id: Some(host_id.clone()),
                since: None,
                latency: None,
                last_ok_at: None,
                error: Some(SpiritError {
                    retryable: *retryable,
                    ..error.clone()
                }),
            },
        }
    }
}

/// The flat bridge representation of a connection state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectionSnapshot {
    /// Stable state string.
    pub state: String,
    /// Active host, if any.
    pub host_id: Option<HostId>,
    /// When the connection became healthy.
    pub since: Option<Timestamp>,
    /// Latency statistics.
    pub latency: Option<LatencyStats>,
    /// Last successful poll.
    pub last_ok_at: Option<Timestamp>,
    /// Current error, if any.
    pub error: Option<SpiritError>,
}

/// Round-trip latency statistics for `fleet status`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LatencyStats {
    /// Last sample.
    pub last_ms: u64,
    /// Exponentially weighted moving average.
    pub ewma_ms: f64,
    /// Median of the recent window.
    pub p50_ms: u64,
    /// p95 of the recent window.
    pub p95_ms: u64,
    /// Number of samples recorded.
    pub samples: u32,
    /// Recent samples, not serialized to the UI.
    #[serde(skip)]
    history: VecDeque<u64>,
}

impl Default for LatencyStats {
    fn default() -> Self {
        Self {
            last_ms: 0,
            ewma_ms: 0.0,
            p50_ms: 0,
            p95_ms: 0,
            samples: 0,
            history: VecDeque::new(),
        }
    }
}

impl LatencyStats {
    /// Records one round-trip sample.
    pub fn record(&mut self, ms: u64) {
        self.last_ms = ms;
        if self.samples == 0 {
            self.ewma_ms = ms as f64;
        } else {
            self.ewma_ms = (0.25 * ms as f64) + (0.75 * self.ewma_ms);
        }
        self.samples = self.samples.saturating_add(1);
        self.history.push_back(ms);
        while self.history.len() > 100 {
            self.history.pop_front();
        }
        let mut sorted: Vec<u64> = self.history.iter().copied().collect();
        sorted.sort_unstable();
        self.p50_ms = percentile(&sorted, 50);
        self.p95_ms = percentile(&sorted, 95);
    }
}

fn percentile(sorted: &[u64], percent: u32) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (sorted.len() as u64 * u64::from(percent)).div_ceil(100);
    let index = usize::try_from(rank.saturating_sub(1)).unwrap_or(0);
    sorted[index.min(sorted.len() - 1)]
}

/// A monotonically increasing event revision.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Revision(u64);

impl Revision {
    /// Starts at zero.
    #[must_use]
    pub const fn new() -> Self {
        Self(0)
    }

    /// Returns the current value.
    #[must_use]
    pub const fn current(self) -> u64 {
        self.0
    }

    /// Increments and returns the next value.
    pub fn advance(&mut self) -> u64 {
        self.0 = self.0.saturating_add(1);
        self.0
    }

    /// Returns true when an incoming revision is newer than the current one.
    #[must_use]
    pub const fn accepts(self, incoming: u64) -> bool {
        incoming > self.0
    }
}

/// Returns true when a store should apply an incoming revision.
#[must_use]
pub const fn accepts_revision(current: u64, incoming: u64) -> bool {
    incoming > current
}
