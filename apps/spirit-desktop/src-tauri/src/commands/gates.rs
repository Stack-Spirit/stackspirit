//! Capability gate reads and read-only probes. D2 never turns a gate Ready without
//! evidence from core's capability probes.

use spirit_core::engine::gate::{
    GateProbeOptions, GateSnapshot, probe_asif, probe_local_orchestrator,
};
use spirit_core::time::Timestamp;
use tauri::State;

use crate::error::CommandResult;
use crate::state::AppState;

#[tauri::command]
pub async fn gates_get(state: State<'_, AppState>) -> CommandResult<GateSnapshot> {
    Ok(state.core().await.gates())
}

#[tauri::command]
pub async fn probe_gates(state: State<'_, AppState>) -> CommandResult<GateSnapshot> {
    let Some(host) = state.active_host() else {
        return Ok(state.core().await.gates());
    };

    let manager = state.connection_manager().await;
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
    let runner = state.runner();
    let (local_orchestrator, local_agent) =
        probe_local_orchestrator(runner.as_ref(), None, &options).await;
    let asif = probe_asif(runner.as_ref(), &options).await;

    let features = state.core().await.gates().features;
    let snapshot = GateSnapshot {
        probed_at: Timestamp::now(),
        local_orchestrator,
        asif,
        local_agent,
        features,
    };
    state.core().await.set_gates(snapshot.clone());
    Ok(snapshot)
}
