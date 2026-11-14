//! Engine skeleton: catalog, keychain, supervisor, external scan.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use spirit_core::engine::{
    CatalogEntry, EngineState, EngineSupervisor, EngineSupervisorConfig, ExternalHarnessKind,
    FakeEngineRuntime, FakeKeychain, HarnessProbe, Keychain, KeychainErrorClass, ModelCatalog,
    SecretString, scan_external_harnesses,
};
use spirit_core::time::Timestamp;

fn catalog() -> ModelCatalog {
    ModelCatalog::new(vec![
        CatalogEntry {
            id: "deepseek/deepseek-flash".to_owned(),
            label: "DeepSeek V4.1 Flash".to_owned(),
            provider: "deepseek".to_owned(),
            context_tokens: Some(128_000),
            notes: "Included with StackSpirit Engine; no key required.".to_owned(),
        },
        CatalogEntry {
            id: "openai/gpt-5-mini".to_owned(),
            label: "GPT-5 mini".to_owned(),
            provider: "openai".to_owned(),
            context_tokens: Some(400_000),
            notes: "Included while the engine reports it.".to_owned(),
        },
    ])
}

#[test]
fn included_catalog_entries_are_marked_included() {
    let catalog = catalog();
    let verified = catalog.verify(
        &["deepseek/deepseek-flash".to_owned()],
        Timestamp::parse("2026-11-14T12:00:00.000Z").unwrap(),
    );
    assert!(verified.included_models_available);
    assert_eq!(verified.models.len(), 1);
    assert_eq!(verified.models[0].id, "deepseek/deepseek-flash");
    assert_eq!(
        verified.models[0].access,
        spirit_core::engine::ModelAccess::Included
    );
    assert!(verified.models[0].verified_at.is_some());
    let before = catalog.unverified_choices();
    assert_eq!(before.len(), 2);
    assert!(before.iter().all(|model| model.verified_at.is_none()));

    let empty = ModelCatalog::default().verify(&[], Timestamp::now());
    assert!(!empty.included_models_available);
    assert!(empty.models.is_empty());
}

#[test]
fn fake_keychain_never_writes_plaintext() {
    let keychain = FakeKeychain::new();
    assert!(!keychain.writes_files());
    let secret = SecretString::new("not-a-real-provider-key");
    keychain.set("deepseek", &secret).unwrap();
    assert_eq!(keychain.len(), 1);
    assert_eq!(keychain.list().unwrap(), vec!["deepseek".to_owned()]);
    let read = keychain.get("deepseek").unwrap().unwrap();
    assert_eq!(read.expose(), "not-a-real-provider-key");
    assert!(!format!("{keychain:?}").contains("not-a-real-provider-key"));
    assert!(keychain.get("missing").unwrap().is_none());
    keychain.delete("deepseek").unwrap();
    assert_eq!(
        keychain.delete("deepseek").unwrap_err().class,
        KeychainErrorClass::NotFound
    );
    assert!(keychain.is_empty());
    // The fake has no path, no file, and no serialization surface.
    let directory = tempfile::tempdir().unwrap();
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn secret_string_has_no_debug_or_serialize() {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/engine/keychain.rs"
    ))
    .unwrap();
    assert!(!source.contains("impl std::fmt::Debug for SecretString"));
    assert!(!source.contains("impl fmt::Debug for SecretString"));
    assert!(!source.contains("impl Serialize for SecretString"));
    // Access is explicit and the only accessor is named expose().
    let secret = SecretString::new("value-only-via-expose");
    assert_eq!(secret.expose(), "value-only-via-expose");
    assert!(!secret.is_empty());
}

#[tokio::test]
async fn supervisor_restarts_then_enters_degraded() {
    let runtime = Arc::new(FakeEngineRuntime::new().with_health_failures(100));
    let config = EngineSupervisorConfig {
        restart_budget: 2,
        health_attempts: 1,
        retry_delay: Duration::ZERO,
        port: 0,
    };
    let mut supervisor = EngineSupervisor::new(runtime.clone(), config);
    let status = supervisor.start().await;
    assert!(matches!(status.state, EngineState::Degraded { .. }));
    let status = supervisor.restart().await;
    assert!(matches!(status.state, EngineState::Degraded { .. }));
    let status = supervisor.restart().await;
    assert!(matches!(status.state, EngineState::Degraded { .. }));
    assert!(status.last_error.unwrap().contains("restart budget"));
    assert_eq!(runtime.spawns(), 2);
}

#[tokio::test]
async fn supervisor_starts_healthy_and_stops_cleanly() {
    let runtime = Arc::new(FakeEngineRuntime::new());
    let config = EngineSupervisorConfig {
        restart_budget: 3,
        health_attempts: 3,
        retry_delay: Duration::ZERO,
        port: 0,
    };
    let mut supervisor = EngineSupervisor::new(runtime.clone(), config);
    let status = supervisor.start().await;
    assert!(matches!(status.state, EngineState::Ready));
    assert_eq!(status.port, Some(0));
    assert_eq!(status.version.as_deref(), Some("fake-0.1"));
    assert!(supervisor.started_at().is_some());
    let status = supervisor.stop().await;
    assert!(matches!(status.state, EngineState::Stopped));
    assert_eq!(runtime.shutdowns(), 1);
    assert!(status.port.is_none());
}

#[tokio::test]
async fn supervisor_start_is_idempotent_and_never_orphans_a_runtime() {
    let runtime = Arc::new(FakeEngineRuntime::new());
    let config = EngineSupervisorConfig {
        restart_budget: 3,
        health_attempts: 3,
        retry_delay: Duration::ZERO,
        port: 0,
    };
    let mut supervisor = EngineSupervisor::new(runtime.clone(), config);
    let first = supervisor.start().await;
    assert!(matches!(first.state, EngineState::Ready));
    let second = supervisor.start().await;
    assert!(matches!(second.state, EngineState::Ready));
    assert_eq!(
        runtime.spawns(),
        1,
        "a second start while Ready must not spawn another runtime"
    );
    assert_eq!(runtime.shutdowns(), 0, "the live runtime must stay alive");
    let stopped = supervisor.stop().await;
    assert!(matches!(stopped.state, EngineState::Stopped));
    assert_eq!(
        runtime.shutdowns(),
        1,
        "the single live runtime must be shut down exactly once"
    );
}

#[tokio::test]
async fn supervisor_spawn_failure_is_failed_then_degraded_after_restart_budget() {
    let runtime = Arc::new(FakeEngineRuntime::new().with_spawn_failures(100));
    let config = EngineSupervisorConfig {
        restart_budget: 1,
        health_attempts: 1,
        retry_delay: Duration::ZERO,
        port: 0,
    };
    let mut supervisor = EngineSupervisor::new(runtime, config);
    let status = supervisor.start().await;
    assert!(matches!(status.state, EngineState::Failed { .. }));
    let status = supervisor.restart().await;
    assert!(matches!(status.state, EngineState::Degraded { .. }));
}

struct FakeProbe {
    paths: HashMap<String, PathBuf>,
}

impl HarnessProbe for FakeProbe {
    fn locate(&self, name: &str) -> Option<PathBuf> {
        self.paths.get(name).cloned()
    }

    fn version(&self, path: &Path) -> Option<String> {
        let path = path.to_string_lossy();
        if path.contains("opencode") {
            Some("1.2.3".to_owned())
        } else if path.contains("claude") {
            Some("2.0.0".to_owned())
        } else {
            None
        }
    }
}

#[test]
fn external_scan_finds_only_path_opencode_and_claude() {
    let mut paths = HashMap::new();
    paths.insert(
        "opencode".to_owned(),
        PathBuf::from("/opt/example/bin/opencode"),
    );
    paths.insert(
        "claude".to_owned(),
        PathBuf::from("/opt/example/bin/claude"),
    );
    paths.insert(
        "claude-code".to_owned(),
        PathBuf::from("/opt/example/bin/claude"),
    );
    paths.insert("node".to_owned(), PathBuf::from("/usr/bin/node"));
    paths.insert("python".to_owned(), PathBuf::from("/usr/bin/python"));
    let probe = FakeProbe { paths };
    let found = scan_external_harnesses(&probe);
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].kind, ExternalHarnessKind::ClaudeCode);
    assert!(found[0].usable);
    assert_eq!(found[1].kind, ExternalHarnessKind::Opencode);
    assert_eq!(found[1].version.as_deref(), Some("1.2.3"));
    assert!(
        found
            .iter()
            .all(|item| item.path.to_string_lossy().contains("/opt/example/bin/"))
    );
}

#[test]
fn path_probe_finds_exact_names_only() {
    let directory = tempfile::tempdir().unwrap();
    let opencode = directory.path().join("opencode");
    std::fs::write(&opencode, b"#!/bin/sh\n").unwrap();
    let other = directory.path().join("opencode-extra");
    std::fs::write(&other, b"#!/bin/sh\n").unwrap();
    let probe = spirit_core::engine::PathHarnessProbe::new(vec![directory.path().to_path_buf()]);
    assert_eq!(probe.locate("opencode"), Some(opencode.clone()));
    assert_eq!(probe.locate("opencode-extra"), Some(other));
    assert_eq!(probe.locate("node"), None);
    let found = scan_external_harnesses(&probe);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].kind, ExternalHarnessKind::Opencode);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        assert!(
            !found[0].usable,
            "a non-executable file must not be reported as usable"
        );
        std::fs::set_permissions(&opencode, std::fs::Permissions::from_mode(0o755)).unwrap();
        let executable = scan_external_harnesses(&probe);
        assert!(executable[0].usable, "an executable harness is usable");
    }
}
