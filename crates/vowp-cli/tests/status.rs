//! `vowp status` integration tests.

mod common;

use vowp_protocol::Timestamp;

/// Writes a heartbeat `age_secs` seconds in the past.
fn write_heartbeat(env: &common::TestEnv, age_secs: i64) {
    let now = Timestamp::now().unix_secs();
    std::fs::write(env.home_path().join("heartbeat"), (now - age_secs).to_string()).expect("heartbeat");
}

#[test]
fn status_check_exit_1_on_stale_heartbeat() {
    let env = common::TestEnv::new();
    write_heartbeat(&env, 600);

    let (code, stdout, stderr) = env.run(&["status", "--check"]);
    assert_eq!(code, 1, "a stale heartbeat must exit 1; stderr: {stderr}");
    assert!(stdout.contains("supervisor: not running"), "{stdout}");
    assert!(stdout.contains("slots: 0/6"), "{stdout}");

    let (code, value, stderr) = env.run_json(&["status", "--check"]);
    assert_eq!(code, 1, "stderr: {stderr}");
    assert_eq!(value["supervisor"]["running"], false);
    assert_eq!(value["supervisor"]["pid"], serde_json::Value::Null);
    assert_eq!(value["supervisor"]["paused"], false);
    let age = value["supervisor"]["heartbeat_age_secs"]
        .as_i64()
        .expect("heartbeat age");
    assert!(age >= 600, "age {age} must include the written 600s");
    assert!(value["supervisor"]["last_tick"].as_str().is_some());
    assert_eq!(value["jobs"], serde_json::json!({}));
    assert_eq!(value["slots"]["busy"], 0);
    assert_eq!(value["slots"]["total"], 6);
    assert_eq!(value["limits"], serde_json::json!([]));
    assert_eq!(value["inbox_open"], 0);
    assert!(value["cost_today_usd"].is_null(), "unknown cost is null, not zero");

    write_heartbeat(&env, 0);
    let (code, value, stderr) = env.run_json(&["status", "--check"]);
    assert_eq!(code, 0, "a fresh heartbeat must pass; stderr: {stderr}");
    let age = value["supervisor"]["heartbeat_age_secs"]
        .as_i64()
        .expect("heartbeat age");
    assert!((0..=5).contains(&age), "fresh age {age}");
}

#[test]
fn status_without_check_never_exits_1_and_reports_running_supervisor() {
    let env = common::TestEnv::new();
    let stale = Timestamp::now().unix_secs() - 900;
    std::fs::write(env.home_path().join("heartbeat"), stale.to_string()).expect("heartbeat");

    let (code, _, stderr) = env.run(&["status"]);
    assert_eq!(code, 0, "status without --check always exits 0; stderr: {stderr}");

    let pid = std::process::id();
    std::fs::write(
        env.home_path().join("supervisor.lock"),
        format!("{pid}\n{}\n", Timestamp::now().as_rfc3339()),
    )
    .expect("lock");
    let (code, value, stderr) = env.run_json(&["status"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(value["supervisor"]["running"], true);
    assert_eq!(value["supervisor"]["pid"], pid);
    assert_eq!(value["supervisor"]["paused"], false);
}

#[test]
fn status_counts_jobs_and_open_inbox_items() {
    let env = common::TestEnv::new();
    env.insert_job("n01-a", "A", vowp_protocol::JobKind::Build, &[]);
    env.insert_job("n02-b", "B", vowp_protocol::JobKind::Build, &[]);
    let store = env.store();
    let now = store.clock().now();
    store
        .insert_inbox(&vowp_protocol::InboxItem {
            id: vowp_protocol::InboxId::new(),
            project: Some(env.config.project.name.clone()),
            job_id: None,
            kind: vowp_protocol::InboxKind::Question,
            title: "need a decision".to_owned(),
            body: "choose".to_owned(),
            options: Vec::new(),
            state: vowp_protocol::InboxState::Open,
            answer: None,
            answered_by: None,
            created_at: now,
            answered_at: None,
        })
        .expect("inbox item");

    let (code, value, stderr) = env.run_json(&["status"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(value["jobs"]["pending"], 2);
    assert_eq!(value["inbox_open"], 1);
}
