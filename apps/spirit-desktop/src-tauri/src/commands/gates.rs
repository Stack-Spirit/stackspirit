//! Capability gate reads and read-only probes. A gate never turns Ready without
//! evidence from core's capability probes.

use spirit_core::engine::GateFeatureFlags;
use spirit_core::engine::gate::{
    AsifGate, GateProbeOptions, GateSnapshot, LocalOrchestratorGate, probe_asif,
    probe_local_orchestrator,
};
use spirit_core::ssh::SystemCommandRunner;
use spirit_core::time::Timestamp;
use tauri::State;

use crate::error::CommandResult;
use crate::state::AppState;

fn disconnected_snapshot() -> GateSnapshot {
    GateSnapshot::default()
}

#[tauri::command]
pub async fn gates_get(state: State<'_, AppState>) -> CommandResult<GateSnapshot> {
    let connection = state.connection_manager().await.status();
    if connection.state != "connected" {
        // No host, or a failed/closed connection: never return another host's Ready.
        state.gate_cache().clear();
        let snapshot = disconnected_snapshot();
        state.core().await.set_gates(snapshot.clone());
        return Ok(snapshot);
    }
    let Some(host_id) = connection.host_id else {
        state.gate_cache().clear();
        let snapshot = disconnected_snapshot();
        state.core().await.set_gates(snapshot.clone());
        return Ok(snapshot);
    };

    if let Some(cached) = state.gate_cache().get(host_id.as_str(), Timestamp::now()) {
        return Ok(cached);
    }
    Ok(disconnected_snapshot())
}

#[tauri::command]
pub async fn probe_gates(state: State<'_, AppState>) -> CommandResult<GateSnapshot> {
    let manager = state.connection_manager().await;
    let connection = manager.status();
    if connection.state != "connected" {
        state.gate_cache().clear();
        let snapshot = disconnected_snapshot();
        state.core().await.set_gates(snapshot.clone());
        return Ok(snapshot);
    }
    let Some(host_id) = connection.host_id else {
        state.gate_cache().clear();
        let snapshot = disconnected_snapshot();
        state.core().await.set_gates(snapshot.clone());
        return Ok(snapshot);
    };

    let host = state
        .hosts()
        .get(&host_id)?
        .ok_or_else(|| spirit_core::error::SpiritError::not_found("host profile"))?;
    let active_project = manager.fleet_view().and_then(|view| {
        view.projects
            .iter()
            .find(|project| project.state == "active")
            .map(|project| project.name.clone())
    });

    let options = GateProbeOptions {
        target: host.ssh.clone(),
        ssh: state.ssh_options(host.identity_file.clone()),
        home: host.vowp_home.clone(),
        active_project,
    };

    let probing = GateSnapshot {
        probed_at: Timestamp::now(),
        local_orchestrator: LocalOrchestratorGate::Probing,
        asif: AsifGate::Probing,
        local_agent: state.core().await.gates().local_agent,
        features: GateFeatureFlags::default(),
    };
    state.gate_cache().put(host_id.as_str(), probing.clone());
    state.core().await.set_gates(probing);

    let runner = state.runner();
    let local_runner = SystemCommandRunner;
    let (local_orchestrator, local_agent) =
        probe_local_orchestrator(runner.as_ref(), Some(&local_runner), &options).await;
    let asif = probe_asif(runner.as_ref(), &options).await;

    // D4 deliberately leaves both action flags false. D5/D6 flip exactly one flag when
    // the corresponding action ships, never before the probe says Ready.
    let snapshot = GateSnapshot {
        probed_at: Timestamp::now(),
        local_orchestrator,
        asif,
        local_agent,
        features: GateFeatureFlags::default(),
    };

    // The host may have disconnected or changed while the probe ran. In that case the
    // result belongs to no visible connection and must not be cached or displayed.
    let current = state.connection_manager().await.status();
    if current.state != "connected" || current.host_id.as_ref() != Some(&host_id) {
        state.gate_cache().invalidate(host_id.as_str());
        let stale = disconnected_snapshot();
        state.core().await.set_gates(stale.clone());
        return Ok(stale);
    }

    state.gate_cache().put(host_id.as_str(), snapshot.clone());
    state.core().await.set_gates(snapshot.clone());
    Ok(snapshot)
}
