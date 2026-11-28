//! Phase 1b harness chat router tests.
//!
//! Every test uses a fake `ChatTransport` delegate that only records calls. No test
//! spawns a child, opens a socket, starts an engine or writes a harness manifest; the
//! registry only reads built-ins or a temporary `chat.json`.

use std::fs;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use spirit_core::engine::chat::{
    CHAT_PREFERENCES_VERSION, ChatAccess, ChatLogPage, ChatLogQuery, ChatMessage, ChatPreferences,
    ChatPreferencesSource, ChatPreferencesStore, ChatSendReceipt, ChatSessionId,
    ChatSessionLocation, ChatSessionState, ChatSessionView, ChatStartInput, ChatTransport,
    HarnessChatGate, HarnessChatProvider, JsonChatPreferences,
};
use spirit_core::error::{SpiritError, SpiritErrorCode};
use spirit_core::harnesses::{HarnessProtocol, HarnessRegistry, builtin_harnesses};
use spirit_core::time::Timestamp;

fn setup_input() -> ChatStartInput {
    ChatStartInput {
        session: ChatSessionId::Setup,
        project: None,
        location: ChatSessionLocation::Local,
        resume: None,
    }
}

fn global_input() -> ChatStartInput {
    ChatStartInput {
        session: ChatSessionId::Global,
        project: None,
        location: ChatSessionLocation::Local,
        resume: None,
    }
}

fn project_input(project: &str) -> ChatStartInput {
    ChatStartInput {
        session: ChatSessionId::Project(project.to_owned()),
        project: Some(project.to_owned()),
        location: ChatSessionLocation::Local,
        resume: None,
    }
}

fn preferences(id: Option<&str>) -> ChatPreferences {
    ChatPreferences {
        version: CHAT_PREFERENCES_VERSION,
        default_harness_id: id.map(str::to_owned),
    }
}

fn ready_gate() -> HarnessChatGate {
    HarnessChatGate::Ready {
        harness_id: "opencode".to_owned(),
        protocol: HarnessProtocol::OpencodeHttp,
    }
}

fn ready_view(session: &ChatSessionId) -> ChatSessionView {
    ChatSessionView {
        session: session.clone(),
        location: ChatSessionLocation::Local,
        state: ChatSessionState::Ready,
        attached: true,
        streaming_supported: true,
        model: None,
        available_models: None,
        usage: None,
        started_at: None,
        last_event_at: None,
        error: None,
        pending_permissions: None,
        retry: None,
        ownership: None,
        access: ChatAccess::Owner,
        writer: None,
    }
}

struct FakeTransport {
    calls: Mutex<Vec<String>>,
    start_error: Option<SpiritError>,
}

impl FakeTransport {
    fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            start_error: None,
        }
    }

    fn failing_start(error: SpiritError) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            start_error: Some(error),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn record(&self, call: String) {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(call);
    }
}

fn assert_calls(delegate: &FakeTransport, expected: &[&str]) {
    let expected: Vec<String> = expected.iter().map(|call| (*call).to_owned()).collect();
    assert_eq!(delegate.calls(), expected);
}

#[async_trait]
impl ChatTransport for FakeTransport {
    async fn start(&self, input: ChatStartInput) -> Result<ChatSessionView, SpiritError> {
        let session = input.session.as_wire();
        self.record(format!("start:{session}"));
        if let Some(error) = &self.start_error {
            return Err(error.clone());
        }
        Ok(ready_view(&input.session))
    }

    async fn attach(&self, session: &ChatSessionId) -> Result<ChatSessionView, SpiritError> {
        let name = session.as_wire();
        self.record(format!("attach:{name}"));
        Ok(ready_view(session))
    }

    async fn send(
        &self,
        session: &ChatSessionId,
        text: &str,
    ) -> Result<ChatSendReceipt, SpiritError> {
        let name = session.as_wire();
        self.record(format!("send:{name}"));
        Ok(ChatSendReceipt {
            session: session.clone(),
            message: ChatMessage::user("msg_test", text, Timestamp::epoch()),
            accepted: true,
        })
    }

    async fn send_turn(
        &self,
        session: &ChatSessionId,
        text: &str,
        _variant: Option<&str>,
        _attachments: &[spirit_core::engine::chat::ChatAttachment],
    ) -> Result<ChatSendReceipt, SpiritError> {
        let name = session.as_wire();
        self.record(format!("send_turn:{name}"));
        Ok(ChatSendReceipt {
            session: session.clone(),
            message: ChatMessage::user("msg_test", text, Timestamp::epoch()),
            accepted: true,
        })
    }

    async fn stop(&self, session: &ChatSessionId) -> Result<ChatSessionView, SpiritError> {
        let name = session.as_wire();
        self.record(format!("stop:{name}"));
        Ok(ready_view(session))
    }

    async fn provision_orchestrator(&self, project: &str) -> Result<ChatSessionView, SpiritError> {
        self.record(format!("provision_orchestrator:{project}"));
        Ok(ready_view(&ChatSessionId::Project(project.to_owned())))
    }

    async fn set_model(
        &self,
        session: &ChatSessionId,
        model_id: &str,
    ) -> Result<ChatSessionView, SpiritError> {
        let name = session.as_wire();
        self.record(format!("set_model:{name}:{model_id}"));
        Ok(ready_view(session))
    }

    async fn log(
        &self,
        session: &ChatSessionId,
        _query: &ChatLogQuery,
    ) -> Result<ChatLogPage, SpiritError> {
        let name = session.as_wire();
        self.record(format!("log:{name}"));
        Ok(ChatLogPage {
            messages: Vec::new(),
            next: None,
            truncated: false,
        })
    }
}

#[tokio::test]
async fn fresh_and_missing_preferences_start_setup_once_on_opencode() {
    let directory = tempfile::tempdir().unwrap();
    let registry = Arc::new(HarnessRegistry::new(directory.path().join("harnesses")));

    let delegate = Arc::new(FakeTransport::new());
    let provider = HarnessChatProvider::new(
        delegate.clone(),
        Arc::new(ChatPreferences::default()),
        registry.clone(),
    );
    let view = provider.start(setup_input()).await.unwrap();
    assert_eq!(view.session, ChatSessionId::Setup);
    assert_calls(&delegate, &["start:setup"]);
    let capture = provider
        .capture(&ChatSessionId::Setup)
        .expect("a ready start captures the harness");
    assert_eq!(capture.harness_id(), "opencode");
    assert_eq!(capture.protocol(), HarnessProtocol::OpencodeHttp);
    assert_eq!(provider.gate(), ready_gate());

    // A JsonChatPreferences with no file is the documented legacy OpenCode default.
    let missing_store = JsonChatPreferences::new(directory.path().join("chat.json"));
    assert!(!missing_store.path().exists());
    let missing_delegate = Arc::new(FakeTransport::new());
    let missing =
        HarnessChatProvider::new(missing_delegate.clone(), Arc::new(missing_store), registry);
    missing.start(setup_input()).await.unwrap();
    assert_calls(&missing_delegate, &["start:setup"]);
    assert_eq!(
        missing
            .capture(&ChatSessionId::Setup)
            .expect("a ready start captures the harness")
            .harness_id(),
        "opencode"
    );
}

#[tokio::test]
async fn acp_default_gates_before_the_delegate_and_stores_no_capture() {
    let directory = tempfile::tempdir().unwrap();
    let registry = Arc::new(HarnessRegistry::new(directory.path().join("harnesses")));
    let delegate = Arc::new(FakeTransport::new());
    let provider = HarnessChatProvider::new(
        delegate.clone(),
        Arc::new(preferences(Some("goose"))),
        registry,
    );

    let error = provider.start(setup_input()).await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Gate);
    assert_eq!(error.message, "harness_not_wired");
    assert_calls(&delegate, &[]);
    assert!(provider.capture(&ChatSessionId::Setup).is_none());
    assert_eq!(
        provider.gate(),
        HarnessChatGate::HarnessNotWired {
            harness_id: "goose".to_owned(),
            protocol: HarnessProtocol::Acp,
        }
    );
}

#[tokio::test]
async fn gate_reasons_are_stable_for_workers_unknown_and_unreadable_settings() {
    let directory = tempfile::tempdir().unwrap();
    let registry = Arc::new(HarnessRegistry::new(directory.path().join("harnesses")));
    let settings = JsonChatPreferences::new(directory.path().join("chat.json"));

    let delegate = Arc::new(FakeTransport::new());
    let provider = HarnessChatProvider::new(
        delegate.clone(),
        Arc::new(preferences(Some("claude-code"))),
        registry.clone(),
    );
    let error = provider.start(setup_input()).await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Gate);
    assert_eq!(error.message, "harness_worker_only");
    assert_calls(&delegate, &[]);
    assert!(provider.capture(&ChatSessionId::Setup).is_none());

    let delegate = Arc::new(FakeTransport::new());
    let provider = HarnessChatProvider::new(
        delegate.clone(),
        Arc::new(preferences(Some("not-registered"))),
        registry.clone(),
    );
    let error = provider.start(setup_input()).await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Gate);
    assert_eq!(error.message, "harness_missing");
    assert_calls(&delegate, &[]);
    assert!(provider.capture(&ChatSessionId::Setup).is_none());

    let corrupt = br#"{not valid json"#;
    fs::write(settings.path(), corrupt).unwrap();
    let delegate = Arc::new(FakeTransport::new());
    let provider = HarnessChatProvider::new(
        delegate.clone(),
        Arc::new(settings.clone()),
        registry.clone(),
    );
    let error = provider.start(setup_input()).await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Gate);
    assert_eq!(error.message, "chat_settings_unreadable");
    assert_calls(&delegate, &[]);
    assert!(provider.capture(&ChatSessionId::Setup).is_none());
    assert_eq!(fs::read(settings.path()).unwrap(), corrupt);

    let future = br#"{"version":99,"default_harness_id":"opencode"}"#;
    fs::write(settings.path(), future).unwrap();
    let delegate = Arc::new(FakeTransport::new());
    let provider = HarnessChatProvider::new(delegate.clone(), Arc::new(settings), registry);
    let error = provider.start(setup_input()).await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Gate);
    assert_eq!(error.message, "chat_settings_unreadable");
    assert_calls(&delegate, &[]);
    assert!(provider.capture(&ChatSessionId::Setup).is_none());
    assert_eq!(
        fs::read(directory.path().join("chat.json")).unwrap(),
        future
    );
}

#[tokio::test]
async fn changing_the_default_does_not_rebind_a_captured_setup_session() {
    let directory = tempfile::tempdir().unwrap();
    let registry = Arc::new(HarnessRegistry::new(directory.path().join("harnesses")));
    let settings = Arc::new(JsonChatPreferences::new(directory.path().join("chat.json")));
    settings.save(&preferences(Some("opencode"))).unwrap();

    let delegate = Arc::new(FakeTransport::new());
    let provider = HarnessChatProvider::new(delegate.clone(), settings.clone(), registry);
    provider.start(setup_input()).await.unwrap();
    let captured = provider
        .capture(&ChatSessionId::Setup)
        .expect("a ready start captures the harness");
    assert_eq!(captured.harness_id(), "opencode");
    assert_eq!(captured.protocol(), HarnessProtocol::OpencodeHttp);

    settings.save(&preferences(Some("goose"))).unwrap();
    assert_eq!(
        provider.gate(),
        HarnessChatGate::HarnessNotWired {
            harness_id: "goose".to_owned(),
            protocol: HarnessProtocol::Acp,
        }
    );

    provider.attach(&ChatSessionId::Setup).await.unwrap();
    assert_calls(&delegate, &["start:setup", "attach:setup"]);
    assert_eq!(
        provider.capture(&ChatSessionId::Setup).unwrap(),
        captured,
        "a default change must not rebind an existing setup session"
    );
}

#[tokio::test]
async fn a_legacy_setup_attach_is_treated_as_opencode() {
    let directory = tempfile::tempdir().unwrap();
    let registry = Arc::new(HarnessRegistry::new(directory.path().join("harnesses")));
    let delegate = Arc::new(FakeTransport::new());
    let provider = HarnessChatProvider::new(
        delegate.clone(),
        Arc::new(preferences(Some("goose"))),
        registry,
    );

    provider.attach(&ChatSessionId::Setup).await.unwrap();
    assert_calls(&delegate, &["attach:setup"]);
    let capture = provider
        .capture(&ChatSessionId::Setup)
        .expect("a legacy setup session is OpenCode");
    assert_eq!(capture.harness_id(), "opencode");
    assert_eq!(capture.protocol(), HarnessProtocol::OpencodeHttp);
}

struct PanickingPreferences;

impl ChatPreferencesSource for PanickingPreferences {
    fn resolve(&self) -> Result<ChatPreferences, spirit_core::engine::chat::ChatSettingsError> {
        panic!("VOWP sessions must not consult the harness default");
    }
}

#[tokio::test]
async fn vowp_sessions_and_provisioning_bypass_the_harness_default() {
    let directory = tempfile::tempdir().unwrap();
    let registry = Arc::new(HarnessRegistry::new(directory.path().join("harnesses")));
    let delegate = Arc::new(FakeTransport::new());
    let provider =
        HarnessChatProvider::new(delegate.clone(), Arc::new(PanickingPreferences), registry);

    let global = provider.start(global_input()).await.unwrap();
    let project = provider.start(project_input("demo")).await.unwrap();
    let provisioned = provider.provision_orchestrator("demo").await.unwrap();

    assert_eq!(global.session, ChatSessionId::Global);
    assert_eq!(project.session, ChatSessionId::Project("demo".to_owned()));
    assert_eq!(
        provisioned.session,
        ChatSessionId::Project("demo".to_owned())
    );
    assert_calls(
        &delegate,
        &[
            "start:global",
            "start:project:demo",
            "provision_orchestrator:demo",
        ],
    );
    assert!(provider.capture(&ChatSessionId::Global).is_none());
    assert!(
        provider
            .capture(&ChatSessionId::Project("demo".to_owned()))
            .is_none()
    );
}

#[tokio::test]
async fn delegate_errors_pass_through_unchanged_and_do_not_capture() {
    let expected = SpiritError::new(SpiritErrorCode::Engine, "engine: the bundled engine exited")
        .with_hint("Restart the setup assistant.");
    let directory = tempfile::tempdir().unwrap();
    let registry = Arc::new(HarnessRegistry::new(directory.path().join("harnesses")));
    let delegate = Arc::new(FakeTransport::failing_start(expected.clone()));
    let provider = HarnessChatProvider::new(
        delegate.clone(),
        Arc::new(ChatPreferences::default()),
        registry,
    );

    let error = provider.start(setup_input()).await.unwrap_err();
    assert_eq!(error, expected);
    assert_calls(&delegate, &["start:setup"]);
    assert!(provider.capture(&ChatSessionId::Setup).is_none());
}

#[tokio::test]
async fn gate_errors_do_not_leak_the_registry_path_or_prose() {
    let directory = tempfile::tempdir().unwrap();
    let registry = HarnessRegistry::new(directory.path().join("harnesses"));
    let mut manifest = builtin_harnesses()
        .into_iter()
        .find(|manifest| manifest.id == "goose")
        .expect("the goose builtin exists");
    manifest.id = "fixture-acp".to_owned();
    manifest.name = "Fixture ACP".to_owned();
    manifest.tos_note = "fixture-secret-prose-marker".to_owned();
    manifest.builtin = false;
    registry.save_custom(manifest).unwrap();

    let provider = HarnessChatProvider::new(
        Arc::new(FakeTransport::new()),
        Arc::new(preferences(Some("fixture-acp"))),
        Arc::new(registry),
    );
    let error = provider.start(setup_input()).await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Gate);
    assert_eq!(error.message, "harness_not_wired");

    let rendered = format!("{error:?}");
    assert!(!rendered.contains(directory.path().to_string_lossy().as_ref()));
    assert!(!rendered.contains("fixture-secret-prose-marker"));
    assert!(!rendered.contains("http"));
    assert!(!rendered.contains("://"));
    assert!(!rendered.contains("sk-"));
}

#[tokio::test]
async fn turn_and_read_methods_delegate_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let registry = Arc::new(HarnessRegistry::new(directory.path().join("harnesses")));
    let delegate = Arc::new(FakeTransport::new());
    let provider = HarnessChatProvider::new(
        delegate.clone(),
        Arc::new(ChatPreferences::default()),
        registry,
    );

    let receipt = provider.send(&ChatSessionId::Setup, "hello").await.unwrap();
    assert_eq!(receipt.session, ChatSessionId::Setup);
    provider
        .send_turn(&ChatSessionId::Setup, "hello", Some("high"), &[])
        .await
        .unwrap();
    provider.stop(&ChatSessionId::Setup).await.unwrap();
    provider
        .set_model(&ChatSessionId::Setup, "opencode/model")
        .await
        .unwrap();
    provider
        .log(&ChatSessionId::Setup, &ChatLogQuery::default())
        .await
        .unwrap();

    assert_calls(
        &delegate,
        &[
            "send:setup",
            "send_turn:setup",
            "stop:setup",
            "set_model:setup:opencode/model",
            "log:setup",
        ],
    );
}
