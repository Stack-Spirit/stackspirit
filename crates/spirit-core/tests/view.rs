//! Deterministic view derivation: lanes, executor parsing, estimates, thresholds.

use serde_json::Value;
use spirit_core::connection::LatencyStats;
use spirit_core::host::HostId;
use spirit_core::time::Timestamp;
use spirit_core::view::{
    CapacityView, EstimateQuery, ExecutorView, FleetView, ResourceState, cpu_resource_state,
    disk_resource_state, ram_resource_state, select_estimate,
};
use spirit_core::wire::{
    CapacityReportDoc, FleetStatus, parse_capacity_report, parse_fleet_queue, parse_fleet_status,
};

const FRESH: &str = include_str!("../fixtures/vowp/fleet-status-fresh.json");
const CAPACITY: &str = include_str!("../fixtures/vowp/capacity-report-full.json");
const DISK_DISAGREEMENT: &str = include_str!("../fixtures/vowp/disk-disagreement.json");
const DISK_REPORT: &str = include_str!("../fixtures/vowp/capacity-report-disk-disagreement.json");
const QUEUE_CAP: &str = include_str!("../fixtures/vowp/fleet-queue-cap.json");
const UNKNOWN: &str = include_str!("../fixtures/vowp/snapshot-unknown-enums.json");

fn fresh() -> FleetStatus {
    parse_fleet_status(FRESH.as_bytes()).unwrap().value
}

fn report() -> CapacityReportDoc {
    parse_capacity_report(CAPACITY.as_bytes()).unwrap().value
}

fn now() -> Timestamp {
    Timestamp::parse("2026-11-14T12:00:10.000Z").unwrap()
}

fn view(status: &FleetStatus, report: Option<&CapacityReportDoc>) -> FleetView {
    FleetView::from_status(
        HostId("01JCF5WQ8ZK3M2V7N4P6R8T0AB".to_owned()),
        status,
        report,
        LatencyStats::default(),
        1,
        now(),
    )
}

#[test]
fn lanes_match_the_contract_mapping() {
    let fleet = view(&fresh(), None);
    assert_eq!(fleet.lanes.running.len(), 3);
    assert_eq!(fleet.lanes.ready.len(), 3);
    // The only gate failure is for a job that is not live review work: history, not a card.
    assert_eq!(fleet.lanes.review_fix.len(), 0);
    assert_eq!(fleet.lanes.history.len(), 1);
    assert_eq!(fleet.lanes.parked_blocked.len(), 1);
    assert!(
        fleet.lanes.done.is_empty(),
        "done cards are summarized, not live"
    );
    assert_eq!(fleet.lanes.counts.get("running"), Some(&3));
    assert_eq!(fleet.lanes.counts.get("ready"), Some(&3));
    assert_eq!(fleet.lanes.counts.get("review_fix"), Some(&1));
    assert_eq!(fleet.lanes.counts.get("parked_blocked"), Some(&1));
    assert_eq!(fleet.lanes.counts.get("done"), Some(&37));
    assert_eq!(fleet.lanes.history[0].gate.as_deref(), Some("secret_scan"));
    assert_eq!(fleet.lanes.history[0].detail, None);
    assert!(fleet.lanes.history[0].gate_at.is_some());
    let ready_ids: Vec<&str> = fleet
        .lanes
        .ready
        .iter()
        .map(|card| card.job_id.as_str())
        .collect();
    assert!(ready_ids.contains(&"ss-5-fleet-inbox"));
    assert!(!ready_ids.contains(&"ss-2-desktop-shell"));
}

#[test]
fn running_cards_keep_a_null_executor_as_shell() {
    let fleet = view(&fresh(), None);
    let shell = fleet
        .running
        .iter()
        .find(|card| card.job_id == "ss-4-engine-onboarding")
        .unwrap();
    assert!(shell.executor.is_none());
    assert_eq!(shell.role, "worker");
    assert!(!shell.stalled);
    let opencode = fleet
        .running
        .iter()
        .find(|card| card.job_id == "ss-2-desktop-shell")
        .unwrap();
    let executor = opencode.executor.as_ref().unwrap();
    assert_eq!(executor.harness, "opencode");
    assert_eq!(executor.provider, "deepseek");
    assert_eq!(executor.model, "deepseek/deepseek-flash");
    assert_eq!(executor.variant.as_deref(), Some("max"));
    assert_eq!(executor.display, "DeepSeek V4.1 Flash · max");
}

#[test]
fn queue_cap_200_is_labeled_as_truncated() {
    let mut value: Value = serde_json::from_str(FRESH).unwrap();
    let rows = parse_fleet_queue(QUEUE_CAP.as_bytes()).unwrap().value;
    let mut queue_values = Vec::new();
    for row in rows {
        let mut card = serde_json::to_value(&row).unwrap();
        card.as_object_mut().unwrap().remove("project");
        queue_values.push(card);
    }
    value["snapshot"]["projects"][0]["queue"] = Value::Array(queue_values);
    let status: FleetStatus = serde_json::from_value(value).unwrap();
    let fleet = view(&status, None);
    assert!(fleet.lanes.queue_truncated);
    assert_eq!(fleet.lanes.queue_shown, 200);
    assert_eq!(fleet.lanes.queue_cap, 200);
    assert_eq!(fleet.lanes.ready.len(), 200);
}

#[test]
fn blocked_card_does_not_invent_a_title() {
    let fleet = view(&fresh(), None);
    let blocked = fleet
        .lanes
        .parked_blocked
        .iter()
        .find(|card| card.job_id == "ss-8-blocked-review")
        .unwrap();
    assert_eq!(blocked.title, None);
    assert_eq!(blocked.state, "parked");
    assert_eq!(blocked.reason.as_deref(), Some("question"));
    assert!(blocked.detail.as_deref().unwrap().contains("owner"));
}

#[test]
fn target_advice_uses_target_then_snapshot_value() {
    let status = fresh();
    let capacity = CapacityView::derive(
        &status.snapshot.capacity,
        &status.snapshot.limits,
        None,
        status.snapshot.generated_at,
    );
    assert_eq!(capacity.advised_workers, Some(8));

    let mut value: Value = serde_json::from_str(FRESH).unwrap();
    value["snapshot"]["capacity"]["target"]["advised_workers"] = Value::Null;
    value["snapshot"]["capacity"]["advised_target"] = Value::from(6);
    let status: FleetStatus = serde_json::from_value(value).unwrap();
    let capacity = CapacityView::derive(
        &status.snapshot.capacity,
        &status.snapshot.limits,
        None,
        status.snapshot.generated_at,
    );
    assert_eq!(capacity.advised_workers, Some(6));

    let mut value: Value = serde_json::from_str(FRESH).unwrap();
    value["snapshot"]["capacity"]["target"]["advised_workers"] = Value::Null;
    value["snapshot"]["capacity"]["advised_target"] = Value::Null;
    let status: FleetStatus = serde_json::from_value(value).unwrap();
    let capacity = CapacityView::derive(
        &status.snapshot.capacity,
        &status.snapshot.limits,
        None,
        status.snapshot.generated_at,
    );
    assert_eq!(capacity.advised_workers, None);
}

#[test]
fn ram_estimate_prefers_exact_then_pooled_then_largest() {
    let status = fresh();
    let report = report();
    let capacity = CapacityView::derive(
        &status.snapshot.capacity,
        &status.snapshot.limits,
        Some(&report),
        status.snapshot.generated_at,
    );
    assert!(capacity.report_loaded);
    let exact = EstimateQuery {
        project: Some("vowp"),
        job_kind: Some("build"),
        harness: Some("opencode"),
        model: Some("deepseek/deepseek-flash"),
        build_mode: Some("warm"),
    };
    let selected = select_estimate(&capacity.estimates, &exact).unwrap();
    assert_eq!(selected.source, "measured");
    assert_eq!(selected.ram_p90_bytes, 1_288_490_188);

    let no_exact = EstimateQuery {
        project: Some("other-project"),
        job_kind: Some("build"),
        harness: Some("opencode"),
        model: Some("deepseek/deepseek-flash"),
        build_mode: Some("warm"),
    };
    let selected = select_estimate(&capacity.estimates, &no_exact).unwrap();
    assert_eq!(selected.source, "pooled");
    assert_eq!(selected.ram_p90_bytes, 1_610_612_736);

    let no_estimate = EstimateQuery {
        project: Some("other-project"),
        job_kind: Some("test"),
        harness: Some("claude-code"),
        model: None,
        build_mode: None,
    };
    let selected = select_estimate(&capacity.estimates, &no_estimate).unwrap();
    assert_eq!(selected.samples, 7);
    assert_eq!(selected.ram_p90_bytes, 2_684_354_560);

    let ram = capacity.ram_with_estimate(&exact);
    assert_eq!(ram.per_worker_source.as_deref(), Some("measured"));
    assert_eq!(ram.per_worker_samples, Some(12));
    assert!(ram.estimated_extra_slots.is_some());
}

#[test]
fn disk_ram_cpu_thresholds_are_deterministic() {
    assert_eq!(disk_resource_state(false, 0, 100), ResourceState::Unknown);
    assert_eq!(disk_resource_state(true, 99, 100), ResourceState::Critical);
    assert_eq!(disk_resource_state(true, 100, 100), ResourceState::Warn);
    assert_eq!(disk_resource_state(true, 124, 100), ResourceState::Warn);
    assert_eq!(disk_resource_state(true, 125, 100), ResourceState::Ok);
    assert_eq!(disk_resource_state(true, 10, 0), ResourceState::Ok);

    assert_eq!(ram_resource_state(false, 0, 100), ResourceState::Unknown);
    assert_eq!(ram_resource_state(true, 99, 100), ResourceState::Critical);
    assert_eq!(ram_resource_state(true, 100, 100), ResourceState::Warn);
    assert_eq!(ram_resource_state(true, 199, 100), ResourceState::Warn);
    assert_eq!(ram_resource_state(true, 200, 100), ResourceState::Ok);

    assert_eq!(
        cpu_resource_state(false, 0.0, 8, None),
        ResourceState::Unknown
    );
    assert_eq!(
        cpu_resource_state(true, 0.0, 0, None),
        ResourceState::Unknown
    );
    assert_eq!(cpu_resource_state(true, 4.0, 8, None), ResourceState::Ok);
    assert_eq!(cpu_resource_state(true, 6.4, 8, None), ResourceState::Warn);
    assert_eq!(
        cpu_resource_state(true, 8.1, 8, None),
        ResourceState::Critical
    );
    assert_eq!(
        cpu_resource_state(true, 1.0, 8, Some(30.0)),
        ResourceState::Critical
    );
    assert_eq!(
        cpu_resource_state(true, 1.0, 8, Some(15.0)),
        ResourceState::Warn
    );
}

#[test]
fn disk_disagreement_prefers_the_capacity_report() {
    let status = parse_fleet_status(DISK_DISAGREEMENT.as_bytes())
        .unwrap()
        .value;
    let report = parse_capacity_report(DISK_REPORT.as_bytes()).unwrap().value;
    let capacity = CapacityView::derive(
        &status.snapshot.capacity,
        &status.snapshot.limits,
        Some(&report),
        status.snapshot.generated_at,
    );
    assert_eq!(capacity.disk.source, "capacity_report");
    assert_eq!(capacity.disk.free_home_bytes, 32_212_254_720);
    assert_eq!(capacity.disk.floor_bytes, 5_368_709_120);
}

#[test]
fn capacity_report_unloaded_shows_unavailable_not_empty() {
    let status = fresh();
    let capacity = CapacityView::derive(
        &status.snapshot.capacity,
        &status.snapshot.limits,
        None,
        status.snapshot.generated_at,
    );
    assert!(!capacity.report_loaded);
    assert!(capacity.estimates.is_empty());
    assert!(capacity.waiting.is_empty());
    assert!(capacity.provider_caps.is_empty());
    assert!(!capacity.reload_pending);
    assert_eq!(capacity.disk.source, "snapshot");
}

#[test]
fn executor_parsing_keeps_unknown_values_raw() {
    let view = ExecutorView::parse("opencode:deepseek/deepseek-flash#max");
    assert_eq!(view.harness, "opencode");
    assert_eq!(view.provider, "deepseek");
    assert_eq!(view.raw, "opencode:deepseek/deepseek-flash#max");
    let claude = ExecutorView::parse("claude-code:model-a@medium");
    assert_eq!(claude.harness, "claude-code");
    assert_eq!(claude.provider, "claude");
    assert_eq!(claude.effort.as_deref(), Some("medium"));
    let cli = ExecutorView::parse("cli:codex");
    assert_eq!(cli.harness, "cli");
    assert_eq!(cli.provider, "codex");
    let acp = ExecutorView::parse("acp:claude-code-acp");
    assert_eq!(acp.model, "claude-code-acp");
    let malformed = ExecutorView::parse("gemini-cli:gemini-pro");
    assert_eq!(malformed.harness, "unknown");
    assert_eq!(malformed.display, "gemini-cli:gemini-pro");
    let wrong_modifier = ExecutorView::parse("opencode:model@high");
    assert_eq!(wrong_modifier.harness, "unknown");
    let shell = ExecutorView::shell();
    assert_eq!(shell.harness, "shell");
}

#[test]
fn unknown_states_are_preserved_in_the_view() {
    let status = parse_fleet_status(UNKNOWN.as_bytes()).unwrap().value;
    let fleet = view(&status, None);
    let unknown = fleet
        .lanes
        .parked_blocked
        .iter()
        .find(|card| card.job_id == "unknown-queue-01")
        .unwrap();
    assert_eq!(unknown.state, "quantum");
    assert!(unknown.is_unknown_state());
    assert!(
        fleet
            .lanes
            .ready
            .iter()
            .all(|card| card.job_id != "unknown-queue-01")
    );
    let running = fleet
        .running
        .iter()
        .find(|card| card.job_id == "unknown-running-01")
        .unwrap();
    assert_eq!(running.executor.as_ref().unwrap().harness, "unknown");
}

#[test]
fn freshness_marks_old_snapshots_stale() {
    let status = fresh();
    let later = Timestamp::parse("2026-11-14T12:10:00.000Z").unwrap();
    let fleet = FleetView::from_status(
        HostId("host".to_owned()),
        &status,
        None,
        LatencyStats::default(),
        1,
        later,
    );
    assert!(fleet.stale);
    assert!(fleet.mutations_disabled());
}
