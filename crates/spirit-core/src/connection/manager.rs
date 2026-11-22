//! The connection manager: state transitions, revisions, events and poll decisions.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;
use tokio::sync::broadcast;

use crate::connection::poll::{CancellationToken, PollPolicy, PollScheduler};
use crate::connection::state::{
    ConnectStage, ConnectionSnapshot, ConnectionState, LatencyStats, Revision, accepts_revision,
};
use crate::engine::EngineStatus;
use crate::engine::gate::GateSnapshot;
use crate::error::{SpiritError, SpiritErrorCode};
use crate::host::profile::HostId;
use crate::time::Timestamp;
use crate::view::FleetView;
use crate::vowp::VowpClient;
use crate::wire::{CapacityReportDoc, FleetStatus, WireWarning};

/// An event emitted to the UI. Revisions increase monotonically across all kinds.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpiritEvent {
    /// Connection state changed.
    Connection {
        /// Monotonic revision.
        revision: u64,
        /// Flat connection snapshot.
        connection: ConnectionSnapshot,
    },
    /// A new fleet view is available.
    Fleet {
        /// Monotonic revision.
        revision: u64,
        /// Derived view model.
        fleet: Box<FleetView>,
    },
    /// Engine state changed.
    Engine {
        /// Monotonic revision.
        revision: u64,
        /// Engine status.
        engine: EngineStatus,
    },
    /// Gate snapshot changed.
    Gates {
        /// Monotonic revision.
        revision: u64,
        /// Gate snapshot.
        gates: GateSnapshot,
    },
    /// A non-fatal or fatal error occurred.
    Error {
        /// Monotonic revision.
        revision: u64,
        /// Safe error.
        error: SpiritError,
    },
}

impl SpiritEvent {
    /// Returns the event revision.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        match self {
            Self::Connection { revision, .. }
            | Self::Fleet { revision, .. }
            | Self::Engine { revision, .. }
            | Self::Gates { revision, .. }
            | Self::Error { revision, .. } => *revision,
        }
    }
}

struct ConnectionInner {
    state: ConnectionState,
    revision: Revision,
    scheduler: PollScheduler,
    latency: LatencyStats,
    last_status: Option<FleetStatus>,
    last_report: Option<CapacityReportDoc>,
    last_status_warnings: Vec<WireWarning>,
    last_capacity_warnings: Vec<WireWarning>,
    last_view: Option<FleetView>,
    last_ok_at: Option<Timestamp>,
    last_failed_stage: Option<ConnectStage>,
    cancel: CancellationToken,
}

/// Owns the single active host connection and its event fan-out.
#[derive(Clone)]
pub struct ConnectionManager {
    inner: Arc<Mutex<ConnectionInner>>,
    events: broadcast::Sender<SpiritEvent>,
}

impl std::fmt::Debug for ConnectionManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionManager")
            .field("status", &self.status())
            .finish_non_exhaustive()
    }
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new(PollPolicy::default())
    }
}

impl ConnectionManager {
    /// Builds a manager with a poll policy.
    #[must_use]
    pub fn new(policy: PollPolicy) -> Self {
        let (events, _) = broadcast::channel(64);
        Self {
            inner: Arc::new(Mutex::new(ConnectionInner {
                state: ConnectionState::Disconnected,
                revision: Revision::new(),
                scheduler: PollScheduler::new(policy),
                latency: LatencyStats::default(),
                last_status: None,
                last_report: None,
                last_status_warnings: Vec::new(),
                last_capacity_warnings: Vec::new(),
                last_view: None,
                last_ok_at: None,
                last_failed_stage: None,
                cancel: CancellationToken::new(),
            })),
            events,
        }
    }

    fn guard(&self) -> std::sync::MutexGuard<'_, ConnectionInner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn emit(&self, event: SpiritEvent) {
        let _ = self.events.send(event);
    }

    /// Subscribes to state events.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<SpiritEvent> {
        self.events.subscribe()
    }

    /// Returns the flat connection snapshot.
    #[must_use]
    pub fn status(&self) -> ConnectionSnapshot {
        self.guard().state.snapshot()
    }

    /// Returns the current state.
    #[must_use]
    pub fn state(&self) -> ConnectionState {
        self.guard().state.clone()
    }

    /// Returns the last good fleet view, if any.
    #[must_use]
    pub fn fleet_view(&self) -> Option<FleetView> {
        self.guard().last_view.clone()
    }

    /// Returns the last good raw status, if any.
    #[must_use]
    pub fn last_status(&self) -> Option<FleetStatus> {
        self.guard().last_status.clone()
    }

    /// Returns the last capacity report, if any.
    #[must_use]
    pub fn last_capacity_report(&self) -> Option<CapacityReportDoc> {
        self.guard().last_report.clone()
    }

    /// Returns the non-fatal warnings from the most recent `fleet status` parse.
    ///
    /// The raw-document drawer shows these next to the parsed fields.
    #[must_use]
    pub fn last_status_warnings(&self) -> Vec<WireWarning> {
        self.guard().last_status_warnings.clone()
    }

    /// Returns the non-fatal warnings from the most recent capacity report parse.
    #[must_use]
    pub fn last_capacity_warnings(&self) -> Vec<WireWarning> {
        self.guard().last_capacity_warnings.clone()
    }

    /// Returns the stage that failed during the last connect attempt.
    #[must_use]
    pub fn last_failed_stage(&self) -> Option<ConnectStage> {
        self.guard().last_failed_stage
    }

    /// Returns true only when the connection is healthy and the last view is fresh.
    #[must_use]
    pub fn mutations_allowed(&self) -> bool {
        let inner = self.guard();
        inner.state.allows_mutations() && inner.last_view.as_ref().is_none_or(|view| !view.stale)
    }

    /// Runs the poll loop until `disconnect` cancels it. One poll runs at a time; the
    /// loop sleeps for the scheduler's delay between attempts.
    pub async fn run_poll_loop(&self, client: std::sync::Arc<dyn VowpClient>) {
        loop {
            let delay = self.next_poll_delay();
            let mut token = self.guard().cancel.clone();
            tokio::select! {
                () = tokio::time::sleep(delay) => {}
                () = token.cancelled() => break,
            }
            if self.guard().state.host_id().is_none() {
                break;
            }
            let _ = self.poll_once(client.as_ref()).await;
        }
    }

    /// Returns the current latency statistics.
    #[must_use]
    pub fn latency(&self) -> LatencyStats {
        self.guard().latency.clone()
    }

    /// Returns the current failure streak.
    #[must_use]
    pub fn consecutive_failures(&self) -> u32 {
        self.guard().scheduler.consecutive_failures()
    }

    /// Returns the next poll delay and advances the scheduler's jitter seed.
    pub fn next_poll_delay(&self) -> std::time::Duration {
        self.guard().scheduler.next_delay()
    }

    /// Returns a clone of the current connection's cancel token. `disconnect` and the
    /// next `connect` both end work bound to this token, so loops started for one
    /// connection never outlive it.
    #[must_use]
    pub fn cancel_token(&self) -> CancellationToken {
        self.guard().cancel.clone()
    }

    /// Returns true after `disconnect` cancels the active poll token.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.guard().cancel.is_cancelled()
    }

    /// Returns true when a UI event with `incoming` should be applied.
    #[must_use]
    pub fn should_apply_revision(&self, incoming: u64) -> bool {
        accepts_revision(self.guard().revision.current(), incoming)
    }

    fn set_connecting(&self, host_id: &HostId, stage: ConnectStage) -> Result<(), SpiritError> {
        let mut inner = self.guard();
        if inner.cancel.is_cancelled() {
            return Err(connect_cancelled());
        }
        inner.state = ConnectionState::Connecting {
            host_id: host_id.clone(),
            stage,
        };
        let revision = inner.revision.advance();
        let snapshot = inner.state.snapshot();
        // Emit while the lock is held so a concurrent `disconnect` cannot place
        // its disconnected snapshot before this one with a lower revision.
        self.emit(SpiritEvent::Connection {
            revision,
            connection: snapshot,
        });
        Ok(())
    }

    fn fail(&self, host_id: &HostId, stage: ConnectStage, error: SpiritError) -> SpiritError {
        let mut inner = self.guard();
        if inner.cancel.is_cancelled() {
            return connect_cancelled();
        }
        inner.last_failed_stage = Some(stage);
        inner.state = ConnectionState::Failed {
            host_id: host_id.clone(),
            error: error.clone(),
            retryable: error.retryable,
        };
        let revision = inner.revision.advance();
        let snapshot = inner.state.snapshot();
        let event_revision = inner.revision.advance();
        self.emit(SpiritEvent::Connection {
            revision,
            connection: snapshot,
        });
        self.emit(SpiritEvent::Error {
            revision: event_revision,
            error: error.clone(),
        });
        error
    }

    /// Connects to one host through a client. The sequence reports the exact failing
    /// stage when it fails.
    ///
    /// `disconnect` cancels an in-flight attempt: the returned future stops at its
    /// current stage, the manager stays `Disconnected`, and no fleet event is emitted.
    pub async fn connect(
        &self,
        client: &dyn VowpClient,
        host_id: HostId,
    ) -> Result<ConnectionSnapshot, SpiritError> {
        let mut token = {
            let mut inner = self.guard();
            inner.cancel = CancellationToken::new();
            inner.last_failed_stage = None;
            inner.last_report = None;
            inner.scheduler.record_success();
            inner.cancel.clone()
        };
        self.set_connecting(&host_id, ConnectStage::ResolvingHostKey)?;
        self.set_connecting(&host_id, ConnectStage::OpeningSsh)?;

        let probe = match tokio::select! {
            result = client.probe() => result,
            () = token.cancelled() => return Err(connect_cancelled()),
        } {
            Ok(probe) => probe,
            Err(error) => {
                let stage = if error.code == SpiritErrorCode::VowpMissing {
                    ConnectStage::CheckingVowp
                } else {
                    ConnectStage::OpeningSsh
                };
                return Err(self.fail(&host_id, stage, error));
            }
        };

        self.set_connecting(&host_id, ConnectStage::CheckingVowp)?;
        if !probe.protocol_compatible {
            let error = SpiritError::vowp_incompatible(
                "vowp version",
                format!("version {} is not compatible", probe.version),
            );
            return Err(self.fail(&host_id, ConnectStage::CheckingVowp, error));
        }

        self.set_connecting(&host_id, ConnectStage::ReadingSnapshot)?;
        let started = Instant::now();
        let status = match tokio::select! {
            result = client.fleet_status_initial() => result,
            () = token.cancelled() => return Err(connect_cancelled()),
        } {
            Ok(status) => status,
            Err(error) => {
                return Err(self.fail(&host_id, ConnectStage::ReadingSnapshot, error));
            }
        };
        let status_warnings = client.take_warnings();
        let round_trip_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let now = Timestamp::now();
        let snapshot = {
            let mut inner = self.guard();
            // A `disconnect` can land after `fleet_status` resolves but before
            // this lock; never resurrect a connected snapshot in that case.
            if inner.cancel.is_cancelled() {
                return Err(connect_cancelled());
            }
            inner.latency.record(round_trip_ms);
            inner.scheduler.record_success();
            inner.last_status = Some(status.clone());
            inner.last_status_warnings = status_warnings;
            inner.last_ok_at = Some(now);
            inner.last_report = None;
            inner.state = ConnectionState::Connected {
                host_id: host_id.clone(),
                since: now,
                latency: inner.latency.clone(),
            };
            let revision = inner.revision.advance();
            let view = FleetView::from_status(
                host_id,
                &status,
                inner.last_report.as_ref(),
                inner.latency.clone(),
                revision,
                now,
            );
            inner.last_view = Some(view.clone());
            let connection = inner.state.snapshot();
            // Emit while the lock is held so a concurrent `disconnect` cannot
            // interleave its event before this connected snapshot.
            self.emit(SpiritEvent::Fleet {
                revision,
                fleet: Box::new(view),
            });
            self.emit(SpiritEvent::Connection {
                revision,
                connection: connection.clone(),
            });
            connection
        };
        Ok(snapshot)
    }

    /// Polls `fleet status` once. A parse failure marks the retained view stale at
    /// once; other failures move the state to `Stale` after three consecutive polls
    /// and start the longer backoff.
    pub async fn poll_once(
        &self,
        client: &dyn VowpClient,
    ) -> Result<Option<FleetView>, SpiritError> {
        let (host_id, since, cancelled) = {
            let inner = self.guard();
            let host_id = inner.state.host_id().cloned();
            let since = match &inner.state {
                ConnectionState::Connected { since, .. } => Some(*since),
                _ => None,
            };
            (host_id, since, inner.cancel.is_cancelled())
        };
        let Some(host_id) = host_id else {
            return Err(SpiritError::new(
                SpiritErrorCode::Internal,
                "poll_once requires a connected host",
            ));
        };
        if cancelled {
            return Err(SpiritError::new(
                SpiritErrorCode::RemoteTimeout,
                "poll cancelled",
            ));
        }

        let started = Instant::now();
        let mut token = self.guard().cancel.clone();
        let result = tokio::select! {
            result = client.fleet_status() => result,
            () = token.cancelled() => {
                return Err(SpiritError::new(
                    SpiritErrorCode::RemoteTimeout,
                    "poll cancelled",
                ));
            }
        };
        match result {
            Ok(status) => {
                let status_warnings = client.take_warnings();
                let round_trip_ms =
                    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                let now = Timestamp::now();
                let mut inner = self.guard();
                if inner.cancel.is_cancelled() {
                    return Err(SpiritError::new(
                        SpiritErrorCode::RemoteTimeout,
                        "poll cancelled",
                    ));
                }
                inner.latency.record(round_trip_ms);
                inner.scheduler.record_success();
                inner.last_status = Some(status.clone());
                inner.last_status_warnings = status_warnings;
                inner.last_ok_at = Some(now);
                inner.state = ConnectionState::Connected {
                    host_id: host_id.clone(),
                    since: since.unwrap_or(now),
                    latency: inner.latency.clone(),
                };
                let revision = inner.revision.advance();
                let view = FleetView::from_status(
                    host_id,
                    &status,
                    inner.last_report.as_ref(),
                    inner.latency.clone(),
                    revision,
                    now,
                );
                inner.last_view = Some(view.clone());
                let connection = inner.state.snapshot();
                // Emit while the lock is held so a concurrent `disconnect` cannot
                // interleave its event before this connected snapshot.
                self.emit(SpiritEvent::Fleet {
                    revision,
                    fleet: Box::new(view.clone()),
                });
                self.emit(SpiritEvent::Connection {
                    revision,
                    connection,
                });
                Ok(Some(view))
            }
            Err(error) => {
                let mut inner = self.guard();
                if inner.cancel.is_cancelled() {
                    return Err(SpiritError::new(
                        SpiritErrorCode::RemoteTimeout,
                        "poll cancelled",
                    ));
                }
                let failures = inner.scheduler.record_failure();
                // A parse failure or any hard (non-retryable) failure invalidates the
                // snapshot immediately: keep the last good view, but mark it stale and
                // surface the original error. Transient transport failures still wait
                // for the three-failure threshold so a single dropped packet does not
                // disable writes.
                let hard_failure = !error.retryable;
                if hard_failure || failures >= inner.scheduler.phase_threshold() {
                    let last_ok_at = inner.last_ok_at.unwrap_or_else(Timestamp::now);
                    inner.state = ConnectionState::Stale {
                        host_id: host_id.clone(),
                        last_ok_at,
                        error: error.clone(),
                    };
                    if let Some(view) = &mut inner.last_view {
                        view.stale = true;
                    }
                }
                let revision = inner.revision.advance();
                let snapshot = inner.state.snapshot();
                let error_revision = inner.revision.advance();
                self.emit(SpiritEvent::Connection {
                    revision,
                    connection: snapshot,
                });
                self.emit(SpiritEvent::Error {
                    revision: error_revision,
                    error: error.clone(),
                });
                drop(inner);
                tracing::debug!(failures, code = %error.code, "fleet poll failed");
                Err(error)
            }
        }
    }

    /// Refreshes the capacity report and re-derives the fleet view.
    ///
    /// The report is applied only while the connection is healthy. A stale or failed
    /// connection keeps its dimmed view, and `disconnect` cancels an in-flight read so
    /// a late report cannot publish fresh data after the connection is gone.
    pub async fn refresh_capacity(
        &self,
        client: &dyn VowpClient,
    ) -> Result<Option<FleetView>, SpiritError> {
        let mut token = self.guard().cancel.clone();
        let report = match tokio::select! {
            result = client.capacity_report() => result,
            () = token.cancelled() => {
                return Err(SpiritError::new(
                    SpiritErrorCode::RemoteTimeout,
                    "capacity refresh cancelled",
                ));
            }
        } {
            Ok(report) => report,
            Err(error) => {
                let revision = self.guard().revision.advance();
                self.emit(SpiritEvent::Error {
                    revision,
                    error: error.clone(),
                });
                return Err(error);
            }
        };
        let capacity_warnings = client.take_warnings();
        let (revision, view) = {
            let mut inner = self.guard();
            if inner.cancel.is_cancelled() {
                return Err(SpiritError::new(
                    SpiritErrorCode::RemoteTimeout,
                    "capacity refresh cancelled",
                ));
            }
            if !matches!(inner.state, ConnectionState::Connected { .. }) {
                return Ok(None);
            }
            inner.last_report = Some(report.clone());
            inner.last_capacity_warnings = capacity_warnings;
            let Some(status) = inner.last_status.clone() else {
                return Ok(None);
            };
            let Some(host_id) = inner.state.host_id().cloned() else {
                return Ok(None);
            };
            let revision = inner.revision.advance();
            let view = FleetView::from_status(
                host_id,
                &status,
                Some(&report),
                inner.latency.clone(),
                revision,
                Timestamp::now(),
            );
            inner.last_view = Some(view.clone());
            (revision, view)
        };
        self.emit(SpiritEvent::Fleet {
            revision,
            fleet: Box::new(view.clone()),
        });
        Ok(Some(view))
    }

    /// Stops polling and closes the connection. The last view is retained, dimmed.
    pub fn disconnect(&self) -> ConnectionSnapshot {
        let (revision, snapshot) = {
            let mut inner = self.guard();
            inner.cancel.cancel();
            inner.state = ConnectionState::Disconnected;
            let revision = inner.revision.advance();
            (revision, inner.state.snapshot())
        };
        self.emit(SpiritEvent::Connection {
            revision,
            connection: snapshot.clone(),
        });
        snapshot
    }

    /// Publishes an engine status change.
    pub fn publish_engine(&self, engine: EngineStatus) {
        let revision = self.guard().revision.advance();
        self.emit(SpiritEvent::Engine { revision, engine });
    }

    /// Publishes a gate snapshot.
    pub fn publish_gates(&self, gates: GateSnapshot) {
        let revision = self.guard().revision.advance();
        self.emit(SpiritEvent::Gates { revision, gates });
    }
}

/// The error returned when `disconnect` cancels an in-flight connect.
fn connect_cancelled() -> SpiritError {
    SpiritError::new(SpiritErrorCode::RemoteTimeout, "connect cancelled")
}
