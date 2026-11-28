//! The Phase 1b transport router in front of the bundled-engine chat.
//!
//! [`HarnessChatProvider`] implements the public [`ChatTransport`] boundary from
//! `docs/HARNESS-CHAT.md` §4.1. For an app-owned `setup` session it resolves the persisted
//! harness default with [`preflight`] before touching the delegate, records the immutable
//! [`ChatHarnessCapture`] on a ready start and refuses every other selection with a typed
//! gate error whose message is exactly the stable gate token. VOWP `global`/`project`
//! sessions are OpenCode by contract and bypass the default entirely.
//!
//! Without a registered adapter the router is inert: it never spawns a child, opens a
//! socket, calls the engine or writes a file. With a [`HarnessChatAdapter`] registered
//! (`with_adapter`), a ready ACP-selected Setup start creates one adapter session, turns
//! stream through a pump task into the router's own [`ChatEventBus`] and an in-memory log,
//! and nothing core-only (native id, workspace, protocol frame) reaches a view, event,
//! error or Debug string. A setup session with no capture and no ACP selection is still
//! treated as OpenCode on attach.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::mpsc;
use ulid::Ulid;

use crate::engine::chat::adapter::{
    HarnessChatAdapter, HarnessEvent, HarnessNativeId, HarnessSession, HarnessStart, HarnessTurn,
};
use crate::engine::chat::harness::{HarnessChatGate, preflight_with};
use crate::engine::chat::local::ChatTransport;
use crate::engine::chat::model::{
    ChatAccess, ChatAttachment, ChatCloseReason, ChatLogPage, ChatLogQuery, ChatMessage,
    ChatSendReceipt, ChatSessionId, ChatSessionLocation, ChatSessionState, ChatSessionView,
    ChatStartInput, ChatToolActivity, validate_user_text,
};
use crate::engine::chat::pump::ChatEventBus;
use crate::engine::chat::selection::{ChatHarnessCapture, ChatPreferencesSource};
use crate::error::{SpiritError, SpiritErrorCode};
use crate::harnesses::{DEFAULT_HARNESS_ID, HarnessProtocol, HarnessRegistry};
use crate::time::Timestamp;

const TURN_EVENT_CAPACITY: usize = 64;
const SHUTDOWN_CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

struct AcpBinding {
    adapter: Arc<dyn HarnessChatAdapter>,
    workspace: PathBuf,
}

/// The core-only state of one ACP-backed session.
struct AcpRecord {
    session: HarnessSession,
    view: ChatSessionView,
    messages: Vec<ChatMessage>,
    /// The assistant message currently streaming and its full text so far.
    current: Option<(String, String)>,
    turn: u64,
    accepted: bool,
}

type SharedRecord = Arc<Mutex<AcpRecord>>;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

enum Route {
    Acp(SharedRecord),
    Delegate,
    Lost,
}

/// Routes app-owned setup chat through the harness gate before the OpenCode delegate.
///
/// `global`/`project` sessions and orchestrator provisioning bypass the gate: VOWP owns
/// those sessions as OpenCode and they are not candidates for the stored default.
pub struct HarnessChatProvider {
    delegate: Arc<dyn ChatTransport>,
    preferences: Arc<dyn ChatPreferencesSource + Send + Sync>,
    registry: Arc<HarnessRegistry>,
    captures: Mutex<HashMap<ChatSessionId, ChatHarnessCapture>>,
    binding: Mutex<Option<AcpBinding>>,
    records: Mutex<HashMap<ChatSessionId, SharedRecord>>,
    bus: Arc<ChatEventBus>,
    start_lock: tokio::sync::Mutex<()>,
}

impl HarnessChatProvider {
    /// Builds the router around the existing local transport, the persisted default and
    /// the harness registry. No ACP adapter is registered.
    #[must_use]
    pub fn new(
        delegate: Arc<dyn ChatTransport>,
        preferences: Arc<dyn ChatPreferencesSource + Send + Sync>,
        registry: Arc<HarnessRegistry>,
    ) -> Self {
        Self {
            delegate,
            preferences,
            registry,
            captures: Mutex::new(HashMap::new()),
            binding: Mutex::new(None),
            records: Mutex::new(HashMap::new()),
            bus: Arc::new(ChatEventBus::default()),
            start_lock: tokio::sync::Mutex::new(()),
        }
    }

    /// Registers a harness adapter and the Setup workspace it starts in.
    #[must_use]
    pub fn with_adapter(
        self,
        adapter: Arc<dyn HarnessChatAdapter>,
        workspace: impl Into<PathBuf>,
    ) -> Self {
        self.set_adapter(adapter, workspace);
        self
    }

    /// Registers (or replaces) the harness adapter. Existing sessions keep running.
    pub fn set_adapter(&self, adapter: Arc<dyn HarnessChatAdapter>, workspace: impl Into<PathBuf>) {
        *lock(&self.binding) = Some(AcpBinding {
            adapter,
            workspace: workspace.into(),
        });
    }

    /// Closes every ACP session, then removes the adapter so ACP is `harness_not_wired`
    /// again.
    pub async fn remove_adapter(&self) {
        self.shutdown().await;
        *lock(&self.binding) = None;
    }

    /// Returns the router's own event bus. It carries ACP events only.
    #[must_use]
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<crate::engine::chat::ChatEvent> {
        self.bus.subscribe()
    }

    /// There is no verified ACP model list yet; never a fake list or zero usage.
    #[must_use]
    pub fn models(&self) -> Option<Vec<crate::engine::chat::ChatModelView>> {
        None
    }

    /// Closes every ACP adapter session, bounded per session. Idempotent.
    pub async fn shutdown(&self) {
        let drained: Vec<(ChatSessionId, SharedRecord)> = lock(&self.records).drain().collect();
        let adapter = self.adapter();
        for (id, record) in drained {
            let session = {
                let mut r = lock(&record);
                r.turn += 1;
                r.view.state = ChatSessionState::Stopped;
                r.view.attached = false;
                r.session.clone()
            };
            if let Some(adapter) = &adapter {
                let _ = tokio::time::timeout(SHUTDOWN_CLOSE_TIMEOUT, adapter.close(&session)).await;
            }
            let event = self.bus.closed(id, ChatCloseReason::EngineStopped);
            let _ = self.bus.publish(event);
        }
    }

    fn adapter(&self) -> Option<Arc<dyn HarnessChatAdapter>> {
        lock(&self.binding)
            .as_ref()
            .map(|binding| Arc::clone(&binding.adapter))
    }

    /// Returns the current gate for a new app-owned setup conversation.
    ///
    /// This is the same read-only preflight the next setup `start` uses, widened by the
    /// registered adapter. It carries only a registry id, a protocol or a typed settings
    /// problem; never a path, endpoint, port, token or native id.
    #[must_use]
    pub fn gate(&self) -> HarnessChatGate {
        let served = lock(&self.binding).as_ref().map(|binding| {
            (
                binding.adapter.harness_id().to_owned(),
                binding.adapter.protocol(),
            )
        });
        preflight_with(
            self.registry.as_ref(),
            self.preferences.as_ref(),
            &|id, protocol| {
                served
                    .as_ref()
                    .is_some_and(|(adapter_id, adapter_protocol)| {
                        adapter_id == id && *adapter_protocol == protocol
                    })
            },
        )
    }

    /// Returns the immutable harness capture recorded for `session`, if any.
    ///
    /// Only an app-owned `setup` start (or the legacy attach described below) records a
    /// capture. The value carries only the registry id and protocol; never a native
    /// session id, endpoint, port, path, workspace or credential.
    #[must_use]
    pub fn capture(&self, session: &ChatSessionId) -> Option<ChatHarnessCapture> {
        lock(&self.captures).get(session).cloned()
    }

    fn store_capture(&self, session: ChatSessionId, capture: ChatHarnessCapture) {
        lock(&self.captures).insert(session, capture);
    }

    fn route(&self, session: &ChatSessionId) -> Route {
        if let Some(record) = lock(&self.records).get(session) {
            return Route::Acp(Arc::clone(record));
        }
        let acp_captured = self
            .capture(session)
            .is_some_and(|capture| capture.protocol() == HarnessProtocol::Acp);
        if acp_captured {
            Route::Lost
        } else {
            Route::Delegate
        }
    }

    async fn start_acp(&self, harness_id: String) -> Result<ChatSessionView, SpiritError> {
        let _guard = self.start_lock.lock().await;
        if let Some(record) = lock(&self.records).get(&ChatSessionId::Setup) {
            return Ok(lock(record).view.clone());
        }
        let (adapter, workspace) = {
            let binding = lock(&self.binding);
            let Some(binding) = binding.as_ref() else {
                return Err(gate_error(&HarnessChatGate::HarnessNotWired {
                    harness_id,
                    protocol: HarnessProtocol::Acp,
                }));
            };
            (Arc::clone(&binding.adapter), binding.workspace.clone())
        };
        let session = adapter.start(HarnessStart::new(workspace)).await?;
        let capture = ChatHarnessCapture::new(harness_id, HarnessProtocol::Acp);
        let now = Timestamp::now();
        let view = ChatSessionView {
            session: ChatSessionId::Setup,
            location: ChatSessionLocation::Local,
            state: ChatSessionState::Ready,
            attached: true,
            streaming_supported: true,
            model: None,
            available_models: None,
            usage: None,
            started_at: Some(now),
            last_event_at: Some(now),
            error: None,
            pending_permissions: None,
            retry: None,
            ownership: None,
            access: ChatAccess::Owner,
            writer: None,
        };
        let record = Arc::new(Mutex::new(AcpRecord {
            session,
            view: view.clone(),
            messages: Vec::new(),
            current: None,
            turn: 0,
            accepted: false,
        }));
        self.store_capture(ChatSessionId::Setup, capture);
        lock(&self.records).insert(ChatSessionId::Setup, record);
        let _ = self.bus.publish(self.bus.session(view.clone()));
        Ok(view)
    }

    async fn send_acp(
        &self,
        session: &ChatSessionId,
        record: SharedRecord,
        text: &str,
        attachments: &[ChatAttachment],
    ) -> Result<ChatSendReceipt, SpiritError> {
        let text = validate_user_text(text)?;
        if !attachments.is_empty() {
            return Err(SpiritError::new(
                SpiritErrorCode::Conflict,
                "chat_attachments_gated: this harness cannot take attachments yet",
            ));
        }
        let adapter = self.adapter().ok_or_else(|| {
            gate_error(&HarnessChatGate::HarnessNotWired {
                harness_id: lock(&record).session.harness_id().to_owned(),
                protocol: HarnessProtocol::Acp,
            })
        })?;
        let (harness_session, turn, user_message) = {
            let mut r = lock(&record);
            if r.view.state == ChatSessionState::Streaming {
                return Err(chat_busy_error());
            }
            r.turn += 1;
            r.accepted = false;
            r.current = None;
            r.view.state = ChatSessionState::Streaming;
            r.view.error = None;
            let now = Timestamp::now();
            r.view.last_event_at = Some(now);
            let message = ChatMessage::user(format!("msg_{}", Ulid::generate()), &text, now);
            r.messages.push(message.clone());
            let _ = self
                .bus
                .publish(self.bus.message(session.clone(), message.clone()));
            let _ = self.bus.publish(self.bus.session(r.view.clone()));
            (r.session.clone(), r.turn, message)
        };
        let (tx, rx) = mpsc::channel(TURN_EVENT_CAPACITY);
        let pump = tokio::spawn(pump_turn(
            Arc::clone(&record),
            Arc::clone(&self.bus),
            session.clone(),
            turn,
            rx,
        ));
        match adapter
            .send_turn(&harness_session, HarnessTurn::new(text), tx)
            .await
        {
            Ok(_) => {
                let mut r = lock(&record);
                if r.turn == turn {
                    r.accepted = true;
                    if pump.is_finished() && r.view.state == ChatSessionState::Streaming {
                        fail_turn(&mut r, &self.bus, session, stream_lost_error());
                    }
                }
                Ok(ChatSendReceipt {
                    session: session.clone(),
                    message: user_message,
                    accepted: true,
                })
            }
            Err(error) => {
                pump.abort();
                let mut r = lock(&record);
                if r.turn == turn {
                    r.turn += 1;
                    r.view.state = ChatSessionState::Ready;
                    r.messages.retain(|m| m.id != user_message.id);
                    let _ = self.bus.publish(self.bus.session(r.view.clone()));
                }
                Err(error)
            }
        }
    }

    async fn stop_acp(
        &self,
        session: &ChatSessionId,
        record: SharedRecord,
    ) -> Result<ChatSessionView, SpiritError> {
        let Some(adapter) = self.adapter() else {
            return Err(session_lost_error());
        };
        let (harness_session, turn) = {
            let r = lock(&record);
            if r.view.state != ChatSessionState::Streaming {
                return Ok(r.view.clone());
            }
            (r.session.clone(), r.turn)
        };
        let abort = adapter.abort(&harness_session).await?;
        let mut r = lock(&record);
        if r.turn != turn || r.view.state != ChatSessionState::Streaming {
            return Ok(r.view.clone());
        }
        let now = Timestamp::now();
        r.view.last_event_at = Some(now);
        if abort.stop_confirmed {
            r.turn += 1;
            r.view.state = ChatSessionState::Stopped;
            r.view.error = None;
            let message_id = finish_current(&mut r);
            let _ = self
                .bus
                .publish(self.bus.done(session.clone(), message_id, false));
        } else {
            r.view.error = Some(
                SpiritError::new(
                    SpiritErrorCode::Engine,
                    "harness_stop_unconfirmed: the harness did not confirm the stop",
                )
                .with_hint("The reply may still finish; wait or try stopping again."),
            );
        }
        let view = r.view.clone();
        let _ = self.bus.publish(self.bus.session(view.clone()));
        Ok(view)
    }
}

/// Marks the streaming assistant message complete and returns its id (or empty).
fn finish_current(record: &mut AcpRecord) -> String {
    let Some((id, _)) = record.current.take() else {
        return String::new();
    };
    if let Some(message) = record.messages.iter_mut().find(|m| m.id == id) {
        message.streaming = false;
    }
    id
}

fn fail_turn(
    record: &mut AcpRecord,
    bus: &ChatEventBus,
    session: &ChatSessionId,
    error: SpiritError,
) {
    finish_current(record);
    record.view.state = ChatSessionState::Error;
    record.view.error = Some(error.clone());
    record.view.last_event_at = Some(Timestamp::now());
    let _ = bus.publish(bus.error(session.clone(), error));
    let _ = bus.publish(bus.session(record.view.clone()));
}

/// Ensures the assistant message `id` exists and is the current one.
fn ensure_assistant(record: &mut AcpRecord, id: &str) {
    if record.current.as_ref().is_some_and(|(cur, _)| cur == id) {
        return;
    }
    finish_current(record);
    if !record.messages.iter().any(|m| m.id == id) {
        let mut message = ChatMessage::model(id, "", Timestamp::now());
        message.streaming = true;
        record.messages.push(message);
    }
    record.current = Some((id.to_owned(), String::new()));
}

fn set_assistant_text(record: &mut AcpRecord, id: &str, text: &str) {
    if let Some((_, current)) = record.current.as_mut() {
        *current = text.to_owned();
    }
    if let Some(message) = record.messages.iter_mut().find(|m| m.id == id) {
        let rebuilt = ChatMessage::model(id, text, message.created_at);
        message.text = rebuilt.text;
    }
}

/// Maps adapter events for one turn into safe chat events and the in-memory log.
async fn pump_turn(
    record: SharedRecord,
    bus: Arc<ChatEventBus>,
    session: ChatSessionId,
    turn: u64,
    mut rx: mpsc::Receiver<HarnessEvent>,
) {
    while let Some(event) = rx.recv().await {
        let mut r = lock(&record);
        if r.turn != turn || r.view.state != ChatSessionState::Streaming {
            return;
        }
        r.view.last_event_at = Some(Timestamp::now());
        match event {
            HarnessEvent::NativeSession { id, .. } => {
                r.session = HarnessSession::new(
                    r.session.harness_id().to_owned(),
                    r.session.protocol(),
                    Some(HarnessNativeId::new(id.as_str())),
                );
            }
            HarnessEvent::MessageStart { message_id } => {
                ensure_assistant(&mut r, &message_id);
                if let Some(message) = r.messages.iter().find(|m| m.id == message_id).cloned() {
                    let _ = bus.publish(bus.message(session.clone(), message));
                }
            }
            HarnessEvent::TextDelta {
                message_id,
                part_id,
                text,
            } => {
                ensure_assistant(&mut r, &message_id);
                let mut full = r.current.as_ref().map(|c| c.1.clone()).unwrap_or_default();
                full.push_str(&text);
                set_assistant_text(&mut r, &message_id, &full);
                let _ = bus.publish(bus.delta(session.clone(), message_id, part_id, text));
            }
            HarnessEvent::TextReplace {
                message_id, text, ..
            } => {
                ensure_assistant(&mut r, &message_id);
                set_assistant_text(&mut r, &message_id, &text);
                if let Some(message) = r.messages.iter().find(|m| m.id == message_id).cloned() {
                    let _ = bus.publish(bus.message(session.clone(), message));
                }
            }
            HarnessEvent::Tool { activity } => {
                let id = match r.current.as_ref() {
                    Some((id, _)) => id.clone(),
                    None => {
                        let id = format!("msg_{}", Ulid::generate());
                        ensure_assistant(&mut r, &id);
                        id
                    }
                };
                if let Some(message) = r.messages.iter_mut().find(|m| m.id == id) {
                    upsert_tool(message, activity);
                    let message = message.clone();
                    let _ = bus.publish(bus.message(session.clone(), message));
                }
            }
            // The adapter's policy already resolved this (default: cancel). The router
            // never approves a tool call and shows no card in this slice.
            HarnessEvent::Permission { .. } => {}
            HarnessEvent::Usage { usage } => {
                r.view.usage = Some(usage);
            }
            HarnessEvent::Done {
                message_id,
                truncated,
            } => {
                ensure_assistant(&mut r, &message_id);
                finish_current(&mut r);
                r.view.state = ChatSessionState::Ready;
                r.view.error = None;
                let _ = bus.publish(bus.done(session.clone(), message_id, truncated));
                let _ = bus.publish(bus.session(r.view.clone()));
                return;
            }
            HarnessEvent::Error { error } => {
                fail_turn(&mut r, &bus, &session, error);
                return;
            }
        }
    }
    let mut r = lock(&record);
    if r.turn == turn && r.accepted && r.view.state == ChatSessionState::Streaming {
        fail_turn(&mut r, &bus, &session, stream_lost_error());
    }
}

fn upsert_tool(message: &mut ChatMessage, activity: ChatToolActivity) {
    let tools = message.tools.get_or_insert_with(Vec::new);
    if let Some(existing) = tools.iter_mut().find(|t| t.id == activity.id) {
        *existing = activity;
    } else if tools.len() < crate::engine::chat::MAX_ACTIVITY_ITEMS {
        tools.push(activity);
    }
}

#[async_trait]
impl ChatTransport for HarnessChatProvider {
    async fn start(&self, input: ChatStartInput) -> Result<ChatSessionView, SpiritError> {
        if !matches!(&input.session, ChatSessionId::Setup) {
            return self.delegate.start(input).await;
        }
        // Resolve the gate before the delegate can start the bundled engine. A non-ready
        // selection must not create a child, a native session or a capture.
        let gate = self.gate();
        match gate {
            HarnessChatGate::Ready {
                harness_id,
                protocol: HarnessProtocol::Acp,
            } => self.start_acp(harness_id).await,
            HarnessChatGate::Ready {
                harness_id,
                protocol,
            } => {
                let capture = ChatHarnessCapture::new(harness_id, protocol);
                let view = self.delegate.start(input).await?;
                self.store_capture(ChatSessionId::Setup, capture);
                Ok(view)
            }
            gate => Err(gate_error(&gate)),
        }
    }

    async fn attach(&self, session: &ChatSessionId) -> Result<ChatSessionView, SpiritError> {
        // Attach never consults the stored default for a running conversation. An ACP
        // session is answered from its own record and never falls through to OpenCode.
        match self.route(session) {
            Route::Acp(record) => return Ok(lock(&record).view.clone()),
            Route::Lost => return Err(session_lost_error()),
            Route::Delegate => {}
        }
        if matches!(session, ChatSessionId::Setup)
            && self.capture(session).is_none()
            && matches!(
                self.gate(),
                HarnessChatGate::Ready {
                    protocol: HarnessProtocol::Acp,
                    ..
                }
            )
        {
            return Err(session_lost_error());
        }
        let view = self.delegate.attach(session).await?;
        if matches!(session, ChatSessionId::Setup) && self.capture(session).is_none() {
            // A setup session with no capture predates this router and is the documented
            // OpenCode default. Recording that keeps a later default change from
            // rebinding it.
            self.store_capture(
                ChatSessionId::Setup,
                ChatHarnessCapture::new(DEFAULT_HARNESS_ID, HarnessProtocol::OpencodeHttp),
            );
        }
        Ok(view)
    }

    async fn send(
        &self,
        session: &ChatSessionId,
        text: &str,
    ) -> Result<ChatSendReceipt, SpiritError> {
        match self.route(session) {
            Route::Acp(record) => self.send_acp(session, record, text, &[]).await,
            Route::Lost => Err(session_lost_error()),
            Route::Delegate => self.delegate.send(session, text).await,
        }
    }

    async fn send_turn(
        &self,
        session: &ChatSessionId,
        text: &str,
        variant: Option<&str>,
        attachments: &[ChatAttachment],
    ) -> Result<ChatSendReceipt, SpiritError> {
        match self.route(session) {
            Route::Acp(record) => self.send_acp(session, record, text, attachments).await,
            Route::Lost => Err(session_lost_error()),
            Route::Delegate => {
                self.delegate
                    .send_turn(session, text, variant, attachments)
                    .await
            }
        }
    }

    async fn stop(&self, session: &ChatSessionId) -> Result<ChatSessionView, SpiritError> {
        match self.route(session) {
            Route::Acp(record) => self.stop_acp(session, record).await,
            Route::Lost => Err(session_lost_error()),
            Route::Delegate => self.delegate.stop(session).await,
        }
    }

    async fn provision_orchestrator(&self, project: &str) -> Result<ChatSessionView, SpiritError> {
        self.delegate.provision_orchestrator(project).await
    }

    async fn set_model(
        &self,
        session: &ChatSessionId,
        model_id: &str,
    ) -> Result<ChatSessionView, SpiritError> {
        match self.route(session) {
            Route::Acp(_) => Err(SpiritError::new(
                SpiritErrorCode::Conflict,
                "chat_model_gated: an orchestrator owns its own model choice",
            )),
            Route::Lost => Err(session_lost_error()),
            Route::Delegate => self.delegate.set_model(session, model_id).await,
        }
    }

    async fn log(
        &self,
        session: &ChatSessionId,
        query: &ChatLogQuery,
    ) -> Result<ChatLogPage, SpiritError> {
        match self.route(session) {
            Route::Acp(record) => {
                let r = lock(&record);
                let end = match query.cursor.as_deref() {
                    None => r.messages.len(),
                    Some(cursor) => cursor
                        .parse::<usize>()
                        .ok()
                        .filter(|end| *end <= r.messages.len())
                        .ok_or_else(|| {
                            SpiritError::new(
                                SpiritErrorCode::InvalidInput,
                                "invalid_cursor: reload the conversation",
                            )
                        })?,
                };
                let start = end.saturating_sub(query.limit());
                Ok(ChatLogPage {
                    messages: r.messages[start..end].to_vec(),
                    next: (start > 0).then(|| start.to_string()),
                    truncated: false,
                })
            }
            Route::Lost => Err(session_lost_error()),
            Route::Delegate => self.delegate.log(session, query).await,
        }
    }
}

/// Maps a non-ready gate to a typed error.
///
/// The message is exactly the stable token the bridge/UI maps to fixed copy. It never
/// carries the selected id, protocol, settings path, registry prose or raw parser text.
fn gate_error(gate: &HarnessChatGate) -> SpiritError {
    SpiritError::new(SpiritErrorCode::Gate, gate.reason())
}

fn session_lost_error() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::NotFound,
        "harness_session_lost: the harness session is not running",
    )
    .with_hint("Start the conversation again.")
}

fn chat_busy_error() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Conflict,
        "chat_busy: wait for the current reply or stop it",
    )
    .with_hint("Send one turn at a time.")
}

fn stream_lost_error() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::RemoteTimeout,
        "harness_stream_lost: the harness connection stopped before the turn finished",
    )
    .retryable()
}
