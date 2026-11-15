//! Fixture-mode fleet, capacity and gates JSON, filtering and explicit-banner tests.

#![cfg(feature = "fixtures")]

mod common;

use spirit_core::engine::GateSnapshot;
use spirit_core::engine::gate::{AsifGate, LocalOrchestratorGate};
use spirit_core::view::{CapacityView, FleetView};

use common::{empty_env, run_cli};

#[tokio::test]
async fn fixture_fleet_capacity_and_gates_return_core_json() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (env, runner) = empty_env(&directory.path().join("stackspirit"));

    let (result, out, err) = run_cli(&["spirit", "--fixtures", "fleet", "--json"], &env).await;
    assert!(result.is_ok(), "fleet failed: {err}");
    assert!(
        err.contains("fixture data"),
        "fixture mode must print a banner: {err}"
    );
    let fleet: FleetView = serde_json::from_str(&out).expect("fleet view JSON");
    assert_eq!(fleet.revision, 1);
    assert_eq!(fleet.host_id.as_str(), "fixture");
    assert!(!fleet.projects.is_empty());
    assert!(!fleet.running.is_empty());

    let (result, out, err) = run_cli(&["spirit", "--fixtures", "capacity", "--json"], &env).await;
    assert!(result.is_ok(), "capacity failed: {err}");
    assert!(err.contains("fixture data"));
    let capacity: CapacityView = serde_json::from_str(&out).expect("capacity view JSON");
    assert!(capacity.report_loaded);
    assert!(capacity.host.is_some());
    assert!(capacity.target.target_workers > 0);
    assert!(!capacity.limits.is_empty());

    let (result, out, err) = run_cli(&["spirit", "--fixtures", "gates", "--json"], &env).await;
    assert!(result.is_ok(), "gates failed: {err}");
    assert!(err.contains("fixture data"));
    let gates: GateSnapshot = serde_json::from_str(&out).expect("gate snapshot JSON");
    assert!(matches!(
        gates.local_orchestrator,
        LocalOrchestratorGate::WaitingForVowpPhase3 { .. }
    ));
    assert!(matches!(gates.asif, AsifGate::WaitingForVowpAsif { .. }));
    assert!(!gates.features.asif_actions);
    assert!(!gates.features.local_orchestrator_actions);

    assert_eq!(runner.call_count(), 0, "fixtures must never open SSH");
}

#[tokio::test]
async fn fixture_project_filter_keeps_only_that_project() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (env, _runner) = empty_env(&directory.path().join("stackspirit"));

    let (result, out, err) = run_cli(
        &[
            "spirit",
            "--fixtures",
            "--fixture",
            "multi-project",
            "fleet",
            "--project",
            "northnoir",
            "--json",
        ],
        &env,
    )
    .await;
    assert!(result.is_ok(), "filtered fleet failed: {err}");
    let fleet: FleetView = serde_json::from_str(&out).expect("filtered fleet JSON");
    assert!(!fleet.projects.is_empty());
    assert!(
        fleet
            .projects
            .iter()
            .all(|project| project.name == "northnoir")
    );
    assert!(fleet.running.iter().all(|card| card.project == "northnoir"));
    for lane in [
        &fleet.lanes.running,
        &fleet.lanes.ready,
        &fleet.lanes.review_fix,
        &fleet.lanes.parked_blocked,
        &fleet.lanes.done,
    ] {
        assert!(lane.iter().all(|card| card.project == "northnoir"));
    }
    assert!(fleet.sessions.iter().all(|session| {
        session
            .project
            .as_ref()
            .is_some_and(|project| project.as_str() == "northnoir")
    }));
    assert!(fleet.orchestrators.iter().all(|orchestrator| {
        orchestrator
            .project
            .as_ref()
            .is_some_and(|project| project.as_str() == "northnoir")
    }));
}

#[tokio::test]
async fn fixture_project_filter_reports_an_unknown_project() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (env, _runner) = empty_env(&directory.path().join("stackspirit"));

    let (result, out, err) = run_cli(
        &[
            "spirit",
            "--fixtures",
            "--fixture",
            "multi-project",
            "fleet",
            "--project",
            "missing",
            "--json",
        ],
        &env,
    )
    .await;
    let error = result.expect_err("unknown project must fail");
    assert_eq!(error.exit, 4);
    assert_eq!(error.error.code.as_str(), "not_found");
    assert!(out.is_empty());
    assert!(err.contains("not_found"));
}

#[tokio::test]
async fn fixture_gate_ready_scenario_is_reported_honestly() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (env, _runner) = empty_env(&directory.path().join("stackspirit"));

    let (result, out, err) = run_cli(
        &[
            "spirit",
            "--fixtures",
            "--fixture",
            "gate-phase3-ready",
            "gates",
            "--json",
        ],
        &env,
    )
    .await;
    assert!(result.is_ok(), "{err}");
    let gates: GateSnapshot = serde_json::from_str(&out).expect("gate snapshot JSON");
    assert!(matches!(
        gates.local_orchestrator,
        LocalOrchestratorGate::Ready { .. }
    ));
    assert!(!gates.features.local_orchestrator_actions);
}

#[tokio::test]
async fn fixture_mode_requires_the_explicit_flag_and_a_supported_command() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (env, _runner) = empty_env(&directory.path().join("stackspirit"));

    let (result, _out, err) = run_cli(
        &["spirit", "--fixture", "fresh-single", "fleet", "--json"],
        &env,
    )
    .await;
    let error = result.expect_err("--fixture alone must fail");
    assert_eq!(error.exit, 2);
    assert!(err.contains("--fixture requires --fixtures"));

    let (result, _out, err) =
        run_cli(&["spirit", "--fixtures", "host", "ls", "--json"], &env).await;
    let error = result.expect_err("host commands must reject fixture mode");
    assert_eq!(error.exit, 2);
    assert!(err.contains("only available for fleet, capacity and gates"));
}

#[tokio::test]
async fn fixture_scenario_errors_list_the_available_names() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (env, _runner) = empty_env(&directory.path().join("stackspirit"));

    let (result, _out, err) = run_cli(
        &[
            "spirit",
            "--fixtures",
            "--fixture",
            "nope",
            "fleet",
            "--json",
        ],
        &env,
    )
    .await;
    let error = result.expect_err("unknown scenario must fail");
    assert_eq!(error.exit, 2);
    assert!(err.contains("unknown fixture scenario"));
    assert!(err.contains("fresh-single"));
}
