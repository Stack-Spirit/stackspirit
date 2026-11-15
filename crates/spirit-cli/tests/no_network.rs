//! Every local and fixture command must leave the injected runner untouched.

mod common;

use common::{empty_env, run_cli};

#[tokio::test]
async fn local_and_fixture_commands_never_call_the_command_runner() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (env, runner) = empty_env(&directory.path().join("stackspirit"));

    #[cfg_attr(not(feature = "fixtures"), allow(unused_mut))]
    let mut commands: Vec<&[&str]> = vec![
        &["spirit", "version", "--json"],
        &["spirit", "host", "ls", "--json"],
        &[
            "spirit",
            "host",
            "add",
            "--label",
            "demo",
            "--ssh",
            "dev@example.com",
            "--fingerprint",
            "SHA256:abcdefghijklmnopqrstuvwxyz123456",
            "--json",
        ],
        &["spirit", "engine", "status", "--json"],
    ];
    #[cfg(feature = "fixtures")]
    {
        commands.push(&["spirit", "--fixtures", "fleet", "--json"]);
        commands.push(&["spirit", "--fixtures", "capacity", "--json"]);
        commands.push(&["spirit", "--fixtures", "gates", "--json"]);
        commands.push(&[
            "spirit",
            "--fixtures",
            "--fixture",
            "multi-project",
            "fleet",
            "--project",
            "northnoir",
            "--json",
        ]);
    }

    for args in &commands {
        let (result, _out, err) = run_cli(args, &env).await;
        assert!(result.is_ok(), "{args:?} failed: {err}");
        assert_eq!(
            runner.call_count(),
            0,
            "{args:?} must not run ssh or any local command"
        );
    }
}

#[tokio::test]
async fn a_live_command_without_fixtures_fails_instead_of_falling_back() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (env, runner) = empty_env(&directory.path().join("stackspirit"));

    let (result, out, err) = run_cli(&["spirit", "fleet", "--json"], &env).await;
    let error = result.expect_err("no host must fail");
    assert_eq!(error.exit, 4);
    assert_eq!(error.error.code.as_str(), "host_not_found");
    assert!(out.is_empty());
    assert!(err.contains("no host is saved"));
    assert_eq!(
        runner.call_count(),
        0,
        "resolution must not open a connection"
    );
}
