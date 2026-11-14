//! Connection state machine, three-failure staleness, cancellation and revisions.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use spirit_core::connection::{
    ConnectionManager, ConnectionState, PollPolicy, Revision, SpiritEvent,
};
use spirit_core::error::{SpiritError, SpiritErrorCode};
use spirit_core::host::HostId;
use spirit_core::time::Timestamp;
use spirit_core::vowp::{MockVowpClient, VowpClient};
use spirit_core::wire::{WireWarning, WireWarningCode, parse_capacity_report, parse_fleet_status};
use tokio::sync::Notify;

const FRESH: &str = include_str!("../fixtures/vowp/fleet-status-fresh.json");
const CAPACITY: &str = include_str!("../fixtures/vowp/capacity-report-full.json");

fn mock() -> MockVowpClient {
    let status = parse_fleet_status(FRESH.as_bytes()).unwrap().value;
    let report = parse_capacity_report(CAPACITY.as_bytes()).unwrap().value;
    MockVowpClient::new(status, report)
}

fn host() -> HostId {
    HostId("01JCF5WQ8ZK3M2V7N4P6R8T0AB".to_owned())
}

/// A client whose `fleet_status` blocks until the future is dropped, so a test can
/// disconnect while a poll is genuinely in flight.
struct BlockingClient {
    inner: MockVowpClient,
    started: Arc<Notify>,
    release: Arc<Notify>,
    block: Arc<AtomicBool>,
}

#[async_trait]
impl VowpClient for BlockingClient {
    async fn probe(&self) -> Result<spirit_core::vowp::VowpProbe, SpiritError> {
        self.inner.probe().await
    }

    async fn fleet_status(&self) -> Result<spirit_core::wire::FleetStatus, SpiritError> {
        if self.block.load(Ordering::SeqCst) {
            self.started.notify_one();
            self.release.notified().await;
        }
        self.inner.fleet_status().await
    }

    async fn capacity_report(&self) -> Result<spirit_core::wire::CapacityReportDoc, SpiritError> {
        self.inner.capacity_report().await
    }

    async fn fleet_queue(&self) -> Result<Vec<spirit_core::wire::FleetQueueRow>, SpiritError> {
        self.inner.fleet_queue().await
    }

    async fn fleet_inbox(
        &self,
        all: bool,
    ) -> Result<Vec<spirit_core::wire::InboxItem>, SpiritError> {
        self.inner.fleet_inbox(all).await
    }

    async fn job_action(
        &self,
        project: &str,
        job: &str,
        action: spirit_core::wire::JobAction,
    ) -> Result<(), SpiritError> {
        self.inner.job_action(project, job, action).await
    }

    async fn answer_inbox(
        &self,
        project: Option<&str>,
        id: &str,
        answer: &str,
    ) -> Result<spirit_core::wire::InboxItem, SpiritError> {
        self.inner.answer_inbox(project, id, answer).await
    }

    async fn set_target(
        &self,
        target: u32,
        mode: spirit_core::wire::TargetMode,
        override_: bool,
        reason: &str,
    ) -> Result<spirit_core::wire::CapacityTarget, SpiritError> {
        self.inner.set_target(target, mode, override_, reason).await
    }
}

#[tokio::test]
async fn connect_probe_sequence_reports_the_failing_stage() {
    let manager = ConnectionManager::default();
    let client = mock().failing_probe(SpiritError::new(
        SpiritErrorCode::SshAuth,
        "SSH authentication failed",
    ));
    let error = manager.connect(&client, host()).await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::SshAuth);
    assert_eq!(
        manager.last_failed_stage(),
        Some(spirit_core::connection::ConnectStage::OpeningSsh)
    );
    assert_eq!(manager.status().state, "failed");

    let manager = ConnectionManager::default();
    let client = mock().failing_status(SpiritError::parse("fleet status", "missing home"));
    let error = manager.connect(&client, host()).await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Parse);
    assert_eq!(
        manager.last_failed_stage(),
        Some(spirit_core::connection::ConnectStage::ReadingSnapshot)
    );

    let manager = ConnectionManager::default();
    let client = mock();
    let status = manager.connect(&client, host()).await.unwrap();
    assert_eq!(status.state, "connected");
    assert!(manager.last_failed_stage().is_none());
    assert!(manager.fleet_view().is_some());
}

#[tokio::test]
async fn three_failures_mark_the_snapshot_stale_and_back_off() {
    let policy = PollPolicy {
        jitter_fraction: 0.0,
        ..PollPolicy::default()
    };
    let manager = ConnectionManager::new(policy);
    let client = mock();
    manager.connect(&client, host()).await.unwrap();
    let failing = client
        .clone()
        .failing_status(SpiritError::remote_timeout(20));
    for expected in 1..=3 {
        let error = manager.poll_once(&failing).await.unwrap_err();
        assert_eq!(error.code, SpiritErrorCode::RemoteTimeout);
        assert_eq!(manager.consecutive_failures(), expected);
    }
    assert!(matches!(manager.state(), ConnectionState::Stale { .. }));
    assert!(!manager.state().allows_mutations());
    assert!(manager.fleet_view().unwrap().stale);
    assert!(manager.next_poll_delay() >= Duration::from_secs(30));
}

#[tokio::test]
async fn disconnect_cancels_in_flight_polls() {
    let manager = ConnectionManager::default();
    let client = mock();
    manager.connect(&client, host()).await.unwrap();
    assert!(!manager.is_cancelled());
    let snapshot = manager.disconnect();
    assert_eq!(snapshot.state, "disconnected");
    assert!(manager.is_cancelled());
    assert!(matches!(manager.state(), ConnectionState::Disconnected));
    assert!(
        manager.fleet_view().is_some(),
        "the last view is retained for the dimmed cockpit"
    );
    let after = manager.poll_once(&client).await.unwrap_err();
    assert_eq!(after.code, SpiritErrorCode::Internal);
}

#[tokio::test]
async fn disconnect_aborts_a_poll_that_is_already_in_flight() {
    let manager = ConnectionManager::default();
    let client = Arc::new(BlockingClient {
        inner: mock(),
        started: Arc::new(Notify::new()),
        release: Arc::new(Notify::new()),
        block: Arc::new(AtomicBool::new(false)),
    });
    manager.connect(client.as_ref(), host()).await.unwrap();
    client.block.store(true, Ordering::SeqCst);
    let mut receiver = manager.subscribe();
    let poll = tokio::spawn({
        let manager = manager.clone();
        let client = client.clone();
        async move { manager.poll_once(client.as_ref()).await }
    });
    client.started.notified().await;
    let snapshot = manager.disconnect();
    assert_eq!(snapshot.state, "disconnected");
    let result = tokio::time::timeout(Duration::from_secs(2), poll)
        .await
        .expect("an in-flight poll must stop when the connection is cancelled")
        .unwrap();
    assert!(result.is_err());
    assert!(matches!(manager.state(), ConnectionState::Disconnected));
    assert!(manager.fleet_view().is_some());
    while let Ok(event) = receiver.try_recv() {
        assert!(
            !matches!(event, SpiritEvent::Fleet { .. }),
            "a poll completing after disconnect must not publish a fleet event"
        );
    }
}

#[tokio::test]
async fn parse_warnings_reach_the_manager_for_the_raw_document_drawer() {
    let status = parse_fleet_status(FRESH.as_bytes()).unwrap().value;
    let report = parse_capacity_report(CAPACITY.as_bytes()).unwrap().value;
    let warning = WireWarning {
        path: "snapshot.limits".to_owned(),
        code: WireWarningCode::UnknownEnum,
        detail: "unknown value \"quota\"; shown as Unknown".to_owned(),
    };
    let client = MockVowpClient::new(status, report).with_warnings(vec![warning.clone()]);
    let manager = ConnectionManager::default();
    manager.connect(&client, host()).await.unwrap();
    assert_eq!(manager.last_status_warnings(), vec![warning.clone()]);
    assert!(
        client.take_warnings().is_empty(),
        "the manager drains warnings instead of leaving them to debug logging"
    );
}

#[tokio::test]
async fn revision_increases_monotonically_and_old_events_are_dropped() {
    let mut revision = Revision::new();
    assert_eq!(revision.current(), 0);
    assert_eq!(revision.advance(), 1);
    assert_eq!(revision.advance(), 2);
    assert!(revision.accepts(3));
    assert!(!revision.accepts(2));
    assert!(!revision.accepts(1));
    assert!(spirit_core::connection::accepts_revision(4, 5));
    assert!(!spirit_core::connection::accepts_revision(5, 4));

    let manager = ConnectionManager::default();
    let mut receiver = manager.subscribe();
    let client = mock();
    manager.connect(&client, host()).await.unwrap();
    manager.poll_once(&client).await.unwrap();
    let mut revisions = Vec::new();
    while let Ok(event) = receiver.try_recv() {
        revisions.push(event.revision());
        match event {
            SpiritEvent::Fleet { fleet, .. } => assert!(fleet.revision > 0),
            SpiritEvent::Connection { connection, .. } => {
                assert!(!connection.state.is_empty());
            }
            _ => {}
        }
    }
    assert!(!revisions.is_empty());
    assert!(revisions.windows(2).all(|pair| pair[0] <= pair[1]));
    assert!(revisions.last().copied().unwrap() >= 2);
}

#[tokio::test]
async fn stale_view_disables_mutations() {
    let policy = PollPolicy {
        jitter_fraction: 0.0,
        ..PollPolicy::default()
    };
    let manager = ConnectionManager::new(policy);
    let client = mock();
    manager.connect(&client, host()).await.unwrap();
    let failing = client
        .clone()
        .failing_status(SpiritError::remote_timeout(20));
    for _ in 0..3 {
        let _ = manager.poll_once(&failing).await;
    }
    assert!(!manager.state().allows_mutations());
    let view = manager.fleet_view().unwrap();
    assert!(view.stale);
    assert!(view.mutations_disabled());
}

#[tokio::test]
async fn capacity_refresh_updates_view_and_keeps_the_report() {
    let manager = ConnectionManager::default();
    let client = mock();
    manager.connect(&client, host()).await.unwrap();
    let view = manager.refresh_capacity(&client).await.unwrap().unwrap();
    assert!(view.capacity.report_loaded);
    assert_eq!(
        view.capacity.provider_caps.get("deepseek").copied(),
        Some(8)
    );
    assert!(manager.last_capacity_report().is_some());
}

#[test]
fn latency_stats_are_deterministic() {
    let mut stats = spirit_core::connection::LatencyStats::default();
    assert_eq!(stats.samples, 0);
    stats.record(40);
    stats.record(60);
    assert_eq!(stats.samples, 2);
    assert_eq!(stats.last_ms, 60);
    assert!(stats.ewma_ms > 40.0 && stats.ewma_ms < 60.0);
    assert_eq!(stats.p50_ms, 40);
    assert_eq!(stats.p95_ms, 60);
}

#[test]
fn connect_stages_have_ui_labels() {
    for stage in [
        spirit_core::connection::ConnectStage::ResolvingHostKey,
        spirit_core::connection::ConnectStage::OpeningSsh,
        spirit_core::connection::ConnectStage::CheckingVowp,
        spirit_core::connection::ConnectStage::ReadingSnapshot,
    ] {
        assert!(!stage.label().is_empty());
    }
    assert!(Timestamp::now().as_rfc3339().ends_with('Z'));
}
