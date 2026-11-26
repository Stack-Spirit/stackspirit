//! Local, dependency-free mirror of the VOWP JSON documents.
//!
//! The module performs no I/O and does not import `/home/user/vowp`. Unknown object
//! fields are ignored; unknown enum values are preserved as `Unknown`. Every parse
//! returns a [`Parsed`] value with a [`WireWarning`] list that the UI may show in its
//! raw-document drawer.

pub mod asif;
pub mod capacity;
pub mod enums;
pub mod error_doc;
pub mod executor;
pub mod ids;
pub mod inbox;
pub mod limit;
pub mod local;
pub mod ops;
pub mod session;
pub mod snapshot;
pub mod surface;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use asif::{
    ASIF_ARCHIVE_CAP_BYTES, ASIF_FORMAT, ASIF_JSON_CAP_BYTES, AsifCounts, AsifExportSummary,
    AsifImportResponse, AsifLoss, AsifReport, AsifSessionRef, parse_asif_export_summary,
    parse_asif_import_response,
};
pub use capacity::{
    BrakeReason, BrakeState, CapacityReport, CapacityReportDoc, CapacitySnapshot, CapacityTarget,
    HostSample, RunningCapacity, UsageEstimate, WaitReason, WaitingJob,
};
pub use enums::*;
pub use error_doc::{ErrorBody, ErrorDocument};
pub use executor::ExecutorRef;
pub use ids::{CapsuleId, EventId, InboxId, JobId, NodeName, ProjectName, RunId, SessionId};
pub use inbox::InboxItem;
pub use limit::Limit;
pub use local::{
    AGENT_PROTOCOL, AgentPairOutput, AgentPathAddRequest, AgentPathMap, AgentStatusDocument,
    AgentSurfaceStatus, MAX_PATH_REF_LEN, NodeCard, OrchestratorNewRequest, PathRef,
    SURFACE_ID_LEN, SurfaceEntry, SurfacePairOutput, SurfacePairRequest, SurfaceRevokeResult,
    SurfaceRotateRequest, SurfaceRotateResult, ensure_duration_arg, ensure_surface_id,
};
pub use ops::{FleetQueueRow, JobAction, SetTargetInput};
pub use session::{SessionOwner, SessionSurfaceCard, SurfaceId, SurfaceRef};
pub use snapshot::{
    BlockedJobCard, FleetStatus, GateFailureCard, InboxSummary, MergeCard, ProjectSnapshot,
    QueueJobCard, RunningJobCard, SNAPSHOT_FORMAT, SNAPSHOT_VERSION, Snapshot, SupervisorSnapshot,
};
pub use surface::{JoinInstructions, OrchestratorCard};

use crate::error::SpiritError;

/// The document name used in parse errors.
pub type DocumentName = &'static str;

/// The class of a non-fatal wire warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireWarningCode {
    /// A known enum field carried a value this build does not know.
    UnknownEnum,
    /// A documented optional field was absent and defaulted.
    MissingOptional,
    /// VOWP capped a list; the view must label the truncation.
    Truncated,
    /// The document used a schema this build may not understand fully.
    Schema,
}

/// A non-fatal parse warning. Warnings are never silently discarded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireWarning {
    /// Dotted path inside the document.
    pub path: String,
    /// Warning class.
    pub code: WireWarningCode,
    /// Human-readable detail, safe to show.
    pub detail: String,
}

impl WireWarning {
    fn unknown_enum(path: impl Into<String>, value: &str) -> Self {
        Self {
            path: path.into(),
            code: WireWarningCode::UnknownEnum,
            detail: format!("unknown value {value:?}; shown as Unknown"),
        }
    }

    fn missing_optional(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            code: WireWarningCode::MissingOptional,
            detail: "optional field absent; using the documented default".to_owned(),
        }
    }
}

/// A parsed document plus its non-fatal warnings.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed<T> {
    /// The parsed document.
    pub value: T,
    /// Non-fatal warnings discovered while parsing.
    pub warnings: Vec<WireWarning>,
}

impl<T> Parsed<T> {
    /// Returns true when the document produced no warnings.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.warnings.is_empty()
    }
}

fn parse_json(bytes: &[u8], document: DocumentName) -> Result<Value, SpiritError> {
    serde_json::from_slice(bytes).map_err(|err| SpiritError::parse(document, err))
}

fn from_value<T: DeserializeOwned>(value: Value, document: DocumentName) -> Result<T, SpiritError> {
    serde_json::from_value(value).map_err(|err| SpiritError::parse(document, err))
}

fn missing(raw: &Value, path: &str) -> bool {
    let mut cursor = raw;
    for segment in path.split('.') {
        let Some(next) = cursor.get(segment) else {
            return true;
        };
        cursor = next;
    }
    false
}

fn push_missing_optional(raw: &Value, paths: &[&str], warnings: &mut Vec<WireWarning>) {
    for path in paths {
        if missing(raw, path) {
            warnings.push(WireWarning::missing_optional(*path));
        }
    }
}

/// Checks the schema/version markers on a raw snapshot object *before* full
/// deserialization, so a changed marker always wins over a missing required field
/// (`VOWP-CONTRACT.md` §12.3). Absent markers are left to serde, which reports the
/// field as missing.
fn ensure_raw_compatible(value: &Value) -> Result<(), SpiritError> {
    if let Some(schema) = value.get("schema").and_then(Value::as_str) {
        if schema != SNAPSHOT_FORMAT {
            return Err(SpiritError::vowp_incompatible(
                "snapshot.schema",
                format!("expected {SNAPSHOT_FORMAT}, got {schema:?}"),
            ));
        }
    }
    if let Some(version) = value.get("version").and_then(Value::as_u64) {
        if version != u64::from(SNAPSHOT_VERSION) {
            return Err(SpiritError::vowp_incompatible(
                "snapshot.version",
                format!("expected {SNAPSHOT_VERSION}, got {version}"),
            ));
        }
    }
    Ok(())
}

/// Parses `vowp --json fleet status` and enforces the schema guard.
pub fn parse_fleet_status(bytes: &[u8]) -> Result<Parsed<FleetStatus>, SpiritError> {
    parse_fleet_status_named(bytes, "fleet status")
}

/// Parses a `fleet status` document with an explicit document name for diagnostics.
pub fn parse_fleet_status_named(
    bytes: &[u8],
    document: DocumentName,
) -> Result<Parsed<FleetStatus>, SpiritError> {
    let raw = parse_json(bytes, document)?;
    if let Some(snapshot) = raw.get("snapshot") {
        ensure_raw_compatible(snapshot)?;
    }
    let mut warnings = Vec::new();
    push_missing_optional(
        &raw,
        &[
            "snapshot.capacity.measured",
            "snapshot.capacity.advised_target",
            "snapshot.supervisor.pid",
            "snapshot.supervisor.last_tick",
        ],
        &mut warnings,
    );
    let status: FleetStatus = from_value(raw, document)?;
    status.snapshot.ensure_compatible()?;
    collect_fleet_warnings(&status, &mut warnings);
    Ok(Parsed {
        value: status,
        warnings,
    })
}

/// Parses a bare `snapshot.json` document and enforces the schema guard.
pub fn parse_snapshot(bytes: &[u8]) -> Result<Parsed<Snapshot>, SpiritError> {
    let raw = parse_json(bytes, "snapshot")?;
    ensure_raw_compatible(&raw)?;
    let mut warnings = Vec::new();
    push_missing_optional(
        &raw,
        &[
            "capacity.measured",
            "capacity.advised_target",
            "supervisor.pid",
            "supervisor.last_tick",
        ],
        &mut warnings,
    );
    let snapshot: Snapshot = from_value(raw, "snapshot")?;
    snapshot.ensure_compatible()?;
    collect_snapshot_warnings(&snapshot, &mut warnings);
    Ok(Parsed {
        value: snapshot,
        warnings,
    })
}

/// Parses `vowp --json capacity report`, including the CLI-only extras.
pub fn parse_capacity_report(bytes: &[u8]) -> Result<Parsed<CapacityReportDoc>, SpiritError> {
    let document = "capacity report";
    let raw = parse_json(bytes, document)?;
    let mut warnings = Vec::new();
    push_missing_optional(
        &raw,
        &[
            "host",
            "provider_caps",
            "reload_pending",
            "target.advised_workers",
        ],
        &mut warnings,
    );
    let report: CapacityReportDoc = from_value(raw, document)?;
    collect_capacity_warnings(&report, &mut warnings);
    Ok(Parsed {
        value: report,
        warnings,
    })
}

/// Parses `vowp --json fleet queue`, a bare array.
pub fn parse_fleet_queue(bytes: &[u8]) -> Result<Parsed<Vec<FleetQueueRow>>, SpiritError> {
    let document = "fleet queue";
    let raw = parse_json(bytes, document)?;
    let rows: Vec<FleetQueueRow> = from_value(raw, document)?;
    let mut warnings = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if !row.state.is_known() {
            warnings.push(WireWarning::unknown_enum(
                format!("[{index}].state"),
                row.state.as_str(),
            ));
        }
        if !row.kind.is_known() {
            warnings.push(WireWarning::unknown_enum(
                format!("[{index}].kind"),
                row.kind.as_str(),
            ));
        }
        if !row.role.is_known() {
            warnings.push(WireWarning::unknown_enum(
                format!("[{index}].role"),
                row.role.as_str(),
            ));
        }
    }
    Ok(Parsed {
        value: rows,
        warnings,
    })
}

/// Parses `vowp --json fleet inbox --all`, a bare array.
pub fn parse_fleet_inbox(bytes: &[u8]) -> Result<Parsed<Vec<InboxItem>>, SpiritError> {
    let document = "fleet inbox";
    let raw = parse_json(bytes, document)?;
    let items: Vec<InboxItem> = from_value(raw, document)?;
    let mut warnings = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if !item.kind.is_known() {
            warnings.push(WireWarning::unknown_enum(
                format!("[{index}].kind"),
                item.kind.as_str(),
            ));
        }
        if !item.state.is_known() {
            warnings.push(WireWarning::unknown_enum(
                format!("[{index}].state"),
                item.state.as_str(),
            ));
        }
    }
    Ok(Parsed {
        value: items,
        warnings,
    })
}

/// Parses a VOWP `--json` error document.
pub fn parse_error_document(bytes: &[u8]) -> Result<ErrorDocument, SpiritError> {
    let raw = parse_json(bytes, "error document")?;
    from_value(raw, "error document")
}

fn collect_fleet_warnings(status: &FleetStatus, warnings: &mut Vec<WireWarning>) {
    if !status.source.is_known() {
        warnings.push(WireWarning::unknown_enum("source", status.source.as_str()));
    }
    collect_snapshot_warnings(&status.snapshot, warnings);
}

fn collect_snapshot_warnings(snapshot: &Snapshot, warnings: &mut Vec<WireWarning>) {
    if !snapshot.capacity.target.mode.is_known() {
        warnings.push(WireWarning::unknown_enum(
            "capacity.target.mode",
            snapshot.capacity.target.mode.as_str(),
        ));
    }
    collect_capacity_sample_warnings("capacity", &snapshot.capacity, warnings);
    for (index, project) in snapshot.projects.iter().enumerate() {
        let base = format!("projects[{index}]");
        if !project.state.is_known() {
            warnings.push(WireWarning::unknown_enum(
                format!("{base}.state"),
                project.state.as_str(),
            ));
        }
        for (job, count) in &project.jobs_by_state {
            if !JobState::parse(job).is_known() {
                warnings.push(WireWarning {
                    path: format!("{base}.jobs_by_state.{job}"),
                    code: WireWarningCode::UnknownEnum,
                    detail: format!("unknown job state {job:?}; count {count} is kept"),
                });
            }
        }
        for (card_index, card) in project.running.iter().enumerate() {
            let card_base = format!("{base}.running[{card_index}]");
            for (field, known, value) in [
                ("kind", card.kind.is_known(), card.kind.as_str()),
                ("role", card.role.is_known(), card.role.as_str()),
            ] {
                if !known {
                    warnings.push(WireWarning::unknown_enum(
                        format!("{card_base}.{field}"),
                        value,
                    ));
                }
            }
            if let Some(raw) = &card.executor {
                let parsed = ExecutorRef::parse(raw);
                if !parsed.is_known() {
                    warnings.push(WireWarning::unknown_enum(
                        format!("{card_base}.executor"),
                        raw,
                    ));
                }
            }
        }
        for (card_index, card) in project.blocked.iter().enumerate() {
            if !card.state.is_known() {
                warnings.push(WireWarning::unknown_enum(
                    format!("{base}.blocked[{card_index}].state"),
                    card.state.as_str(),
                ));
            }
        }
        for (card_index, card) in project.queue.iter().enumerate() {
            let card_base = format!("{base}.queue[{card_index}]");
            if !card.state.is_known() {
                warnings.push(WireWarning::unknown_enum(
                    format!("{card_base}.state"),
                    card.state.as_str(),
                ));
            }
            if !card.kind.is_known() {
                warnings.push(WireWarning::unknown_enum(
                    format!("{card_base}.kind"),
                    card.kind.as_str(),
                ));
            }
            if !card.role.is_known() {
                warnings.push(WireWarning::unknown_enum(
                    format!("{card_base}.role"),
                    card.role.as_str(),
                ));
            }
        }
    }
    for (index, limit) in snapshot.limits.iter().enumerate() {
        if !limit.kind.is_known() {
            warnings.push(WireWarning::unknown_enum(
                format!("limits[{index}].kind"),
                limit.kind.as_str(),
            ));
        }
        if let Some(harness) = &limit.harness {
            if !harness.is_known() {
                warnings.push(WireWarning::unknown_enum(
                    format!("limits[{index}].harness"),
                    harness.as_str(),
                ));
            }
        }
    }
    for (index, item) in snapshot.inbox.iter().enumerate() {
        if !item.kind.is_known() {
            warnings.push(WireWarning::unknown_enum(
                format!("inbox[{index}].kind"),
                item.kind.as_str(),
            ));
        }
    }
    for (index, session) in snapshot.sessions.iter().enumerate() {
        let base = format!("sessions[{index}]");
        if !session.state.is_known() {
            warnings.push(WireWarning::unknown_enum(
                format!("{base}.state"),
                session.state.as_str(),
            ));
        }
        if let Some(role) = &session.role {
            if !role.is_known() {
                warnings.push(WireWarning::unknown_enum(
                    format!("{base}.role"),
                    role.as_str(),
                ));
            }
        }
        if let Some(surface) = &session.active_surface {
            if !surface.kind.is_known() {
                warnings.push(WireWarning::unknown_enum(
                    format!("{base}.active_surface.kind"),
                    surface.kind.as_str(),
                ));
            }
        }
    }
    for (index, orchestrator) in snapshot.orchestrators.iter().enumerate() {
        if !orchestrator.app.is_known() {
            warnings.push(WireWarning::unknown_enum(
                format!("orchestrators[{index}].app"),
                orchestrator.app.as_str(),
            ));
        }
    }
}

fn collect_capacity_sample_warnings(
    base: &str,
    capacity: &CapacitySnapshot,
    warnings: &mut Vec<WireWarning>,
) {
    if let Some(host) = &capacity.measured {
        if !host.source.is_known() {
            warnings.push(WireWarning::unknown_enum(
                format!("{base}.measured.source"),
                host.source.as_str(),
            ));
        }
    }
    if !capacity.brake.active {
        return;
    }
    for (index, reason) in capacity.brake.reasons.iter().enumerate() {
        if !reason.code.is_known() {
            warnings.push(WireWarning::unknown_enum(
                format!("{base}.brake.reasons[{index}].code"),
                reason.code.as_str(),
            ));
        }
    }
}

fn collect_capacity_warnings(report: &CapacityReportDoc, warnings: &mut Vec<WireWarning>) {
    let doc = &report.report;
    if let Some(host) = &doc.host {
        if !host.source.is_known() {
            warnings.push(WireWarning::unknown_enum(
                "host.source",
                host.source.as_str(),
            ));
        }
    }
    if !doc.target.mode.is_known() {
        warnings.push(WireWarning::unknown_enum(
            "target.mode",
            doc.target.mode.as_str(),
        ));
    }
    for (index, reason) in doc.brakes.reasons.iter().enumerate() {
        if !reason.code.is_known() {
            warnings.push(WireWarning::unknown_enum(
                format!("brakes.reasons[{index}].code"),
                reason.code.as_str(),
            ));
        }
    }
    for (index, estimate) in doc.estimates.iter().enumerate() {
        let base = format!("estimates[{index}]");
        if !estimate.source.is_known() {
            warnings.push(WireWarning::unknown_enum(
                format!("{base}.source"),
                estimate.source.as_str(),
            ));
        }
        if let Some(kind) = &estimate.job_kind {
            if !kind.is_known() {
                warnings.push(WireWarning::unknown_enum(
                    format!("{base}.job_kind"),
                    kind.as_str(),
                ));
            }
        }
        if let Some(harness) = &estimate.harness {
            if !harness.is_known() {
                warnings.push(WireWarning::unknown_enum(
                    format!("{base}.harness"),
                    harness.as_str(),
                ));
            }
        }
        if let Some(mode) = &estimate.build_mode {
            if !mode.is_known() {
                warnings.push(WireWarning::unknown_enum(
                    format!("{base}.build_mode"),
                    mode.as_str(),
                ));
            }
        }
    }
    for (index, waiting) in doc.waiting.iter().enumerate() {
        if !waiting.reason.code.is_known() {
            warnings.push(WireWarning::unknown_enum(
                format!("waiting[{index}].reason.code"),
                waiting.reason.code.as_str(),
            ));
        }
    }
    for (index, limit) in doc.limits.iter().enumerate() {
        if !limit.kind.is_known() {
            warnings.push(WireWarning::unknown_enum(
                format!("limits[{index}].kind"),
                limit.kind.as_str(),
            ));
        }
        if let Some(harness) = &limit.harness {
            if !harness.is_known() {
                warnings.push(WireWarning::unknown_enum(
                    format!("limits[{index}].harness"),
                    harness.as_str(),
                ));
            }
        }
    }
}
