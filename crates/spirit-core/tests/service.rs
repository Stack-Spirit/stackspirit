//! The headless `SpiritCore` facade: hosts, connection, engine and gate wiring.

use std::sync::Arc;

use spirit_core::engine::{
    EngineSupervisor, EngineSupervisorConfig, FakeEngineRuntime, GateSnapshot,
};
use spirit_core::host::{HostDraft, HostKeyPin, InMemoryHostStore, NewHostProfile};
use spirit_core::service::SpiritCore;
use spirit_core::ssh::SshTarget;
use spirit_core::time::Timestamp;
use spirit_core::vowp::{MockVowpClient, VowpClient};
use spirit_core::wire::{TargetMode, parse_capacity_report, parse_fleet_status};

const FRESH: &str = include_str!("../fixtures/vowp/fleet-status-fresh.json");
const CAPACITY: &str = include_str!("../fixtures/vowp/capacity-report-full.json");

fn supervisor() -> EngineSupervisor {
    EngineSupervisor::new(
        Arc::new(FakeEngineRuntime::new()),
        EngineSupervisorConfig {
            restart_budget: 3,
            health_attempts: 1,
            retry_delay: std::time::Duration::ZERO,
            port: 0,
        },
    )
}

fn host_input() -> NewHostProfile {
    NewHostProfile {
        draft: HostDraft {
            label: "primary-vps".to_owned(),
            ssh: SshTarget::new("owner", "192.0.2.10", 22).unwrap(),
            identity_file: None,
            vowp_home: None,
            default_project: Some("vowp".to_owned()),
        },
        host_key: HostKeyPin {
            algorithm: "ssh-ed25519".to_owned(),
            sha256: "SHA256:AbCdEfGhIjKlMnOpQrStUvWxYz0123456789ABCDEFG".to_owned(),
            pinned_at: Timestamp::epoch(),
        },
    }
}

#[tokio::test]
async fn spirit_core_wires_hosts_connection_engine_and_gates_without_tauri() {
    let store = Arc::new(InMemoryHostStore::new());
    let mut core = SpiritCore::new(store.clone(), supervisor());
    let host = core.add_host(host_input()).unwrap();
    assert_eq!(core.list_hosts().unwrap().len(), 1);
    assert_eq!(
        core.host_store().get(&host.id).unwrap().unwrap().label,
        "primary-vps"
    );

    let status = parse_fleet_status(FRESH.as_bytes()).unwrap().value;
    let report = parse_capacity_report(CAPACITY.as_bytes()).unwrap().value;
    let client = MockVowpClient::new(status, report);
    let connected = core.connect(host.id.clone(), &client).await.unwrap();
    assert_eq!(connected.state, "connected");
    let view = core.fleet_view().unwrap();
    assert!(core.mutations_allowed() == !view.stale);
    assert!(core.refresh_capacity(&client).await.unwrap().is_some());
    let target = client
        .set_target(9, TargetMode::Fixed, true, "owner")
        .await
        .unwrap();
    assert_eq!(target.target_workers, 9);
    core.disconnect();
    assert!(!core.mutations_allowed());

    let status = core.start_engine().await;
    assert!(matches!(
        status.state,
        spirit_core::engine::EngineState::Ready
    ));
    let status = core.stop_engine().await;
    assert!(matches!(
        status.state,
        spirit_core::engine::EngineState::Stopped
    ));

    let secret = spirit_core::SecretString::new("not-a-real-key");
    let key = core.store_key("deepseek", &secret).unwrap();
    assert_eq!(key.state, "saved");
    assert_eq!(core.key_status().unwrap().len(), 1);
    assert_eq!(core.key_status().unwrap()[0].provider, "deepseek");
    core.delete_key("deepseek").unwrap();
    assert!(core.key_status().unwrap().is_empty());

    let gates = GateSnapshot::default();
    core.set_gates(gates.clone());
    assert_eq!(core.gates(), gates);
    let mut receiver = core.subscribe();
    core.set_gates(GateSnapshot::default());
    let event = receiver.try_recv().unwrap();
    assert!(event.revision() > 0);
    let _ = spirit_core::ConnectionManager::default();
}
