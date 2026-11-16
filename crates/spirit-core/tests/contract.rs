//! Contract tests over the golden VOWP fixtures.
//!
//! The fixtures are complete documents copied from `docs/VOWP-CONTRACT.md`; these tests
//! are the gate against VOWP drift. They run without the `fixtures` feature.

use serde_json::{Value, json};

use spirit_core::error::SpiritErrorCode;
use spirit_core::time::Timestamp;
use spirit_core::view::{JobCardView, build_lanes};
use spirit_core::wire::{
    FleetStatus, JobState, SetTargetInput, Snapshot, parse_capacity_report, parse_error_document,
    parse_fleet_inbox, parse_fleet_queue, parse_fleet_status, parse_snapshot,
};

const FRESH: &str = include_str!("../fixtures/vowp/fleet-status-fresh.json");
const STALE: &str = include_str!("../fixtures/vowp/fleet-status-stale-cache.json");
const SUPERVISOR_DOWN: &str = include_str!("../fixtures/vowp/fleet-status-supervisor-down.json");
const CAPACITY_FULL: &str = include_str!("../fixtures/vowp/capacity-report-full.json");
const CAPACITY_EMPTY: &str = include_str!("../fixtures/vowp/capacity-report-empty.json");
const QUEUE: &str = include_str!("../fixtures/vowp/fleet-queue.json");
const QUEUE_CAP: &str = include_str!("../fixtures/vowp/fleet-queue-cap.json");
const INBOX: &str = include_str!("../fixtures/vowp/fleet-inbox.json");
const ERROR_NOT_FOUND: &str = include_str!("../fixtures/vowp/error-not-found.json");
const ERROR_GUARD: &str = include_str!("../fixtures/vowp/error-guard.json");
const UNKNOWN: &str = include_str!("../fixtures/vowp/snapshot-unknown-enums.json");
const MULTI: &str = include_str!("../fixtures/vowp/snapshot-multi-project.json");

fn fresh() -> FleetStatus {
    parse_fleet_status(FRESH.as_bytes())
        .expect("fresh fixture parses")
        .value
}

#[test]
fn fleet_status_fixture_round_trips_documented_fields() {
    let parsed = parse_fleet_status(FRESH.as_bytes()).unwrap();
    assert!(
        parsed.is_clean(),
        "fresh fixture should not warn: {:?}",
        parsed.warnings
    );
    let status = parsed.value;
    assert_eq!(status.source.as_str(), "local");
    assert_eq!(status.fetched_at.as_rfc3339(), "2026-11-14T12:00:04.812Z");
    assert!(!status.stale);
    let snapshot = &status.snapshot;
    assert_eq!(snapshot.home.as_str(), "vps");
    assert_eq!(snapshot.sequence, 4412);
    assert_eq!(snapshot.supervisor.pid, Some(4812));
    assert!(snapshot.supervisor.running);
    assert_eq!(snapshot.projects.len(), 1);
    let project = &snapshot.projects[0];
    assert_eq!(project.name.as_str(), "vowp");
    assert_eq!(project.running.len(), 3);
    assert_eq!(project.queue.len(), 3);
    assert_eq!(project.blocked.len(), 1);
    assert_eq!(project.gate_failures.len(), 1);
    assert_eq!(project.jobs_by_state.get("ready"), Some(&3));
    assert_eq!(snapshot.limits.len(), 1);
    assert_eq!(snapshot.inbox.len(), 1);
    assert_eq!(snapshot.sessions.len(), 1);
    assert_eq!(snapshot.orchestrators.len(), 1);
    let value = serde_json::to_value(&status).unwrap();
    assert_eq!(
        value["snapshot"]["projects"][0]["running"][0]["job"],
        "ss-2-desktop-shell"
    );
    assert_eq!(
        value["snapshot"]["inbox"][0]["title"],
        "Choose the included model set"
    );
    assert!(value["snapshot"]["inbox"][0].get("body").is_none());
}

#[test]
fn set_target_input_accepts_the_frozen_bridge_payload() {
    let input: SetTargetInput = serde_json::from_value(json!({
        "target": 9,
        "mode": "fixed",
        "override": true,
        "reason": "owner raised the target"
    }))
    .expect("the frozen SpiritBridge setTarget payload must deserialize");
    assert_eq!(input.target, 9);
    assert_eq!(input.mode.as_str(), "fixed");
    assert!(input.override_);
    let value = serde_json::to_value(&input).unwrap();
    assert_eq!(value.get("override"), Some(&json!(true)));
    assert!(
        value.get("override_").is_none(),
        "the wire key is `override`, not the Rust field name: {value}"
    );
}

#[test]
fn snapshot_schema_and_version_are_exact() {
    let snapshot = fresh().snapshot;
    snapshot.ensure_compatible().unwrap();

    let mut value: Value = serde_json::from_str(FRESH).unwrap();
    value["snapshot"]["schema"] = json!("vowp-snapshot/2");
    let error = parse_fleet_status(value.to_string().as_bytes()).unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::VowpIncompatible);
    assert!(error.message.contains("snapshot.schema"));

    let mut value: Value = serde_json::from_str(FRESH).unwrap();
    value["snapshot"]["version"] = json!(2);
    let error = parse_fleet_status(value.to_string().as_bytes()).unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::VowpIncompatible);
    assert!(error.message.contains("snapshot.version"));

    let snapshot: Snapshot =
        serde_json::from_value(serde_json::from_str::<Value>(FRESH).unwrap()["snapshot"].clone())
            .unwrap();
    snapshot.ensure_compatible().unwrap();
    let parsed = parse_snapshot(serde_json::to_vec(&snapshot).unwrap().as_slice()).unwrap();
    assert_eq!(parsed.value.version, 1);
}

#[test]
fn changed_schema_wins_over_missing_required_fields() {
    let mut value: Value = serde_json::from_str(FRESH).unwrap();
    value["snapshot"]["schema"] = json!("vowp-snapshot/2");
    value["snapshot"].as_object_mut().unwrap().remove("home");
    let error = parse_fleet_status(value.to_string().as_bytes()).unwrap_err();
    assert_eq!(
        error.code,
        SpiritErrorCode::VowpIncompatible,
        "VOWP-CONTRACT.md §12.3: a changed schema must report incompatible, not parse"
    );
    assert!(error.message.contains("snapshot.schema"));

    let mut value: Value = serde_json::from_str(FRESH).unwrap();
    value["snapshot"]["version"] = json!(2);
    value["snapshot"].as_object_mut().unwrap().remove("home");
    let snapshot = value["snapshot"].to_string();
    let error = parse_snapshot(snapshot.as_bytes()).unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::VowpIncompatible);
    assert!(error.message.contains("snapshot.version"));
}

#[test]
fn snapshot_capacity_card_parses_every_host_sample_field() {
    let snapshot = fresh().snapshot;
    let sample = snapshot.capacity.measured.expect("host sample");
    assert_eq!(sample.at.as_rfc3339(), "2026-11-14T11:59:45.000Z");
    assert_eq!(sample.interval_secs, 15);
    assert_eq!(sample.mem_total_bytes, 16_777_216_000);
    assert_eq!(sample.mem_available_bytes, 13_314_392_064);
    assert_eq!(sample.swap_used_bytes, 0);
    assert_eq!(sample.load1, 2.1);
    assert_eq!(sample.load5, 1.8);
    assert_eq!(sample.load15, 1.6);
    assert_eq!(sample.cpu_count, 8);
    assert_eq!(sample.cpu_quota_cores, None);
    assert_eq!(sample.disk_free_home_bytes, 45_206_814_720);
    assert_eq!(sample.disk_free_worktrees_bytes, Some(41_567_838_208));
    assert_eq!(sample.pressure_cpu_avg10, Some(4.2));
    assert_eq!(sample.pressure_mem_avg10, Some(1.1));
    assert_eq!(sample.pressure_io_avg10, Some(0.8));
    assert_eq!(sample.source.as_str(), "proc");
    let value = serde_json::to_value(&sample).unwrap();
    assert!(value["cpu_quota_cores"].is_null());
    let target = &snapshot.capacity.target;
    assert_eq!(target.target_workers, 8);
    assert_eq!(target.advised_workers, Some(8));
    assert_eq!(target.mode.as_str(), "fixed");
    assert!(!snapshot.capacity.brake.active);
}

#[test]
fn snapshot_project_cards_parse_running_blocked_queue_and_gates() {
    let status = fresh();
    let project = &status.snapshot.projects[0];
    let running = &project.running[0];
    assert_eq!(running.role.as_str(), "worker");
    assert_eq!(
        running.executor.as_deref(),
        Some("opencode:deepseek/deepseek-flash#max")
    );
    assert!(project.running[2].executor.is_none());
    let blocked = &project.blocked[0];
    assert_eq!(blocked.state.as_str(), "parked");
    let blocked_value = serde_json::to_value(blocked).unwrap();
    assert!(blocked_value.get("title").is_none());
    let queue = &project.queue[0];
    assert_eq!(queue.title, "Build the visual fleet canvas");
    assert_eq!(queue.priority, 50);
    assert_eq!(queue.waiting_reason.as_deref(), Some("RAM 1.2 GB short"));
    let gate = &project.gate_failures[0];
    assert_eq!(gate.gate, "secret_scan");
    assert_eq!(
        project.last_merge.as_ref().unwrap().job.as_str(),
        "ss-1-core"
    );
    assert_eq!(project.cost_today_usd, Some(1.37));
}

#[test]
fn snapshot_inbox_summary_has_no_body_field() {
    let status = fresh();
    let summary = &status.snapshot.inbox[0];
    let value = serde_json::to_value(summary).unwrap();
    assert!(value.get("body").is_none());
    assert!(value.get("options").is_none());
    assert_eq!(value["kind"], "question");
    assert_eq!(value["age_secs"], 1080);
}

#[test]
fn snapshot_session_surface_card_parses_surface_ref() {
    let status = fresh();
    let session = &status.snapshot.sessions[0];
    assert_eq!(session.role.as_ref().unwrap().as_str(), "worker");
    assert_eq!(session.state.as_str(), "running");
    let surface = session.active_surface.as_ref().unwrap();
    assert_eq!(surface.kind.as_str(), "opencode");
    assert_eq!(surface.holder, "vps opencode");
    assert_eq!(
        surface.heartbeat_at.as_rfc3339(),
        "2026-11-14T11:59:55.000Z"
    );
    let value = serde_json::to_value(session).unwrap();
    assert_eq!(
        value["active_surface"]["surface_id"],
        "01JCF5WQ8ZK3M2V7N4P6R8T0AG"
    );
}

#[test]
fn snapshot_orchestrator_card_tolerates_a_missing_or_present_node() {
    let status = fresh();
    let orchestrator = &status.snapshot.orchestrators[0];
    let value = serde_json::to_value(orchestrator).unwrap();
    assert!(
        value["node"].is_null(),
        "a home-hosted card has no node and the view must not invent one"
    );
    assert_eq!(value["app"], "opencode");
    assert_eq!(
        value["join"]["phone"],
        "Open the VOWP board and join vowp-main"
    );

    let cards: Vec<spirit_core::wire::OrchestratorCard> = serde_json::from_str(include_str!(
        "../fixtures/vowp/orchestrator-list-local.json"
    ))
    .unwrap();
    assert_eq!(cards[0].node.as_ref().unwrap().as_str(), "mac-studio");
}

#[test]
fn capacity_report_parses_brakes_plural_and_cli_extras() {
    let parsed = parse_capacity_report(CAPACITY_FULL.as_bytes()).unwrap();
    let doc = parsed.value;
    assert!(!doc.report.brakes.active);
    assert_eq!(doc.provider_caps.get("deepseek"), Some(&8));
    assert!(!doc.reload_pending);
    let value = serde_json::to_value(&doc).unwrap();
    assert!(value.get("brakes").is_some());
    assert!(value.get("brake").is_none());
    assert_eq!(value["provider_caps"]["deepseek"], 8);
    assert_eq!(value["reload_pending"], false);
}

#[test]
fn capacity_report_parses_estimates_and_waiting() {
    let doc = parse_capacity_report(CAPACITY_FULL.as_bytes())
        .unwrap()
        .value;
    assert_eq!(doc.report.estimates.len(), 4);
    assert_eq!(doc.report.estimates[0].source.as_str(), "measured");
    assert_eq!(doc.report.estimates[1].source.as_str(), "pooled");
    assert_eq!(doc.report.estimates[2].source.as_str(), "default");
    assert_eq!(doc.report.estimates[0].ram_p90_bytes, 1_288_490_188);
    assert_eq!(doc.report.waiting.len(), 1);
    assert_eq!(doc.report.waiting[0].reason.code.as_str(), "ram");
    assert_eq!(doc.report.waiting[0].reason.detail, "RAM 1.2 GB short");
    let empty = parse_capacity_report(CAPACITY_EMPTY.as_bytes())
        .unwrap()
        .value;
    assert!(empty.report.host.is_none());
    assert!(empty.report.estimates.is_empty());
    assert!(empty.report.waiting.is_empty());
    assert!(empty.report.limits.is_empty());
}

#[test]
fn fleet_queue_is_an_array_with_waiting_reason() {
    let rows = parse_fleet_queue(QUEUE.as_bytes()).unwrap().value;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].project.as_str(), "vowp");
    assert_eq!(rows[0].title, "Build the visual fleet canvas");
    assert_eq!(rows[0].waiting_reason.as_deref(), Some("RAM 1.2 GB short"));
    assert_eq!(rows[1].state.as_str(), "pending");
    let cap = parse_fleet_queue(QUEUE_CAP.as_bytes()).unwrap().value;
    assert_eq!(cap.len(), 200);
}

#[test]
fn fleet_inbox_is_an_array_with_full_body() {
    let items = parse_fleet_inbox(INBOX.as_bytes()).unwrap().value;
    assert_eq!(items.len(), 2);
    assert_eq!(
        items[0].body,
        "Which included models should the first release guarantee?"
    );
    assert_eq!(items[0].options.len(), 2);
    assert_eq!(items[0].state.as_str(), "open");
    assert_eq!(items[1].state.as_str(), "answered");
    assert_eq!(items[1].answer.as_deref(), Some("Acknowledged"));
}

#[test]
fn error_document_maps_every_exit_code() {
    let not_found = parse_error_document(ERROR_NOT_FOUND.as_bytes()).unwrap();
    assert_eq!(not_found.error.kind.as_str(), "not_found");
    let mapped = not_found.to_spirit_error(3);
    assert_eq!(mapped.code, SpiritErrorCode::NotFound);
    assert_eq!(mapped.hint.as_deref(), Some("vowp session ls"));

    let guard = parse_error_document(ERROR_GUARD.as_bytes()).unwrap();
    let mapped = guard.to_spirit_error(5);
    assert_eq!(mapped.code, SpiritErrorCode::Refused);
    assert!(mapped.message.contains("above the advised"));

    let expected = [
        (1, SpiritErrorCode::VowpCommand),
        (2, SpiritErrorCode::InvalidInput),
        (3, SpiritErrorCode::NotFound),
        (4, SpiritErrorCode::Conflict),
        (5, SpiritErrorCode::Refused),
        (6, SpiritErrorCode::Limit),
        (7, SpiritErrorCode::Remote),
        (75, SpiritErrorCode::Refused),
        (127, SpiritErrorCode::VowpMissing),
    ];
    for (exit, code) in expected {
        assert_eq!(spirit_core::error::code_for_exit(exit), code);
    }
}

#[test]
fn timestamp_regex_and_canonical_round_trip() {
    let timestamp = Timestamp::parse("2026-11-14T13:00:00.123+01:00").unwrap();
    let canonical = timestamp.as_rfc3339();
    assert_eq!(canonical, "2026-11-14T12:00:00.123Z");
    assert_eq!(canonical.len(), 24);
    assert!(canonical.ends_with('Z'));
    assert_eq!(&canonical[4..5], "-");
    assert_eq!(&canonical[10..11], "T");
    assert_eq!(&canonical[19..20], ".");
    assert!(canonical[20..23].chars().all(|c| c.is_ascii_digit()));
    let serialized = serde_json::to_string(&timestamp).unwrap();
    assert_eq!(serialized, "\"2026-11-14T12:00:00.123Z\"");
    let back: Timestamp = serde_json::from_str(&serialized).unwrap();
    assert_eq!(back, timestamp);
    assert!(Timestamp::parse("2026-11-14T12:00:00.1234567890Z").is_err());
}

#[test]
fn unknown_enum_value_is_unknown_not_ready() {
    let parsed = parse_fleet_status(UNKNOWN.as_bytes()).unwrap();
    assert!(
        parsed
            .warnings
            .iter()
            .any(|warning| warning.code == spirit_core::wire::WireWarningCode::UnknownEnum)
    );
    let status = parsed.value;
    let project = &status.snapshot.projects[0];
    let unknown_row = project
        .queue
        .iter()
        .find(|row| row.state.as_str() == "quantum")
        .expect("unknown row");
    assert!(!unknown_row.state.is_known());
    assert_eq!(
        JobState::parse("quantum"),
        JobState::Unknown("quantum".to_owned())
    );
    assert_ne!(JobState::parse("quantum"), JobState::Ready);
    let lanes = build_lanes(&status.snapshot);
    assert!(
        !lanes
            .ready
            .iter()
            .any(|card: &JobCardView| card.job_id == unknown_row.job.as_str())
    );
    let parked = lanes
        .parked_blocked
        .iter()
        .find(|card| card.job_id == unknown_row.job.as_str())
        .expect("unknown state is parked, not dropped");
    assert!(parked.is_unknown_state());
    assert_eq!(parked.state, "quantum");
    assert_eq!(lane_count(&lanes, "ready"), 1);
}

fn lane_count(lanes: &spirit_core::view::JobLanes, lane: &str) -> u32 {
    lanes.counts.get(lane).copied().unwrap_or(0)
}

#[test]
fn unknown_snapshot_host_sample_source_produces_a_warning() {
    let mut value: Value = serde_json::from_str(FRESH).unwrap();
    value["snapshot"]["capacity"]["measured"]["source"] = json!("ebpf");
    let parsed = parse_fleet_status(value.to_string().as_bytes()).unwrap();
    assert_eq!(
        parsed
            .value
            .snapshot
            .capacity
            .measured
            .as_ref()
            .unwrap()
            .source
            .as_str(),
        "ebpf"
    );
    assert!(
        parsed
            .warnings
            .iter()
            .any(|warning| warning.path == "capacity.measured.source"
                && warning.code == spirit_core::wire::WireWarningCode::UnknownEnum),
        "unknown host-sample source must warn: {:?}",
        parsed.warnings
    );
}

#[test]
fn unknown_object_field_is_ignored() {
    let mut value: Value = serde_json::from_str(FRESH).unwrap();
    value["future_top_level"] = json!({"anything": true});
    value["snapshot"]["future_snapshot_field"] = json!(42);
    value["snapshot"]["projects"][0]["future_project_field"] = json!("ignored");
    value["snapshot"]["projects"][0]["running"][0]["future_card_field"] = json!([1, 2, 3]);
    let status = parse_fleet_status(value.to_string().as_bytes())
        .unwrap()
        .value;
    assert_eq!(status.snapshot.home.as_str(), "vps");
}

#[test]
fn missing_required_field_fails_with_document_name() {
    let mut value: Value = serde_json::from_str(FRESH).unwrap();
    value["snapshot"].as_object_mut().unwrap().remove("home");
    let error = parse_fleet_status(value.to_string().as_bytes()).unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Parse);
    assert!(error.message.contains("fleet status"));
    assert!(error.message.contains("home"));

    let mut value: Value = serde_json::from_str(FRESH).unwrap();
    value["snapshot"]["projects"][0]
        .as_object_mut()
        .unwrap()
        .remove("jobs_by_state");
    let error = parse_fleet_status(value.to_string().as_bytes()).unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Parse);
    assert!(error.message.contains("jobs_by_state"));
}

#[test]
fn no_fixture_contains_a_secret_pattern() {
    let directory = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/vowp");
    let mut checked = 0;
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let patterns = [
            ["BEGIN ", "RSA ", "PRIVATE KEY"].concat(),
            ["BEGIN ", "OPENSSH ", "PRIVATE KEY"].concat(),
            ["BEGIN ", "PRIVATE KEY"].concat(),
            ["g", "hp", "_"].concat(),
            ["git", "hub", "_pat_"].concat(),
            ["AK", "IA"].concat(),
            ["api", "_key="].concat(),
            ["pass", "word="].concat(),
            ["sec", "ret="].concat(),
            ["to", "ken="].concat(),
        ];
        for pattern in &patterns {
            assert!(
                !text.contains(pattern),
                "fixture {} contains a secret-looking pattern",
                path.display()
            );
        }
        let sk = ["s", "k", "-"].concat();
        for token in
            text.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '/' | '.')))
        {
            assert!(
                !token.starts_with(&sk),
                "fixture {} contains a secret-shaped token",
                path.display()
            );
        }
        checked += 1;
    }
    assert!(
        checked >= 25,
        "expected the golden fixture set, got {checked}"
    );
}

#[test]
fn stale_cache_and_supervisor_down_documents_parse() {
    let stale = parse_fleet_status(STALE.as_bytes()).unwrap().value;
    assert!(stale.stale);
    assert!(stale.source_is_cache());
    let down = parse_fleet_status(SUPERVISOR_DOWN.as_bytes())
        .unwrap()
        .value;
    assert!(!down.snapshot.supervisor.running);
    assert_eq!(
        down.snapshot.supervisor.last_tick.unwrap().as_rfc3339(),
        "2026-11-14T09:00:00.000Z"
    );
    let multi = parse_fleet_status(MULTI.as_bytes()).unwrap().value;
    assert_eq!(multi.snapshot.projects.len(), 3);
    assert_eq!(multi.snapshot.projects[1].name.as_str(), "northnoir");
}

#[test]
fn every_documented_enum_string_is_accepted() {
    use spirit_core::wire::{
        BrakeCode, HarnessKind, JobKind, LimitRowKind, ProjectState, WaitCode,
    };
    for value in [
        "pending",
        "ready",
        "running",
        "stalled",
        "gating",
        "reviewing",
        "fixing",
        "merging",
        "done",
        "failed",
        "parked",
        "replanned",
        "cancelled",
    ] {
        assert!(JobState::parse(value).is_known(), "{value}");
    }
    for value in [
        "plan", "spec", "build", "test", "verify", "fix", "merge", "docs", "research", "promote",
        "release", "decision",
    ] {
        assert!(JobKind::parse(value).is_known(), "{value}");
    }
    for value in ["active", "paused", "archived"] {
        assert!(ProjectState::parse(value).is_known(), "{value}");
    }
    for value in ["claude-code", "opencode", "cli", "acp", "vowp"] {
        assert!(HarnessKind::parse(value).is_known(), "{value}");
    }
    for value in ["quota", "credits", "rate", "context", "auth", "unavailable"] {
        assert!(LimitRowKind::parse(value).is_known(), "{value}");
    }
    for value in ["ram", "cpu", "disk", "provider", "brake", "target"] {
        assert!(WaitCode::parse(value).is_known(), "{value}");
    }
    for value in ["memory_pressure", "swap_growth", "disk_low", "provider"] {
        assert!(BrakeCode::parse(value).is_known(), "{value}");
    }
}
