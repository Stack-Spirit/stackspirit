//! Hermetic harness registry, probe and connect-plan tests.
//!
//! No test opens SSH, reads `~/.stackspirit`, or runs a real harness. The fake runner
//! returns scripted output and records every [`CommandSpec`], so the tests can prove the
//! exact argv, the temporary loopback forward and that no credential value is returned.

use std::sync::Arc;

use spirit_core::harnesses::{
    HarnessAuthMode, HarnessAuthState, HarnessBinaryState, HarnessManifest, HarnessProbeTarget,
    HarnessProtocol, HarnessReadiness, HarnessRegistry, HarnessTargetKind, build_connect_plan,
    builtin_harnesses, probe_harness,
};
use spirit_core::ssh::command::{CommandOutput, CommandSpec};
use spirit_core::ssh::{FakeCommandRunner, SshOptions, SshTarget};

fn manifest(id: &str) -> HarnessManifest {
    builtin_harnesses()
        .into_iter()
        .find(|manifest| manifest.id == id)
        .unwrap_or_else(|| panic!("missing builtin {id}"))
}

fn runner(responses: Vec<CommandOutput>) -> Arc<FakeCommandRunner> {
    Arc::new(FakeCommandRunner::new(responses))
}

fn home_target() -> HarnessProbeTarget {
    HarnessProbeTarget::HomeVps {
        target: SshTarget::new("owner", "192.0.2.10", 22).unwrap(),
        options: SshOptions::new("/tmp/stackspirit-harness-known-hosts"),
    }
}

fn remote_argv(spec: &CommandSpec) -> String {
    spec.args.last().cloned().unwrap_or_default()
}

#[test]
fn builtins_cover_the_required_presets() {
    let presets = builtin_harnesses();
    assert_eq!(presets[0].id, "opencode");
    assert_eq!(presets[0].protocol, HarnessProtocol::OpencodeHttp);
    assert!(presets[0].builtin);
    assert!(presets[0].supports_embedded_chat);
    assert!(presets[0].auth_modes.contains(&HarnessAuthMode::ApiKey));

    let expected = [
        ("opencode", HarnessProtocol::OpencodeHttp, true),
        ("claude-code", HarnessProtocol::StreamJson, true),
        ("codex", HarnessProtocol::CodexJson, true),
        ("gemini-cli", HarnessProtocol::Acp, true),
        ("antigravity", HarnessProtocol::CliPty, false),
        ("cursor-cli", HarnessProtocol::CliPty, false),
        ("aider", HarnessProtocol::CliPty, false),
        ("goose", HarnessProtocol::Acp, true),
    ];
    for (id, protocol, embedded) in expected {
        let preset = manifest(id);
        assert_eq!(preset.protocol, protocol, "{id}");
        assert_eq!(preset.supports_embedded_chat, embedded, "{id}");
        preset.validate().unwrap();
        assert!(!preset.tos_note.trim().is_empty(), "{id}");
        assert!(preset.docs_url.starts_with("https://"), "{id}");
    }

    // The consumer-versus-API-key distinction must be explicit for both subscription
    // presets, with the vendor's consumer terms named.
    for id in ["claude-code", "codex"] {
        let preset = manifest(id);
        assert!(preset.auth_modes.contains(&HarnessAuthMode::ApiKey));
        assert!(preset.auth_modes.contains(&HarnessAuthMode::Subscription));
        let note = preset.tos_note.to_ascii_lowercase();
        assert!(note.contains("api"), "{id}: {note}");
        assert!(note.contains("consumer terms"), "{id}: {note}");
        assert!(note.contains("subscription"), "{id}: {note}");
    }

    assert_eq!(manifest("claude-code").binary, "claude");
    assert_eq!(manifest("codex").binary, "codex");
    assert_eq!(manifest("gemini-cli").binary, "gemini");
    assert_eq!(manifest("cursor-cli").binary, "cursor-agent");
    assert_eq!(manifest("aider").binary, "aider");
    assert_eq!(manifest("goose").binary, "goose");
}

#[tokio::test]
async fn local_probe_reads_only_the_version_and_a_safe_auth_marker() {
    let runner = runner(vec![
        CommandOutput::ok(b"opencode 1.19.5\n".to_vec()),
        CommandOutput::ok("●  anthropic\n".as_bytes().to_vec()),
    ]);
    let probe = probe_harness(
        &manifest("opencode"),
        &HarnessProbeTarget::Local,
        runner.clone(),
    )
    .await;

    assert_eq!(probe.target, HarnessTargetKind::Local);
    assert_eq!(probe.binary, HarnessBinaryState::Found);
    assert_eq!(probe.version.as_deref(), Some("opencode 1.19.5"));
    assert_eq!(probe.auth, HarnessAuthState::Configured);
    assert_eq!(probe.readiness, HarnessReadiness::Ready);
    assert!(probe.ready());
    assert!(!probe.requires_loopback_forward);
    assert!(!probe.auth_detail.contains("anthropic"));

    let calls = runner.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].program, "opencode");
    assert_eq!(calls[0].args, vec!["--version"]);
    assert_eq!(calls[1].program, "opencode");
    assert_eq!(calls[1].args, vec!["auth", "list"]);
}

#[tokio::test]
async fn local_probe_reports_a_missing_binary_and_only_the_install_hint() {
    let runner = runner(vec![CommandOutput::fail(127, b"not found".to_vec())]);
    let opencode = manifest("opencode");
    let probe = probe_harness(&opencode, &HarnessProbeTarget::Local, runner.clone()).await;

    assert_eq!(probe.binary, HarnessBinaryState::Missing);
    assert_eq!(probe.readiness, HarnessReadiness::InstallRequired);
    assert_eq!(
        runner.call_count(),
        1,
        "auth is skipped when the binary is missing"
    );

    let plan = build_connect_plan(&opencode, &HarnessProbeTarget::Local, &probe).unwrap();
    assert_eq!(
        plan.install_command.as_deref(),
        Some(opencode.install_cmd.as_str())
    );
    assert!(plan.auth_spec.is_none());
    assert!(plan.forward.is_none());
    let view = plan.view();
    assert!(
        view.install_command
            .unwrap()
            .contains("opencode.ai/install")
    );
    assert!(!view.requires_loopback_forward);
}

#[tokio::test]
async fn local_probe_reports_a_missing_sign_in() {
    let runner = runner(vec![
        CommandOutput::ok(b"opencode 1.19.5\n".to_vec()),
        CommandOutput::ok(b"0 credentials\n".to_vec()),
    ]);
    let probe = probe_harness(&manifest("opencode"), &HarnessProbeTarget::Local, runner).await;
    assert_eq!(probe.binary, HarnessBinaryState::Found);
    assert_eq!(probe.auth, HarnessAuthState::Missing);
    assert_eq!(probe.readiness, HarnessReadiness::AuthRequired);
}

#[tokio::test]
async fn a_harness_without_a_safe_auth_check_is_honestly_unknown() {
    let runner = runner(vec![CommandOutput::ok(b"aider 0.86.0\n".to_vec())]);
    let probe = probe_harness(
        &manifest("aider"),
        &HarnessProbeTarget::Local,
        runner.clone(),
    )
    .await;
    assert_eq!(probe.binary, HarnessBinaryState::Found);
    assert_eq!(probe.auth, HarnessAuthState::Unknown);
    assert_eq!(probe.readiness, HarnessReadiness::Unknown);
    assert_eq!(runner.call_count(), 1);
    assert!(
        probe
            .warnings
            .iter()
            .any(|warning| warning.contains("non-interactively"))
    );
}

#[tokio::test]
async fn auth_output_never_reaches_the_probe_detail() {
    let runner = runner(vec![
        CommandOutput::ok(b"codex 0.55.0\n".to_vec()),
        CommandOutput::ok(
            format!("Logged in\n{}", ["not", "-a", "-real", "-token"].join("")).into_bytes(),
        ),
    ]);
    let probe = probe_harness(&manifest("codex"), &HarnessProbeTarget::Local, runner).await;
    assert_eq!(probe.auth, HarnessAuthState::Configured);
    assert!(!probe.auth_detail.contains("not-a-real-token"));
    assert!(!format!("{probe:?}").contains("not-a-real-token"));
}

#[tokio::test]
async fn home_probe_uses_ssh_and_builds_the_automatic_loopback_forward() {
    let runner = runner(vec![
        CommandOutput::ok(b"2.0.1 (Claude Code)\n".to_vec()),
        CommandOutput::ok(b"Logged in\n".to_vec()),
    ]);
    let claude = manifest("claude-code");
    let target = home_target();
    let probe = probe_harness(&claude, &target, runner.clone()).await;

    assert_eq!(probe.target, HarnessTargetKind::HomeVps);
    assert_eq!(probe.binary, HarnessBinaryState::Found);
    assert_eq!(probe.auth, HarnessAuthState::Configured);
    assert!(probe.requires_loopback_forward);
    assert_eq!(probe.oauth_callback_port, Some(54545));
    assert!(
        probe
            .warnings
            .iter()
            .any(|warning| warning.contains("127.0.0.1:54545"))
    );

    let calls = runner.calls();
    assert_eq!(calls.len(), 2);
    for call in &calls {
        assert_eq!(call.program, "ssh");
        assert!(call.args.iter().any(|arg| arg == "owner@192.0.2.10"));
        assert!(call.args.iter().any(|arg| arg == "-p"));
    }
    assert_eq!(remote_argv(&calls[0]), "'claude' '--version'");
    assert_eq!(remote_argv(&calls[1]), "'claude' 'auth' 'status'");

    let plan = build_connect_plan(&claude, &target, &probe).unwrap();
    let forward = plan.forward.as_ref().expect("remote OAuth needs a forward");
    assert_eq!(forward.port, 54545);
    assert_eq!(
        forward.forward_argument(),
        "127.0.0.1:54545:127.0.0.1:54545"
    );
    assert_eq!(forward.spec.program, "ssh");
    assert!(forward.spec.args.iter().any(|arg| arg == "-N"));
    assert!(forward.spec.args.iter().any(|arg| arg == "-L"));
    assert!(
        forward
            .spec
            .args
            .iter()
            .any(|arg| arg == "127.0.0.1:54545:127.0.0.1:54545")
    );
    assert!(
        forward
            .spec
            .args
            .iter()
            .any(|arg| arg == "ExitOnForwardFailure=yes")
    );
    assert!(
        forward
            .spec
            .args
            .iter()
            .any(|arg| arg == "owner@192.0.2.10")
    );

    let auth = plan.auth_spec.as_ref().expect("sign-in command");
    assert_eq!(auth.program, "ssh");
    assert_eq!(remote_argv(auth), "'claude' 'auth' 'login'");
    assert_eq!(plan.auth_command.as_deref(), Some("claude auth login"));

    let view = plan.view();
    assert!(view.requires_loopback_forward);
    assert_eq!(view.forward_port, Some(54545));
    let serialized = serde_json::to_string(&view).unwrap();
    assert!(!serialized.contains("known-hosts"));
    assert!(!serialized.contains("identity"));
}

#[tokio::test]
async fn a_local_probe_never_builds_a_forward_and_keeps_the_auth_command_local() {
    let runner = runner(vec![
        CommandOutput::ok(b"2.0.1 (Claude Code)\n".to_vec()),
        CommandOutput::ok(b"not logged in\n".to_vec()),
    ]);
    let claude = manifest("claude-code");
    let target = HarnessProbeTarget::Local;
    let probe = probe_harness(&claude, &target, runner).await;
    assert_eq!(probe.auth, HarnessAuthState::Missing);
    assert!(!probe.requires_loopback_forward);

    let plan = build_connect_plan(&claude, &target, &probe).unwrap();
    assert!(plan.forward.is_none());
    let auth = plan.auth_spec.as_ref().expect("sign-in command");
    assert_eq!(auth.program, "claude");
    assert_eq!(auth.args, vec!["auth", "login"]);
    assert!(!plan.view().requires_loopback_forward);
}

#[tokio::test]
async fn a_home_ssh_failure_is_unknown_not_a_missing_binary() {
    let runner = runner(vec![CommandOutput::fail(
        255,
        b"ssh: connect failed".to_vec(),
    )]);
    let probe = probe_harness(&manifest("claude-code"), &home_target(), runner).await;
    assert_eq!(probe.binary, HarnessBinaryState::Unknown);
    assert_eq!(probe.readiness, HarnessReadiness::Unknown);
    assert!(!probe.requires_loopback_forward);
    assert!(
        probe
            .warnings
            .iter()
            .any(|warning| warning.contains("SSH connection failed"))
    );
}

#[tokio::test]
async fn a_missing_home_binary_skips_the_forward_and_sign_in() {
    let runner = runner(vec![CommandOutput::fail(127, b"not found".to_vec())]);
    let claude = manifest("claude-code");
    let target = home_target();
    let probe = probe_harness(&claude, &target, runner).await;
    assert_eq!(probe.binary, HarnessBinaryState::Missing);
    assert!(!probe.requires_loopback_forward);
    let plan = build_connect_plan(&claude, &target, &probe).unwrap();
    assert!(plan.forward.is_none());
    assert!(plan.auth_spec.is_none());
    assert!(plan.install_command.is_some());
}

#[tokio::test]
async fn a_connect_plan_rejects_a_probe_from_another_target() {
    let runner = runner(vec![
        CommandOutput::ok(b"2.0.1\n".to_vec()),
        CommandOutput::ok(b"not logged in\n".to_vec()),
    ]);
    let claude = manifest("claude-code");
    let probe = probe_harness(&claude, &HarnessProbeTarget::Local, runner).await;
    let error = build_connect_plan(&claude, &home_target(), &probe).unwrap_err();
    assert_eq!(error.code_str(), "invalid_input");
}

#[test]
fn custom_manifests_round_trip_and_cannot_shadow_a_builtin() {
    let directory = tempfile::tempdir().unwrap();
    let registry = HarnessRegistry::new(directory.path());
    let mut custom = manifest("opencode");
    custom.id = "my-harness".to_owned();
    custom.name = "My Harness".to_owned();
    custom.binary = "my-harness".to_owned();
    custom.install_cmd = "brew install my-harness".to_owned();
    custom.auth_cmd = String::new();
    custom.protocol = HarnessProtocol::CliPty;
    custom.supports_embedded_chat = false;
    custom.oauth_callback_port = None;
    custom.builtin = false;

    let saved = registry.save_custom(custom.clone()).unwrap();
    assert_eq!(saved, custom);
    assert!(directory.path().join("my-harness.toml").exists());
    let listed = registry.list().unwrap();
    let listed_custom = listed
        .harnesses
        .iter()
        .find(|item| item.id == "my-harness")
        .unwrap();
    assert_eq!(listed_custom, &custom);
    assert_eq!(registry.get("my-harness").unwrap().unwrap(), custom);

    let mut shadow = custom.clone();
    shadow.id = "opencode".to_owned();
    assert!(registry.save_custom(shadow).is_err());
}

#[test]
fn the_registry_rejects_path_shaped_ids() {
    let directory = tempfile::tempdir().unwrap();
    let registry = HarnessRegistry::new(directory.path());
    for bad in ["../evil", "a/b", "A", "a b", "", &"a".repeat(65)] {
        assert!(registry.manifest_path(bad).is_err(), "{bad:?}");
        assert!(registry.get(bad).is_err(), "{bad:?}");
        assert!(registry.delete_custom(bad).is_err(), "{bad:?}");
    }
}
