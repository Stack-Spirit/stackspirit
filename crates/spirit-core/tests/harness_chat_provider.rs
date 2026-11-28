//! Phase 2b: the router runs an ACP Setup chat through a registered adapter.
//!
//! A fake `HarnessChatAdapter` records calls and emits scripted events. No real process,
//! socket, keychain or engine is touched.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::{broadcast, mpsc};

use spirit_core::engine::chat::{
    CHAT_PREFERENCES_VERSION, ChatAccess, ChatEvent, ChatLogPage, ChatLogQuery, ChatMessage,
    ChatPreferences, ChatSendReceipt, ChatSessionId, ChatSessionLocation, ChatSessionState,
    ChatSessionView, ChatStartInput, ChatToolActivity, ChatToolStatus, ChatTransport, HarnessAbort,
    HarnessAccepted, HarnessChatAdapter, HarnessChatGate, HarnessChatProvider, HarnessEvent,
    HarnessNativeId, HarnessSession, HarnessStart, HarnessTurn,
};
use spirit_core::error::{SpiritError, SpiritErrorCode};
use spirit_core::harnesses::{HarnessProtocol, HarnessRegistry};
use spirit_core::time::Timestamp;

const NATIVE_ID: &str = "native-188-secret";
const WORKSPACE: &str = "/tmp/ss-188-private-workspace";

#[derive(Default)]
struct FakeAdapter {
    calls: Mutex<Vec<&'static str>>,
    starts: AtomicUsize,
    hold_turn: bool,
    confirm_stop: bool,
}

impl FakeAdapter {
    fn calls(&self) -> Vec<&'static str> {
        self.calls.lock().unwrap().clone()
    }
    fn count(&self, name: &str) -> usize {
        self.calls().iter().filter(|c| **c == name).count()
    }
}

#[async_trait]
impl HarnessChatAdapter for FakeAdapter {
    fn harness_id(&self) -> &str {
        "goose"
    }
    fn protocol(&self) -> HarnessProtocol {
        HarnessProtocol::Acp
    }
    async fn preflight(&self) -> Result<HarnessChatGate, SpiritError> {
        Ok(HarnessChatGate::Ready {
            harness_id: "goose".to_owned(),
            protocol: HarnessProtocol::Acp,
        })
    }
    async fn start(&self, input: HarnessStart) -> Result<HarnessSession, SpiritError> {
        assert_eq!(input.workspace().to_str(), Some(WORKSPACE));
        self.starts.fetch_add(1, Ordering::SeqCst);
        self.calls.lock().unwrap().push("start");
        Ok(HarnessSession::new(
            "goose",
            HarnessProtocol::Acp,
            Some(HarnessNativeId::new(NATIVE_ID)),
        ))
    }
    async fn send_turn(
        &self,
        _session: &HarnessSession,
        turn: HarnessTurn,
        events: mpsc::Sender<HarnessEvent>,
    ) -> Result<HarnessAccepted, SpiritError> {
        self.calls.lock().unwrap().push("send");
        let message_id = "msg_assistant".to_owned();
        assert!(!turn.text().is_empty());
        if self.hold_turn {
            // Keep the sender alive until the test aborts; the turn never ends.
            tokio::spawn(async move {
                let _events = events;
                tokio::time::sleep(Duration::from_secs(30)).await;
            });
            return Ok(HarnessAccepted { message_id });
        }
        let id = message_id.clone();
        tokio::spawn(async move {
            let script = vec![
                HarnessEvent::MessageStart {
                    message_id: id.clone(),
                },
                HarnessEvent::TextDelta {
                    message_id: id.clone(),
                    part_id: None,
                    text: "Hello".to_owned(),
                },
                HarnessEvent::Tool {
                    activity: ChatToolActivity {
                        id: "tool_1".to_owned(),
                        name: "read".to_owned(),
                        status: ChatToolStatus::Completed,
                        summary: None,
                    },
                },
                HarnessEvent::TextDelta {
                    message_id: id.clone(),
                    part_id: None,
                    text: " world".to_owned(),
                },
                HarnessEvent::Done {
                    message_id: id,
                    truncated: false,
                },
            ];
            for event in script {
                if events.send(event).await.is_err() {
                    return;
                }
            }
        });
        Ok(HarnessAccepted { message_id })
    }
    async fn abort(&self, _session: &HarnessSession) -> Result<HarnessAbort, SpiritError> {
        self.calls.lock().unwrap().push("abort");
        Ok(if self.confirm_stop {
            HarnessAbort::stopped()
        } else {
            HarnessAbort::unconfirmed()
        })
    }
    async fn close(&self, _session: &HarnessSession) -> Result<(), SpiritError> {
        self.calls.lock().unwrap().push("close");
        Ok(())
    }
}

#[derive(Default)]
struct FakeDelegate {
    calls: Mutex<Vec<String>>,
}

impl FakeDelegate {
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
    fn rec(&self, s: String) {
        self.calls.lock().unwrap().push(s);
    }
}

fn view(session: &ChatSessionId) -> ChatSessionView {
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

#[async_trait]
impl ChatTransport for FakeDelegate {
    async fn start(&self, input: ChatStartInput) -> Result<ChatSessionView, SpiritError> {
        self.rec(format!("start:{}", input.session.as_wire()));
        Ok(view(&input.session))
    }
    async fn attach(&self, session: &ChatSessionId) -> Result<ChatSessionView, SpiritError> {
        self.rec(format!("attach:{}", session.as_wire()));
        Ok(view(session))
    }
    async fn send(
        &self,
        session: &ChatSessionId,
        text: &str,
    ) -> Result<ChatSendReceipt, SpiritError> {
        self.rec(format!("send:{}", session.as_wire()));
        Ok(ChatSendReceipt {
            session: session.clone(),
            message: ChatMessage::user("msg_d", text, Timestamp::epoch()),
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
        self.rec(format!("send_turn:{}", session.as_wire()));
        Ok(ChatSendReceipt {
            session: session.clone(),
            message: ChatMessage::user("msg_d", text, Timestamp::epoch()),
            accepted: true,
        })
    }
    async fn stop(&self, session: &ChatSessionId) -> Result<ChatSessionView, SpiritError> {
        self.rec(format!("stop:{}", session.as_wire()));
        Ok(view(session))
    }
    async fn provision_orchestrator(&self, project: &str) -> Result<ChatSessionView, SpiritError> {
        self.rec(format!("provision:{project}"));
        Ok(view(&ChatSessionId::Project(project.to_owned())))
    }
    async fn set_model(
        &self,
        session: &ChatSessionId,
        _m: &str,
    ) -> Result<ChatSessionView, SpiritError> {
        self.rec(format!("set_model:{}", session.as_wire()));
        Ok(view(session))
    }
    async fn log(
        &self,
        session: &ChatSessionId,
        _q: &ChatLogQuery,
    ) -> Result<ChatLogPage, SpiritError> {
        self.rec(format!("log:{}", session.as_wire()));
        Ok(ChatLogPage {
            messages: Vec::new(),
            next: None,
            truncated: false,
        })
    }
}

fn prefs(id: Option<&str>) -> Arc<ChatPreferences> {
    Arc::new(ChatPreferences {
        version: CHAT_PREFERENCES_VERSION,
        default_harness_id: id.map(str::to_owned),
    })
}

fn setup() -> ChatStartInput {
    ChatStartInput {
        session: ChatSessionId::Setup,
        project: None,
        location: ChatSessionLocation::Local,
        resume: None,
    }
}

struct Rig {
    provider: HarnessChatProvider,
    delegate: Arc<FakeDelegate>,
    _dir: tempfile::TempDir,
}

fn rig(selected: Option<&str>, adapter: Option<Arc<FakeAdapter>>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(HarnessRegistry::new(dir.path().join("harnesses")));
    let delegate = Arc::new(FakeDelegate::default());
    let mut provider = HarnessChatProvider::new(delegate.clone(), prefs(selected), registry);
    if let Some(adapter) = adapter {
        provider = provider.with_adapter(adapter, WORKSPACE);
    }
    Rig {
        provider,
        delegate,
        _dir: dir,
    }
}

async fn next_until<F: Fn(&ChatEvent) -> bool>(
    rx: &mut broadcast::Receiver<ChatEvent>,
    seen: &mut Vec<ChatEvent>,
    stop: F,
) {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("event in time")
            .unwrap();
        let done = stop(&event);
        seen.push(event);
        if done {
            return;
        }
    }
}

fn assert_clean(text: &str) {
    for needle in [NATIVE_ID, WORKSPACE, "ss-188-private"] {
        assert!(!text.contains(needle), "leaked {needle} in {text}");
    }
}

#[tokio::test]
async fn without_an_adapter_acp_stays_gated_and_makes_no_record() {
    let rig = rig(Some("goose"), None);
    assert_eq!(rig.provider.gate().reason(), "harness_not_wired");
    let error = rig.provider.start(setup()).await.unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Gate);
    assert_eq!(error.message, "harness_not_wired");
    assert!(rig.provider.capture(&ChatSessionId::Setup).is_none());
    assert!(rig.delegate.calls().is_empty());
    // No adapter: a legacy setup attach keeps delegating to OpenCode.
    rig.provider.attach(&ChatSessionId::Setup).await.unwrap();
    assert_eq!(rig.delegate.calls(), vec!["attach:setup"]);
}

#[tokio::test]
async fn adapter_ready_attach_without_a_record_is_session_lost() {
    let rig = rig(Some("goose"), Some(Arc::new(FakeAdapter::default())));
    let lost = rig
        .provider
        .attach(&ChatSessionId::Setup)
        .await
        .unwrap_err();
    assert!(lost.message.starts_with("harness_session_lost"));
    assert!(rig.delegate.calls().is_empty());
}

#[tokio::test]
async fn acp_setup_start_send_log_stop_and_shutdown() {
    let adapter = Arc::new(FakeAdapter {
        confirm_stop: true,
        ..Default::default()
    });
    let rig = rig(Some("goose"), Some(adapter.clone()));
    let mut rx = rig.provider.subscribe();
    assert!(rig.provider.gate().is_ready());

    let started = rig.provider.start(setup()).await.unwrap();
    assert_eq!(started.state, ChatSessionState::Ready);
    assert!(started.attached && started.streaming_supported);
    assert_eq!(started.access, ChatAccess::Owner);
    assert!(started.model.is_none() && started.usage.is_none() && started.error.is_none());
    let again = rig.provider.start(setup()).await.unwrap();
    assert_eq!(again.started_at, started.started_at);
    assert_eq!(adapter.starts.load(Ordering::SeqCst), 1);
    let capture = rig.provider.capture(&ChatSessionId::Setup).unwrap();
    assert_eq!(capture.protocol(), HarnessProtocol::Acp);
    assert!(rig.provider.models().is_none());
    assert!(rig.delegate.calls().is_empty());
    assert_eq!(
        rig.provider
            .attach(&ChatSessionId::Setup)
            .await
            .unwrap()
            .session,
        ChatSessionId::Setup
    );

    let receipt = rig
        .provider
        .send(&ChatSessionId::Setup, "  hi there  ")
        .await
        .unwrap();
    assert!(receipt.accepted);
    assert_eq!(receipt.message.text, vec!["hi there".to_owned()]);

    let mut seen = Vec::new();
    next_until(&mut rx, &mut seen, |e| matches!(e, ChatEvent::Done { .. })).await;
    let revisions: Vec<u64> = seen.iter().map(ChatEvent::revision).collect();
    assert!(revisions.windows(2).all(|w| w[0] < w[1]), "{revisions:?}");
    let deltas: Vec<&str> = seen
        .iter()
        .filter_map(|e| match e {
            ChatEvent::Delta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas, vec!["Hello", " world"]);
    // The follow-up session event lands right after Done.
    next_until(&mut rx, &mut seen, |e| {
        matches!(e, ChatEvent::Session { .. })
    })
    .await;

    let page = rig
        .provider
        .log(&ChatSessionId::Setup, &ChatLogQuery::default())
        .await
        .unwrap();
    assert_eq!(page.messages.len(), 2);
    assert_eq!(page.messages[0].text, vec!["hi there".to_owned()]);
    assert_eq!(page.messages[1].text, vec!["Hello world".to_owned()]);
    assert!(!page.messages[1].streaming);
    assert_eq!(page.messages[1].tools.as_ref().map(Vec::len), Some(1));
    let small = rig
        .provider
        .log(
            &ChatSessionId::Setup,
            &ChatLogQuery {
                cursor: None,
                limit: Some(1),
            },
        )
        .await
        .unwrap();
    assert_eq!(small.messages.len(), 1);
    let older = rig
        .provider
        .log(
            &ChatSessionId::Setup,
            &ChatLogQuery {
                cursor: small.next,
                limit: Some(1),
            },
        )
        .await
        .unwrap();
    assert_eq!(older.messages[0].text, vec!["hi there".to_owned()]);
    assert!(older.next.is_none());

    let model = rig
        .provider
        .set_model(&ChatSessionId::Setup, "x/y")
        .await
        .unwrap_err();
    assert!(model.message.starts_with("chat_model_gated"));

    // Stop while idle is an honest no-op that does not call the adapter.
    let idle = rig.provider.stop(&ChatSessionId::Setup).await.unwrap();
    assert_eq!(idle.state, ChatSessionState::Ready);
    assert_eq!(adapter.count("abort"), 0);

    rig.provider.shutdown().await;
    rig.provider.shutdown().await;
    assert_eq!(adapter.count("close"), 1);
    let lost = rig
        .provider
        .log(&ChatSessionId::Setup, &ChatLogQuery::default())
        .await;
    assert!(
        lost.unwrap_err()
            .message
            .starts_with("harness_session_lost")
    );

    let mut dump = format!("{started:?}{receipt:?}{page:?}{seen:?}");
    dump.push_str(&serde_json::to_string(&seen).unwrap());
    dump.push_str(&serde_json::to_string(&started).unwrap());
    assert_clean(&dump);
    assert!(rig.delegate.calls().is_empty());
}

#[tokio::test]
async fn busy_second_send_then_confirmed_and_unconfirmed_stop() {
    for confirm in [true, false] {
        let adapter = Arc::new(FakeAdapter {
            hold_turn: true,
            confirm_stop: confirm,
            ..Default::default()
        });
        let rig = rig(Some("goose"), Some(adapter.clone()));
        rig.provider.start(setup()).await.unwrap();
        rig.provider
            .send(&ChatSessionId::Setup, "one")
            .await
            .unwrap();
        let busy = rig
            .provider
            .send(&ChatSessionId::Setup, "two")
            .await
            .unwrap_err();
        assert_eq!(busy.code, SpiritErrorCode::Conflict);
        assert!(busy.message.starts_with("chat_busy"));
        assert_eq!(adapter.count("send"), 1);

        let stopped = rig.provider.stop(&ChatSessionId::Setup).await.unwrap();
        assert_eq!(adapter.count("abort"), 1);
        if confirm {
            assert_eq!(stopped.state, ChatSessionState::Stopped);
            assert!(stopped.error.is_none());
        } else {
            assert_eq!(stopped.state, ChatSessionState::Streaming);
            let error = stopped.error.expect("unconfirmed stop is reported");
            assert!(error.message.starts_with("harness_stop_unconfirmed"));
            assert_clean(&format!("{error:?}"));
        }
        rig.provider.shutdown().await;
    }
}

#[tokio::test]
async fn invalid_text_is_rejected_before_the_adapter() {
    let adapter = Arc::new(FakeAdapter::default());
    let rig = rig(Some("goose"), Some(adapter.clone()));
    rig.provider.start(setup()).await.unwrap();
    let error = rig
        .provider
        .send(&ChatSessionId::Setup, "   ")
        .await
        .unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::InvalidInput);
    assert_eq!(adapter.count("send"), 0);
}

#[tokio::test]
async fn removing_the_adapter_restores_the_gate() {
    let adapter = Arc::new(FakeAdapter::default());
    let rig = rig(Some("goose"), Some(adapter.clone()));
    rig.provider.start(setup()).await.unwrap();
    rig.provider.remove_adapter().await;
    assert_eq!(adapter.count("close"), 1);
    assert_eq!(rig.provider.gate().reason(), "harness_not_wired");
    let error = rig.provider.start(setup()).await.unwrap_err();
    assert_eq!(error.message, "harness_not_wired");
}

#[tokio::test]
async fn adapter_only_readies_its_own_acp_manifest_and_refuses_workers() {
    let adapter = Arc::new(FakeAdapter::default());
    // A different ACP harness than the adapter serves stays not wired.
    let other = rig(Some("kiro"), Some(adapter.clone()));
    assert_eq!(other.provider.gate().reason(), "harness_not_wired");
    // Unknown and worker-only selections still refuse.
    let missing = rig(Some("nope-not-real"), Some(adapter.clone()));
    assert_eq!(missing.provider.gate().reason(), "harness_missing");
    let error = missing.provider.start(setup()).await.unwrap_err();
    assert_eq!(error.message, "harness_missing");
    let worker = rig(Some("aider"), Some(adapter.clone()));
    assert_eq!(worker.provider.gate().reason(), "harness_worker_only");
    assert_eq!(
        worker.provider.start(setup()).await.unwrap_err().message,
        "harness_worker_only"
    );
    assert_eq!(adapter.starts.load(Ordering::SeqCst), 0);
    assert!(missing.delegate.calls().is_empty());
    assert!(worker.delegate.calls().is_empty());
}

#[tokio::test]
async fn opencode_global_and_project_still_delegate_once() {
    let adapter = Arc::new(FakeAdapter::default());
    let rig = rig(None, Some(adapter.clone()));
    rig.provider.start(setup()).await.unwrap();
    rig.provider
        .send(&ChatSessionId::Setup, "hi")
        .await
        .unwrap();
    rig.provider.stop(&ChatSessionId::Setup).await.unwrap();
    rig.provider
        .log(&ChatSessionId::Setup, &ChatLogQuery::default())
        .await
        .unwrap();
    rig.provider
        .start(ChatStartInput {
            session: ChatSessionId::Global,
            project: None,
            location: ChatSessionLocation::Local,
            resume: None,
        })
        .await
        .unwrap();
    rig.provider
        .send(&ChatSessionId::Project("demo".to_owned()), "hi")
        .await
        .unwrap();
    assert_eq!(
        rig.delegate.calls(),
        vec![
            "start:setup",
            "send:setup",
            "stop:setup",
            "log:setup",
            "start:global",
            "send:project:demo"
        ]
    );
    assert!(adapter.calls().is_empty());
}
