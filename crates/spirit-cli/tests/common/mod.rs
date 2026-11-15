#![allow(dead_code)]

use std::path::Path;
use std::sync::Arc;

use clap::Parser;
use spirit_cli::{Cli, CliEnv, CliError, CliPaths};
use spirit_core::host::HostProfile;
use spirit_core::ssh::{CommandRunner, FakeCommandRunner};

pub const FINGERPRINT: &str = "SHA256:abcdefghijklmnopqrstuvwxyz123456";
pub const PUBLIC_KEY: &str = "AAAAC3NzaC1lZDI1NTE5AAAAITestKeyTestKeyTestKeyTestKey";

pub async fn run_cli(args: &[&str], env: &CliEnv) -> (Result<(), CliError>, String, String) {
    let cli = Cli::try_parse_from(args).unwrap_or_else(|error| panic!("{args:?}: {error}"));
    let mut out = Vec::new();
    let mut err = Vec::new();
    let result = spirit_cli::run(&cli, env, &mut out, &mut err).await;
    if let Err(error) = &result {
        error
            .report(cli.json, &mut err)
            .expect("error report is writable");
    }
    (
        result,
        String::from_utf8(out).expect("stdout is UTF-8"),
        String::from_utf8(err).expect("stderr is UTF-8"),
    )
}

pub fn empty_env(root: &Path) -> (CliEnv, Arc<FakeCommandRunner>) {
    let runner = Arc::new(FakeCommandRunner::new(Vec::new()));
    (env_with_runner(root, runner.clone()), runner)
}

pub fn env_with_runner(root: &Path, runner: Arc<FakeCommandRunner>) -> CliEnv {
    let runner: Arc<dyn CommandRunner> = runner;
    CliEnv::system(CliPaths::for_root(root))
        .with_runner(runner.clone())
        .with_local_runner(runner)
}

pub async fn add_host(env: &CliEnv, label: &str, ssh: &str) -> HostProfile {
    let (result, out, err) = run_cli(
        &[
            "spirit",
            "host",
            "add",
            "--label",
            label,
            "--ssh",
            ssh,
            "--fingerprint",
            FINGERPRINT,
            "--json",
        ],
        env,
    )
    .await;
    assert!(result.is_ok(), "add failed: {err}");
    serde_json::from_str(&out).expect("host profile JSON")
}

pub fn keyscan_output() -> String {
    format!("example.com ssh-ed25519 {PUBLIC_KEY}\n")
}

pub fn keygen_output() -> String {
    format!("256 {FINGERPRINT} example.com (ED25519)\n")
}

pub fn fleet_status_json() -> Vec<u8> {
    include_str!("../../../spirit-core/fixtures/vowp/fleet-status-fresh.json")
        .as_bytes()
        .to_vec()
}

pub fn error_code(result: &Result<(), CliError>) -> &str {
    match result {
        Ok(()) => "ok",
        Err(error) => error.error.code.as_str(),
    }
}
