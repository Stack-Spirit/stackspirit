//! Regression: a `SIGTERM` must stop the bundled sidecar instead of orphaning it.
//!
//! Found by the ss-88 Xvfb smoke: the desktop app installed no signal handler, so the
//! `opencode serve` child was reparented to init and kept running. This test runs the real
//! pinned sidecar through the same `shutdown_on_signal` the app installs.
#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use spirit_core::engine::{
    EngineRuntime, ProcessEngineRuntime, current_target_triple, sidecar_file_name,
};

fn pid_alive(pid: u32) -> bool {
    // A zombie still answers `kill -0`; read the state instead.
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|stat| stat.rsplit(')').next().map(|rest| rest.trim().to_owned()))
        .is_some_and(|rest| !rest.starts_with('Z'))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sigterm_stops_the_sidecar() {
    let triple = current_target_triple().expect("supported target");
    let binary = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(sidecar_file_name(triple));
    assert!(
        binary.is_file(),
        "pinned sidecar missing: run `node scripts/fetch-engine.mjs`"
    );
    let app_data = tempfile::tempdir().expect("tempdir");
    let runtime: Arc<dyn EngineRuntime> = Arc::new(
        ProcessEngineRuntime::with_binary(binary).with_private_state(app_data.path().to_path_buf()),
    );
    let info = runtime.spawn(0).await.expect("spawn sidecar");
    let pid = info.pid.expect("sidecar pid");
    assert!(pid_alive(pid), "sidecar should be running");

    let waiter = tokio::spawn(spirit_desktop_lib::shutdown_on_signal(runtime.clone()));
    // Let the handler register before the signal arrives.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let status = std::process::Command::new("kill")
        .args(["-TERM", &std::process::id().to_string()])
        .status()
        .expect("send SIGTERM to this test process");
    assert!(status.success());

    let handled = tokio::time::timeout(Duration::from_secs(10), waiter)
        .await
        .expect("handler must fire")
        .expect("task");
    assert!(handled, "signal handler was not installed");
    for _ in 0..40 {
        if !pid_alive(pid) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("sidecar {pid} survived SIGTERM");
}
