//! Phase 1a harness chat selection tests: persisted default, typed gates and a
//! transport-less dry run.
//!
//! No test starts a harness, spawns a child, opens a socket or calls an engine. The dry
//! run tests use a temporary registry and a temporary `chat.json` only.

use std::fs;
use std::path::Path;

use spirit_core::engine::chat::{
    CHAT_PREFERENCES_VERSION, ChatHarnessCapture, ChatPreferences, ChatPreferencesSource,
    ChatPreferencesStore, ChatSettingsError, HarnessChatGate, JsonChatPreferences, dry_run,
    gate_for_manifest, preflight,
};
use spirit_core::harnesses::{
    HarnessManifest, HarnessProtocol, HarnessRegistry, HarnessSurface, builtin_harnesses,
};

fn ready_gate() -> HarnessChatGate {
    HarnessChatGate::Ready {
        harness_id: "opencode".to_owned(),
        protocol: HarnessProtocol::OpencodeHttp,
    }
}

fn preferences(id: Option<&str>) -> ChatPreferences {
    ChatPreferences {
        version: CHAT_PREFERENCES_VERSION,
        default_harness_id: id.map(str::to_owned),
    }
}

fn builtin(id: &str) -> HarnessManifest {
    builtin_harnesses()
        .into_iter()
        .find(|manifest| manifest.id == id)
        .unwrap_or_else(|| panic!("missing builtin {id}"))
}

fn entries(path: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn missing_null_and_fresh_selection_are_opencode_ready() {
    let directory = tempfile::tempdir().unwrap();
    let registry = HarnessRegistry::new(directory.path().join("harnesses"));
    let store = JsonChatPreferences::new(directory.path().join("chat.json"));

    assert_eq!(
        dry_run(&registry, &ChatPreferences::default()),
        ready_gate()
    );

    fs::write(store.path(), br#"{"version":1,"default_harness_id":null}"#).unwrap();
    assert_eq!(dry_run(&registry, &store), ready_gate());

    fs::write(store.path(), br#"{"version":1}"#).unwrap();
    assert_eq!(dry_run(&registry, &store), ready_gate());

    store.save(&preferences(Some("opencode"))).unwrap();
    assert_eq!(store.load().unwrap().selected_harness_id(), "opencode");
    assert_eq!(preflight(&registry, &store), ready_gate());
}

#[test]
fn unknown_id_gates_missing_and_is_not_replaced() {
    let directory = tempfile::tempdir().unwrap();
    let registry = HarnessRegistry::new(directory.path().join("harnesses"));
    let store = JsonChatPreferences::new(directory.path().join("chat.json"));
    store.save(&preferences(Some("not-registered"))).unwrap();
    let before = fs::read(store.path()).unwrap();

    let gate = preflight(&registry, &store);
    assert_eq!(
        gate,
        HarnessChatGate::HarnessMissing {
            harness_id: "not-registered".to_owned(),
        }
    );
    assert!(!gate.is_ready());
    assert_eq!(gate.reason(), "harness_missing");
    assert_eq!(
        store.load().unwrap().default_harness_id.as_deref(),
        Some("not-registered"),
        "an unknown id must be returned unchanged, not replaced with opencode"
    );
    assert_eq!(fs::read(store.path()).unwrap(), before);
}

#[test]
fn workers_only_harnesses_gate_worker_only() {
    let directory = tempfile::tempdir().unwrap();
    let registry = HarnessRegistry::new(directory.path().join("harnesses"));

    for id in ["claude-code", "gemini-cli", "antigravity"] {
        let gate = dry_run(&registry, &preferences(Some(id)));
        assert_eq!(
            gate,
            HarnessChatGate::HarnessWorkerOnly {
                harness_id: id.to_owned(),
            },
            "{id} must never back the embedded chat"
        );
        assert_eq!(gate.reason(), "harness_worker_only");
        assert!(gate.capture().is_none());
    }
}

#[test]
fn every_non_opencode_orchestrator_builtin_is_not_wired() {
    for manifest in builtin_harnesses() {
        if manifest.surface != HarnessSurface::OrchestratorAndWorkers {
            continue;
        }
        if manifest.id == "opencode" {
            assert!(gate_for_manifest(&manifest).is_ready());
            continue;
        }
        let gate = gate_for_manifest(&manifest);
        assert_eq!(
            gate,
            HarnessChatGate::HarnessNotWired {
                harness_id: manifest.id.clone(),
                protocol: manifest.protocol,
            },
            "{} has an ACP/adapter module but no wired chat transport",
            manifest.id
        );
        assert_eq!(gate.reason(), "harness_not_wired");
    }
}

#[test]
fn dry_run_is_pure_and_creates_no_child_or_file() {
    let directory = tempfile::tempdir().unwrap();
    let registry = HarnessRegistry::new(directory.path().join("harnesses"));
    let store = JsonChatPreferences::new(directory.path().join("chat.json"));
    store.save(&preferences(Some("goose"))).unwrap();

    let entries_before = entries(directory.path());
    let settings_before = fs::read(store.path()).unwrap();
    let first = dry_run(&registry, &store);
    let second = dry_run(&registry, &store);

    assert_eq!(first, second);
    assert_eq!(
        first,
        HarnessChatGate::HarnessNotWired {
            harness_id: "goose".to_owned(),
            protocol: HarnessProtocol::Acp,
        }
    );
    assert_eq!(fs::read(store.path()).unwrap(), settings_before);
    assert_eq!(entries(directory.path()), entries_before);
}

#[test]
fn capture_keeps_the_id_and_protocol_without_a_path_or_token() {
    let gate = HarnessChatGate::HarnessNotWired {
        harness_id: "custom-acp".to_owned(),
        protocol: HarnessProtocol::Acp,
    };
    let capture = gate.capture().expect("a resolvable harness is captured");
    assert_eq!(capture.harness_id(), "custom-acp");
    assert_eq!(capture.protocol(), HarnessProtocol::Acp);

    let debug = format!("{capture:?}");
    assert!(debug.contains("custom-acp"));
    assert!(debug.contains("Acp"));
    assert!(!debug.contains('/'));
    assert!(!debug.contains("token"));
    assert!(!debug.contains(concat!("s", "k-")));

    let direct = ChatHarnessCapture::new("opencode", HarnessProtocol::OpencodeHttp);
    assert_eq!(direct.harness_id(), "opencode");
    assert_eq!(direct.protocol(), HarnessProtocol::OpencodeHttp);
}

#[test]
fn corrupt_and_future_settings_gate_without_rewriting() {
    let directory = tempfile::tempdir().unwrap();
    let registry = HarnessRegistry::new(directory.path().join("harnesses"));
    let store = JsonChatPreferences::new(directory.path().join("chat.json"));

    let corrupt = br#"{not valid json"#;
    fs::write(store.path(), corrupt).unwrap();
    assert_eq!(store.load(), Err(ChatSettingsError::Unreadable));
    let gate = dry_run(&registry, &store);
    assert_eq!(
        gate,
        HarnessChatGate::ChatSettingsUnreadable {
            problem: ChatSettingsError::Unreadable,
        }
    );
    assert_eq!(gate.reason(), "chat_settings_unreadable");
    assert!(!gate.is_ready());
    assert_eq!(fs::read(store.path()).unwrap(), corrupt);

    let future = br#"{"version":99,"default_harness_id":"opencode"}"#;
    fs::write(store.path(), future).unwrap();
    let problem = ChatSettingsError::UnsupportedVersion {
        found: 99,
        supported: CHAT_PREFERENCES_VERSION,
    };
    assert_eq!(store.load(), Err(problem));
    let gate = dry_run(&registry, &store);
    assert_eq!(gate, HarnessChatGate::ChatSettingsUnreadable { problem });
    assert_eq!(gate.reason(), "chat_settings_unreadable");
    assert_eq!(fs::read(store.path()).unwrap(), future);

    let json = serde_json::to_string(&gate).unwrap();
    assert!(json.contains("chat_settings_unreadable"));
    assert!(!json.contains('/'));
    assert!(!json.contains("token"));
}

#[test]
fn custom_manifests_from_a_temp_registry_are_respected() {
    let directory = tempfile::tempdir().unwrap();
    let registry = HarnessRegistry::new(directory.path().join("harnesses"));
    let store = JsonChatPreferences::new(directory.path().join("chat.json"));

    let mut custom_acp = builtin("goose");
    custom_acp.id = "custom-acp".to_owned();
    custom_acp.name = "Custom ACP".to_owned();
    custom_acp.builtin = false;
    registry.save_custom(custom_acp).unwrap();
    store.save(&preferences(Some("custom-acp"))).unwrap();
    assert_eq!(
        dry_run(&registry, &store),
        HarnessChatGate::HarnessNotWired {
            harness_id: "custom-acp".to_owned(),
            protocol: HarnessProtocol::Acp,
        }
    );

    let mut custom_worker = builtin("claude-code");
    custom_worker.id = "custom-worker".to_owned();
    custom_worker.name = "Custom Worker".to_owned();
    custom_worker.builtin = false;
    registry.save_custom(custom_worker).unwrap();
    store.save(&preferences(Some("custom-worker"))).unwrap();
    assert_eq!(
        dry_run(&registry, &store),
        HarnessChatGate::HarnessWorkerOnly {
            harness_id: "custom-worker".to_owned(),
        }
    );

    let mut custom_http = builtin("opencode");
    custom_http.id = "custom-http".to_owned();
    custom_http.name = "Custom HTTP".to_owned();
    custom_http.builtin = false;
    registry.save_custom(custom_http).unwrap();
    store.save(&preferences(Some("custom-http"))).unwrap();
    assert_eq!(
        dry_run(&registry, &store),
        HarnessChatGate::HarnessNotWired {
            harness_id: "custom-http".to_owned(),
            protocol: HarnessProtocol::OpencodeHttp,
        },
        "only the opencode id resolves to the existing bundled provider"
    );
}

#[test]
fn a_cli_pty_manifest_claim_is_rejected_even_with_embedded_chat_true() {
    let mut manifest = builtin("opencode");
    manifest.protocol = HarnessProtocol::CliPty;

    assert_eq!(
        gate_for_manifest(&manifest),
        HarnessChatGate::HarnessProtocolUnsupported {
            harness_id: "opencode".to_owned(),
            protocol: HarnessProtocol::CliPty,
        }
    );
    assert_eq!(
        gate_for_manifest(&manifest).reason(),
        "harness_protocol_unsupported"
    );
}

#[test]
fn invalid_selected_id_is_safely_reported_as_missing() {
    let directory = tempfile::tempdir().unwrap();
    let registry = HarnessRegistry::new(directory.path().join("harnesses"));
    let store = JsonChatPreferences::new(directory.path().join("chat.json"));
    store.save(&preferences(Some("../../private"))).unwrap();

    let gate = dry_run(&registry, &store);
    assert_eq!(
        gate,
        HarnessChatGate::HarnessMissing {
            harness_id: "invalid".to_owned(),
        }
    );
    let json = serde_json::to_string(&gate).unwrap();
    assert!(!json.contains("private"));
    assert!(!json.contains('/'));
}

#[test]
fn settings_source_can_be_an_already_loaded_preference() {
    let directory = tempfile::tempdir().unwrap();
    let registry = HarnessRegistry::new(directory.path().join("harnesses"));
    let loaded = preferences(Some("goose"));

    assert_eq!(loaded.resolve(), Ok(loaded.clone()));
    assert_eq!(
        dry_run(&registry, &loaded),
        HarnessChatGate::HarnessNotWired {
            harness_id: "goose".to_owned(),
            protocol: HarnessProtocol::Acp,
        }
    );
}
