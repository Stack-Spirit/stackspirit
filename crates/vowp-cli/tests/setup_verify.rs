//! Tests for `vowp setup verify` fixture checks.
//!
//! Every check runs in a temporary node home with real git and a fake opencode script.
//! Nothing reads the real `~/.vowp`, the default tmux server, the network or a real
//! harness.

use std::path::Path;
use std::process::Command;
use std::sync::Once;

use crate::setup::verify;
use tempfile::TempDir;
use vowp_protocol::{
    CapacitySnapshot, NodeName, SNAPSHOT_FORMAT, SNAPSHOT_VERSION, Snapshot, SupervisorSnapshot, Timestamp,
};

/// Builds the real `vowp` binary once so the chat check can run the same code a user runs.
fn ensure_vowp_binary() {
    static BUILD: Once = Once::new();
    BUILD.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let status = Command::new(cargo)
            .args(["build", "--quiet", "--bin", "vowp"])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .status()
            .expect("run cargo build --bin vowp");
        assert!(status.success(), "cargo build --bin vowp failed");
    });
}

/// Returns a minimal but valid home snapshot.
fn snapshot(home: &str) -> Snapshot {
    let now = Timestamp::now();
    Snapshot {
        schema: SNAPSHOT_FORMAT.to_owned(),
        version: SNAPSHOT_VERSION,
        sequence: 1,
        generated_at: now,
        home: NodeName::parse(home).expect("home name"),
        supervisor: SupervisorSnapshot {
            running: true,
            last_tick: Some(now),
            tick_secs: 60,
            ..SupervisorSnapshot::default()
        },
        projects: Vec::new(),
        capacity: CapacitySnapshot::default(),
        limits: Vec::new(),
        inbox: Vec::new(),
        sessions: Vec::new(),
        orchestrators: Vec::new(),
    }
}

/// Writes the control config and snapshot cache `setup control --apply` leaves behind.
fn write_control_home(root: &Path, home: &str, fetched_at: Timestamp) {
    std::fs::create_dir_all(root).expect("control dir");
    let config = serde_json::json!({
        "role": "control",
        "home": home,
        "ssh_target": "fake.example",
        "remote_home": "/home/vowp/.vowp",
        "created_at": fetched_at,
    });
    std::fs::write(
        root.join("control.json"),
        serde_json::to_vec_pretty(&config).expect("control json"),
    )
    .expect("control.json");
    std::fs::write(
        root.join("snapshot.json"),
        serde_json::to_vec_pretty(&snapshot(home)).expect("snapshot json"),
    )
    .expect("snapshot.json");
    let meta = serde_json::json!({"fetched_at": fetched_at, "source": "remote"});
    std::fs::write(
        root.join("snapshot.meta.json"),
        serde_json::to_vec_pretty(&meta).expect("meta json"),
    )
    .expect("snapshot.meta.json");
}

#[test]
fn setup_verify_fixture_job_merges() {
    let check = verify::fixture_job_merges().expect("fixture job check");
    assert_eq!(check.status, "ok", "{}", check.detail);
    assert_eq!(check.name, "fixture.job_merge");
    assert!(
        check.detail.contains("merged into main"),
        "the check must prove the merge, not only a done state: {}",
        check.detail
    );
}

#[test]
fn setup_verify_snapshot_readable_from_control() {
    let control = TempDir::new().expect("temp control home");
    let fetched_at = Timestamp::now();
    write_control_home(control.path(), "fresh-vps", fetched_at);

    let check = verify::snapshot_readable_from_control(control.path()).expect("snapshot check");
    assert_eq!(check.status, "ok", "{}", check.detail);
    assert_eq!(check.name, "control.snapshot");
    assert!(
        check.detail.contains("snapshot as of"),
        "the check must report the fetch time: {}",
        check.detail
    );
    assert!(
        check.detail.contains("fresh-vps"),
        "the check must name the home: {}",
        check.detail
    );
    assert!(
        check.detail.contains(&fetched_at.as_rfc3339()),
        "the detail must show the cached `as of` time: {}",
        check.detail
    );

    std::fs::remove_file(control.path().join("snapshot.json")).expect("remove snapshot");
    let missing = verify::snapshot_readable_from_control(control.path()).expect("missing check");
    assert_eq!(missing.status, "fail", "{}", missing.detail);
    assert!(
        missing.detail.contains("no readable snapshot cache"),
        "{}",
        missing.detail
    );
}

#[test]
fn setup_verify_snapshot_rejects_a_different_home() {
    let control = TempDir::new().expect("temp control home");
    write_control_home(control.path(), "other-home", Timestamp::now());
    let config = serde_json::json!({
        "role": "control",
        "home": "fresh-vps",
        "ssh_target": "fake.example",
        "remote_home": "/home/vowp/.vowp",
        "created_at": Timestamp::now(),
    });
    std::fs::write(
        control.path().join("control.json"),
        serde_json::to_vec_pretty(&config).expect("control json"),
    )
    .expect("control.json");

    let check = verify::snapshot_readable_from_control(control.path()).expect("snapshot check");
    assert_eq!(check.status, "fail", "{}", check.detail);
    assert!(
        check.detail.contains("other-home") && check.detail.contains("fresh-vps"),
        "{}",
        check.detail
    );
}

#[test]
fn setup_verify_orchestrator_answers_chat() {
    ensure_vowp_binary();
    let check = verify::orchestrator_answers_chat().expect("orchestrator chat check");
    assert_eq!(check.status, "ok", "{}", check.detail);
    assert_eq!(check.name, "orchestrator.chat");
    assert!(
        check.detail.contains("answered through `vowp chat`"),
        "{}",
        check.detail
    );
    assert!(
        check.detail.contains("session "),
        "the check must name the orchestrator session: {}",
        check.detail
    );
}
