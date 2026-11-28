//! Hermetic harness registry, probe and connect-plan tests.
//!
//! No test opens SSH, reads `~/.stackspirit`, or runs a real harness. The fake runner
//! returns scripted output and records every [`CommandSpec`], so the tests can prove the
//! exact argv, the temporary loopback forward and that no credential value is returned.

use std::sync::Arc;

use serde_json::{Value, json};
use spirit_core::harnesses::{
    AcpClient, AcpPermissionDecision, CollectingObserver, HarnessAuthMode, HarnessAuthState,
    HarnessBinaryState, HarnessManifest, HarnessProbeTarget, HarnessProtocol, HarnessReadiness,
    HarnessRegistry, HarnessSurface, HarnessTargetKind, build_connect_plan, builtin_harnesses,
    probe_harness,
};
use spirit_core::ssh::command::{CommandOutput, CommandSpec};
use spirit_core::ssh::{FakeCommandRunner, SshOptions, SshTarget};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, duplex};

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

    assert_eq!(presets.len(), 25);
    let expected = [
        (
            "opencode",
            HarnessProtocol::OpencodeHttp,
            HarnessSurface::OrchestratorAndWorkers,
        ),
        (
            "cursor-cli",
            HarnessProtocol::Acp,
            HarnessSurface::OrchestratorAndWorkers,
        ),
        (
            "github-copilot",
            HarnessProtocol::Acp,
            HarnessSurface::OrchestratorAndWorkers,
        ),
        (
            "auggie",
            HarnessProtocol::Acp,
            HarnessSurface::OrchestratorAndWorkers,
        ),
        (
            "kiro",
            HarnessProtocol::Acp,
            HarnessSurface::OrchestratorAndWorkers,
        ),
        (
            "goose",
            HarnessProtocol::Acp,
            HarnessSurface::OrchestratorAndWorkers,
        ),
        (
            "codex",
            HarnessProtocol::CodexJson,
            HarnessSurface::OrchestratorAndWorkers,
        ),
        (
            "cline",
            HarnessProtocol::Acp,
            HarnessSurface::OrchestratorAndWorkers,
        ),
        (
            "kilo",
            HarnessProtocol::Acp,
            HarnessSurface::OrchestratorAndWorkers,
        ),
        (
            "qwen-code",
            HarnessProtocol::Acp,
            HarnessSurface::OrchestratorAndWorkers,
        ),
        (
            "mistral-vibe",
            HarnessProtocol::Acp,
            HarnessSurface::OrchestratorAndWorkers,
        ),
        (
            "kimi-code",
            HarnessProtocol::Acp,
            HarnessSurface::OrchestratorAndWorkers,
        ),
        (
            "minimax-code",
            HarnessProtocol::Acp,
            HarnessSurface::OrchestratorAndWorkers,
        ),
        (
            "glm-agent",
            HarnessProtocol::Acp,
            HarnessSurface::OrchestratorAndWorkers,
        ),
        (
            "pi-acp",
            HarnessProtocol::Acp,
            HarnessSurface::OrchestratorAndWorkers,
        ),
        (
            "devin-cli",
            HarnessProtocol::Acp,
            HarnessSurface::OrchestratorAndWorkers,
        ),
        (
            "factory-droid",
            HarnessProtocol::Acp,
            HarnessSurface::OrchestratorAndWorkers,
        ),
        (
            "grok-build",
            HarnessProtocol::Acp,
            HarnessSurface::OrchestratorAndWorkers,
        ),
        (
            "junie",
            HarnessProtocol::Acp,
            HarnessSurface::OrchestratorAndWorkers,
        ),
        (
            "claude-code",
            HarnessProtocol::StreamJson,
            HarnessSurface::WorkersOnlyTerminal,
        ),
        (
            "gemini-cli",
            HarnessProtocol::Acp,
            HarnessSurface::WorkersOnlyTerminal,
        ),
        (
            "antigravity",
            HarnessProtocol::CliPty,
            HarnessSurface::WorkersOnlyTerminal,
        ),
        (
            "aider",
            HarnessProtocol::CliPty,
            HarnessSurface::WorkersOnlyTerminal,
        ),
        (
            "crush",
            HarnessProtocol::CliPty,
            HarnessSurface::WorkersOnlyTerminal,
        ),
        (
            "amp",
            HarnessProtocol::CliPty,
            HarnessSurface::WorkersOnlyTerminal,
        ),
    ];
    for (id, protocol, surface) in expected {
        let preset = manifest(id);
        assert_eq!(preset.protocol, protocol, "{id}");
        assert_eq!(preset.surface, surface, "{id}");
        assert_eq!(
            preset.supports_embedded_chat,
            surface.accepts_embedded_chat(),
            "{id}"
        );
        preset.validate().unwrap();
        assert!(!preset.tos_note.trim().is_empty(), "{id}");
        assert!(preset.docs_url.starts_with("https://"), "{id}");
    }

    // The consumer-versus-API-key distinction must be explicit wherever a consumer
    // subscription is accepted, with the vendor's consumer terms named.
    for id in ["claude-code", "codex", "gemini-cli"] {
        let preset = manifest(id);
        assert!(preset.auth_modes.contains(&HarnessAuthMode::ApiKey));
        assert!(preset.auth_modes.contains(&HarnessAuthMode::Subscription));
        let note = preset.tos_note.to_ascii_lowercase();
        assert!(note.contains("api"), "{id}: {note}");
        assert!(note.contains("consumer terms"), "{id}: {note}");
        assert!(note.contains("subscription"), "{id}: {note}");
    }

    // Workers only means a stock CLI; StackSpirit never claims to wrap its consumer
    // OAuth sign-in in the embedded chat.
    for id in [
        "claude-code",
        "gemini-cli",
        "antigravity",
        "aider",
        "crush",
        "amp",
    ] {
        let preset = manifest(id);
        assert_eq!(preset.surface, HarnessSurface::WorkersOnlyTerminal, "{id}");
        assert!(!preset.supports_embedded_chat, "{id}");
        assert!(
            !preset.embedded_chat(),
            "{id} must not be embedded-chat eligible"
        );
    }

    assert_eq!(manifest("claude-code").binary, "claude");
    assert_eq!(manifest("codex").binary, "codex");
    assert_eq!(manifest("gemini-cli").binary, "gemini");
    assert_eq!(manifest("cursor-cli").binary, "agent");
    assert_eq!(manifest("github-copilot").binary, "copilot");
    assert_eq!(manifest("kiro").binary, "kiro-cli");
    assert_eq!(manifest("aider").binary, "aider");
    assert_eq!(manifest("goose").binary, "goose");
    assert_eq!(manifest("crush").binary, "crush");
    assert_eq!(manifest("amp").binary, "amp");
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

async fn acp_write(writer: &mut DuplexStream, value: Value) {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    writer.write_all(&bytes).await.unwrap();
    writer.flush().await.unwrap();
}

async fn acp_read(reader: &mut BufReader<DuplexStream>) -> Value {
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    serde_json::from_str(line.trim()).unwrap()
}

#[tokio::test]
async fn acp_client_runs_the_core_handshake_and_permission_path() {
    let (client_read, server_write) = duplex(64 * 1024);
    let (server_read, client_write) = duplex(64 * 1024);
    let mut client = AcpClient::from_io(client_read, client_write);
    let server = tokio::spawn(async move {
        let mut reader = BufReader::new(server_read);
        let mut writer = server_write;

        let initialize = acp_read(&mut reader).await;
        assert_eq!(initialize["method"], "initialize");
        assert_eq!(initialize["params"]["protocolVersion"], 1);
        assert_eq!(
            initialize["params"]["clientCapabilities"]["fs"]["readTextFile"],
            false
        );
        acp_write(
            &mut writer,
            json!({
                "jsonrpc": "2.0",
                "id": initialize["id"],
                "result": {
                    "protocolVersion": 1,
                    "agentCapabilities": { "loadSession": false }
                }
            }),
        )
        .await;

        let new_session = acp_read(&mut reader).await;
        assert_eq!(new_session["method"], "session/new");
        assert_eq!(new_session["params"]["cwd"], "/tmp/stackspirit-acp");
        assert_eq!(new_session["params"]["mcpServers"], json!([]));
        acp_write(
            &mut writer,
            json!({
                "jsonrpc": "2.0",
                "id": new_session["id"],
                "result": { "sessionId": "acp-session-1" }
            }),
        )
        .await;

        let prompt = acp_read(&mut reader).await;
        assert_eq!(prompt["method"], "session/prompt");
        assert_eq!(prompt["params"]["sessionId"], "acp-session-1");
        assert_eq!(prompt["params"]["prompt"][0]["text"], "run the tests");
        acp_write(
            &mut writer,
            json!({
                "jsonrpc": "2.0",
                "method": "session/update",
                "params": {
                    "sessionId": "acp-session-1",
                    "update": {
                        "sessionUpdate": "agent_message_chunk",
                        "content": { "type": "text", "text": "Running." }
                    }
                }
            }),
        )
        .await;
        acp_write(
            &mut writer,
            json!({
                "jsonrpc": "2.0",
                "id": 41,
                "method": "session/request_permission",
                "params": {
                    "sessionId": "acp-session-1",
                    "toolCall": { "toolCallId": "tool-1" },
                    "options": [
                        {
                            "optionId": "allow-once",
                            "name": "Allow once",
                            "kind": "allow_once"
                        }
                    ]
                }
            }),
        )
        .await;
        let permission = acp_read(&mut reader).await;
        assert_eq!(permission["id"], 41);
        assert_eq!(permission["result"]["outcome"]["optionId"], "allow-once");
        acp_write(
            &mut writer,
            json!({
                "jsonrpc": "2.0",
                "id": prompt["id"],
                "result": { "stopReason": "end_turn" }
            }),
        )
        .await;
    });

    let initialized = client.initialize().await.unwrap();
    assert_eq!(initialized.protocol_version, 1);
    assert!(initialized.auth_method_ids.is_empty());
    assert_eq!(
        client.new_session("/tmp/stackspirit-acp").await.unwrap(),
        "acp-session-1"
    );

    let mut observer = CollectingObserver {
        decision: AcpPermissionDecision::AllowOnce,
        ..CollectingObserver::default()
    };
    let result = client
        .prompt_with_observer("run the tests", &mut observer)
        .await
        .unwrap();
    assert_eq!(result.stop_reason, "end_turn");
    assert_eq!(observer.updates.len(), 1);
    assert_eq!(observer.updates[0].text(), Some("Running."));
    assert_eq!(observer.permission_requests.len(), 1);
    assert_eq!(observer.decisions, vec![AcpPermissionDecision::AllowOnce]);
    server.await.unwrap();
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
    custom.surface = HarnessSurface::WorkersOnlyTerminal;
    custom.supports_embedded_chat = false;
    custom.oauth_callback_port = None;
    custom.builtin = false;

    let saved = registry.save_custom(custom.clone()).unwrap();
    assert_eq!(saved, custom);
    let path = directory.path().join("my-harness.toml");
    assert!(path.exists());
    let encoded = std::fs::read_to_string(&path).unwrap();
    assert!(encoded.contains("surface = \"workers_only_terminal\""));
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

fn acp_custom(id: &str) -> HarnessManifest {
    let mut custom = manifest("goose");
    custom.id = id.to_owned();
    custom.name = "Custom ACP".to_owned();
    custom.binary = "custom-acp".to_owned();
    custom.auth_cmd = String::new();
    custom.builtin = false;
    custom.chat_argv = vec!["acp".to_owned(), "--stdio".to_owned()];
    custom
}

#[test]
fn chat_argv_validation_accepts_acp_and_rejects_unsafe_values() {
    acp_custom("custom-acp").validate().unwrap();

    for bad in [
        "acp;ls",
        "a|b",
        "a&b",
        "$HOME",
        "`x`",
        "a>b",
        "'acp'",
        "\"acp\"",
        "a\\b",
        "(x)",
        "two words",
        "line\nbreak",
        "nul\0byte",
        "",
    ] {
        let mut custom = acp_custom("custom-acp");
        custom.chat_argv = vec![bad.to_owned()];
        assert!(custom.validate().is_err(), "{bad:?}");
    }

    let mut secret = acp_custom("custom-acp");
    secret.chat_argv = vec![concat!("s", "k-", "FAKEFAKEFAKE").to_owned()];
    let message = secret.validate().unwrap_err().to_string();
    assert!(!message.contains("FAKEFAKE"));

    let mut many = acp_custom("custom-acp");
    many.chat_argv = vec!["a".to_owned(); 17];
    assert!(many.validate().is_err());
    let mut long = acp_custom("custom-acp");
    long.chat_argv = vec!["a".repeat(129)];
    assert!(long.validate().is_err());

    let mut workers = manifest("claude-code");
    workers.chat_argv = vec!["acp".to_owned()];
    assert!(workers.validate().is_err());
    let mut pty = acp_custom("custom-acp");
    pty.protocol = HarnessProtocol::CliPty;
    pty.surface = HarnessSurface::WorkersOnlyTerminal;
    pty.supports_embedded_chat = false;
    assert!(pty.validate().is_err());
}

#[test]
fn chat_argv_round_trips_and_defaults_to_empty() {
    let directory = tempfile::tempdir().unwrap();
    let registry = HarnessRegistry::new(directory.path());
    let custom = acp_custom("custom-acp");
    registry.save_custom(custom.clone()).unwrap();
    let encoded = std::fs::read_to_string(directory.path().join("custom-acp.toml")).unwrap();
    assert!(encoded.contains("chat_argv = [\"acp\", \"--stdio\"]"));
    assert_eq!(registry.get("custom-acp").unwrap().unwrap(), custom);

    let mut bare = acp_custom("bare-acp");
    bare.chat_argv.clear();
    registry.save_custom(bare).unwrap();
    let encoded = std::fs::read_to_string(directory.path().join("bare-acp.toml")).unwrap();
    assert!(!encoded.contains("chat_argv"));
    assert!(
        registry
            .get("bare-acp")
            .unwrap()
            .unwrap()
            .chat_argv
            .is_empty()
    );
}

#[test]
fn builtin_acp_presets_carry_only_known_launch_argv() {
    assert_eq!(manifest("goose").chat_argv, ["acp"]);
    assert_eq!(manifest("cursor-cli").chat_argv, ["acp"]);
    assert_eq!(manifest("github-copilot").chat_argv, ["--acp"]);
    assert_eq!(manifest("auggie").chat_argv, ["--acp"]);
    assert_eq!(manifest("kiro").chat_argv, ["acp"]);
    assert!(manifest("opencode").chat_argv.is_empty());
    for preset in builtin_harnesses() {
        preset.validate().unwrap();
    }
}
