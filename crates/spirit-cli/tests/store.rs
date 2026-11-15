//! CLI host-store tests: versioned round trip, mode 0600, temp isolation and trust.

mod common;

use std::path::Path;
use std::sync::Arc;

use spirit_cli::CliPaths;
use spirit_core::host::HostProfile;
use spirit_core::ssh::FakeCommandRunner;
use spirit_core::ssh::command::CommandOutput;

use common::{
    FINGERPRINT, PUBLIC_KEY, add_host, empty_env, env_with_runner, keygen_output, keyscan_output,
    run_cli,
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
async fn host_store_round_trip_is_versioned_and_private() {
    let directory = tempfile::tempdir().expect("temp dir");
    let root = directory.path().join("stackspirit");
    let (env, runner) = empty_env(&root);

    let host = add_host(&env, "demo", "dev@example.com:2222").await;
    assert_eq!(host.ssh.port, 2222);

    let hosts_file = root.join("hosts.json");
    assert!(hosts_file.starts_with(directory.path()));
    assert!(hosts_file.exists());
    assert_eq!(
        runner.call_count(),
        0,
        "adding a host must not touch the network"
    );

    #[cfg(unix)]
    {
        let mode = std::fs::metadata(&hosts_file)
            .expect("hosts metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "the host store must be mode 0600");
        let root_mode = std::fs::metadata(&root)
            .expect("root metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(root_mode, 0o700, "the config directory must be mode 0700");
    }

    let raw = std::fs::read_to_string(&hosts_file).expect("host store text");
    let value: serde_json::Value = serde_json::from_str(&raw).expect("host store JSON");
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["hosts"].as_array().expect("hosts array").len(), 1);
    assert_eq!(value["hosts"][0]["id"], host.id.as_str());
    assert_eq!(value["hosts"][0]["host_key"]["sha256"], FINGERPRINT);

    let lower = raw.to_ascii_lowercase();
    for forbidden in [
        concat!("api", "_key"),
        concat!("api", "key"),
        concat!("tok", "en"),
        concat!("sec", "ret"),
        concat!("private", " key"),
        concat!("be", "gin "),
    ] {
        assert!(
            !lower.contains(forbidden),
            "host store unexpectedly contains {forbidden:?}: {raw}"
        );
    }

    let (result, out, err) = run_cli(&["spirit", "host", "ls", "--json"], &env).await;
    assert!(result.is_ok(), "{err}");
    let listed: Vec<HostProfile> = serde_json::from_str(&out).expect("host list JSON");
    assert_eq!(listed, vec![host.clone()]);

    let (result, out, err) = run_cli(
        &["spirit", "host", "show", host.id.as_str(), "--json"],
        &env,
    )
    .await;
    assert!(result.is_ok(), "{err}");
    let shown: HostProfile = serde_json::from_str(&out).expect("host show JSON");
    assert_eq!(shown, host);

    let (result, out, err) = run_cli(
        &["spirit", "host", "remove", host.id.as_str(), "--json"],
        &env,
    )
    .await;
    assert!(result.is_ok(), "{err}");
    let removed: serde_json::Value = serde_json::from_str(&out).expect("remove JSON");
    assert_eq!(removed["removed"], true);
    assert_eq!(removed["id"], host.id.as_str());

    let (result, out, err) = run_cli(&["spirit", "host", "ls", "--json"], &env).await;
    assert!(result.is_ok(), "{err}");
    let listed: Vec<HostProfile> = serde_json::from_str(&out).expect("empty host list JSON");
    assert!(listed.is_empty());
    #[cfg(unix)]
    assert_eq!(
        std::fs::metadata(&hosts_file)
            .expect("hosts metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn config_root_uses_xdg_home_and_never_a_relative_xdg_value() {
    let directory = tempfile::tempdir().expect("temp dir");
    let xdg = directory.path().join("xdg");
    let home = directory.path().join("home");

    let xdg_paths = CliPaths::resolve(Some(&xdg), Some(&home)).expect("xdg paths");
    assert_eq!(xdg_paths.root(), xdg.join("stackspirit"));

    let home_paths = CliPaths::resolve(None, Some(&home)).expect("home paths");
    assert_eq!(home_paths.root(), home.join(".config").join("stackspirit"));

    let relative_paths =
        CliPaths::resolve(Some(Path::new("relative")), Some(&home)).expect("relative fallback");
    assert_eq!(
        relative_paths.root(),
        home.join(".config").join("stackspirit")
    );

    let error = CliPaths::resolve(None, None).expect_err("no home must fail");
    assert_eq!(error.exit, 5);
}

#[tokio::test]
async fn trust_scans_verifies_and_installs_a_private_known_hosts_entry() {
    let directory = tempfile::tempdir().expect("temp dir");
    let root = directory.path().join("stackspirit");
    let runner = Arc::new(FakeCommandRunner::new(vec![
        CommandOutput::ok(keyscan_output()),
        CommandOutput::ok(keygen_output()),
    ]));
    let env = env_with_runner(&root, runner);
    let host = add_host(&env, "demo", "dev@example.com").await;

    std::fs::create_dir_all(&root).expect("config dir");
    std::fs::write(
        root.join("known_hosts"),
        "other.example ssh-rsa AAAAB3NzaOtherKey\n",
    )
    .expect("seed known_hosts");

    let (result, out, err) = run_cli(
        &[
            "spirit",
            "host",
            "trust",
            host.id.as_str(),
            "--fingerprint",
            FINGERPRINT,
            "--json",
        ],
        &env,
    )
    .await;
    assert!(result.is_ok(), "trust failed: {err}");

    let updated: HostProfile = serde_json::from_str(&out).expect("trusted profile JSON");
    assert_eq!(updated.host_key.sha256, FINGERPRINT);

    let known_hosts = root.join("known_hosts");
    let text = std::fs::read_to_string(&known_hosts).expect("known_hosts text");
    assert!(text.contains(PUBLIC_KEY));
    assert!(text.contains("other.example"));
    assert!(text.contains("example.com ssh-ed25519"));
    #[cfg(unix)]
    assert_eq!(
        std::fs::metadata(&known_hosts)
            .expect("known_hosts metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[tokio::test]
async fn trust_rejects_a_fingerprint_that_does_not_match_the_scan() {
    let directory = tempfile::tempdir().expect("temp dir");
    let root = directory.path().join("stackspirit");
    let runner = Arc::new(FakeCommandRunner::new(vec![
        CommandOutput::ok(keyscan_output()),
        CommandOutput::ok("256 SHA256:zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz example.com (ED25519)\n"),
    ]));
    let env = env_with_runner(&root, runner);
    let host = add_host(&env, "demo", "dev@example.com").await;

    let (result, _out, err) = run_cli(
        &[
            "spirit",
            "host",
            "trust",
            host.id.as_str(),
            "--fingerprint",
            FINGERPRINT,
            "--json",
        ],
        &env,
    )
    .await;
    let error = result.expect_err("mismatched pin must fail");
    assert_eq!(error.exit, 3);
    assert_eq!(error.error.code.as_str(), "host_key_mismatch");
    assert!(err.contains("host_key_mismatch"));
    assert!(!root.join("known_hosts").exists());
}
