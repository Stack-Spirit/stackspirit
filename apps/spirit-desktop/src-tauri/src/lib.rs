//! StackSpirit desktop host: a Tauri 2 shell around the headless `spirit-core` facade.

mod commands;
mod error;
mod events;
mod state;

use tauri::Manager;
use tracing_subscriber::EnvFilter;

use crate::state::AppState;

/// Builds and runs the desktop application.
pub fn run() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let (state, events) = AppState::new(app_data_dir)?;
            events::spawn_event_forwarder(app.handle().clone(), events);
            events::spawn_chat_event_forwarder(app.handle().clone(), state.chat().subscribe());
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::hosts::list_hosts,
            commands::hosts::add_host,
            commands::hosts::update_host,
            commands::hosts::remove_host,
            commands::hosts::scan_fingerprint,
            commands::hosts::trust_fingerprint,
            commands::hosts::probe_host,
            commands::onboarding::onboarding_key_status,
            commands::onboarding::onboarding_create_key,
            commands::onboarding::onboarding_install_key,
            commands::secrets::secrets_list,
            commands::secrets::secrets_set,
            commands::secrets::secrets_remove,
            commands::connection::connect_host,
            commands::connection::disconnect_host,
            commands::connection::connection_status,
            commands::fleet::fleet_get,
            commands::fleet::fleet_refresh,
            commands::fleet::fleet_raw_document,
            commands::fleet::fleet_inbox,
            commands::fleet::answer_inbox,
            commands::fleet::job_action,
            commands::fleet::set_target,
            commands::cluster::cluster_form,
            commands::cluster::cluster_health,
            commands::cluster::cluster_promote,
            commands::cluster::cluster_backup_policy,
            commands::cluster::cluster_remove_worker,
            commands::engine::engine_status,
            commands::engine::engine_models,
            commands::engine::select_model,
            commands::engine::store_key,
            commands::engine::verify_key,
            commands::engine::delete_key,
            commands::engine::key_status,
            commands::engine::scan_external,
            commands::engine::select_external,
            commands::engine::use_bundled,
            commands::engine::restart_engine,
            commands::chat::chat_start,
            commands::chat::chat_attach,
            commands::chat::chat_send,
            commands::chat::chat_stop,
            commands::chat::chat_log,
            commands::asif::gates_get_with_build_flags,
            commands::asif::probe_gates_with_build_flags,
            commands::asif::asif_export,
            commands::asif::asif_import,
            commands::local_orchestrator::local_orchestrator_overview,
            commands::local_orchestrator::local_agent_status,
            commands::local_orchestrator::local_agent_pair,
            commands::local_orchestrator::local_agent_paths,
            commands::local_orchestrator::local_agent_path_add,
            commands::local_orchestrator::local_agent_path_remove,
            commands::local_orchestrator::local_surface_pair,
            commands::local_orchestrator::local_surface_list,
            commands::local_orchestrator::local_surface_rotate,
            commands::local_orchestrator::local_surface_revoke,
            commands::local_orchestrator::local_orchestrator_new,
            commands::local_orchestrator::local_orchestrator_list,
            commands::local_orchestrator::local_orchestrator_stop,
            commands::local_orchestrator::local_orchestrator_resume,
        ])
        .run(tauri::generate_context!())
        .expect("error while running StackSpirit");
}
