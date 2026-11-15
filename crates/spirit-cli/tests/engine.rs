//! `engine status` is read-only and never carries a provider key.

mod common;

use common::{empty_env, run_cli};

#[tokio::test]
async fn engine_status_reads_preferences_without_starting_anything() {
    let directory = tempfile::tempdir().expect("temp dir");
    let root = directory.path().join("stackspirit");
    std::fs::create_dir_all(&root).expect("config dir");
    std::fs::write(
        root.join("engine.json"),
        br#"{
            "version": 1,
            "choice": {"type": "bundled"},
            "selected_model_id": "opencode/big-pickle",
            "onboarding_complete": true
        }"#,
    )
    .expect("engine preferences");

    let (env, runner) = empty_env(&root);
    let (result, out, err) = run_cli(&["spirit", "engine", "status", "--json"], &env).await;
    assert!(result.is_ok(), "{err}");
    assert_eq!(
        runner.call_count(),
        0,
        "engine status must not run processes"
    );

    let value: serde_json::Value = serde_json::from_str(&out).expect("engine status JSON");
    assert_eq!(value["read_only"], true);
    assert_eq!(value["state"]["state"], "stopped");
    assert_eq!(value["choice"]["type"], "bundled");
    assert_eq!(
        value["preferences"]["selected_model_id"],
        "opencode/big-pickle"
    );
    assert_eq!(value["keychain"]["checked"], false);
    assert!(value["port"].is_null());

    let lower = out.to_ascii_lowercase();
    for forbidden in [
        concat!("api", "_key"),
        concat!("api", "key"),
        concat!("s", "k-"),
        concat!("private", " key"),
    ] {
        assert!(
            !lower.contains(forbidden),
            "engine status unexpectedly contains {forbidden:?}: {out}"
        );
    }
}

#[tokio::test]
async fn engine_status_reports_a_corrupt_preferences_file() {
    let directory = tempfile::tempdir().expect("temp dir");
    let root = directory.path().join("stackspirit");
    std::fs::create_dir_all(&root).expect("config dir");
    std::fs::write(root.join("engine.json"), b"{not json").expect("corrupt preferences");

    let (env, _runner) = empty_env(&root);
    let (result, out, _err) = run_cli(&["spirit", "engine", "status", "--json"], &env).await;
    let error = result.expect_err("corrupt preferences must fail");
    assert_eq!(error.exit, 5);
    assert_eq!(error.error.code.as_str(), "parse");
    assert!(out.is_empty());
}
