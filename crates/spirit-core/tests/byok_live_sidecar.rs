//! Real bundled-sidecar BYOK round trip (`ss-88`).
//!
//! Nothing here mocks the engine: the test starts the pinned `opencode` sidecar fetched by
//! `node scripts/fetch-engine.mjs` and drives key add → verify → chat model list → model
//! switch → log audit → key removal through the production types
//! (`ProcessEngineRuntime`, `EngineSupervisor`, `LocalChatProvider`, `verify_provider_key`).
//!
//! The only key used is a clearly synthetic, non-secret string. It must never appear in
//! `opencode.log`, the supervisor log tail or any file inside the engine's private tree.
//!
//! Run with `scripts/cargo-gate.sh test -p spirit-core --features keychain --test
//! byok_live_sidecar` (the runner `scripts/verify-byok-xvfb.mjs` does). The sidecar is
//! never resolved from `PATH`; a missing binary fails the test loudly.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use spirit_core::engine::process::classify_message_response;
use spirit_core::engine::{
    ChatEngine, ChatModelTier, ChatModelView, ChatSessionId, ChatSessionLocation, ChatStartInput,
    EngineRuntime, EngineState, EngineStatus, EngineSupervisor, EngineSupervisorConfig,
    FakeEngineRuntime, FakeKeychain, KeyVerification, Keychain, LocalChatProvider,
    ProcessEngineRuntime, ProviderKeyState, SecretString, current_target_triple, sidecar_file_name,
    verify_provider_key,
};
use spirit_core::error::{SpiritError, SpiritErrorCode};
use spirit_core::time::Timestamp;
use tokio::sync::Mutex as AsyncMutex;

const PINNED_VERSION: &str = "1.19.5";
const PROVIDER: &str = "openrouter";

/// A clearly synthetic key. It is assembled at runtime so no scanner sees a key literal.
fn synthetic_key() -> String {
    ["sk", "or", "v1", "test-dummy-not-a-real-key-00000000"].join("-")
}

// ───────────────────────── sidecar + HTTP helpers ─────────────────────────

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

/// The fetched pinned sidecar. Never falls back to `PATH`.
fn pinned_sidecar() -> PathBuf {
    let triple = current_target_triple().expect("unsupported target triple");
    let path = repo_root()
        .join("apps/spirit-desktop/src-tauri/binaries")
        .join(sidecar_file_name(triple));
    assert!(
        path.is_file(),
        "pinned sidecar missing at {}: run `node scripts/fetch-engine.mjs` first",
        path.display()
    );
    path
}

fn http_json(port: u16, path: &str) -> Value {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream =
        TcpStream::connect_timeout(&address, Duration::from_secs(2)).expect("connect engine");
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .expect("timeout");
    // HTTP/1.0 keeps the response un-chunked.
    write!(
        stream,
        "GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nAccept: application/json\r\n\r\n"
    )
    .expect("write request");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("read response");
    let boundary = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("http headers");
    let head = String::from_utf8_lossy(&raw[..boundary]).to_string();
    assert!(head.contains(" 200 "), "GET {path} failed: {head}");
    serde_json::from_slice(&raw[boundary + 4..]).unwrap_or_else(|_| panic!("GET {path}: not JSON"))
}

/// `provider id -> model count` from `/config/providers`.
fn config_providers(port: u16) -> Vec<(String, usize)> {
    let document = http_json(port, "/config/providers");
    document["providers"]
        .as_array()
        .expect("providers array")
        .iter()
        .map(|provider| {
            (
                provider["id"].as_str().unwrap_or_default().to_owned(),
                provider["models"]
                    .as_object()
                    .map_or(0, serde_json::Map::len),
            )
        })
        .collect()
}

fn model_count(providers: &[(String, usize)], id: &str) -> usize {
    providers
        .iter()
        .find(|(provider, _)| provider == id)
        .map_or(0, |(_, count)| *count)
}

fn connected_providers(port: u16) -> Vec<String> {
    http_json(port, "/provider")["connected"]
        .as_array()
        .expect("connected array")
        .iter()
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect()
}

fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => stack.push(path),
                Ok(kind) if kind.is_file() => found.push(path),
                _ => {}
            }
        }
    }
    found.sort();
    found
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

/// Statistics from the audit of the live `opencode.log`.
#[derive(Debug)]
struct LogAudit {
    lines: usize,
    errors: usize,
    leaks: usize,
}

/// Audits `opencode.log`, the supervisor log tail and every engine-tree file.
fn audit_engine(app_data: &Path, supervisor_logs: &[String], key: &str) -> LogAudit {
    let engine = app_data.join("engine");
    let log_path = engine.join("data/opencode/log/opencode.log");
    let log = std::fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!("opencode.log unreadable at {}: {error}", log_path.display())
    });
    assert!(!log.trim().is_empty(), "opencode.log must not be empty");

    let forbidden = [
        "panic",
        "panicked",
        "SQLITE_BUSY",
        "database is locked",
        "unhandled",
        "uncaught",
        "segmentation fault",
        "fatal",
    ];
    let mut errors = 0;
    for line in log
        .lines()
        .chain(supervisor_logs.iter().map(String::as_str))
    {
        let lower = line.to_ascii_lowercase();
        if forbidden
            .iter()
            .any(|needle| lower.contains(&needle.to_ascii_lowercase()))
        {
            errors += 1;
            eprintln!("engine log problem: {line}");
        }
    }

    let mut leaks = usize::from(log.contains(key));
    leaks += supervisor_logs
        .iter()
        .filter(|line| line.contains(key))
        .count();
    for path in files_under(&engine) {
        // The SQLite WAL/db are binary, so compare raw bytes.
        if let Ok(bytes) = std::fs::read(&path)
            && contains_bytes(&bytes, key.as_bytes())
        {
            leaks += 1;
            eprintln!(
                "key found in {}",
                path.strip_prefix(app_data).unwrap().display()
            );
        }
    }
    LogAudit {
        lines: log.lines().count(),
        errors,
        leaks,
    }
}

fn assert_no_auth_json(app_data: &Path) {
    let found: Vec<_> = files_under(&app_data.join("engine/data"))
        .into_iter()
        .filter(|path| path.file_name().is_some_and(|name| name == "auth.json"))
        .collect();
    assert!(found.is_empty(), "plaintext auth.json written: {found:?}");
}

// ───────────────────────── chat engine adapter ─────────────────────────

/// The same shape as the desktop's `DesktopChatEngine`: chat resolves the one supervised
/// engine and lists free runtime models plus models of keychain-configured providers.
struct LiveChatEngine {
    supervisor: Arc<AsyncMutex<EngineSupervisor>>,
    runtime: Arc<ProcessEngineRuntime>,
    keychain: Arc<dyn Keychain>,
    cache: Mutex<Cache>,
}

#[derive(Default)]
struct Cache {
    selected: Option<ChatModelView>,
    available: Vec<ChatModelView>,
    status: EngineStatus,
}

impl LiveChatEngine {
    fn cache(&self) -> std::sync::MutexGuard<'_, Cache> {
        self.cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    async fn usable(&self, port: u16) -> Vec<ChatModelView> {
        let configured = self.keychain.list().unwrap_or_default();
        let mut views: Vec<ChatModelView> = self
            .runtime
            .models(port)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter_map(|model| {
                let tier = if model.free {
                    ChatModelTier::Included
                } else if configured.contains(&model.provider) {
                    ChatModelTier::Byok
                } else {
                    return None;
                };
                Some(ChatModelView {
                    label: if model.label.is_empty() {
                        model.id.clone()
                    } else {
                        model.label.clone()
                    },
                    id: model.id,
                    provider: model.provider,
                    tier,
                    context_tokens: model.context_tokens,
                })
            })
            .collect();
        views.sort_by(|left, right| {
            (
                left.tier != ChatModelTier::Included,
                &left.provider,
                &left.id,
            )
                .cmp(&(
                    right.tier != ChatModelTier::Included,
                    &right.provider,
                    &right.id,
                ))
        });
        views
    }
}

#[async_trait]
impl ChatEngine for LiveChatEngine {
    async fn ensure_ready(&self) -> Result<u16, SpiritError> {
        let status = {
            let mut supervisor = self.supervisor.lock().await;
            if !matches!(supervisor.status().state, EngineState::Ready) {
                supervisor.start().await;
            }
            supervisor.status()
        };
        let (EngineState::Ready, Some(port)) = (&status.state, status.port) else {
            return Err(SpiritError::new(
                SpiritErrorCode::Engine,
                "engine_not_ready: the live sidecar is not ready",
            ));
        };
        let available = self.usable(port).await;
        let mut cache = self.cache();
        // A selection that the restarted engine no longer offers is dropped.
        if let Some(selected) = &cache.selected
            && !available.iter().any(|model| model.id == selected.id)
        {
            cache.selected = None;
        }
        if cache.selected.is_none() {
            cache.selected = available
                .iter()
                .find(|model| model.tier == ChatModelTier::Included)
                .cloned();
        }
        cache.available = available;
        cache.status = status;
        Ok(port)
    }

    fn selected_model(&self) -> Option<ChatModelView> {
        self.cache().selected.clone()
    }

    fn available_models(&self) -> Vec<ChatModelView> {
        self.cache().available.clone()
    }

    async fn select_model(
        &self,
        model_id: &str,
        _persist: bool,
    ) -> Result<ChatModelView, SpiritError> {
        let mut cache = self.cache();
        let view = cache
            .available
            .iter()
            .find(|model| model.id == model_id)
            .cloned()
            .ok_or_else(|| {
                SpiritError::new(SpiritErrorCode::NotFound, "that model is not available")
            })?;
        cache.selected = Some(view.clone());
        Ok(view)
    }

    fn status(&self) -> EngineStatus {
        self.cache().status.clone()
    }
}

// ───────────────────────── the live flow ─────────────────────────

struct Harness {
    _app: tempfile::TempDir,
    app_data: PathBuf,
    runtime: Arc<ProcessEngineRuntime>,
    supervisor: Arc<AsyncMutex<EngineSupervisor>>,
    keychain: Arc<dyn Keychain>,
    chat: LocalChatProvider,
}

impl Harness {
    fn new(keychain: Arc<dyn Keychain>) -> Self {
        let app = tempfile::tempdir().expect("tempdir");
        let app_data = app.path().to_path_buf();
        let runtime = Arc::new(
            ProcessEngineRuntime::with_binary(pinned_sidecar())
                .with_private_state(app_data.clone())
                .with_key_source(keychain.clone()),
        );
        let supervisor = Arc::new(AsyncMutex::new(EngineSupervisor::new(
            runtime.clone(),
            EngineSupervisorConfig::default(),
        )));
        let engine = Arc::new(LiveChatEngine {
            supervisor: supervisor.clone(),
            runtime: runtime.clone(),
            keychain: keychain.clone(),
            cache: Mutex::new(Cache::default()),
        });
        let workspace = app_data.join("engine/workspaces/setup");
        let chat = LocalChatProvider::new(engine, workspace);
        Self {
            _app: app,
            app_data,
            runtime,
            supervisor,
            keychain,
            chat,
        }
    }

    async fn port(&self) -> u16 {
        let status = self.supervisor.lock().await.status();
        assert!(
            matches!(status.state, EngineState::Ready),
            "engine not ready: {:?}",
            status.state
        );
        status.port.expect("engine port")
    }
}

fn setup_input() -> ChatStartInput {
    ChatStartInput {
        session: ChatSessionId::Setup,
        project: None,
        location: ChatSessionLocation::Local,
        resume: Some(false),
    }
}

async fn run_live_flow(label: &str, keychain: Arc<dyn Keychain>) {
    let key = synthetic_key();
    let secret = SecretString::new(key.clone());
    let harness = Harness::new(keychain);

    // 1. Start the bundled sidecar.
    let status = harness.supervisor.lock().await.start().await;
    assert!(
        matches!(status.state, EngineState::Ready),
        "[{label}] sidecar did not become ready: {:?} / {:?}",
        status.state,
        status.last_error
    );
    assert_eq!(status.version.as_deref(), Some(PINNED_VERSION));
    let port = harness.port().await;
    let health = http_json(port, "/global/health");
    assert_eq!(health["healthy"], Value::Bool(true));
    assert_eq!(health["version"].as_str(), Some(PINNED_VERSION));
    let initial = config_providers(port);
    assert!(
        model_count(&initial, "opencode") > 0,
        "[{label}] OpenCode free models missing: {initial:?}"
    );
    assert_eq!(model_count(&initial, PROVIDER), 0);
    assert!(!connected_providers(port).contains(&PROVIDER.to_owned()));
    assert!(harness.keychain.get(PROVIDER).unwrap().is_none());

    // 2. Connect openrouter. The sidecar restarts with an ephemeral auth document.
    let first_port = port;
    let saved = harness
        .supervisor
        .lock()
        .await
        .connect_provider(harness.keychain.as_ref(), PROVIDER, &secret)
        .await
        .expect("connect provider");
    assert_eq!(saved.state, ProviderKeyState::Saved);
    let port = harness.port().await;
    assert_ne!(
        port, first_port,
        "[{label}] connect must restart the sidecar"
    );
    assert_no_auth_json(&harness.app_data);
    let connected = config_providers(port);
    let openrouter_models = model_count(&connected, PROVIDER);
    assert!(
        openrouter_models >= 100,
        "[{label}] expected the 100+ model OpenRouter catalog, got {openrouter_models}"
    );
    assert!(model_count(&connected, "opencode") > 0);
    let live_connected = connected_providers(port);
    assert!(
        live_connected.contains(&PROVIDER.to_owned()),
        "[{label}] /provider does not report openrouter connected: {live_connected:?}"
    );

    // 3. Verify through the isolated ephemeral-XDG probe. The key must survive.
    let before_db = files_under(&harness.app_data.join("engine/data"));
    let verified = verify_provider_key(
        harness.keychain.as_ref(),
        harness.runtime.as_ref(),
        Some(port),
        PROVIDER,
        Timestamp::now(),
    )
    .await
    .expect("verify provider key");
    assert!(
        matches!(
            verified.state,
            ProviderKeyState::Invalid | ProviderKeyState::Unverified
        ),
        "[{label}] a synthetic key must be invalid (or unverified offline), got {:?}",
        verified.state
    );
    assert_ne!(verified.state, ProviderKeyState::Missing);
    let stored = harness
        .keychain
        .get(PROVIDER)
        .expect("keychain readable")
        .expect("key wiped by verification");
    assert_eq!(stored.expose(), key, "[{label}] the saved key changed");
    assert!(
        harness
            .keychain
            .list()
            .unwrap()
            .contains(&PROVIDER.to_owned()),
        "[{label}] provider vanished from the keychain index"
    );
    // The probe used its own XDG tree: the live engine's data tree gained no auth file and
    // the engine is still healthy on the same port.
    assert_no_auth_json(&harness.app_data);
    assert_eq!(
        http_json(port, "/global/health")["healthy"],
        Value::Bool(true)
    );
    drop(before_db);
    eprintln!("[{label}] live verify state: {:?}", verified.state);

    // 4. Setup chat lists both OpenCode Free and real openrouter/* models.
    harness.chat.refresh_setup_models().await; // no session yet: a no-op
    let session = harness
        .chat
        .start(&setup_input())
        .await
        .expect("start chat");
    let available = session.available_models.clone().expect("available models");
    assert!(
        available
            .iter()
            .any(|model| model.provider == "opencode" && model.tier == ChatModelTier::Included),
        "[{label}] OpenCode Free models missing from chat"
    );
    let router: Vec<_> = available
        .iter()
        .filter(|model| model.id.starts_with("openrouter/"))
        .collect();
    assert!(
        router.len() >= 100,
        "[{label}] only {} openrouter models",
        router.len()
    );
    assert!(
        router
            .iter()
            .any(|model| model.id.contains("anthropic/") || model.id.contains("google/")),
        "[{label}] no anthropic/ or google/ models in the chat list"
    );
    assert!(router.iter().all(|model| model.tier == ChatModelTier::Byok));

    // 5. Switch to a real openrouter/* model.
    let target = router
        .iter()
        .find(|model| model.id.contains("anthropic/"))
        .or_else(|| router.first())
        .map(|model| model.id.clone())
        .expect("an openrouter model");
    let switched = harness
        .chat
        .set_model(&ChatSessionId::Setup, &target)
        .await
        .expect("set_model");
    assert_eq!(
        switched.model.as_ref().map(|model| model.id.as_str()),
        Some(target.as_str())
    );

    // 6. Audit opencode.log, the supervisor log tail and the engine tree.
    let supervisor_logs = harness.supervisor.lock().await.logs();
    let audit = audit_engine(&harness.app_data, &supervisor_logs, &key);
    eprintln!(
        "[{label}] opencode.log: lines={} errors={} leaks={}",
        audit.lines, audit.errors, audit.leaks
    );
    assert_eq!(audit.errors, 0, "[{label}] sidecar log reported errors");
    assert_eq!(audit.leaks, 0, "[{label}] the synthetic key leaked");
    assert!(
        harness
            .supervisor
            .lock()
            .await
            .status()
            .logs
            .iter()
            .all(|line| !line.contains(&key)),
        "[{label}] EngineStatus.logs leaked the key"
    );

    // 7. Disconnect: key removed, sidecar restarted, openrouter models gone.
    let before_disconnect = port;
    let removed = harness
        .supervisor
        .lock()
        .await
        .disconnect_provider(harness.keychain.as_ref(), PROVIDER)
        .await
        .expect("disconnect provider");
    assert_eq!(removed.state, ProviderKeyState::Missing);
    assert!(harness.keychain.get(PROVIDER).unwrap().is_none());
    assert!(
        !harness
            .keychain
            .list()
            .unwrap()
            .contains(&PROVIDER.to_owned())
    );
    let port = harness.port().await;
    assert_ne!(port, before_disconnect, "[{label}] disconnect must restart");
    let after = config_providers(port);
    assert_eq!(
        model_count(&after, PROVIDER),
        0,
        "[{label}] models linger: {after:?}"
    );
    assert!(model_count(&after, "opencode") > 0);
    harness.chat.refresh_setup_models().await;
    let refreshed = harness.chat.set_model(&ChatSessionId::Setup, &target).await;
    assert!(
        refreshed.is_err(),
        "[{label}] removed model still selectable"
    );

    // Final audit after the restarts, then a clean shutdown.
    let supervisor_logs = harness.supervisor.lock().await.logs();
    let audit = audit_engine(&harness.app_data, &supervisor_logs, &key);
    eprintln!(
        "[{label}] final opencode.log: lines={} errors={} leaks={}",
        audit.lines, audit.errors, audit.leaks
    );
    assert_eq!((audit.errors, audit.leaks), (0, 0));
    harness.supervisor.lock().await.stop().await;
}

// ───────────────────────── tests ─────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_sidecar_byok_flow_with_fake_keychain() {
    run_live_flow("fake-keychain", Arc::new(FakeKeychain::new())).await;
}

/// The real OS keychain flow. Set `BYOK_REQUIRE_OS_KEYCHAIN=1` (the runner does) to turn
/// an unavailable keyutils backend into a failure instead of a skip.
#[cfg(feature = "keychain")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_sidecar_byok_flow_with_os_keychain() {
    use spirit_core::engine::OsKeychain;

    let index = tempfile::tempdir().expect("index dir");
    let service = format!("dev.stackspirit.byok-live.{}", ulid::Ulid::generate());
    let keychain = OsKeychain::new(service).with_index(index.path().join("keychain-index.json"));
    // Probe the platform backend with a throwaway value before relying on it.
    let probe = SecretString::new("probe-not-a-secret");
    let usable = keychain.set("probe-provider", &probe).is_ok()
        && keychain
            .get("probe-provider")
            .ok()
            .flatten()
            .is_some_and(|value| value.expose() == "probe-not-a-secret");
    let _ = keychain.delete("probe-provider");
    if !usable {
        assert!(
            std::env::var("BYOK_REQUIRE_OS_KEYCHAIN").as_deref() != Ok("1"),
            "the OS keychain (keyutils) is required but unavailable"
        );
        eprintln!("skipping: platform keychain unavailable");
        return;
    }
    run_live_flow("os-keychain", Arc::new(keychain)).await;
}

/// Happy-path and transient classifications must never touch the saved key.
#[tokio::test]
async fn verification_classifications_keep_the_key_intact() {
    let key = synthetic_key();
    let cases = [
        (KeyVerification::Verified, ProviderKeyState::Saved),
        (KeyVerification::Unverified, ProviderKeyState::Unverified),
        (KeyVerification::Invalid, ProviderKeyState::Invalid),
    ];
    for (engine_result, expected) in cases {
        let keychain = FakeKeychain::new();
        keychain
            .set(PROVIDER, &SecretString::new(key.clone()))
            .unwrap();
        let runtime = FakeEngineRuntime::new().with_key_verification(engine_result);
        let status = verify_provider_key(&keychain, &runtime, Some(1), PROVIDER, Timestamp::now())
            .await
            .unwrap();
        assert_eq!(status.state, expected);
        assert_eq!(keychain.get(PROVIDER).unwrap().unwrap().expose(), key);
    }

    // The classifier maps real provider replies: success → Verified, 401 → Invalid,
    // 429 / outage / malformed → Unverified (never Invalid, never Missing).
    assert_eq!(
        classify_message_response(br#"{"info":{"role":"assistant"},"parts":[]}"#),
        KeyVerification::Verified
    );
    assert_eq!(
        classify_message_response(br#"{"info":{"error":{"data":{"statusCode":401}}}}"#),
        KeyVerification::Invalid
    );
    assert_eq!(
        classify_message_response(br#"{"info":{"error":{"data":{"statusCode":429}}}}"#),
        KeyVerification::Unverified
    );
    assert_eq!(
        classify_message_response(br#"{"info":{"error":{"data":{"statusCode":503}}}}"#),
        KeyVerification::Unverified
    );
    assert_eq!(
        classify_message_response(b"not json"),
        KeyVerification::Unverified
    );

    // A missing key is the only way to get `Missing`.
    let empty = FakeKeychain::new();
    let runtime = FakeEngineRuntime::new();
    let status = verify_provider_key(&empty, &runtime, Some(1), PROVIDER, Timestamp::now())
        .await
        .unwrap();
    assert_eq!(status.state, ProviderKeyState::Missing);
}
