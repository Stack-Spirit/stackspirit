//! Compile-time golden fixture loader, enabled only by the `fixtures` feature.
//!
//! Fixtures are complete documents copied from `docs/VOWP-CONTRACT.md`; they contain
//! documentation addresses and fake values only. This module must never be enabled in a
//! release build.

use serde::{Deserialize, Serialize};

use crate::engine::gate::GateSnapshot;
use crate::engine::{CatalogEntry, ExternalHarness, ModelCatalog};
use crate::error::SpiritError;
use crate::vowp::{MockVowpClient, VowpClient, VowpProbe};
use crate::wire::{
    CapacityReportDoc, FleetQueueRow, FleetStatus, InboxItem, parse_capacity_report,
    parse_fleet_inbox, parse_fleet_queue, parse_fleet_status,
};

/// Every documented fixture scenario from `docs/PLAN.md` §10.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scenario {
    /// Three of eight worker slots busy, no brake.
    FreshSingle,
    /// Supervisor up, no running jobs, target 4.
    Idle,
    /// Eight running, twelve queued, three blocked, one provider limit.
    Busy,
    /// Disk below the reserve floor with a disk brake.
    StorageLow,
    /// Memory below the reserve with a memory-pressure brake.
    MemoryPressure,
    /// A provider quota limit.
    ProviderLimit,
    /// Supervisor stopped and an old last tick.
    SupervisorDown,
    /// Cached, stale snapshot.
    StaleCache,
    /// Three projects for filtering.
    MultiProject,
    /// A 200-row queue at the VOWP cap.
    QueueCap,
    /// Unknown state, limit kind and executor harness.
    UnknownStates,
    /// No advised worker target.
    NoAdvice,
    /// Snapshot and report disagree on free disk; the report wins.
    DiskDisagreement,
    /// Included models available.
    EngineIncluded,
    /// No included models; BYOK empty state.
    EngineNoIncluded,
    /// An external opencode installation detected.
    EngineExternal,
    /// Local-orchestrator gate waiting with exact capabilities.
    GatePhase3Waiting,
    /// Synthetic local-orchestrator ready evidence.
    GatePhase3Ready,
    /// ASIF gate waiting for `v4-asif-*`.
    GateAsifWaiting,
    /// Synthetic ASIF export/import evidence.
    GateAsifReady,
}

impl Scenario {
    /// Returns the fixture name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FreshSingle => "fresh-single",
            Self::Idle => "idle",
            Self::Busy => "busy",
            Self::StorageLow => "storage-low",
            Self::MemoryPressure => "memory-pressure",
            Self::ProviderLimit => "provider-limit",
            Self::SupervisorDown => "supervisor-down",
            Self::StaleCache => "stale-cache",
            Self::MultiProject => "multi-project",
            Self::QueueCap => "queue-cap",
            Self::UnknownStates => "unknown-states",
            Self::NoAdvice => "no-advice",
            Self::DiskDisagreement => "disk-disagreement",
            Self::EngineIncluded => "engine-included",
            Self::EngineNoIncluded => "engine-no-included",
            Self::EngineExternal => "engine-external",
            Self::GatePhase3Waiting => "gate-phase3-waiting",
            Self::GatePhase3Ready => "gate-phase3-ready",
            Self::GateAsifWaiting => "gate-asif-waiting",
            Self::GateAsifReady => "gate-asif-ready",
        }
    }

    /// Parses a scenario name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|item| item.as_str() == name)
    }

    /// Every scenario.
    pub const ALL: [Self; 20] = [
        Self::FreshSingle,
        Self::Idle,
        Self::Busy,
        Self::StorageLow,
        Self::MemoryPressure,
        Self::ProviderLimit,
        Self::SupervisorDown,
        Self::StaleCache,
        Self::MultiProject,
        Self::QueueCap,
        Self::UnknownStates,
        Self::NoAdvice,
        Self::DiskDisagreement,
        Self::EngineIncluded,
        Self::EngineNoIncluded,
        Self::EngineExternal,
        Self::GatePhase3Waiting,
        Self::GatePhase3Ready,
        Self::GateAsifWaiting,
        Self::GateAsifReady,
    ];

    fn fleet_status_json(self) -> Option<&'static str> {
        Some(match self {
            Self::FreshSingle | Self::QueueCap => FLEET_FRESH,
            Self::Idle => FLEET_IDLE,
            Self::Busy => FLEET_BUSY,
            Self::StorageLow => FLEET_STORAGE_LOW,
            Self::MemoryPressure => FLEET_MEMORY_PRESSURE,
            Self::ProviderLimit => FLEET_PROVIDER_LIMIT,
            Self::SupervisorDown => FLEET_SUPERVISOR_DOWN,
            Self::StaleCache => FLEET_STALE_CACHE,
            Self::MultiProject => FLEET_MULTI_PROJECT,
            Self::UnknownStates => FLEET_UNKNOWN_STATES,
            Self::NoAdvice => FLEET_NO_ADVICE,
            Self::DiskDisagreement => FLEET_DISK_DISAGREEMENT,
            Self::EngineIncluded
            | Self::EngineNoIncluded
            | Self::EngineExternal
            | Self::GatePhase3Waiting
            | Self::GatePhase3Ready
            | Self::GateAsifWaiting
            | Self::GateAsifReady => return None,
        })
    }

    fn capacity_json(self) -> Option<&'static str> {
        Some(match self {
            Self::FreshSingle
            | Self::Busy
            | Self::MultiProject
            | Self::QueueCap
            | Self::UnknownStates
            | Self::StaleCache => CAPACITY_FULL,
            Self::Idle
            | Self::SupervisorDown
            | Self::NoAdvice
            | Self::EngineIncluded
            | Self::EngineNoIncluded
            | Self::EngineExternal => CAPACITY_EMPTY,
            Self::StorageLow => CAPACITY_STORAGE_LOW,
            Self::MemoryPressure => CAPACITY_MEMORY_PRESSURE,
            Self::ProviderLimit => CAPACITY_PROVIDER_LIMIT,
            Self::DiskDisagreement => CAPACITY_DISK_DISAGREEMENT,
            Self::GatePhase3Waiting | Self::GatePhase3Ready => CAPACITY_FULL,
            Self::GateAsifWaiting | Self::GateAsifReady => CAPACITY_EMPTY,
        })
    }

    fn queue_json(self) -> Option<&'static str> {
        match self {
            Self::QueueCap => Some(QUEUE_CAP),
            Self::FreshSingle
            | Self::Idle
            | Self::Busy
            | Self::StorageLow
            | Self::MemoryPressure
            | Self::ProviderLimit
            | Self::MultiProject
            | Self::UnknownStates
            | Self::NoAdvice
            | Self::DiskDisagreement
            | Self::StaleCache
            | Self::SupervisorDown => Some(QUEUE),
            Self::EngineIncluded
            | Self::EngineNoIncluded
            | Self::EngineExternal
            | Self::GatePhase3Waiting
            | Self::GatePhase3Ready
            | Self::GateAsifWaiting
            | Self::GateAsifReady => None,
        }
    }

    fn gate_json(self) -> Option<&'static str> {
        Some(match self {
            Self::GatePhase3Waiting => GATE_PHASE3_WAITING,
            Self::GatePhase3Ready => GATE_PHASE3_READY,
            Self::GateAsifWaiting => GATE_ASIF_WAITING,
            Self::GateAsifReady => GATE_ASIF_READY,
            _ => return None,
        })
    }
}

macro_rules! fixture {
    ($name:ident, $file:literal) => {
        const $name: &str = include_str!(concat!("../fixtures/vowp/", $file));
    };
}

fixture!(FLEET_FRESH, "fleet-status-fresh.json");
fixture!(FLEET_STALE_CACHE, "fleet-status-stale-cache.json");
fixture!(FLEET_SUPERVISOR_DOWN, "fleet-status-supervisor-down.json");
fixture!(FLEET_IDLE, "idle.json");
fixture!(FLEET_BUSY, "busy.json");
fixture!(FLEET_STORAGE_LOW, "storage-low.json");
fixture!(FLEET_MEMORY_PRESSURE, "memory-pressure.json");
fixture!(FLEET_PROVIDER_LIMIT, "provider-limit.json");
fixture!(FLEET_MULTI_PROJECT, "snapshot-multi-project.json");
fixture!(FLEET_UNKNOWN_STATES, "snapshot-unknown-enums.json");
fixture!(FLEET_NO_ADVICE, "no-advice.json");
fixture!(FLEET_DISK_DISAGREEMENT, "disk-disagreement.json");
fixture!(CAPACITY_FULL, "capacity-report-full.json");
fixture!(CAPACITY_EMPTY, "capacity-report-empty.json");
fixture!(CAPACITY_STORAGE_LOW, "capacity-report-storage-low.json");
fixture!(
    CAPACITY_MEMORY_PRESSURE,
    "capacity-report-memory-pressure.json"
);
fixture!(
    CAPACITY_PROVIDER_LIMIT,
    "capacity-report-provider-limit.json"
);
fixture!(
    CAPACITY_DISK_DISAGREEMENT,
    "capacity-report-disk-disagreement.json"
);
fixture!(QUEUE, "fleet-queue.json");
fixture!(QUEUE_CAP, "fleet-queue-cap.json");
fixture!(INBOX, "fleet-inbox.json");
fixture!(GATE_PHASE3_WAITING, "gate-phase3-waiting.json");
fixture!(GATE_PHASE3_READY, "gate-phase3-ready.json");
fixture!(GATE_ASIF_WAITING, "gate-asif-waiting.json");
fixture!(GATE_ASIF_READY, "gate-asif-ready.json");
fixture!(ENGINE_INCLUDED, "engine-included.json");
fixture!(ENGINE_NO_INCLUDED, "engine-no-included.json");
fixture!(ENGINE_EXTERNAL, "engine-external.json");

/// Loads and parses the scenario's `fleet status` document.
pub fn fleet_status(scenario: Scenario) -> Result<FleetStatus, SpiritError> {
    let json = scenario.fleet_status_json().ok_or_else(|| {
        SpiritError::new(
            crate::error::SpiritErrorCode::NotFound,
            format!("scenario {} has no fleet status fixture", scenario.as_str()),
        )
    })?;
    Ok(parse_fleet_status(json.as_bytes())?.value)
}

/// Loads and parses the scenario's capacity report.
pub fn capacity_report(scenario: Scenario) -> Result<CapacityReportDoc, SpiritError> {
    let json = scenario.capacity_json().ok_or_else(|| {
        SpiritError::new(
            crate::error::SpiritErrorCode::NotFound,
            format!("scenario {} has no capacity fixture", scenario.as_str()),
        )
    })?;
    Ok(parse_capacity_report(json.as_bytes())?.value)
}

/// Loads the scenario's queue rows.
pub fn fleet_queue(scenario: Scenario) -> Result<Vec<FleetQueueRow>, SpiritError> {
    let Some(json) = scenario.queue_json() else {
        return Ok(Vec::new());
    };
    Ok(parse_fleet_queue(json.as_bytes())?.value)
}

/// Loads the full inbox rows.
pub fn fleet_inbox() -> Result<Vec<InboxItem>, SpiritError> {
    Ok(parse_fleet_inbox(INBOX.as_bytes())?.value)
}

/// Loads a gate snapshot fixture.
pub fn gate_snapshot(scenario: Scenario) -> Result<GateSnapshot, SpiritError> {
    let json = scenario.gate_json().ok_or_else(|| {
        SpiritError::new(
            crate::error::SpiritErrorCode::NotFound,
            format!("scenario {} has no gate fixture", scenario.as_str()),
        )
    })?;
    let mut snapshot: GateSnapshot =
        serde_json::from_str(json).map_err(|err| SpiritError::parse("gate snapshot", err))?;
    // The D6 flag is derived, never copied from a fixture's stale value. A Ready gate
    // with a connected local agent enables local actions; every other state keeps them
    // off.
    snapshot.enforce_local_orchestrator_flag();
    Ok(snapshot)
}

/// The engine catalog fixture shape.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EngineFixture {
    /// Build-time catalog entries.
    pub models: Vec<CatalogEntry>,
    /// Model ids the runtime reports as available.
    pub available: Vec<String>,
}

/// Loads an engine catalog scenario.
pub fn engine_catalog(scenario: Scenario) -> Result<(ModelCatalog, Vec<String>), SpiritError> {
    let json = match scenario {
        Scenario::EngineIncluded => ENGINE_INCLUDED,
        Scenario::EngineNoIncluded => ENGINE_NO_INCLUDED,
        _ => {
            return Err(SpiritError::new(
                crate::error::SpiritErrorCode::NotFound,
                format!("scenario {} has no engine catalog", scenario.as_str()),
            ));
        }
    };
    let fixture: EngineFixture =
        serde_json::from_str(json).map_err(|err| SpiritError::parse("engine catalog", err))?;
    Ok((ModelCatalog::new(fixture.models), fixture.available))
}

/// The external-harness fixture shape.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExternalFixture {
    /// Detected harnesses.
    pub harnesses: Vec<ExternalHarness>,
}

/// Loads the external-harness fixture.
pub fn external_harnesses() -> Result<Vec<ExternalHarness>, SpiritError> {
    let fixture: ExternalFixture = serde_json::from_str(ENGINE_EXTERNAL)
        .map_err(|err| SpiritError::parse("external harnesses", err))?;
    Ok(fixture.harnesses)
}

/// A `VowpClient` over the golden fixtures. Mutations succeed and return the fixture
/// target; no network or filesystem is touched.
#[derive(Debug, Clone)]
pub struct FixtureVowpClient {
    scenario: Scenario,
    inner: MockVowpClient,
}

impl FixtureVowpClient {
    /// Builds a client for a scenario.
    pub fn new(scenario: Scenario) -> Result<Self, SpiritError> {
        let status = fleet_status(scenario)?;
        let capacity = capacity_report(scenario)?;
        let queue = fleet_queue(scenario)?;
        let inbox = fleet_inbox()?;
        let inner = MockVowpClient::new(status, capacity)
            .with_probe(VowpProbe {
                version: "0.4.0-fixture".to_owned(),
                latency_ms: 42,
                protocol_compatible: true,
            })
            .with_queue(queue)
            .with_inbox(inbox);
        Ok(Self { scenario, inner })
    }

    /// Returns the scenario.
    #[must_use]
    pub const fn scenario(&self) -> Scenario {
        self.scenario
    }
}

#[async_trait::async_trait]
impl VowpClient for FixtureVowpClient {
    async fn probe(&self) -> Result<VowpProbe, SpiritError> {
        self.inner.probe().await
    }

    async fn fleet_status(&self) -> Result<FleetStatus, SpiritError> {
        self.inner.fleet_status().await
    }

    async fn capacity_report(&self) -> Result<CapacityReportDoc, SpiritError> {
        self.inner.capacity_report().await
    }

    async fn fleet_queue(&self) -> Result<Vec<FleetQueueRow>, SpiritError> {
        self.inner.fleet_queue().await
    }

    async fn fleet_inbox(&self, all: bool) -> Result<Vec<InboxItem>, SpiritError> {
        self.inner.fleet_inbox(all).await
    }

    async fn job_action(
        &self,
        project: &str,
        job: &str,
        action: crate::wire::JobAction,
        reason: Option<&str>,
    ) -> Result<(), SpiritError> {
        self.inner.job_action(project, job, action, reason).await
    }

    async fn answer_inbox(
        &self,
        project: Option<&str>,
        id: &str,
        answer: &str,
    ) -> Result<InboxItem, SpiritError> {
        self.inner.answer_inbox(project, id, answer).await
    }

    async fn dismiss_inbox(
        &self,
        project: Option<&str>,
        id: &str,
    ) -> Result<InboxItem, SpiritError> {
        self.inner.dismiss_inbox(project, id).await
    }

    async fn set_target(
        &self,
        target: u32,
        mode: crate::wire::TargetMode,
        override_: bool,
        reason: &str,
    ) -> Result<crate::wire::CapacityTarget, SpiritError> {
        self.inner.set_target(target, mode, override_, reason).await
    }
}
