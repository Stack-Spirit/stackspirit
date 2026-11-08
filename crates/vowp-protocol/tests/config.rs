//! Configuration parsing, defaults, validation and node-file merging.
//! Spec: `docs/design/types.md` §11, `docs/design/architecture.md` §5 and
//! `docs/design/crates/vowp-protocol.md` §6.

use pretty_assertions::assert_eq;

use vowp_protocol::config::{Approval, CapsuleConfig, MergeConfig, NodeConfig, NodeFile, ProjectConfig, VowpConfig};
use vowp_protocol::{Executor, ProtocolError};

/// A minimal valid project configuration; tests append tables to it.
const BASE: &str = r#"
[project]
name = "vowp"

[profiles.planner]
ladder = ["claude-code:claude-sonnet-5-5@medium"]

[profiles.keeper]
ladder = ["opencode:deepseek/deepseek-flash#max"]

[profiles.worker]
ladder = ["opencode:deepseek/deepseek-flash#max"]

[profiles.verifier]
ladder = ["opencode:deepseek/deepseek-flash#max"]
"#;

/// Parses a configuration, panicking with the error on failure.
fn parse(s: &str) -> VowpConfig {
    VowpConfig::from_toml(s).unwrap_or_else(|err| panic!("config should parse: {err}\n{s}"))
}

/// Asserts that a configuration is rejected as a config error.
fn reject(s: &str) {
    match VowpConfig::from_toml(s) {
        Ok(_) => panic!("config should be rejected:\n{s}"),
        Err(err) => assert!(
            matches!(err, ProtocolError::Config(_)),
            "expected a Config error, got {err:?}"
        ),
    }
}

#[test]
fn config_example_parses() {
    let config = parse(include_str!("fixtures/vowp.example.toml"));
    config.validate().expect("the canonical example must validate");

    assert_eq!(config.profiles["planner"].ladder.len(), 2);
    assert_eq!(
        config.profiles["planner"].ladder[0].to_string(),
        "claude-code:claude-sonnet-5-5@medium"
    );
    assert_eq!(
        config.profiles["planner"].ladder[1].to_string(),
        "opencode:deepseek/deepseek-flash#max"
    );

    assert_eq!(config.providers["deepseek"].max_concurrent, 6);
    assert!(config.providers["claude"].prices.is_empty());

    let schedule_names: Vec<&str> = config.schedules.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(schedule_names, ["morning-report", "nightly-backup"]);
    assert_eq!(config.schedules[0].cron.as_deref(), Some("0 7 * * 1-5"));
    assert_eq!(config.schedules[0].tz.as_deref(), Some("+02:00"));

    assert_eq!(config.env["work"].approval, Approval::Human);
    assert_eq!(config.node.name.as_str(), "vps");
    assert_eq!(config.node.tz_offset_minutes, 120);
}

#[test]
fn config_rejects_unknown_fields_and_bad_values() {
    parse(BASE); // the baseline must be valid, or the cases below prove nothing.

    // Rule 1: missing role profile, empty ladder, duplicate rung, invalid executor.
    reject(
        r#"
[project]
name = "vowp"
[profiles.planner]
ladder = ["claude-code:claude-sonnet-5-5@medium"]
[profiles.keeper]
ladder = ["opencode:deepseek/deepseek-flash#max"]
[profiles.worker]
ladder = ["opencode:deepseek/deepseek-flash#max"]
"#,
    );
    reject(&BASE.replace("ladder = [\"opencode:deepseek/deepseek-flash#max\"]", "ladder = []"));
    reject(&BASE.replace(
        "ladder = [\"claude-code:claude-sonnet-5-5@medium\"]",
        "ladder = [\"claude-code:claude-sonnet-5-5@medium\", \"claude-code:claude-sonnet-5-5@medium\"]",
    ));
    reject(&BASE.replace(
        "ladder = [\"claude-code:claude-sonnet-5-5@medium\"]",
        "ladder = [\"not-an-executor\"]",
    ));

    // Rule 2: a provider referenced by a rung has zero concurrency.
    reject(&format!("{BASE}\n[providers.deepseek]\nmax_concurrent = 0\n"));

    // Rule 3: cron/every exclusivity, cron field count, every pattern and minimum, tz, names.
    reject(&format!(
        "{BASE}\n[[schedules]]\nname = \"both\"\ncron = \"0 7 * * *\"\nevery = \"1h\"\naction = {{ kind = \"loop\", loop_name = \"l4-planner\" }}\n"
    ));
    reject(&format!(
        "{BASE}\n[[schedules]]\nname = \"short-cron\"\ncron = \"0 7 * *\"\naction = {{ kind = \"loop\", loop_name = \"l4-planner\" }}\n"
    ));
    reject(&format!(
        "{BASE}\n[[schedules]]\nname = \"too-fast\"\nevery = \"30s\"\naction = {{ kind = \"loop\", loop_name = \"l4-planner\" }}\n"
    ));
    reject(&format!(
        "{BASE}\n[[schedules]]\nname = \"weekly\"\nevery = \"1w\"\naction = {{ kind = \"loop\", loop_name = \"l4-planner\" }}\n"
    ));
    reject(&format!(
        "{BASE}\n[[schedules]]\nname = \"bad-tz\"\ncron = \"0 7 * * *\"\ntz = \"Europe/Oslo\"\naction = {{ kind = \"loop\", loop_name = \"l4-planner\" }}\n"
    ));
    reject(&format!(
        "{BASE}\n[[schedules]]\nname = \"same\"\ncron = \"0 7 * * *\"\naction = {{ kind = \"loop\", loop_name = \"l4-planner\" }}\n[[schedules]]\nname = \"same\"\ncron = \"0 8 * * *\"\naction = {{ kind = \"loop\", loop_name = \"l4-planner\" }}\n"
    ));

    // Rule 4: environment checkout and policy approval.
    reject(&format!("{BASE}\n[env.work]\ncheckout = \"\"\n"));
    reject(&format!(
        "{BASE}\n[env.work]\ncheckout = \"~/work\"\napproval = \"policy\"\n"
    ));

    // Rule 5: gate validation and file-existence anti-patterns.
    reject(&format!(
        "{BASE}\n[gates]\nbad = [{{ cmd = {{ run = \"\", expect_exit = 0 }} }}]\n"
    ));
    reject(&format!(
        "{BASE}\n[gates]\nbad = [{{ cmd = {{ run = \"true\", expect_pattern = \"(\" }} }}]\n"
    ));
    reject(&format!(
        "{BASE}\n[gates]\nbad = [{{ cmd = {{ run = \"test -f Cargo.toml\" }} }}]\n"
    ));
    reject(&format!(
        "{BASE}\n[gates]\nbad = [{{ script = {{ run = \"ls -la\" }} }}]\n"
    ));
    reject(&format!(
        "{BASE}\n[gates]\nbad = [{{ deploy = {{ run = \"[ -f target/release/vowp ]\", probe = {{ run = \"true\" }} }} }}]\n"
    ));
    reject(&format!(
        "{BASE}\n[gates]\nbad = [{{ deploy = {{ run = \"true\", probe = {{ run = \"test -f target/release/vowp\" }} }} }}]\n"
    ));

    // Rule 6: loop names.
    reject(&format!("{BASE}\n[loops.l10-nope]\ninterval_secs = 5\n"));

    // Rule 7: always-allowed globs must compile.
    reject(&BASE.replace(
        "[project]\nname = \"vowp\"",
        "[project]\nname = \"vowp\"\nalways_allowed = [\"[\"]",
    ));

    // Rule 8: unknown keys, including the flattened loops table.
    reject(&format!("{BASE}\n[node]\nnot_a_key = 1\n"));
    reject(&format!("{BASE}\n[loops.l4-planner]\nnot_a_key = 1\n"));
    reject(&format!(
        "{BASE}\n[[schedules]]\nname = \"extra\"\ncron = \"0 7 * * *\"\nnot_a_key = 1\naction = {{ kind = \"loop\", loop_name = \"l4-planner\" }}\n"
    ));

    // Rule 9: node counts and tick.
    reject(&format!("{BASE}\n[node]\nmax_workers = 0\n"));
    reject(&format!("{BASE}\n[node]\nmax_verifiers = 0\n"));
    reject(&format!("{BASE}\n[node]\ntick_secs = 0\n"));
}

#[test]
fn config_defaults_are_documented_values() {
    let node = NodeConfig::default();
    assert_eq!(node.name.as_str(), "local");
    assert_eq!(node.max_workers, 4);
    assert_eq!(node.max_verifiers, 2);
    assert_eq!(node.tick_secs, 60);
    assert_eq!(node.tz_offset_minutes, 0);
    assert!(node.cwd_allow.is_empty());
    assert_eq!(node.home, None);

    let merge = MergeConfig::default();
    assert!(merge.rebase);
    assert_eq!(merge.timeout_secs, 2700);
    assert!(merge.secret_scan);
    assert!(merge.check_cmds.is_empty());

    let capsule = CapsuleConfig::default();
    assert_eq!(capsule.executor, None);
    assert_eq!(capsule.min_interval_secs, 120);
    assert_eq!(capsule.max_input_chars, 200_000);
    assert_eq!(capsule.tail_turns, 2);
}

#[test]
fn project_defaults_are_documented_values() {
    let config = parse(BASE);
    let expected = ProjectConfig {
        name: config.project.name.clone(),
        path: None,
        remote: "origin".to_owned(),
        default_branch: "main".to_owned(),
        check_cmds: Vec::new(),
        lock_files: vec!["Cargo.lock".to_owned()],
        worktree_root: None,
        always_allowed: vec!["Cargo.lock".to_owned()],
        push: true,
    };
    assert_eq!(config.project, expected);
    assert_eq!(config.merge, MergeConfig::default());
    assert_eq!(config.capsule, CapsuleConfig::default());
}

#[test]
fn example_matches_architecture_md() {
    let docs = include_str!("../../../docs/design/architecture.md");
    let section = docs
        .split_once("### 5.2 Annotated example")
        .expect("architecture.md must document §5.2")
        .1;
    let after_fence = section
        .split_once("```toml\n")
        .expect("§5.2 must contain a toml block")
        .1;
    let block = after_fence
        .split_once("\n```")
        .expect("the toml block must be closed")
        .0;
    assert_eq!(
        format!("{block}\n"),
        include_str!("fixtures/vowp.example.toml"),
        "the fixture must be the canonical §5.2 block byte for byte"
    );
}

#[test]
fn node_file_parses_node_wide_subset() {
    let node = NodeFile::from_toml(
        r#"
[node]
name = "vps"
max_workers = 8
tz_offset_minutes = 120

[profiles.keeper]
ladder = ["opencode:deepseek/deepseek-flash#max"]

[providers.deepseek]
max_concurrent = 6

[harnesses.opencode]
extra_args = ["--auto"]

[capsule]
tail_turns = 4
"#,
    )
    .expect("node config parses");

    assert_eq!(node.node.name.as_str(), "vps");
    assert_eq!(node.node.max_workers, 8);
    assert_eq!(node.node.tz_offset_minutes, 120);
    assert_eq!(node.profiles["keeper"].ladder.len(), 1);
    assert_eq!(node.providers["deepseek"].max_concurrent, 6);
    assert_eq!(node.harnesses["opencode"].extra_args, ["--auto"]);
    assert_eq!(node.capsule.tail_turns, 4);

    assert_eq!(NodeFile::from_toml("").expect("empty node file"), NodeFile::default());
    assert!(
        NodeFile::from_toml("[project]\nname = \"vowp\"\n").is_err(),
        "the node file has no [project] table"
    );
    assert!(
        NodeFile::from_toml("[node]\nnot_a_key = 1\n").is_err(),
        "unknown node keys are errors"
    );
}

#[test]
fn with_node_defaults_fills_missing_entries_and_scalars() {
    let project = parse(&format!("{BASE}\n[node]\nname = \"project-node\"\nmax_workers = 8\n"));
    let node = NodeFile::from_toml(
        r#"
[node]
name = "vps"
max_workers = 2
max_verifiers = 3
tick_secs = 30
cwd_allow = ["/tmp/extra"]
home = "/tmp/vowp-home"
tz_offset_minutes = 120

[profiles.planner]
ladder = ["cli:node-planner"]

[profiles.worker]
ladder = ["cli:node-worker"]

[providers.deepseek]
max_concurrent = 6

[harnesses.opencode]
extra_args = ["--node"]

[capsule]
executor = "opencode:deepseek/deepseek-flash#max"
min_interval_secs = 60
max_input_chars = 1000
tail_turns = 4
"#,
    )
    .expect("node config parses");

    let merged = project.clone().with_node_defaults(&node);
    merged.validate().expect("the merged config must validate");

    // Explicit non-default project values win.
    assert_eq!(merged.node.name.as_str(), "project-node");
    assert_eq!(merged.node.max_workers, 8);
    // Omitted project values come from the node file.
    assert_eq!(merged.node.max_verifiers, 3);
    assert_eq!(merged.node.tick_secs, 30);
    assert_eq!(merged.node.cwd_allow, ["/tmp/extra"]);
    assert_eq!(merged.node.home.as_deref(), Some("/tmp/vowp-home"));
    assert_eq!(merged.node.tz_offset_minutes, 120);

    // Project entries win; missing entries are filled from the node file.
    assert_eq!(
        merged.profiles["planner"].ladder[0].to_string(),
        "claude-code:claude-sonnet-5-5@medium"
    );
    assert_eq!(
        merged.profiles["worker"].ladder[0].to_string(),
        "opencode:deepseek/deepseek-flash#max"
    );
    assert_eq!(merged.providers["deepseek"].max_concurrent, 6);
    assert_eq!(merged.harnesses["opencode"].extra_args, ["--node"]);

    // Capsule fields are filled field by field.
    assert_eq!(
        merged.capsule.executor.as_ref().map(Executor::to_string).as_deref(),
        Some("opencode:deepseek/deepseek-flash#max")
    );
    assert_eq!(merged.capsule.min_interval_secs, 60);
    assert_eq!(merged.capsule.max_input_chars, 1000);
    assert_eq!(merged.capsule.tail_turns, 4);
}

#[test]
fn with_node_defaults_keeps_explicit_capsule_fields() {
    let project = parse(&format!("{BASE}\n[capsule]\nmin_interval_secs = 999\ntail_turns = 9\n"));
    let node = NodeFile::from_toml(
        r#"
[capsule]
executor = "opencode:deepseek/deepseek-flash#max"
min_interval_secs = 60
max_input_chars = 1000
tail_turns = 4
"#,
    )
    .expect("node config parses");

    let merged = project.with_node_defaults(&node);
    assert_eq!(merged.capsule.min_interval_secs, 999);
    assert_eq!(merged.capsule.tail_turns, 9);
    assert_eq!(merged.capsule.max_input_chars, 1000);
    assert_eq!(
        merged.capsule.executor.as_ref().map(Executor::to_string).as_deref(),
        Some("opencode:deepseek/deepseek-flash#max")
    );
}

#[test]
fn with_node_defaults_supplies_all_role_profiles() {
    let mut project = parse(BASE);
    project.profiles.retain(|name, _| name == "worker");
    let node = NodeFile::from_toml(
        r#"
[profiles.planner]
ladder = ["cli:node-planner"]

[profiles.keeper]
ladder = ["cli:node-keeper"]

[profiles.verifier]
ladder = ["cli:node-verifier"]
"#,
    )
    .expect("node config parses");

    let merged = project.with_node_defaults(&node);
    merged.validate().expect("node profiles complete the project config");
    assert_eq!(merged.profiles.len(), 4);
    assert_eq!(merged.profiles["planner"].ladder[0].to_string(), "cli:node-planner");
    assert_eq!(merged.profiles["verifier"].ladder[0].to_string(), "cli:node-verifier");
}

#[test]
fn with_defaults_for_tests_is_valid() {
    let config = VowpConfig::with_defaults_for_tests();
    config.validate().expect("the test defaults must validate");
    assert_eq!(config.project.name.as_str(), "test");
    assert_eq!(config.profiles.len(), 4);
}
