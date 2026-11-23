//! Provider catalog grouping and live model refresh after connecting a provider.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use spirit_core::engine::process::parse_providers;
use spirit_core::engine::process::providers::{
    ProviderCatalog, parse_provider_catalog, provider_rank,
};
use spirit_core::engine::{
    EngineHealth, EngineProvider, EngineRuntime, EngineRuntimeInfo, EngineState, EngineSupervisor,
    EngineSupervisorConfig, FakeKeychain, KeyVerification, Keychain, ProviderKeyState,
    RuntimeModel, SecretString,
};

const PROVIDER_DOCUMENT: &str = r#"{
  "connected": ["opencode"],
  "all": [
    {"id": "zed", "name": "Zed", "models": {}},
    {"id": "openai", "name": "OpenAI", "models": {
      "gpt-5-mini": {"name": "GPT-5 mini", "cost": {"input": 1.25}}
    }},
    {"id": "deepseek", "name": "DeepSeek", "models": {
      "deepseek-flash": {"name": "DeepSeek Flash", "cost": {"input": 1}}
    }},
    {"id": "opencode-go", "name": "OpenCode Go", "models": {
      "go-pro": {"name": "Go Pro", "cost": {"input": 2}}
    }},
    {"id": "opencode", "name": "OpenCode", "models": {
      "big-pickle": {"name": "Big Pickle", "cost": {"input": 0}},
      "iceman-free": {"name": "Iceman Free", "cost": {"input": 0}},
      "paid": {"name": "Paid", "cost": {"input": 2}}
    }},
    {"id": "anthropic", "name": "Anthropic", "models": {
      "claude-sonnet": {"name": "Claude Sonnet", "cost": {"input": 3}}
    }}
  ]
}"#;

fn catalog() -> ProviderCatalog {
    parse_provider_catalog(PROVIDER_DOCUMENT.as_bytes())
}

#[test]
fn provider_catalog_groups_connected_and_available_with_opencode_first() {
    let catalog = catalog();
    let connected: Vec<&str> = catalog
        .connected()
        .iter()
        .map(|provider| provider.id.as_str())
        .collect();
    let available: Vec<&str> = catalog
        .available()
        .iter()
        .map(|provider| provider.id.as_str())
        .collect();

    assert_eq!(connected, ["opencode"]);
    // Popular providers lead the available group; the rest stay alphabetical.
    assert_eq!(
        available,
        ["opencode-go", "anthropic", "openai", "deepseek", "zed"]
    );

    let opencode = catalog.opencode().expect("OpenCode must be in the catalog");
    assert!(opencode.connected);
    assert_eq!(opencode.free_model_count, 2);
    assert_eq!(opencode.model_count, 3);
    assert_eq!(
        catalog
            .search("deep")
            .first()
            .map(|provider| provider.id.as_str()),
        Some("deepseek")
    );
    assert_eq!(catalog.search("opencode").len(), 2);
    assert_eq!(catalog.search("  ").len(), catalog.len());
    assert_eq!(provider_rank("opencode"), 0);
    assert_eq!(provider_rank("zed"), usize::MAX);

    // Only the id and label cross the bridge, in the same display order.
    let bridged = catalog.engine_providers();
    let ids: Vec<&str> = bridged
        .iter()
        .map(|provider| provider.id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "opencode",
            "opencode-go",
            "anthropic",
            "openai",
            "deepseek",
            "zed"
        ]
    );
    assert_eq!(bridged[0].label, "OpenCode");
}

#[test]
fn provider_catalog_marks_every_configured_provider_as_connected() {
    let catalog = parse_provider_catalog(
        br#"{"providers":[{"id":"groq","name":"Groq","models":{}},{"id":"opencode","name":"OpenCode","models":{}}]}"#,
    );
    assert_eq!(catalog.connected().len(), 2);
    assert!(catalog.available().is_empty());
}

#[test]
fn the_engine_process_parser_keeps_the_display_order() {
    let providers = parse_providers(PROVIDER_DOCUMENT.as_bytes());
    let ids: Vec<&str> = providers
        .iter()
        .map(|provider| provider.id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "opencode",
            "opencode-go",
            "anthropic",
            "openai",
            "deepseek",
            "zed"
        ]
    );
    assert!(providers.iter().all(|provider| !provider.label.is_empty()));
}

#[derive(Debug, Default)]
struct RuntimeCounters {
    spawns: usize,
    shutdowns: usize,
}

/// A runtime that mirrors the bundled sidecar: stored keys unlock that provider's
/// models and change the model list on the next spawn, exactly like
/// `OPENCODE_AUTH_CONTENT` does for the real child.
struct KeychainRuntime {
    keychain: Arc<FakeKeychain>,
    providers: Vec<EngineProvider>,
    counters: Mutex<RuntimeCounters>,
}

impl KeychainRuntime {
    fn new(keychain: Arc<FakeKeychain>) -> Self {
        Self {
            keychain,
            providers: catalog().engine_providers(),
            counters: Mutex::new(RuntimeCounters::default()),
        }
    }

    fn configured(&self, provider: &str) -> bool {
        provider == "opencode"
            || self
                .keychain
                .list()
                .unwrap_or_default()
                .iter()
                .any(|configured| configured == provider)
    }

    fn spawn_count(&self) -> usize {
        self.counters
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .spawns
    }
}

#[async_trait]
impl EngineRuntime for KeychainRuntime {
    async fn spawn(&self, port: u16) -> Result<EngineRuntimeInfo, spirit_core::error::SpiritError> {
        self.counters
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .spawns += 1;
        Ok(EngineRuntimeInfo {
            pid: Some(4_242),
            port,
            version: Some("fake-1.19.5".to_owned()),
            binary_path: None,
        })
    }

    async fn probe_health(
        &self,
        _info: &EngineRuntimeInfo,
    ) -> Result<EngineHealth, spirit_core::error::SpiritError> {
        Ok(EngineHealth {
            version: Some("fake-1.19.5".to_owned()),
        })
    }

    async fn shutdown(
        &self,
        _info: &EngineRuntimeInfo,
    ) -> Result<(), spirit_core::error::SpiritError> {
        self.counters
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .shutdowns += 1;
        Ok(())
    }

    async fn models(
        &self,
        _port: u16,
    ) -> Result<Vec<RuntimeModel>, spirit_core::error::SpiritError> {
        let mut models = vec![RuntimeModel {
            id: "opencode/big-pickle".to_owned(),
            provider: "opencode".to_owned(),
            label: "Big Pickle".to_owned(),
            context_tokens: None,
            free: true,
        }];
        if self.configured("deepseek") {
            models.push(RuntimeModel {
                id: "deepseek/deepseek-flash".to_owned(),
                provider: "deepseek".to_owned(),
                label: "DeepSeek Flash".to_owned(),
                context_tokens: Some(128_000),
                free: false,
            });
        }
        Ok(models)
    }

    async fn providers(
        &self,
        _port: u16,
    ) -> Result<Vec<EngineProvider>, spirit_core::error::SpiritError> {
        Ok(self.providers.clone())
    }

    async fn verify_key(
        &self,
        _port: u16,
        _provider: &str,
        _secret: &SecretString,
    ) -> KeyVerification {
        KeyVerification::Unverified
    }
}

fn supervisor_config() -> EngineSupervisorConfig {
    EngineSupervisorConfig {
        restart_budget: 3,
        health_attempts: 1,
        retry_delay: Duration::ZERO,
        port: 0,
    }
}

#[tokio::test]
async fn refresh_catalog_rereads_a_live_engine_without_respawning_it() {
    let keychain = Arc::new(FakeKeychain::new());
    let runtime = Arc::new(KeychainRuntime::new(keychain.clone()));
    let mut supervisor = EngineSupervisor::new(runtime.clone(), supervisor_config());

    assert!(matches!(supervisor.start().await.state, EngineState::Ready));
    assert_eq!(runtime.spawn_count(), 1);

    let refreshed = supervisor.refresh_catalog().await;
    assert!(matches!(refreshed.state, EngineState::Ready));
    assert_eq!(runtime.spawn_count(), 1, "refresh must not respawn");
    assert_eq!(
        refreshed
            .providers
            .iter()
            .map(|provider| provider.id.as_str())
            .collect::<Vec<_>>(),
        [
            "opencode",
            "opencode-go",
            "anthropic",
            "openai",
            "deepseek",
            "zed"
        ]
    );
}

#[tokio::test]
async fn connecting_a_provider_unlocks_its_models_without_an_app_restart() {
    let keychain = Arc::new(FakeKeychain::new());
    let runtime = Arc::new(KeychainRuntime::new(keychain.clone()));
    let mut supervisor = EngineSupervisor::new(runtime.clone(), supervisor_config());

    let started = supervisor.start().await;
    assert!(matches!(started.state, EngineState::Ready));
    assert_eq!(runtime.spawn_count(), 1);
    assert_eq!(supervisor.runtime_models().await.len(), 1);
    assert_eq!(
        started
            .providers
            .iter()
            .map(|provider| provider.id.as_str())
            .collect::<Vec<_>>(),
        [
            "opencode",
            "opencode-go",
            "anthropic",
            "openai",
            "deepseek",
            "zed"
        ]
    );

    let secret = SecretString::new("not-a-real-provider-key");
    let saved = supervisor
        .connect_provider(keychain.as_ref(), "deepseek", &secret)
        .await
        .unwrap();
    assert_eq!(saved.state, ProviderKeyState::Saved);
    assert_eq!(runtime.spawn_count(), 2, "the sidecar must respawn once");

    let refreshed = supervisor.status();
    assert!(matches!(refreshed.state, EngineState::Ready));
    let models = supervisor.runtime_models().await;
    assert!(
        models
            .iter()
            .any(|model| model.id == "deepseek/deepseek-flash"),
        "the newly unlocked model must be visible immediately: {models:?}"
    );
    assert_eq!(
        keychain.get("deepseek").unwrap().unwrap().expose(),
        "not-a-real-provider-key"
    );
    let serialized = serde_json::to_string(&refreshed).unwrap();
    assert!(!serialized.contains("not-a-real-provider-key"));

    let removed = supervisor
        .disconnect_provider(keychain.as_ref(), "deepseek")
        .await
        .unwrap();
    assert_eq!(removed.state, ProviderKeyState::Missing);
    assert!(
        !supervisor
            .runtime_models()
            .await
            .iter()
            .any(|model| model.id == "deepseek/deepseek-flash"),
        "removing the key must drop the unlocked model list"
    );
    assert_eq!(runtime.spawn_count(), 3);
}

#[tokio::test]
async fn the_core_connect_path_the_desktop_commands_call_refreshes_models_and_status() {
    use spirit_core::SpiritCore;
    use spirit_core::host::InMemoryHostStore;

    let keychain = Arc::new(FakeKeychain::new());
    let runtime = Arc::new(KeychainRuntime::new(keychain.clone()));
    let supervisor = EngineSupervisor::new(runtime.clone(), supervisor_config());
    let mut core = SpiritCore::new(Arc::new(InMemoryHostStore::new()), supervisor)
        .with_keychain(keychain.clone());
    let mut events = core.subscribe();

    let started = core.start_engine().await;
    let opencode = started
        .providers
        .iter()
        .find(|provider| provider.id == "opencode")
        .unwrap();
    assert!(opencode.connected, "the engine's connected fact must cross");
    assert!(
        !started
            .providers
            .iter()
            .find(|provider| provider.id == "deepseek")
            .unwrap()
            .connected
    );

    let secret = SecretString::new("not-a-real-provider-key");
    let saved = core.connect_provider("deepseek", &secret).await.unwrap();
    assert_eq!(saved.state, "saved");
    assert_eq!(runtime.spawn_count(), 2);
    assert!(
        keychain
            .list()
            .unwrap()
            .iter()
            .any(|provider| provider == "deepseek")
    );
    let json = serde_json::to_string(&core.engine_status()).unwrap();
    assert!(!json.contains("not-a-real-provider-key"));
    // Starting and the connect restart each published an engine event.
    let mut published = 0;
    while events.try_recv().is_ok() {
        published += 1;
    }
    assert!(published >= 2, "connect must publish the refreshed status");

    let removed = core.disconnect_provider("deepseek").await.unwrap();
    assert_eq!(removed.state, "missing");
    assert!(keychain.list().unwrap().is_empty());
    // Removing a key that is already gone stays idempotent.
    assert_eq!(
        core.disconnect_provider("deepseek").await.unwrap().state,
        "missing"
    );
}

#[test]
fn openrouter_verification_probes_paid_models_before_rate_limited_free_ones() {
    let document = br#"{"providers":[{"id":"openrouter","models":{
        "meta/free-a:free": {"cost": {"input": 0}},
        "vendor/cheap": {"cost": {"input": 0.1}},
        "vendor/pricey": {"cost": {"input": 3}}
    }}]}"#;
    let chosen = spirit_core::engine::process::select_verification_models(document, "openrouter");
    assert_eq!(
        chosen,
        ["vendor/cheap", "vendor/pricey", "meta/free-a:free"]
    );
}

#[test]
fn openrouter_key_verdicts_follow_the_provider_answer_not_the_first_non_200() {
    use spirit_core::engine::process::classify_message_response;
    let ok = br#"{"info":{"role":"assistant"},"parts":[{"type":"text","text":"OK"}]}"#;
    assert_eq!(classify_message_response(ok), KeyVerification::Verified);
    let rejected = br#"{"info":{"error":{"statusCode":401}}}"#;
    assert_eq!(
        classify_message_response(rejected),
        KeyVerification::Invalid
    );
    // A model-level 402/429 is not a verdict on the key.
    let limited = br#"{"info":{"error":{"statusCode":429}}}"#;
    assert_eq!(
        classify_message_response(limited),
        KeyVerification::Unverified
    );
}
