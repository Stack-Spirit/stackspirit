//! Poll scheduling, jitter, staleness and cancellation.

use std::time::Duration;

use tokio::sync::watch;

use crate::time::Timestamp;
use crate::wire::FleetStatus;

/// Poll cadence and freshness policy.
#[derive(Debug, Clone, PartialEq)]
pub struct PollPolicy {
    /// Fleet poll cadence while connected.
    pub fleet_connected: Duration,
    /// Fleet poll cadence while idle or backing off.
    pub fleet_idle: Duration,
    /// Consecutive failures that mark the snapshot stale.
    pub stale_after_failures: u32,
    /// Backoff cadence after staleness.
    pub stale_backoff: Duration,
    /// Jitter as a fraction of the base delay (0.10 = ±10%).
    pub jitter_fraction: f64,
    /// Minimum freshness window, regardless of tick length.
    pub min_freshness: Duration,
    /// Multiplier applied to the supervisor tick for the freshness window.
    pub tick_multiplier: u64,
}

impl Default for PollPolicy {
    fn default() -> Self {
        Self {
            fleet_connected: Duration::from_secs(4),
            fleet_idle: Duration::from_secs(15),
            stale_after_failures: 3,
            stale_backoff: Duration::from_secs(30),
            jitter_fraction: 0.10,
            min_freshness: Duration::from_secs(180),
            tick_multiplier: 3,
        }
    }
}

/// The scheduler's coarse phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollPhase {
    /// Healthy polling.
    Connected,
    /// A few failures; polling at the idle cadence.
    Backoff,
    /// Three or more failures; stale data with a longer backoff.
    StaleBackoff,
}

/// Deterministic poll scheduler with jitter and three-failure staleness.
#[derive(Debug, Clone)]
pub struct PollScheduler {
    policy: PollPolicy,
    consecutive_failures: u32,
    seed: u64,
}

impl PollScheduler {
    /// Builds a scheduler from a policy.
    #[must_use]
    pub fn new(policy: PollPolicy) -> Self {
        Self {
            policy,
            consecutive_failures: 0,
            seed: 0x9E37_79B9_7F4A_7C15,
        }
    }

    /// Returns the active phase.
    #[must_use]
    pub fn phase(&self) -> PollPhase {
        if self.consecutive_failures >= self.policy.stale_after_failures {
            PollPhase::StaleBackoff
        } else if self.consecutive_failures > 0 {
            PollPhase::Backoff
        } else {
            PollPhase::Connected
        }
    }

    /// Records a successful poll and resets the failure count.
    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
    }

    /// Records a failure and returns the new streak.
    pub fn record_failure(&mut self) -> u32 {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        self.consecutive_failures
    }

    /// Returns the current failure streak.
    #[must_use]
    pub const fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures
    }

    /// Returns the number of consecutive failures that marks the data stale.
    #[must_use]
    pub const fn phase_threshold(&self) -> u32 {
        self.policy.stale_after_failures
    }

    /// Returns the next delay, with bounded deterministic jitter.
    pub fn next_delay(&mut self) -> Duration {
        let base = match self.phase() {
            PollPhase::Connected => self.policy.fleet_connected,
            PollPhase::Backoff => self.policy.fleet_idle,
            PollPhase::StaleBackoff => self.policy.stale_backoff,
        };
        let fraction = self.policy.jitter_fraction.clamp(0.0, 0.5);
        let span = (base.as_millis() as f64 * fraction).round() as u128;
        let offset = if span == 0 {
            0
        } else {
            self.seed = self
                .seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            // Sample in `0..=2 * span`, then subtract `span`: the documented
            // cadence is `base ± jitter_fraction`, not `base .. base + 2 * span`.
            u128::from(self.seed) % ((span * 2) + 1)
        };
        let millis = if offset >= span {
            base.as_millis().saturating_add(offset - span)
        } else {
            base.as_millis().saturating_sub(span - offset)
        };
        Duration::from_millis(u64::try_from(millis).unwrap_or(u64::MAX))
    }

    /// Returns the freshness window: `max(tick_multiplier * tick_secs, min_freshness)`.
    #[must_use]
    pub fn freshness_window(&self, tick_secs: u64) -> Duration {
        let by_tick = Duration::from_secs(tick_secs.saturating_mul(self.policy.tick_multiplier));
        by_tick.max(self.policy.min_freshness)
    }

    /// Evaluates freshness for one `fleet status` answer.
    #[must_use]
    pub fn staleness(&self, status: &FleetStatus, now: Timestamp) -> StaleAssessment {
        if status.source_is_cache() {
            return StaleAssessment {
                stale: true,
                reason: Some("VOWP returned a cached answer".to_owned()),
            };
        }
        if status.stale {
            return StaleAssessment {
                stale: true,
                reason: Some("VOWP marked the snapshot stale".to_owned()),
            };
        }
        let window = self.freshness_window(status.snapshot.supervisor.tick_secs);
        let age = now.since(status.fetched_at);
        if age > window {
            StaleAssessment {
                stale: true,
                reason: Some(format!(
                    "snapshot is older than the {}s freshness window",
                    window.as_secs()
                )),
            }
        } else {
            StaleAssessment {
                stale: false,
                reason: None,
            }
        }
    }
}

/// Freshness result for one snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleAssessment {
    /// Whether the snapshot is stale.
    pub stale: bool,
    /// Safe explanation, when stale.
    pub reason: Option<String>,
}

/// A cancellation token shared by poll calls. Clones observe the same signal.
#[derive(Debug, Clone)]
pub struct CancellationToken {
    sender: watch::Sender<bool>,
    receiver: watch::Receiver<bool>,
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancellationToken {
    /// Builds an uncancelled token.
    #[must_use]
    pub fn new() -> Self {
        let (sender, receiver) = watch::channel(false);
        Self { sender, receiver }
    }

    /// Requests cancellation.
    pub fn cancel(&self) {
        let _ = self.sender.send(true);
    }

    /// Returns true after [`cancel`](Self::cancel).
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        *self.receiver.borrow()
    }

    /// Waits until cancellation is requested.
    pub async fn cancelled(&mut self) {
        if self.is_cancelled() {
            return;
        }
        let _ = self.receiver.changed().await;
    }
}
