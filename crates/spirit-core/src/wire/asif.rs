//! ASIF 0.4 export summaries and import responses.
//!
//! The types mirror the adapter-free subset in `vowp_protocol::asif`, plus the CLI-local
//! export summary. The remote output path and staging path are deliberately **not**
//! represented: `path` and `report_path` are ignored by serde, so they can never reach a
//! view or a renderer. The import session reference likewise omits `cwd`, which is a
//! remote path the UI does not need.
//!
//! ASIF is a review draft. Nothing here claims conformance, lossless translation or a
//! portable continuation; every report carries its declared losses and the unsupported
//! operation list that VOWP reported.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::SpiritError;
use crate::time::Timestamp;
use crate::wire::enums::{SessionRole, SessionState};
use crate::wire::executor::ExecutorRef;
use crate::wire::ids::{CapsuleId, JobId, NodeName, ProjectName, SessionId};
use crate::wire::{
    Parsed, WireWarning, WireWarningCode, from_value, parse_json, push_missing_optional,
};

/// The only ASIF wire format value this build accepts.
pub const ASIF_FORMAT: &str = "asif";

/// The adapter's hard archive cap: 272 MiB (`VOWP-CONTRACT.md` §11.2.3).
pub const ASIF_ARCHIVE_CAP_BYTES: u64 = 272 * 1024 * 1024;

/// The JSON document cap for one ASIF summary or import response (16 MiB).
pub const ASIF_JSON_CAP_BYTES: usize = 16 * 1024 * 1024;

/// Record counts reported by an ASIF export or import.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AsifCounts {
    /// Source events read.
    pub events_in: u64,
    /// Target events written.
    pub events_out: u64,
    /// Resources written or preserved.
    pub resources: u64,
    /// Capsules read or written.
    pub capsules: u64,
}

/// One declared ASIF information change or omission.
///
/// The adapter's ordering class (`Blocking`, `Integrity`, `Normalization`, `Omission`)
/// is not on the wire; VOWP applies the strict filter before it prints this summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AsifLoss {
    /// Machine-readable loss kind, for example `cwd-omitted`.
    pub kind: String,
    /// Stage at which the change happened.
    pub stage: String,
    /// Affected coverage domain, when the adapter names one.
    #[serde(default)]
    pub scope: Option<String>,
    /// Path-free explanation. It never contains source content or secrets.
    pub message: String,
}

impl AsifLoss {
    /// Returns true when this build knows the documented loss stage.
    #[must_use]
    pub fn stage_is_known(&self) -> bool {
        matches!(
            self.stage.as_str(),
            "capture" | "normalization" | "redaction" | "translation"
        )
    }
}

/// Stable, adapter-free summary of one ASIF export or import report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AsifReport {
    /// Adapter version that performed the translation.
    pub adapter_version: String,
    /// ASIF draft version interpreted by the adapter.
    pub asif_version: String,
    /// Pinned ASIF commit interpreted by the adapter.
    pub asif_pin: String,
    /// Source session id, when the direction records one.
    #[serde(default)]
    pub source_session: Option<String>,
    /// Import target session id.
    #[serde(default)]
    pub target_session: Option<String>,
    /// Record counts for this translation.
    pub counts: AsifCounts,
    /// Number of declared losses.
    pub loss_count: u64,
    /// Declared losses in stable order.
    pub losses: Vec<AsifLoss>,
    /// VOWP operations not captured by the ASIF document.
    pub unsupported: Vec<String>,
}

impl AsifReport {
    /// Returns true when `loss_count` equals the number of summarized losses.
    #[must_use]
    pub fn loss_count_matches(&self) -> bool {
        u64::try_from(self.losses.len()).is_ok_and(|count| count == self.loss_count)
    }
}

/// The CLI-local `vowp session export --format asif --json` summary.
///
/// `path` and `report_path` from the CLI document are intentionally discarded by serde.
/// A caller must never render or log a remote path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AsifExportSummary {
    /// Interchange format; must be [`ASIF_FORMAT`].
    pub format: String,
    /// Exported source session.
    pub session: SessionId,
    /// Package size in bytes, as VOWP reported it.
    pub bytes: u64,
    /// Declared report summary.
    pub report: AsifReport,
}

impl AsifExportSummary {
    /// Returns true when the summary names the expected source session.
    #[must_use]
    pub fn is_for_session(&self, session: &SessionId) -> bool {
        self.session == *session
    }
}

/// The imported session row. `cwd` is parsed and discarded because it is a remote path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AsifSessionRef {
    /// New session id.
    pub id: SessionId,
    /// Bound project, when the source project exists locally.
    #[serde(default)]
    pub project: Option<ProjectName>,
    /// Bound job, when the source job exists locally.
    #[serde(default)]
    pub job_id: Option<JobId>,
    /// Session role, when the package carried one.
    #[serde(default)]
    pub role: Option<SessionRole>,
    /// Human-readable title.
    pub title: String,
    /// Fork parent, when one was recorded.
    #[serde(default)]
    pub parent_session_id: Option<SessionId>,
    /// Node that owns the imported log.
    pub node: NodeName,
    /// Lifecycle state. An import must be `idle`.
    pub state: SessionState,
    /// Executor evidence as the canonical executor string, when the package carried it.
    #[serde(default)]
    pub executor: Option<String>,
    /// Sequence number of the last imported event.
    pub last_seq: u64,
    /// Capsule written most recently.
    #[serde(default)]
    pub last_capsule_id: Option<CapsuleId>,
    /// Size of the imported canonical log in bytes.
    pub log_bytes: u64,
    /// When the session was created.
    pub created_at: Timestamp,
    /// When the row last changed.
    pub updated_at: Timestamp,
}

/// The shared body of `vowp session import --format asif --json`.
///
/// `warnings` are reindex warnings (for example `project ... left null`), not losses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AsifImportResponse {
    /// The new registered session row.
    pub session: AsifSessionRef,
    /// Adapter report summary for the import.
    pub report: AsifReport,
    /// Non-fatal reindex warnings.
    pub warnings: Vec<String>,
}

impl AsifImportResponse {
    /// Returns true when the imported session is idle, as the contract requires.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.session.state == SessionState::Idle
    }

    /// Rejects a response that claims an imported session is doing work.
    pub fn ensure_idle(&self) -> Result<(), SpiritError> {
        if self.is_idle() {
            Ok(())
        } else {
            Err(SpiritError::vowp_incompatible(
                "asif import response",
                format!(
                    "imported session must be idle, got {:?}",
                    self.session.state.as_str()
                ),
            ))
        }
    }
}

/// Parses the `--json` export summary and rejects a non-ASIF format.
pub fn parse_asif_export_summary(bytes: &[u8]) -> Result<Parsed<AsifExportSummary>, SpiritError> {
    let document = "asif export summary";
    let raw = parse_json(bytes, document)?;
    ensure_export_format(&raw)?;
    let mut warnings = Vec::new();
    push_missing_optional(
        &raw,
        &["report.source_session", "report.target_session"],
        &mut warnings,
    );
    let summary: AsifExportSummary = from_value(raw, document)?;
    collect_report_warnings(&summary.report, &mut warnings);
    if let Some(source) = &summary.report.source_session {
        if source != summary.session.as_str() {
            return Err(SpiritError::vowp_incompatible(
                "asif export summary",
                "report.source_session does not match the exported session",
            ));
        }
    }
    Ok(Parsed {
        value: summary,
        warnings,
    })
}

/// Parses the `session import --format asif --json` response.
pub fn parse_asif_import_response(bytes: &[u8]) -> Result<Parsed<AsifImportResponse>, SpiritError> {
    let document = "asif import response";
    let raw = parse_json(bytes, document)?;
    let mut warnings = Vec::new();
    push_missing_optional(
        &raw,
        &[
            "session.project",
            "session.job_id",
            "session.role",
            "session.parent_session_id",
            "session.executor",
            "session.last_capsule_id",
            "report.source_session",
            "report.target_session",
        ],
        &mut warnings,
    );
    let response: AsifImportResponse = from_value(raw, document)?;
    collect_import_warnings(&response, &mut warnings);
    collect_report_warnings(&response.report, &mut warnings);
    if let Some(target) = &response.report.target_session {
        if target != response.session.id.as_str() {
            return Err(SpiritError::vowp_incompatible(
                "asif import response",
                "report.target_session does not match the new session",
            ));
        }
    }
    Ok(Parsed {
        value: response,
        warnings,
    })
}

fn ensure_export_format(raw: &Value) -> Result<(), SpiritError> {
    let Some(format) = raw.get("format").and_then(Value::as_str) else {
        return Ok(());
    };
    if format == ASIF_FORMAT {
        Ok(())
    } else {
        Err(SpiritError::vowp_incompatible(
            "asif export summary.format",
            format!("expected {ASIF_FORMAT:?}, got {format:?}"),
        ))
    }
}

fn collect_report_warnings(report: &AsifReport, warnings: &mut Vec<WireWarning>) {
    if !report.loss_count_matches() {
        warnings.push(WireWarning {
            path: "report.loss_count".to_owned(),
            code: WireWarningCode::Schema,
            detail: "loss_count does not equal the number of summarized losses".to_owned(),
        });
    }
    for (index, loss) in report.losses.iter().enumerate() {
        if !loss.stage_is_known() {
            warnings.push(WireWarning {
                path: format!("report.losses[{index}].stage"),
                code: WireWarningCode::UnknownEnum,
                detail: format!("unknown loss stage {:?}; shown verbatim", loss.stage),
            });
        }
    }
}

fn collect_import_warnings(response: &AsifImportResponse, warnings: &mut Vec<WireWarning>) {
    let session = &response.session;
    if !session.state.is_known() {
        warnings.push(WireWarning {
            path: "session.state".to_owned(),
            code: WireWarningCode::UnknownEnum,
            detail: format!("unknown session state {:?}", session.state.as_str()),
        });
    }
    if let Some(role) = &session.role {
        if !role.is_known() {
            warnings.push(WireWarning {
                path: "session.role".to_owned(),
                code: WireWarningCode::UnknownEnum,
                detail: format!("unknown session role {:?}", role.as_str()),
            });
        }
    }
    if let Some(executor) = &session.executor {
        if !ExecutorRef::parse(executor).is_known() {
            warnings.push(WireWarning {
                path: "session.executor".to_owned(),
                code: WireWarningCode::UnknownEnum,
                detail: "unknown executor string; shown as Unknown harness".to_owned(),
            });
        }
    }
}
