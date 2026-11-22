//! Active-versus-history lane separation (ss-108).

use serde_json::{Value, json};
use spirit_core::connection::LatencyStats;
use spirit_core::host::HostId;
use spirit_core::time::Timestamp;
use spirit_core::view::FleetView;
use spirit_core::wire::{FleetStatus, parse_fleet_status};

const FRESH: &str = include_str!("../fixtures/vowp/fleet-status-fresh.json");

fn view_of(mutate: impl FnOnce(&mut Value)) -> FleetView {
    let mut doc: Value = serde_json::from_str(FRESH).unwrap();
    mutate(&mut doc["snapshot"]["projects"][0]);
    let status: FleetStatus = parse_fleet_status(doc.to_string().as_bytes())
        .unwrap()
        .value;
    FleetView::from_status(
        HostId("01JCF5WQ8ZK3M2V7N4P6R8T0AB".to_owned()),
        &status,
        None,
        LatencyStats::default(),
        1,
        Timestamp::parse("2026-11-14T12:00:10.000Z").unwrap(),
    )
}

fn failure(job: &str, at: &str) -> Value {
    json!({"job": job, "gate": "secret_scan", "at": at, "evidence": "redacted"})
}

#[test]
fn historical_gate_failures_never_create_unknown_review_rows() {
    let fleet = view_of(|project| {
        project["gate_failures"] = json!([
            failure("ss-99-project-add-bridge", "2026-11-14T10:00:00.000Z"),
            failure("ss-99-project-add-bridge", "2026-11-14T10:05:00.000Z"),
            failure("ss-99-project-add-bridge", "2026-11-14T10:10:00.000Z"),
            failure("ss-98-gone", "2026-11-14T09:00:00.000Z"),
        ]);
    });
    assert!(
        fleet
            .lanes
            .review_fix
            .iter()
            .all(|card| card.kind != "unknown" && card.role != "unknown"),
        "no unknown · unknown rows in the active review lane: {:?}",
        fleet.lanes.review_fix
    );
    assert!(
        !fleet
            .lanes
            .review_fix
            .iter()
            .any(|card| card.job_id.starts_with("ss-99") || card.job_id == "ss-98-gone")
    );
    let history: Vec<&str> = fleet
        .lanes
        .history
        .iter()
        .map(|card| card.job_id.as_str())
        .collect();
    assert_eq!(
        history
            .iter()
            .filter(|id| **id == "ss-99-project-add-bridge")
            .count(),
        1,
        "historical failures are de-duplicated by job id: {history:?}"
    );
    assert!(history.contains(&"ss-98-gone"));
}

#[test]
fn live_review_jobs_appear_once_enriched_from_the_queue() {
    let fleet = view_of(|project| {
        project["queue"].as_array_mut().unwrap().push(json!({
            "job": "ss-9-gate-check", "title": "Gate check", "kind": "build", "role": "worker",
            "priority": 50, "state": "gating", "age_secs": 5, "waiting_reason": null
        }));
    });
    let rows: Vec<_> = fleet
        .lanes
        .review_fix
        .iter()
        .filter(|card| card.job_id == "ss-9-gate-check")
        .collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, "build");
    assert_eq!(rows[0].gate.as_deref(), Some("secret_scan"));
}

#[test]
fn cancelled_jobs_are_history_not_needs_attention() {
    let fleet = view_of(|project| {
        project["jobs_by_state"]["cancelled"] = json!(2);
        project["queue"].as_array_mut().unwrap().push(json!({
            "job": "ss-50-cancelled", "title": "Old idea", "kind": "build", "role": "worker",
            "priority": 1, "state": "cancelled", "age_secs": 9, "waiting_reason": null
        }));
    });
    assert!(
        !fleet
            .lanes
            .parked_blocked
            .iter()
            .any(|card| card.job_id == "ss-50-cancelled" || card.state == "cancelled")
    );
    assert!(
        fleet
            .lanes
            .history
            .iter()
            .any(|c| c.job_id == "ss-50-cancelled")
    );
    assert_eq!(fleet.lanes.counts.get("parked_blocked"), Some(&1));
    assert_eq!(fleet.lanes.counts.get("cancelled"), Some(&2));
}

#[test]
fn done_count_comes_from_jobs_by_state() {
    let fleet = view_of(|_| {});
    assert_eq!(fleet.lanes.counts.get("done"), Some(&37));
    assert_eq!(fleet.projects[0].jobs_by_state.get("done"), Some(&37));
}
