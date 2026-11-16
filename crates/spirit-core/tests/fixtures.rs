//! Loader tests for the feature-gated golden fixtures.

#![cfg(feature = "fixtures")]

use spirit_core::fixtures::{
    FixtureVowpClient, Scenario, capacity_report, engine_catalog, external_harnesses, fleet_inbox,
    fleet_queue, fleet_status, gate_snapshot,
};
use spirit_core::vowp::VowpClient;
use spirit_core::wire::TargetMode;

#[test]
fn every_scenario_loads_its_documents() {
    for scenario in Scenario::ALL {
        if matches!(
            scenario,
            Scenario::EngineIncluded
                | Scenario::EngineNoIncluded
                | Scenario::EngineExternal
                | Scenario::GatePhase3Waiting
                | Scenario::GatePhase3Ready
                | Scenario::GateAsifWaiting
                | Scenario::GateAsifReady
        ) {
            continue;
        }
        let status = fleet_status(scenario).unwrap_or_else(|error| panic!("{scenario:?}: {error}"));
        assert!(!status.snapshot.home.as_str().is_empty());
        let report = capacity_report(scenario).unwrap();
        assert!(report.report.generated_at.unix_secs() > 0);
    }
    assert_eq!(
        Scenario::from_name("fresh-single"),
        Some(Scenario::FreshSingle)
    );
    assert_eq!(Scenario::from_name("not-a-scenario"), None);
}

#[test]
fn fixture_queue_and_inbox_shapes_are_arrays() {
    assert_eq!(fleet_queue(Scenario::FreshSingle).unwrap().len(), 2);
    assert_eq!(fleet_queue(Scenario::QueueCap).unwrap().len(), 200);
    assert_eq!(fleet_inbox().unwrap().len(), 2);
}

#[test]
fn gate_and_engine_fixtures_load() {
    let waiting = gate_snapshot(Scenario::GatePhase3Waiting).unwrap();
    assert!(matches!(
        waiting.local_orchestrator,
        spirit_core::engine::LocalOrchestratorGate::WaitingForVowpPhase3 { .. }
    ));
    assert!(!waiting.features.local_orchestrator_actions);
    assert!(!waiting.local_orchestrator_enabled());

    let local_ready = gate_snapshot(Scenario::GatePhase3Ready).unwrap();
    assert!(local_ready.features.local_orchestrator_actions);
    assert!(local_ready.local_orchestrator_enabled());

    let ready = gate_snapshot(Scenario::GateAsifReady).unwrap();
    assert!(matches!(
        ready.asif,
        spirit_core::engine::AsifGate::Ready {
            export: true,
            import: true,
            ..
        }
    ));
    let (catalog, available) = engine_catalog(Scenario::EngineIncluded).unwrap();
    assert_eq!(catalog.entries.len(), 2);
    assert_eq!(available.len(), 2);
    let (empty, available) = engine_catalog(Scenario::EngineNoIncluded).unwrap();
    assert!(empty.entries.is_empty());
    assert!(available.is_empty());
    assert_eq!(external_harnesses().unwrap().len(), 1);
}

#[tokio::test]
async fn fixture_client_implements_the_contract() {
    let client = FixtureVowpClient::new(Scenario::FreshSingle).unwrap();
    assert_eq!(client.scenario(), Scenario::FreshSingle);
    let status = client.fleet_status().await.unwrap();
    assert_eq!(status.snapshot.projects[0].running.len(), 3);
    let report = client.capacity_report().await.unwrap();
    assert_eq!(report.provider_caps.get("deepseek"), Some(&8));
    let queue = client.fleet_queue().await.unwrap();
    assert_eq!(queue.len(), 2);
    let inbox = client.fleet_inbox(true).await.unwrap();
    assert_eq!(inbox.len(), 2);
    let target = client
        .set_target(9, TargetMode::Fixed, true, "fixture owner")
        .await
        .unwrap();
    assert_eq!(target.target_workers, 9);
    assert!(target.is_override);
    client
        .job_action(
            "vowp",
            "ss-1-core",
            spirit_core::wire::JobAction::Retry,
            None,
        )
        .await
        .unwrap();
}
