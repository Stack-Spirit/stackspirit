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

#[test]
fn gate_failure_of_a_running_job_is_live_review_not_history() {
    let fleet = view_of(|project| {
        project["gate_failures"] = json!([
            failure("ss-2-desktop-shell", "2026-11-14T10:00:00.000Z"),
            failure("ss-98-gone", "2026-11-14T09:00:00.000Z"),
        ]);
    });
    let live: Vec<_> = fleet
        .lanes
        .review_fix
        .iter()
        .filter(|card| card.job_id == "ss-2-desktop-shell")
        .collect();
    assert_eq!(live.len(), 1, "{:?}", fleet.lanes.review_fix);
    assert_eq!(live[0].kind, "build");
    assert_eq!(live[0].role, "worker");
    assert_eq!(live[0].gate.as_deref(), Some("secret_scan"));
    assert!(
        !fleet
            .lanes
            .running
            .iter()
            .any(|card| card.job_id == "ss-2-desktop-shell"),
        "a job lives in at most one active lane: {:?}",
        fleet.lanes.running
    );
    let history: Vec<&str> = fleet
        .lanes
        .history
        .iter()
        .map(|c| c.job_id.as_str())
        .collect();
    assert!(!history.contains(&"ss-2-desktop-shell"), "{history:?}");
    assert!(history.contains(&"ss-98-gone"));
}

#[test]
fn last_merge_reaches_the_project_view() {
    let fleet = view_of(|project| {
        project["last_merge"] = json!({
            "job": "ss-4-merged", "at": "2026-11-14T10:00:00.000Z",
            "commit": "0123456789abcdef", "branch": "job/ss-4-merged"
        });
    });
    let merge = fleet.projects[0].last_merge.as_ref().expect("merge");
    assert_eq!(merge.job_id, "ss-4-merged");
    assert_eq!(merge.branch, "job/ss-4-merged");
}

fn failure_with(job: &str, gate: &str, at: &str, evidence: &str) -> Value {
    json!({"job": job, "gate": gate, "at": at, "evidence": evidence})
}

#[test]
fn live_running_job_uses_the_newest_gate_failure() {
    // VOWP emits newest first.
    let fleet = view_of(|project| {
        project["gate_failures"] = json!([
            failure_with(
                "ss-2-desktop-shell",
                "review",
                "2026-11-14T10:10:00.000Z",
                "new evidence"
            ),
            failure_with(
                "ss-2-desktop-shell",
                "secret_scan",
                "2026-11-14T10:00:00.000Z",
                "old evidence"
            ),
        ]);
    });
    let live: Vec<_> = fleet
        .lanes
        .review_fix
        .iter()
        .filter(|card| card.job_id == "ss-2-desktop-shell")
        .collect();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].gate.as_deref(), Some("review"));
    assert_eq!(live[0].gate_evidence.as_deref(), Some("new evidence"));
    assert_eq!(
        live[0].gate_at,
        Some(Timestamp::parse("2026-11-14T10:10:00.000Z").unwrap())
    );
    assert!(
        !fleet
            .lanes
            .running
            .iter()
            .any(|card| card.job_id == "ss-2-desktop-shell")
    );
}

#[test]
fn history_uses_the_newest_gate_failure_in_any_input_order() {
    for newest_first in [true, false] {
        let fleet = view_of(|project| {
            let new = failure_with(
                "ss-98-gone",
                "review",
                "2026-11-14T09:30:00.000Z",
                "new evidence",
            );
            let old = failure_with(
                "ss-98-gone",
                "secret_scan",
                "2026-11-14T09:00:00.000Z",
                "old evidence",
            );
            project["gate_failures"] = if newest_first {
                json!([new, old])
            } else {
                json!([old, new])
            };
        });
        let rows: Vec<_> = fleet
            .lanes
            .history
            .iter()
            .filter(|card| card.job_id == "ss-98-gone")
            .collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].gate.as_deref(), Some("review"));
        assert_eq!(rows[0].gate_evidence.as_deref(), Some("new evidence"));
        assert_eq!(
            rows[0].gate_at,
            Some(Timestamp::parse("2026-11-14T09:30:00.000Z").unwrap())
        );
    }
}

fn queue_row(job: &str, kind: &str, state: &str, extra: Value) -> Value {
    let mut row = json!({
        "job": job, "title": job, "kind": kind, "role": "worker",
        "priority": 50, "state": state, "age_secs": 5, "waiting_reason": null
    });
    if let (Some(row), Some(extra)) = (row.as_object_mut(), extra.as_object()) {
        row.extend(extra.clone());
    }
    row
}

fn ready_card<'a>(fleet: &'a FleetView, job: &str) -> &'a spirit_core::view::JobCardView {
    fleet
        .lanes
        .ready
        .iter()
        .find(|card| card.job_id == job)
        .unwrap_or_else(|| panic!("{job} not in ready lane"))
}

#[test]
fn queue_needs_deserialize_onto_job_cards() {
    let fleet = view_of(|project| {
        project["queue"].as_array_mut().unwrap().push(queue_row(
            "ss-118",
            "build",
            "pending",
            json!({"needs": ["ss-116b", "ss-117"]}),
        ));
    });
    assert_eq!(ready_card(&fleet, "ss-118").needs, ["ss-116b", "ss-117"]);
}

#[test]
fn queue_rows_without_needs_stay_compatible() {
    // The fresh fixture rows carry no `needs` key at all.
    let fleet = view_of(|_| {});
    assert!(!fleet.lanes.ready.is_empty());
    assert!(fleet.lanes.ready.iter().all(|card| card.needs.is_empty()));
}

#[test]
fn verify_and_fix_children_chain_behind_their_parents() {
    let fleet = view_of(|project| {
        let queue = project["queue"].as_array_mut().unwrap();
        queue.push(queue_row("ss-116b", "build", "ready", json!({})));
        queue.push(queue_row(
            "ss-116b-verify-0",
            "verify",
            "pending",
            json!({}),
        ));
        queue.push(queue_row("ss-116b-fix-1", "fix", "pending", json!({})));
        queue.push(queue_row(
            "ss-116b-verify-1",
            "verify",
            "pending",
            json!({}),
        ));
        queue.push(queue_row(
            "ss-9-verify-0",
            "verify",
            "pending",
            json!({"needs": ["explicit"]}),
        ));
    });
    assert!(ready_card(&fleet, "ss-116b").needs.is_empty());
    assert_eq!(ready_card(&fleet, "ss-116b-verify-0").needs, ["ss-116b"]);
    assert_eq!(
        ready_card(&fleet, "ss-116b-fix-1").needs,
        ["ss-116b-verify-0"]
    );
    assert_eq!(
        ready_card(&fleet, "ss-116b-verify-1").needs,
        ["ss-116b-fix-1"]
    );
    assert_eq!(ready_card(&fleet, "ss-9-verify-0").needs, ["explicit"]);
}

#[test]
fn planned_executor_reaches_queued_cards() {
    let fleet = view_of(|project| {
        project["queue"].as_array_mut().unwrap().push(queue_row(
            "ss-200",
            "build",
            "pending",
            json!({"planned_executor": "opencode:deepseek/deepseek-v4-pro"}),
        ));
    });
    let planned = ready_card(&fleet, "ss-200")
        .planned_executor
        .as_ref()
        .expect("planned executor");
    assert_eq!(planned.harness, "opencode");
    assert_eq!(planned.provider, "deepseek");
}

#[test]
fn queue_rows_without_planned_executor_stay_compatible() {
    let fleet = view_of(|_| {});
    assert!(!fleet.lanes.ready.is_empty());
    assert!(
        fleet
            .lanes
            .ready
            .iter()
            .all(|card| card.planned_executor.is_none())
    );
}

#[test]
fn running_usage_and_billing_fields_deserialize_and_default_to_none() {
    let old = view_of(|_| {});
    assert!(!old.lanes.running.is_empty());
    assert!(old.lanes.running.iter().all(|card| {
        card.tokens_input.is_none()
            && card.context_window.is_none()
            && card.cost_usd.is_none()
            && card.billing_mode.is_none()
    }));

    let fleet = view_of(|project| {
        let running = project["running"][0].as_object_mut().unwrap();
        running.insert("tokens_input".into(), json!(100_000));
        running.insert("tokens_output".into(), json!(42_000));
        running.insert("tokens_cache_read".into(), json!(9_000));
        running.insert("context_window".into(), json!(200_000));
        running.insert("cost_usd".into(), json!(0.14));
        running.insert("billing_mode".into(), json!("api"));
    });
    let card = fleet
        .lanes
        .running
        .iter()
        .chain(&fleet.lanes.review_fix)
        .find(|card| card.tokens_input.is_some())
        .expect("usage card");
    assert_eq!(card.tokens_output, Some(42_000));
    assert_eq!(card.tokens_cache_read, Some(9_000));
    assert_eq!(card.context_window, Some(200_000));
    assert_eq!(card.cost_usd, Some(0.14));
    assert_eq!(card.billing_mode.as_deref(), Some("api"));
}
