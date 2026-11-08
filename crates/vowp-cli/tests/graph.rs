//! `vowp graph` integration tests: golden renderings and the YAML round trip.

mod common;

use vowp_protocol::JobKind;

#[test]
fn graph_ascii_and_mermaid_golden() {
    let env = common::TestEnv::new();
    env.insert_job("a-job", "Alpha", JobKind::Build, &[]);
    env.insert_job("b-job", "Beta", JobKind::Build, &["a-job"]);

    let (code, stdout, stderr) = env.run(&["graph"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "L0 a-job [pending] Alpha\nL1 b-job [pending] Beta  <- a-job\n");

    let (code, stdout, stderr) = env.run(&["graph", "--mermaid"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        stdout,
        "graph TD\n    a_job[\"a-job (pending)\"]\n    b_job[\"b-job (pending)\"]\n    a_job --> b_job\n"
    );
}

#[test]
fn graph_export_import_roundtrip() {
    let env = common::TestEnv::new();
    env.insert_job("first", "First job", JobKind::Build, &[]);
    env.insert_job("second", "Second job", JobKind::Verify, &["first"]);
    let first_export = env.project_path().join(".vowp/graph-1.yaml");
    let second_export = env.project_path().join(".vowp/graph-2.yaml");

    let (code, _, stderr) = env.run(&["graph", "--export", first_export.to_str().expect("path")]);
    assert_eq!(code, 0, "{stderr}");
    let exported = std::fs::read_to_string(&first_export).expect("export file");
    assert!(exported.contains("version: 1"), "{exported}");
    assert!(exported.contains("    needs: [first]"), "{exported}");

    let store = env.store();
    for id in ["first", "second"] {
        store
            .delete_job(&vowp_protocol::JobId::parse(id).expect("slug"))
            .expect("delete pending job");
    }
    assert!(
        store
            .list_jobs(&vowp_store::JobFilter::default())
            .expect("jobs")
            .is_empty()
    );

    let (code, value, stderr) = env.run_json(&["graph", "--import", first_export.to_str().expect("path")]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(value["imported"].as_array().expect("imported").len(), 2);
    assert_eq!(value["conflicts"].as_array().expect("conflicts").len(), 0);

    let (code, _, stderr) = env.run(&["graph", "--export", second_export.to_str().expect("path")]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        std::fs::read_to_string(&first_export).expect("first export"),
        std::fs::read_to_string(&second_export).expect("second export")
    );

    // Importing the same file again is a conflict, exit 4, and adds nothing.
    let (code, value, _) = env.run_json(&["graph", "--import", first_export.to_str().expect("path")]);
    assert_eq!(code, 4);
    assert_eq!(value["imported"].as_array().expect("imported").len(), 0);
    assert_eq!(value["conflicts"].as_array().expect("conflicts").len(), 2);
}

#[test]
fn graph_mermaid_has_every_edge_kind() {
    let env = common::TestEnv::new();
    env.insert_job("base", "Base", JobKind::Build, &[]);
    env.insert_job("child", "Child", JobKind::Build, &["base"]);
    let store = env.store();
    store
        .add_edge(&vowp_protocol::Edge {
            src: vowp_protocol::JobId::parse("child").expect("slug"),
            dst: vowp_protocol::JobId::parse("base").expect("slug"),
            kind: vowp_protocol::EdgeKind::Verifies,
        })
        .expect("verifies edge");
    let (code, stdout, stderr) = env.run(&["graph", "--mermaid"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("child -.->|verifies| base"), "{stdout}");
}
