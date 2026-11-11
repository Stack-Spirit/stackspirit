//! `vowp capacity` CLI tests: JSON output and the exit-5 owner refusal.

mod common;

#[test]
fn capacity_set_json_writes_the_target() {
    let env = common::TestEnv::new();
    let (code, value, stderr) = env.run_json(&[
        "capacity",
        "set",
        "--target",
        "2",
        "--reason",
        "two workers",
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(value["mode"], "fixed");
    assert_eq!(value["target_workers"], 2);
    assert_eq!(value["reason"], "two workers");
    assert_eq!(value["set_by"], "owner");

    let latest = env
        .store()
        .latest_capacity_target()
        .expect("target query")
        .expect("a target row");
    assert_eq!(latest.target_workers, 2);
    assert_eq!(latest.reason, "two workers");
}

#[test]
fn capacity_set_above_advice_refuses_with_exit_5() {
    let env = common::TestEnv::new();
    let (code, stdout, stderr) = env.run(&["capacity", "set", "--target", "9"]);
    assert_eq!(
        code, 5,
        "an above-advice target must be refused: stdout {stdout}; stderr {stderr}"
    );
    assert!(
        stderr.contains("above the advised"),
        "the refusal names the advice: {stderr}"
    );
    assert!(
        env.store()
            .latest_capacity_target()
            .expect("target query")
            .is_none(),
        "a refused write must not change the target"
    );

    let (code, value, stderr) = env.run_json(&["capacity", "set", "--target", "9", "--override"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(value["target_workers"], 9);
    assert_eq!(value["is_override"], true);
}
