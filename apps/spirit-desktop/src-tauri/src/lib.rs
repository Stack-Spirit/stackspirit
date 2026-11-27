//! StackSpirit desktop host: a Tauri 2 shell around the headless `spirit-core` facade.

mod commands;
mod error;
mod events;
mod harnesses;
mod state;

use std::sync::Arc;

use spirit_core::engine::EngineRuntime;
use tauri::{Manager, RunEvent};
use tracing_subscriber::EnvFilter;

use crate::state::AppState;

/// Stops the bundled engine synchronously. Safe to call more than once.
///
/// The app is torn down without running `Drop` for managed state on every exit path
/// (a signal, or Tauri's own `process::exit`), so the sidecar must be stopped explicitly
/// or it is reparented to init and keeps running.
pub fn stop_engine(runtime: &dyn EngineRuntime) {
    runtime.kill_now();
}

/// Waits for `SIGTERM` or `SIGINT`, stops the engine, and returns `true` once it did.
/// Returns `false` immediately where signals cannot be installed.
#[cfg(unix)]
pub async fn shutdown_on_signal(runtime: Arc<dyn EngineRuntime>) -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    static RECEIVED: AtomicBool = AtomicBool::new(false);

    extern "C" fn on_signal(_: i32) {
        // Async-signal-safe: only touches an atomic.
        RECEIVED.store(true, Ordering::SeqCst);
    }
    unsafe extern "C" {
        fn signal(signum: i32, handler: extern "C" fn(i32)) -> usize;
    }
    const SIGINT: i32 = 2;
    const SIGTERM: i32 = 15;
    const SIG_ERR: usize = usize::MAX;

    // SAFETY: `signal` is the libc function; the handler only stores to an atomic.
    let installed =
        unsafe { signal(SIGTERM, on_signal) != SIG_ERR && signal(SIGINT, on_signal) != SIG_ERR };
    if !installed {
        return false;
    }
    while !RECEIVED.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    stop_engine(runtime.as_ref());
    true
}

/// Signals are not wired on non-Unix targets; the exit events still stop the engine.
#[cfg(not(unix))]
pub async fn shutdown_on_signal(_runtime: Arc<dyn EngineRuntime>) -> bool {
    false
}

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
            let runtime = state.engine_runtime();
            app.manage(state);
            app.manage(harnesses::HarnessConnects::default());
            // SIGTERM/SIGINT: stop the sidecar, then leave through Tauri's normal exit.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if shutdown_on_signal(runtime).await {
                    handle.exit(0);
                }
            });
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
            harnesses::harnesses_list,
            harnesses::harnesses_save_custom,
            harnesses::harnesses_delete_custom,
            harnesses::harnesses_probe,
            harnesses::harnesses_connect_start,
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
            commands::fleet::dismiss_inbox,
            commands::fleet::job_action,
            commands::fleet::set_target,
            commands::fleet::set_job_executor,
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
            commands::chat::chat_provision_orchestrator,
            commands::chat::chat_attach,
            commands::chat::chat_send,
            commands::chat::chat_set_model,
            commands::chat::chat_stop,
            commands::chat::chat_reply_permission,
            commands::chat::chat_log,
            commands::asif::gates_get_with_build_flags,
            commands::asif::probe_gates_with_build_flags,
            commands::asif::asif_export,
            commands::asif::asif_import,
            commands::fleet::session_takeover,
            commands::fleet::session_tail,
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
            commands::local_orchestrator::local_orchestrator_move,
            commands::projects::project_add,
            commands::history::project_history,
            commands::history::project_gc_preview,
            commands::history::project_gc_apply,
            commands::files::files_list,
            commands::files::files_preview,
            commands::files::files_mkdir,
            commands::files::files_move,
            commands::files::files_delete,
            commands::files::files_upload,
            commands::files::files_download,
        ])
        .build(tauri::generate_context!())
        .expect("error while building StackSpirit")
        .run(|app, event| {
            // Quit, window close and `app.exit` all pass through one of these.
            if matches!(event, RunEvent::ExitRequested { .. } | RunEvent::Exit) {
                if let Some(connects) = app.try_state::<harnesses::HarnessConnects>() {
                    connects.stop_all();
                }
                if let Some(state) = app.try_state::<AppState>() {
                    stop_engine(state.engine_runtime().as_ref());
                }
            }
        });
}
