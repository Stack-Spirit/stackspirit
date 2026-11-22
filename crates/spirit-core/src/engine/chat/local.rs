//! The bundled-engine local transport: health, session, message, log, abort and pump.
//!
//! The transport never spawns an engine. It calls the injected [`ChatEngine`], which
//! the desktop wires to the existing `EngineSupervisor` singleton. The loopback port,
//! workspace and native session ids stay in this module: no serialized DTO or error
//! carries an endpoint URL, port, credential or path.

use std::collections::HashMap;
use std::fmt;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::{Mutex, mpsc};
use ulid::Ulid;
use zeroize::Zeroizing;

use crate::engine::chat::endpoint::{
    ChatEndpointResolver, LoopbackAuth, LoopbackHost, ResolvedEndpoint, VowpEndpointResolver,
    parse_workspace_path,
};
use crate::engine::chat::model::{
    ChatCloseReason, ChatEvent, ChatLogPage, ChatLogQuery, ChatMessage, ChatMessageOrigin,
    ChatModelTier, ChatModelView, ChatPermissionDecision, ChatPermissionPrompt, ChatRetryStatus,
    ChatRole, ChatSendReceipt, ChatSessionId, ChatSessionLocation, ChatSessionState,
    ChatSessionView, ChatStartInput, ChatToolActivity, ChatToolStatus, ChatUsageView,
    MAX_ACTIVITY_ITEMS, MAX_MESSAGE_CHARS, cap_text, display_snippet, normalize_text,
    validate_project_name, validate_user_text,
};
use crate::engine::chat::pump::{
    ApplyOutcome, ChatEventApplier, ChatEventBus, Mapped, SseEventMapper, retry_status,
};
use crate::engine::chat::sse::{
    SseFrame, SseParser, StreamSupport, error_status, parse_engine_event, parse_session_status,
};
use crate::engine::{EngineChoice, EngineStatus};
use crate::error::{SpiritError, SpiritErrorCode};
use crate::time::{Clock, SystemClock, Timestamp};

/// The 10-minute turn cap from `docs/ORCHESTRATOR-CHAT.md` §2.2.4.
const TURN_CAP: Duration = Duration::from_secs(10 * 60);
/// Loopback TCP connect budget.
const CONNECT_TIMEOUT: Duration = Duration::from_millis(700);
/// Health read budget.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(3);
/// Session/metadata read budget.
const SESSION_TIMEOUT: Duration = Duration::from_secs(3);
/// Abort read budget; the caller retries once.
const ABORT_TIMEOUT: Duration = Duration::from_secs(3);
/// SSE response-header budget.
const SSE_HEADER_TIMEOUT: Duration = Duration::from_secs(5);
/// One SSE idle read slice. Sixty slices make the 60 s idle rule.
const SSE_READ_TIMEOUT: Duration = Duration::from_secs(1);
/// The SSE idle cap from §2.2.4.
const SSE_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// The largest accepted local response body.
const MAX_HTTP_BODY: usize = 16 * 1024 * 1024;
/// SSE reconnect attempts before `stream_lost`.
const MAX_SSE_ATTEMPTS: usize = 5;
/// The bounded reconnects from §2.2.4.
const SSE_BACKOFF: [Duration; 5] = [
    Duration::from_millis(250),
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
];
/// Broadcast capacity for chat events.
const EVENT_CAPACITY: usize = 256;
/// SSE signal channel capacity.
const SSE_CAPACITY: usize = 64;
/// The title of the single local setup session.
const SETUP_TITLE: &str = "StackSpirit setup";

/// A core-only v1 chat endpoint.
///
/// The setup assistant uses the bundled engine's private workspace header. A
/// VOWP-reported `global`/`project` orchestrator supplies its own loopback credential and,
/// when VOWP or the app's session list reports one, its orchestrator workspace header.
/// The struct has no unredacted `Debug`; the port, workspace and credential never appear
/// in a serialized chat value.
#[derive(Clone)]
struct ChatEndpoint {
    host: LoopbackHost,
    port: u16,
    /// The workspace header, when one is known.
    directory: Option<PathBuf>,
    /// True only for the one bundled setup engine; the event pump uses it to avoid
    /// treating an orchestrator as the setup server.
    setup: bool,
    /// The loopback credential; `None` for the unauthenticated bundled engine.
    auth: Option<Arc<LoopbackAuth>>,
}

impl ChatEndpoint {
    fn engine(port: u16, workspace: &Path) -> Self {
        Self {
            host: LoopbackHost::V4,
            port,
            directory: Some(workspace.to_path_buf()),
            setup: true,
            auth: None,
        }
    }

    fn orchestrator(endpoint: ResolvedEndpoint) -> Self {
        Self {
            host: endpoint.host,
            port: endpoint.port,
            directory: endpoint.workspace,
            setup: false,
            auth: endpoint.auth.map(Arc::new),
        }
    }

    fn socket_address(&self) -> SocketAddr {
        self.host.socket_address(self.port)
    }
}

impl fmt::Debug for ChatEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatEndpoint")
            .field("host", &"[redacted]")
            .field("port", &"[redacted]")
            .field("directory", &self.directory.as_ref().map(|_| "[redacted]"))
            .field("auth", &self.auth)
            .finish()
    }
}

/// The engine boundary the chat transport needs.
///
/// The desktop implements this by delegating to the existing engine supervisor; chat
/// never calls `spawn`, `reserve_loopback_port` or `Command::new` itself.
#[async_trait]
pub trait ChatEngine: Send + Sync {
    /// Ensures the bundled engine is healthy and returns its current loopback port.
    async fn ensure_ready(&self) -> Result<u16, SpiritError>;

    /// Returns the locally selected model, if any.
    fn selected_model(&self) -> Option<ChatModelView> {
        None
    }

    /// Returns every model the running engine reports as usable now.
    ///
    /// The default only knows the selected model. A real engine adapter overrides this
    /// with verified included models plus models whose provider key is configured.
    fn available_models(&self) -> Vec<ChatModelView> {
        self.selected_model().into_iter().collect()
    }

    /// Selects a model for the setup assistant.
    ///
    /// `persist` is true only for an explicit owner choice. The automatic default passes
    /// false so a launch never looks like the owner saved a model in Settings.
    async fn select_model(
        &self,
        _model_id: &str,
        _persist: bool,
    ) -> Result<ChatModelView, SpiritError> {
        Err(model_selection_unavailable_error())
    }

    /// Returns the engine status for the external-engine gate.
    ///
    /// This is core-only and is never serialized into a chat DTO.
    fn status(&self) -> EngineStatus {
        EngineStatus::default()
    }
}

/// The chat transport boundary. The desktop command layer and the future server
/// transport share this trait.
#[async_trait]
pub trait ChatTransport: Send + Sync {
    /// Ensures the engine and creates/attaches the native session.
    async fn start(&self, input: ChatStartInput) -> Result<ChatSessionView, SpiritError>;
    /// Reuses the stored native session; never creates one.
    async fn attach(&self, session: &ChatSessionId) -> Result<ChatSessionView, SpiritError>;
    /// Accepts one user turn and returns immediately.
    async fn send(
        &self,
        session: &ChatSessionId,
        text: &str,
    ) -> Result<ChatSendReceipt, SpiritError>;
    /// Aborts the turn when supported and returns the stopped view.
    async fn stop(&self, session: &ChatSessionId) -> Result<ChatSessionView, SpiritError>;
    /// One-click Start for a project orchestrator: attaches VOWP's local endpoint or falls
    /// back to a project session on the bundled engine. Never a `chat_not_configured`
    /// dead end.
    async fn provision_orchestrator(&self, project: &str) -> Result<ChatSessionView, SpiritError>;
    /// Switches the setup assistant's model and returns the updated view.
    async fn set_model(
        &self,
        session: &ChatSessionId,
        model_id: &str,
    ) -> Result<ChatSessionView, SpiritError>;
    /// Reads one backward log page.
    async fn log(
        &self,
        session: &ChatSessionId,
        query: &ChatLogQuery,
    ) -> Result<ChatLogPage, SpiritError>;
}

/// A core-only native session summary. It is never a serialized chat DTO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSession {
    /// Native session id, core-only.
    pub id: String,
    /// Native title, when reported.
    pub title: String,
}

/// One discovered native session and the workspace that owns it. Core-only.
struct DiscoveredSession {
    id: String,
    workspace: PathBuf,
}

/// The local chat provider for the bundled StackSpirit Engine.
pub struct LocalChatProvider {
    inner: Arc<Inner>,
}

struct Inner {
    engine: Arc<dyn ChatEngine>,
    workspace: PathBuf,
    resolver: Arc<dyn ChatEndpointResolver>,
    stream_support: StreamSupport,
    bus: ChatEventBus,
    state: Mutex<ProviderState>,
    pumps: std::sync::Mutex<HashMap<u16, PumpSlot>>,
    clock: Arc<dyn Clock>,
}

#[derive(Default)]
struct ProviderState {
    sessions: HashMap<ChatSessionId, SessionRecord>,
}

struct SessionRecord {
    native_session_id: String,
    /// The endpoint this session is attached to. Core-only.
    endpoint: Option<ChatEndpoint>,
    /// The VOWP home session reported with the endpoint. Core-only.
    vowp_session_id: Option<String>,
    /// True when a project session runs on the bundled engine because VOWP has no local
    /// endpoint for it yet (the one-click Start fallback). It behaves like the setup session.
    engine_backed: bool,
    state: ChatSessionState,
    attached: bool,
    started_at: Option<Timestamp>,
    last_event_at: Option<Timestamp>,
    error: Option<SpiritError>,
    usage: Option<ChatUsageView>,
    applier: ChatEventApplier,
    mapper: SseEventMapper,
    active_turn: Option<String>,
    turn_completed: bool,
    /// Permission requests OpenCode is waiting on, oldest first.
    pending_permissions: Vec<ChatPermissionPrompt>,
    /// An active `session.status` retry, when OpenCode is sleeping before another try.
    retry: Option<ChatRetryStatus>,
    /// True while a coalesced tool-activity log refresh is queued.
    tool_refresh_queued: bool,
    /// Identifies the in-flight background turn so a late response after Stop is ignored.
    turn_seq: u64,
    /// True between a Stop and the next send; late frames from the aborted turn are dropped.
    turn_cancelled: bool,
}

impl SessionRecord {
    fn new(native_session_id: String) -> Self {
        Self {
            native_session_id,
            endpoint: None,
            vowp_session_id: None,
            engine_backed: false,
            state: ChatSessionState::Idle,
            attached: false,
            started_at: None,
            last_event_at: None,
            error: None,
            usage: None,
            applier: ChatEventApplier::new(),
            mapper: SseEventMapper::new(),
            active_turn: None,
            turn_completed: false,
            pending_permissions: Vec::new(),
            retry: None,
            tool_refresh_queued: false,
            turn_seq: 0,
            turn_cancelled: false,
        }
    }
}

#[derive(Default)]
struct PumpSlot {
    /// True for the one bundled-engine endpoint. A restarted setup engine replaces its
    /// old pump instead of leaving two setup streams alive.
    setup: bool,
    cancel: Option<Arc<AtomicBool>>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        let mut pumps = self
            .pumps
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (_, mut slot) in pumps.drain() {
            if let Some(cancel) = slot.cancel.take() {
                cancel.store(true, Ordering::SeqCst);
            }
            if let Some(handle) = slot.handle.take() {
                handle.abort();
            }
        }
    }
}

impl LocalChatProvider {
    /// Builds a local provider around an injected engine and private setup workspace.
    #[must_use]
    pub fn new(engine: Arc<dyn ChatEngine>, workspace: PathBuf) -> Self {
        Self {
            inner: Arc::new(Inner {
                engine,
                workspace,
                resolver: Arc::new(VowpEndpointResolver::new()),
                stream_support: StreamSupport::default(),
                bus: ChatEventBus::new(EVENT_CAPACITY),
                state: Mutex::new(ProviderState::default()),
                pumps: std::sync::Mutex::new(HashMap::new()),
                clock: Arc::new(SystemClock),
            }),
        }
    }

    /// Replaces the VOWP endpoint resolver.
    ///
    /// Production uses the installed `vowp` CLI; tests inject a resolver pointed at a
    /// fake `vowp` on `PATH`.
    #[must_use]
    pub fn with_endpoint_resolver(mut self, resolver: Arc<dyn ChatEndpointResolver>) -> Self {
        let inner = Arc::get_mut(&mut self.inner)
            .expect("the endpoint resolver must be configured before the provider is shared");
        inner.resolver = resolver;
        self
    }

    /// Replaces the clock. Tests use it; production uses the system clock.
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.clock = clock;
        }
        self
    }

    /// Enables the SSE path only when probe evidence pinned the frames.
    ///
    /// The production constructor always starts [`StreamSupport::Unpinned`]; fake
    /// engine tests explicitly opt in to the contract shape.
    #[must_use]
    pub fn with_stream_support(mut self, support: StreamSupport) -> Self {
        let inner = Arc::get_mut(&mut self.inner)
            .expect("stream support must be configured before the provider is shared");
        inner.stream_support = support;
        self
    }

    /// Subscribes to the revisioned event fan-out.
    #[must_use]
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<ChatEvent> {
        self.inner.bus.subscribe()
    }

    /// Returns the honest live-text capability.
    #[must_use]
    pub fn streaming_support(&self) -> &StreamSupport {
        &self.inner.stream_support
    }

    /// Returns the exact safe reason live text is disabled, when it is.
    #[must_use]
    pub fn streaming_support_reason(&self) -> Option<&str> {
        self.inner.stream_support.reason()
    }

    /// Re-points every bundled-engine session (the setup assistant and any project
    /// orchestrator that fell back to the bundled engine) at the possibly restarted
    /// engine and broadcasts a fresh `ChatEvent::Session` carrying the current model list.
    ///
    /// A key change restarts the sidecar on a new port. Without this, mounted chat views
    /// keep the stale `available_models` and a stale-port pump would report
    /// `stream_lost`. Does nothing when no bundled-engine session has been registered.
    pub async fn refresh_setup_models(&self) {
        self.inner.clone().rebind_engine_sessions().await;
    }

    /// Lists the native engine sessions. Core-only; the renderer never receives these ids.
    pub async fn native_sessions(&self) -> Result<Vec<NativeSession>, SpiritError> {
        let endpoint = {
            let state = self.inner.state.lock().await;
            state
                .sessions
                .get(&ChatSessionId::Setup)
                .and_then(|record| record.endpoint.clone())
        }
        .ok_or_else(chat_not_started_error)?;
        let response = self
            .inner
            .request(
                "GET",
                endpoint,
                "/session".to_owned(),
                None,
                SESSION_TIMEOUT,
            )
            .await?;
        ensure_success(&response)?;
        Ok(parse_native_sessions(&response.body))
    }

    /// Ensures the right endpoint and attaches the native session.
    ///
    /// The setup assistant starts or reuses the bundled engine. A Local `global` or
    /// `project` session resolves VOWP's local orchestrator endpoint and attaches to the
    /// mirrored native session; it never creates one and never starts a second engine.
    pub async fn start(&self, input: &ChatStartInput) -> Result<ChatSessionView, SpiritError> {
        validate_start(input)?;
        match &input.session {
            ChatSessionId::Setup => self.start_setup(input).await,
            ChatSessionId::Global | ChatSessionId::Project(_) => {
                self.start_orchestrator(input).await
            }
        }
    }

    async fn is_engine_backed(&self, session: &ChatSessionId) -> bool {
        let state = self.inner.state.lock().await;
        state
            .sessions
            .get(session)
            .is_some_and(|record| record.engine_backed)
    }

    /// One-click Start for a project orchestrator. It first looks for VOWP's local
    /// endpoint; when there is none (`chat_not_configured`) it falls back to a
    /// project-titled session on the bundled engine, which uses the owner's connected
    /// OpenCode or BYOK models, so the owner can chat immediately. Any other failure is
    /// returned unchanged. Idempotent: a running fallback session is reused.
    pub async fn provision_orchestrator(
        &self,
        project: &str,
    ) -> Result<ChatSessionView, SpiritError> {
        validate_project_name(project)?;
        let session = ChatSessionId::Project(project.to_owned());
        if self.is_engine_backed(&session).await {
            return self.attach_setup(&session).await;
        }
        let existing = {
            let state = self.inner.state.lock().await;
            state
                .sessions
                .get(&session)
                .and_then(|record| record.vowp_session_id.clone())
        };
        match self
            .inner
            .resolve_local_endpoint(project, existing.as_deref())
            .await
        {
            Ok(resolved) => self.open_orchestrator(project, &session, resolved).await,
            Err(error) if error.message.starts_with("chat_not_configured") => {
                self.start_engine_backed(&session, project).await
            }
            Err(error) => Err(error),
        }
    }

    async fn start_engine_backed(
        &self,
        session: &ChatSessionId,
        project: &str,
    ) -> Result<ChatSessionView, SpiritError> {
        let port = self.inner.ensure_engine().await?;
        self.inner.check_health(port).await?;
        let endpoint = ChatEndpoint::engine(port, &self.inner.workspace);
        let _ = self.inner.resolve_setup_model().await;
        let native = self
            .inner
            .create_native_session_titled(&endpoint, &format!("{project} orchestrator"))
            .await?;
        let body = self.inner.read_log(&endpoint, &native).await?;
        let usage = parse_usage(&body, self.inner.selected_context_limit());
        let now = self.inner.clock.now();
        let view = {
            let mut state = self.inner.state.lock().await;
            let mut record = SessionRecord::new(native);
            record.endpoint = Some(endpoint.clone());
            record.engine_backed = true;
            record.state = ChatSessionState::Ready;
            record.attached = true;
            record.started_at = Some(now);
            record.last_event_at = Some(now);
            record.usage = usage;
            record
                .applier
                .replace_log(parse_log(&body, self.inner.clock.now()));
            record.applier.observe_revision(self.inner.bus.revision());
            record.mapper.reset_from_messages(record.applier.messages());
            let view = self.inner.view_for(session, &record);
            state.sessions.insert(session.clone(), record);
            view
        };
        let view = self.inner.clone().emit_session(view).await;
        ensure_pump(&self.inner, &endpoint);
        Ok(self.inner.current_view(session).await.unwrap_or(view))
    }

    async fn start_setup(&self, input: &ChatStartInput) -> Result<ChatSessionView, SpiritError> {
        let port = self.inner.ensure_engine().await?;
        self.inner.check_health(port).await?;
        let endpoint = ChatEndpoint::engine(port, &self.inner.workspace);
        // An unsaved first run defaults to the first verified free runtime model. A
        // selection failure is not fatal: the view honestly reports no model.
        let _ = self.inner.resolve_setup_model().await;

        let existing = {
            let state = self.inner.state.lock().await;
            state
                .sessions
                .get(&input.session)
                .map(|record| record.native_session_id.clone())
        };
        let native = if input.resumes() {
            if let Some(native) = existing {
                match self.inner.get_native_session(&endpoint, &native).await {
                    Ok(()) => native,
                    Err(error) if error.code == SpiritErrorCode::NotFound => {
                        self.inner.remove_session(&input.session).await;
                        return Err(session_lost_error());
                    }
                    Err(error) => return Err(error),
                }
            } else {
                self.inner.create_native_session(&endpoint).await?
            }
        } else {
            self.inner.create_native_session(&endpoint).await?
        };
        let body = self.inner.read_log(&endpoint, &native).await?;
        let usage = parse_usage(&body, self.inner.selected_context_limit());
        let now = self.inner.clock.now();
        let view = {
            let mut state = self.inner.state.lock().await;
            let record = state
                .sessions
                .entry(input.session.clone())
                .or_insert_with(|| SessionRecord::new(native.clone()));
            let preserve_turn =
                record.native_session_id == native && record.state == ChatSessionState::Streaming;
            record.native_session_id = native;
            record.endpoint = Some(endpoint.clone());
            record.vowp_session_id = None;
            if !preserve_turn {
                record.state = ChatSessionState::Ready;
                record.active_turn = None;
                record.turn_completed = false;
                record.retry = None;
                record.turn_cancelled = false;
                record.turn_seq = record.turn_seq.saturating_add(1);
            }
            record.attached = true;
            record.started_at.get_or_insert(now);
            record.last_event_at = Some(now);
            record.error = None;
            record.usage = usage;
            record
                .applier
                .replace_log(parse_log(&body, self.inner.clock.now()));
            record.applier.observe_revision(self.inner.bus.revision());
            record.mapper.reset_from_messages(record.applier.messages());
            self.inner.view_for(&input.session, record)
        };
        let view = self.inner.clone().emit_session(view).await;
        ensure_pump(&self.inner, &endpoint);
        // A retry or permission that started before the UI attached must be visible now.
        self.inner.clone().sync_permissions(&input.session).await;
        self.inner.clone().sync_status(&input.session).await;
        Ok(self
            .inner
            .current_view(&input.session)
            .await
            .unwrap_or(view))
    }

    async fn start_orchestrator(
        &self,
        input: &ChatStartInput,
    ) -> Result<ChatSessionView, SpiritError> {
        if self.is_engine_backed(&input.session).await {
            return self.attach_setup(&input.session).await;
        }
        if !input.resumes() {
            return Err(chat_gated_error(
                "a new global/project conversation must be created in VOWP; StackSpirit only attaches to the mirrored session",
            ));
        }
        let name = orchestrator_name(&input.session).ok_or_else(|| {
            SpiritError::invalid_input("session", "the setup session does not use a VOWP endpoint")
        })?;
        let existing = {
            let state = self.inner.state.lock().await;
            state
                .sessions
                .get(&input.session)
                .and_then(|record| record.vowp_session_id.clone())
        };
        let resolved = self
            .inner
            .resolve_local_endpoint(name, existing.as_deref())
            .await?;
        self.open_orchestrator(name, &input.session, resolved).await
    }

    async fn open_orchestrator(
        &self,
        name: &str,
        session: &ChatSessionId,
        resolved: ResolvedEndpoint,
    ) -> Result<ChatSessionView, SpiritError> {
        let reported_native = resolved.native_session_id.clone();
        let vowp_session_id = resolved.session_id.clone();
        let mut endpoint = ChatEndpoint::orchestrator(resolved);
        let native = match reported_native {
            Some(native) => native,
            None => {
                let discovered = self.inner.discover_native_session(&endpoint, name).await?;
                if endpoint.directory.is_none() {
                    endpoint.directory = Some(discovered.workspace);
                }
                discovered.id
            }
        };
        match self.inner.get_native_session(&endpoint, &native).await {
            Ok(()) => {}
            Err(error) if error.code == SpiritErrorCode::NotFound => {
                return Err(session_lost_error());
            }
            Err(error) => return Err(error),
        }
        let body = self.inner.read_log(&endpoint, &native).await?;
        // The orchestrator owns its model choice; only its reported token metadata is
        // used, and the context limit stays `null` because the app does not know it.
        let usage = parse_usage(&body, None);
        let now = self.inner.clock.now();
        let view = {
            let mut state = self.inner.state.lock().await;
            let record = state
                .sessions
                .entry(session.clone())
                .or_insert_with(|| SessionRecord::new(native.clone()));
            let preserve_turn =
                record.native_session_id == native && record.state == ChatSessionState::Streaming;
            record.native_session_id = native;
            record.endpoint = Some(endpoint.clone());
            record.vowp_session_id = Some(vowp_session_id);
            if !preserve_turn {
                record.state = ChatSessionState::Ready;
                record.active_turn = None;
                record.turn_completed = false;
                record.retry = None;
                record.turn_cancelled = false;
                record.turn_seq = record.turn_seq.saturating_add(1);
            }
            record.attached = true;
            record.started_at.get_or_insert(now);
            record.last_event_at = Some(now);
            record.error = None;
            record.usage = usage;
            record
                .applier
                .replace_log(parse_log(&body, self.inner.clock.now()));
            record.applier.observe_revision(self.inner.bus.revision());
            record.mapper.reset_from_messages(record.applier.messages());
            self.inner.view_for(session, record)
        };
        let view = self.inner.clone().emit_session(view).await;
        ensure_pump(&self.inner, &endpoint);
        // A turn that was already waiting on a permission or retry when the UI (re)attached.
        self.inner.clone().sync_permissions(session).await;
        self.inner.clone().sync_status(session).await;
        Ok(self.inner.current_view(session).await.unwrap_or(view))
    }

    /// Reuses the stored native session. It never creates one.
    pub async fn attach(&self, session: &ChatSessionId) -> Result<ChatSessionView, SpiritError> {
        validate_attach(session)?;
        match session {
            ChatSessionId::Setup => self.attach_setup(session).await,
            ChatSessionId::Global | ChatSessionId::Project(_) => {
                self.attach_orchestrator(session).await
            }
        }
    }

    async fn attach_setup(&self, session: &ChatSessionId) -> Result<ChatSessionView, SpiritError> {
        let native = {
            let state = self.inner.state.lock().await;
            state
                .sessions
                .get(session)
                .map(|record| record.native_session_id.clone())
        }
        .ok_or_else(chat_not_started_error)?;
        let port = self.inner.ensure_engine().await?;
        self.inner.check_health(port).await?;
        let endpoint = ChatEndpoint::engine(port, &self.inner.workspace);
        let _ = self.inner.resolve_setup_model().await;
        match self.inner.get_native_session(&endpoint, &native).await {
            Ok(()) => {}
            Err(error) if error.code == SpiritErrorCode::NotFound => {
                self.inner.remove_session(session).await;
                return Err(session_lost_error());
            }
            Err(error) => return Err(error),
        }
        let body = self.inner.read_log(&endpoint, &native).await?;
        let usage = parse_usage(&body, self.inner.selected_context_limit());
        let now = self.inner.clock.now();
        let view = {
            let mut state = self.inner.state.lock().await;
            let record = state
                .sessions
                .get_mut(session)
                .ok_or_else(chat_not_started_error)?;
            if record.native_session_id != native {
                return Err(session_lost_error());
            }
            let preserve_turn = record.state == ChatSessionState::Streaming;
            if !preserve_turn {
                record.state = ChatSessionState::Ready;
                record.active_turn = None;
                record.turn_completed = false;
                record.retry = None;
                record.turn_cancelled = false;
                record.turn_seq = record.turn_seq.saturating_add(1);
            }
            record.endpoint = Some(endpoint.clone());
            record.attached = true;
            record.started_at.get_or_insert(now);
            record.last_event_at = Some(now);
            record.error = None;
            record.usage = usage;
            record
                .applier
                .replace_log(parse_log(&body, self.inner.clock.now()));
            record.applier.observe_revision(self.inner.bus.revision());
            record.mapper.reset_from_messages(record.applier.messages());
            self.inner.view_for(session, record)
        };
        let view = self.inner.clone().emit_session(view).await;
        ensure_pump(&self.inner, &endpoint);
        // A turn that was already waiting on a permission or retry when the UI (re)attached.
        self.inner.clone().sync_permissions(session).await;
        self.inner.clone().sync_status(session).await;
        Ok(self.inner.current_view(session).await.unwrap_or(view))
    }

    async fn attach_orchestrator(
        &self,
        session: &ChatSessionId,
    ) -> Result<ChatSessionView, SpiritError> {
        if self.is_engine_backed(session).await {
            return self.attach_setup(session).await;
        }
        let name = orchestrator_name(session).ok_or_else(chat_not_started_error)?;
        let existing = {
            let state = self.inner.state.lock().await;
            state
                .sessions
                .get(session)
                .and_then(|record| record.vowp_session_id.clone())
        };
        let resolved = self
            .inner
            .resolve_local_endpoint(name, existing.as_deref())
            .await?;
        let result = self.open_orchestrator(name, session, resolved).await;
        if let Err(error) = &result
            && error.code == SpiritErrorCode::NotFound
        {
            self.inner.remove_session(session).await;
        }
        result
    }

    /// Accepts one user turn and returns immediately. Assistant text arrives as events.
    pub async fn send(
        &self,
        session: &ChatSessionId,
        text: &str,
    ) -> Result<ChatSendReceipt, SpiritError> {
        let text = validate_user_text(text)?;
        let (native, stored_endpoint, setup) = {
            let state = self.inner.state.lock().await;
            let record = state
                .sessions
                .get(session)
                .ok_or_else(chat_not_started_error)?;
            let setup = matches!(session, ChatSessionId::Setup) || record.engine_backed;
            if !record.attached {
                return Err(chat_not_started_error());
            }
            match record.state {
                ChatSessionState::Streaming => return Err(chat_busy_error()),
                ChatSessionState::Ready => {}
                _ => return Err(chat_not_started_error()),
            }
            let endpoint = record
                .endpoint
                .clone()
                .ok_or_else(|| engine_not_ready_error("the local endpoint is not attached"))?;
            (record.native_session_id.clone(), endpoint, setup)
        };
        let endpoint = if setup {
            let port = self.inner.ensure_engine().await?;
            self.inner.check_health(port).await?;
            let endpoint = ChatEndpoint::engine(port, &self.inner.workspace);
            if endpoint.port != stored_endpoint.port {
                match self.inner.get_native_session(&endpoint, &native).await {
                    Ok(()) => {}
                    Err(error) if error.code == SpiritErrorCode::NotFound => {
                        self.inner.remove_session(session).await;
                        return Err(session_lost_error());
                    }
                    Err(error) => return Err(error),
                }
            }
            endpoint
        } else {
            match self
                .inner
                .get_native_session(&stored_endpoint, &native)
                .await
            {
                Ok(()) => {}
                Err(error) if error.code == SpiritErrorCode::NotFound => {
                    self.inner.remove_session(session).await;
                    return Err(session_lost_error());
                }
                Err(error) => return Err(error),
            }
            stored_endpoint
        };
        // The setup assistant defaults to the first verified free runtime model when the
        // owner has not saved a choice. A VOWP orchestrator owns its own OpenCode config,
        // so no model is invented or copied from the setup engine; the message omits
        // `model` and lets the orchestrator choose.
        let model = if setup {
            Some(
                self.inner
                    .resolve_setup_model()
                    .await?
                    .ok_or_else(no_model_error)?,
            )
        } else {
            None
        };

        let now = self.inner.clock.now();
        // OpenCode validates outgoing message ids and rejects anything that does not
        // start with `msg`. It echoes this id in the native log, so the optimistic
        // message upserts by id instead of appearing twice.
        let user = ChatMessage::user(format!("msg_{}", Ulid::generate()), &text, now);
        let (view, turn_seq) = {
            let mut state = self.inner.state.lock().await;
            let record = state
                .sessions
                .get_mut(session)
                .ok_or_else(chat_not_started_error)?;
            match record.state {
                ChatSessionState::Streaming => return Err(chat_busy_error()),
                ChatSessionState::Ready => {}
                _ => return Err(chat_not_started_error()),
            }
            record.endpoint = Some(endpoint.clone());
            record.state = ChatSessionState::Streaming;
            record.error = None;
            record.retry = None;
            record.active_turn = Some(user.id.clone());
            record.turn_completed = false;
            record.turn_cancelled = false;
            record.turn_seq = record.turn_seq.saturating_add(1);
            record.last_event_at = Some(now);
            (self.inner.view_for(session, record), record.turn_seq)
        };
        self.inner.clone().emit_session(view).await;
        let user_event = user.clone();
        let session_for_event = session.clone();
        self.inner
            .clone()
            .emit_with(move |events| events.message(session_for_event, user_event))
            .await;

        let body = message_body(model.as_ref(), &text, &user.id);
        ensure_pump(&self.inner, &endpoint);
        spawn_turn(
            self.inner.clone(),
            endpoint,
            native,
            session.clone(),
            body,
            turn_seq,
        );
        Ok(ChatSendReceipt {
            session: session.clone(),
            message: user,
            accepted: true,
        })
    }

    /// Answers one pending OpenCode permission request and returns the refreshed view.
    ///
    /// Nothing is approved implicitly; this is only called from an owner click.
    pub async fn reply_permission(
        &self,
        session: &ChatSessionId,
        permission_id: &str,
        decision: ChatPermissionDecision,
    ) -> Result<ChatSessionView, SpiritError> {
        self.inner
            .clone()
            .reply_permission(session, permission_id, decision)
            .await
    }

    /// Aborts the current turn when supported and returns the stopped view.
    ///
    /// Stop is a turn control, not a session teardown: the native session stays attached,
    /// `active_turn`, `retry` and `pending_permissions` are cleared, and the background
    /// turn is invalidated so no late text can reopen it. The view keeps reporting
    /// [`ChatSessionState::Stopped`] (the documented "the owner stopped the turn" state)
    /// and the legacy `closed`/`stopped` notice; clients re-attach to the same native
    /// session for the next turn, so the composer is usable without a Start click.
    ///
    /// When the engine does not confirm the abort, the view carries a non-fatal
    /// `stop_unconfirmed` notice instead of pretending the turn stopped.
    pub async fn stop(&self, session: &ChatSessionId) -> Result<ChatSessionView, SpiritError> {
        let now = self.inner.clock.now();
        let (native, endpoint, streaming) = {
            let mut state = self.inner.state.lock().await;
            let record = state
                .sessions
                .get_mut(session)
                .ok_or_else(chat_not_started_error)?;
            let endpoint = record
                .endpoint
                .clone()
                .ok_or_else(|| engine_not_ready_error("the local endpoint is not attached"))?;
            let streaming = record.state == ChatSessionState::Streaming;
            // Mark the turn cancelled before the abort request so a racing send cannot
            // start a second turn while the first abort is in flight, and invalidate the
            // background POST so its late response cannot overwrite the stopped view.
            record.state = ChatSessionState::Stopped;
            record.attached = true;
            record.active_turn = None;
            record.turn_completed = true;
            record.turn_cancelled = true;
            record.turn_seq = record.turn_seq.saturating_add(1);
            record.retry = None;
            record.pending_permissions.clear();
            record.last_event_at = Some(now);
            record.error = None;
            (record.native_session_id.clone(), endpoint, streaming)
        };
        let confirmed = if streaming {
            self.inner.abort(&endpoint, &native).await.unwrap_or(false)
        } else {
            true
        };
        let view = {
            let mut state = self.inner.state.lock().await;
            let record = state
                .sessions
                .get_mut(session)
                .ok_or_else(chat_not_started_error)?;
            record.error = if confirmed {
                None
            } else {
                Some(stop_unconfirmed_error())
            };
            self.inner.view_for(session, record)
        };
        let view = self.inner.clone().emit_session(view).await;
        // The pinned 1.19.5 contract emits this notice on Stop, and existing clients
        // (and tests) rely on it. The session record stays attached, so a client that
        // wants another turn re-attaches; a real teardown uses the other close reasons.
        self.inner
            .clone()
            .emit_with(|events| events.closed(session.clone(), ChatCloseReason::Stopped))
            .await;
        Ok(view)
    }

    /// Switches the setup assistant's model and returns the updated view.
    ///
    /// The explicit owner choice is persisted by the engine adapter. An orchestrator
    /// session owns its own model config, so switching it here is refused rather than
    /// pretending the app can change it.
    pub async fn set_model(
        &self,
        session: &ChatSessionId,
        model_id: &str,
    ) -> Result<ChatSessionView, SpiritError> {
        let model_id = validate_model_id(model_id)?;
        if !matches!(session, ChatSessionId::Setup) && !self.is_engine_backed(session).await {
            return Err(chat_model_switch_gated_error());
        }
        {
            let state = self.inner.state.lock().await;
            let record = state
                .sessions
                .get(session)
                .filter(|record| record.attached)
                .ok_or_else(chat_not_started_error)?;
            if record.endpoint.is_none() {
                return Err(chat_not_started_error());
            }
        }
        self.inner
            .engine
            .select_model(&model_id, true)
            .await
            .map_err(|error| error.with_hint("Choose an available model and retry."))?;
        let view = {
            let state = self.inner.state.lock().await;
            let record = state
                .sessions
                .get(session)
                .ok_or_else(chat_not_started_error)?;
            self.inner.view_for(session, record)
        };
        Ok(self.inner.clone().emit_session(view).await)
    }

    /// Reads the active native log, newest page first.
    pub async fn log(
        &self,
        session: &ChatSessionId,
        query: &ChatLogQuery,
    ) -> Result<ChatLogPage, SpiritError> {
        let (native, endpoint) = {
            let state = self.inner.state.lock().await;
            let record = state
                .sessions
                .get(session)
                .ok_or_else(chat_not_started_error)?;
            let endpoint = record
                .endpoint
                .clone()
                .ok_or_else(|| engine_not_ready_error("the local endpoint is not attached"))?;
            (record.native_session_id.clone(), endpoint)
        };
        let body = match self.inner.read_log(&endpoint, &native).await {
            Ok(body) => body,
            Err(error) if error.code == SpiritErrorCode::NotFound => {
                self.inner.remove_session(session).await;
                return Err(session_lost_error());
            }
            Err(error) => return Err(error),
        };
        let messages = parse_log(&body, self.inner.clock.now());
        page_messages(messages, query)
    }
}

#[async_trait]
impl ChatTransport for LocalChatProvider {
    async fn start(&self, input: ChatStartInput) -> Result<ChatSessionView, SpiritError> {
        LocalChatProvider::start(self, &input).await
    }

    async fn attach(&self, session: &ChatSessionId) -> Result<ChatSessionView, SpiritError> {
        LocalChatProvider::attach(self, session).await
    }

    async fn send(
        &self,
        session: &ChatSessionId,
        text: &str,
    ) -> Result<ChatSendReceipt, SpiritError> {
        LocalChatProvider::send(self, session, text).await
    }

    async fn stop(&self, session: &ChatSessionId) -> Result<ChatSessionView, SpiritError> {
        LocalChatProvider::stop(self, session).await
    }

    async fn provision_orchestrator(&self, project: &str) -> Result<ChatSessionView, SpiritError> {
        LocalChatProvider::provision_orchestrator(self, project).await
    }

    async fn set_model(
        &self,
        session: &ChatSessionId,
        model_id: &str,
    ) -> Result<ChatSessionView, SpiritError> {
        LocalChatProvider::set_model(self, session, model_id).await
    }

    async fn log(
        &self,
        session: &ChatSessionId,
        query: &ChatLogQuery,
    ) -> Result<ChatLogPage, SpiritError> {
        LocalChatProvider::log(self, session, query).await
    }
}

impl Inner {
    fn view_for(&self, session: &ChatSessionId, record: &SessionRecord) -> ChatSessionView {
        // The setup assistant shows the bundled engine's selected model and its verified
        // usable list. A VOWP orchestrator owns its own config, so the core neither copies
        // the setup model nor invents an orchestrator model list.
        let (model, available_models) =
            if matches!(session, ChatSessionId::Setup) || record.engine_backed {
                (
                    self.engine.selected_model(),
                    Some(self.engine.available_models()),
                )
            } else {
                (None, None)
            };
        ChatSessionView {
            session: session.clone(),
            location: ChatSessionLocation::Local,
            state: record.state,
            attached: record.attached,
            streaming_supported: self.stream_support.is_supported(),
            model,
            available_models,
            usage: record.usage.clone(),
            started_at: record.started_at,
            last_event_at: record.last_event_at,
            error: record.error.clone(),
            pending_permissions: (!record.pending_permissions.is_empty())
                .then(|| record.pending_permissions.clone()),
            retry: record.retry.clone(),
        }
    }

    /// Rebinds every bundled-engine session to the engine's current port, replaces the
    /// stale-port pump, and emits refreshed `Ready` views with the current model list.
    async fn rebind_engine_sessions(self: Arc<Self>) {
        let sessions: Vec<ChatSessionId> = {
            let state = self.state.lock().await;
            state
                .sessions
                .iter()
                .filter(|(session, record)| {
                    matches!(session, ChatSessionId::Setup) || record.engine_backed
                })
                .map(|(session, _)| session.clone())
                .collect()
        };
        if sessions.is_empty() {
            return;
        }
        // An unready or external engine still gets a fresh model list below.
        if let Ok(port) = self.engine.ensure_ready().await
            && !matches!(self.engine.status().choice, EngineChoice::External { .. })
        {
            let endpoint = ChatEndpoint::engine(port, &self.workspace);
            let mut moved = Vec::new();
            let mut attached = false;
            {
                let mut state = self.state.lock().await;
                for session in &sessions {
                    if let Some(record) = state.sessions.get_mut(session) {
                        if record.endpoint.as_ref().map(|e| e.port) != Some(port) {
                            moved.push(session.clone());
                        }
                        record.endpoint = Some(endpoint.clone());
                        attached |= record.attached;
                    }
                }
            }
            if attached {
                ensure_pump(&self, &endpoint);
            }
            // A session left in Connecting/Error by the dead port is Ready again once the
            // new stream is up; the pump's connect handler reloads the log and syncs.
            let now = self.clock.now();
            let mut state = self.state.lock().await;
            for session in &moved {
                if let Some(record) = state.sessions.get_mut(session)
                    && record.attached
                    && matches!(
                        record.state,
                        ChatSessionState::Connecting | ChatSessionState::Error
                    )
                    && record.error.as_ref().is_none_or(is_stream_lost)
                {
                    record.state = ChatSessionState::Ready;
                    record.error = None;
                    record.last_event_at = Some(now);
                }
            }
        }
        for session in &sessions {
            if let Some(view) = self.current_view(session).await {
                self.clone().emit_session(view).await;
            }
        }
    }

    /// True when the bundled engine is now reachable on a port other than `port`.
    async fn engine_moved_from(&self, port: u16) -> bool {
        self.engine
            .ensure_ready()
            .await
            .is_ok_and(|current| current != port)
            && !matches!(self.engine.status().choice, EngineChoice::External { .. })
    }

    async fn current_view(&self, session: &ChatSessionId) -> Option<ChatSessionView> {
        let state = self.state.lock().await;
        state
            .sessions
            .get(session)
            .map(|record| self.view_for(session, record))
    }

    /// Reads `GET /permission`, keeps the entries for this session's native id and
    /// publishes the view when the pending set changed. Errors are ignored: the SSE
    /// events remain the primary signal and this is a best-effort reconciliation.
    async fn sync_permissions(self: Arc<Self>, session: &ChatSessionId) {
        let (endpoint, native) = {
            let state = self.state.lock().await;
            let Some(record) = state.sessions.get(session) else {
                return;
            };
            let Some(endpoint) = record.endpoint.clone() else {
                return;
            };
            (endpoint, record.native_session_id.clone())
        };
        let Ok(response) = self
            .request(
                "GET",
                endpoint,
                "/permission".to_owned(),
                None,
                SESSION_TIMEOUT,
            )
            .await
        else {
            return;
        };
        if !(200..300).contains(&response.status) {
            return;
        }
        let now = self.clock.now();
        let view = {
            let mut state = self.state.lock().await;
            let Some(record) = state.sessions.get_mut(session) else {
                return;
            };
            if record.native_session_id != native {
                return;
            }
            let mut found = parse_permissions(&response.body, &native, now);
            for prompt in &mut found {
                if let Some(existing) = record
                    .pending_permissions
                    .iter()
                    .find(|existing| existing.id == prompt.id)
                {
                    prompt.created_at = existing.created_at;
                }
            }
            if found == record.pending_permissions {
                return;
            }
            record.pending_permissions = found;
            record.last_event_at = Some(now);
            self.view_for(session, record)
        };
        self.emit_session(view).await;
    }

    /// Reads `GET /session/status`, keeps the entry for this session's native id and
    /// publishes the view when the retry state changed.
    ///
    /// This is the attach-time fallback: a session already sleeping in a retry loop must
    /// show its notice before the next SSE frame. An unreadable or unsupported response
    /// is ignored instead of clearing a retry the stream already delivered.
    async fn sync_status(self: Arc<Self>, session: &ChatSessionId) {
        let (endpoint, native) = {
            let state = self.state.lock().await;
            let Some(record) = state.sessions.get(session) else {
                return;
            };
            let Some(endpoint) = record.endpoint.clone() else {
                return;
            };
            (endpoint, record.native_session_id.clone())
        };
        let Ok(response) = self
            .request(
                "GET",
                endpoint,
                "/session/status".to_owned(),
                None,
                SESSION_TIMEOUT,
            )
            .await
        else {
            return;
        };
        if !(200..300).contains(&response.status) {
            return;
        }
        let Ok(retry) = parse_session_status_map(&response.body, &native) else {
            return;
        };
        let now = self.clock.now();
        let view = {
            let mut state = self.state.lock().await;
            let Some(record) = state.sessions.get_mut(session) else {
                return;
            };
            if record.native_session_id != native
                || matches!(
                    record.state,
                    ChatSessionState::Stopped | ChatSessionState::Error
                )
            {
                return;
            }
            if retry == record.retry {
                return;
            }
            let had_retry = record.retry.is_some();
            record.retry = retry;
            if record.retry.is_some() {
                record.state = ChatSessionState::Streaming;
            } else if had_retry
                && record.active_turn.is_none()
                && record.state == ChatSessionState::Streaming
            {
                record.state = ChatSessionState::Ready;
            }
            record.last_event_at = Some(now);
            self.view_for(session, record)
        };
        self.emit_session(view).await;
    }

    /// Answers one pending permission and returns the refreshed view.
    async fn reply_permission(
        self: Arc<Self>,
        session: &ChatSessionId,
        permission_id: &str,
        decision: ChatPermissionDecision,
    ) -> Result<ChatSessionView, SpiritError> {
        let id = safe_native_id(permission_id)
            .map_err(|_| SpiritError::invalid_input("permission", "invalid permission id"))?;
        let (endpoint, native) = {
            let state = self.state.lock().await;
            let record = state
                .sessions
                .get(session)
                .ok_or_else(chat_not_started_error)?;
            let endpoint = record
                .endpoint
                .clone()
                .ok_or_else(|| engine_not_ready_error("the local endpoint is not attached"))?;
            (endpoint, record.native_session_id.clone())
        };
        // A prompt the stream already delivered is authoritative; do not let a
        // transiently empty `GET /permission` erase it before the owner's reply. Only
        // reconcile when the id is unknown, so a request the stream missed can still be
        // discovered and answered.
        let was_known = {
            let state = self.state.lock().await;
            state.sessions.get(session).is_some_and(|record| {
                record
                    .pending_permissions
                    .iter()
                    .any(|prompt| prompt.id == permission_id)
            })
        };
        if !was_known {
            self.clone().sync_permissions(session).await;
        }
        let known = {
            let state = self.state.lock().await;
            state.sessions.get(session).is_some_and(|record| {
                record
                    .pending_permissions
                    .iter()
                    .any(|prompt| prompt.id == permission_id)
            })
        };
        if known {
            let body =
                serde_json::to_vec(&json!({ "reply": decision.as_wire() })).unwrap_or_default();
            let response = self
                .request(
                    "POST",
                    endpoint.clone(),
                    format!("/permission/{id}/reply"),
                    Some(body),
                    SESSION_TIMEOUT,
                )
                .await?;
            if matches!(response.status, 404 | 405) {
                // Older engines answer on the session route with `response`.
                let body = serde_json::to_vec(&json!({ "response": decision.as_wire() }))
                    .unwrap_or_default();
                let legacy = self
                    .request(
                        "POST",
                        endpoint,
                        format!("/session/{}/permissions/{id}", safe_native_id(&native)?),
                        Some(body),
                        SESSION_TIMEOUT,
                    )
                    .await?;
                if legacy.status == 404 {
                    // Neither route exists. Do not hide the card: the turn is still
                    // waiting, and the owner must see that the answer was not sent.
                    return Err(engine_request_error(
                        "the engine exposes no permission reply route",
                    ));
                }
                ensure_success(&legacy)?;
            } else {
                ensure_success(&response)?;
            }
        }
        let now = self.clock.now();
        let view = {
            let mut state = self.state.lock().await;
            let record = state
                .sessions
                .get_mut(session)
                .ok_or_else(chat_not_started_error)?;
            record
                .pending_permissions
                .retain(|prompt| prompt.id != permission_id);
            record.last_event_at = Some(now);
            self.view_for(session, record)
        };
        Ok(self.emit_session(view).await)
    }

    /// Returns a saved selection, or defaults to the first verified free model.
    ///
    /// A BYOK model is a legitimate fallback when the engine reports no free model. A
    /// zero-model engine returns `None`; no fake included model is ever constructed.
    async fn resolve_setup_model(&self) -> Result<Option<ChatModelView>, SpiritError> {
        if let Some(model) = self.engine.selected_model() {
            return Ok(Some(model));
        }
        let available = self.engine.available_models();
        let Some(preferred) = preferred_setup_model(&available) else {
            return Ok(None);
        };
        self.engine
            .select_model(&preferred.id, false)
            .await
            .map(Some)
    }

    fn selected_context_limit(&self) -> Option<u64> {
        self.engine
            .selected_model()
            .and_then(|model| model.context_tokens)
    }

    async fn ensure_engine(&self) -> Result<u16, SpiritError> {
        if matches!(self.engine.status().choice, EngineChoice::External { .. }) {
            return Err(engine_external_error());
        }
        self.engine.ensure_ready().await
    }

    async fn resolve_local_endpoint(
        &self,
        name: &str,
        expected_session_id: Option<&str>,
    ) -> Result<ResolvedEndpoint, SpiritError> {
        self.resolver.resolve(name, expected_session_id).await
    }

    /// Lists the host's OpenCode sessions and returns the newest one.
    ///
    /// This is the only path that discovers a native id. It never creates a session,
    /// never guesses when the list is empty or unreadable, and names only the
    /// orchestrator and the failure class in errors. The URL, credential, native ids and
    /// workspace path stay core-only.
    async fn discover_native_session(
        &self,
        endpoint: &ChatEndpoint,
        name: &str,
    ) -> Result<DiscoveredSession, SpiritError> {
        let response = self
            .request(
                "GET",
                endpoint.clone(),
                "/api/session".to_owned(),
                None,
                SESSION_TIMEOUT,
            )
            .await
            .map_err(|_| chat_not_configured_error(name, "is not reachable"))?;
        if matches!(response.status, 401 | 403) {
            return Err(chat_auth_error(name));
        }
        if !(200..300).contains(&response.status) {
            return Err(chat_not_configured_error(
                name,
                "did not list its OpenCode sessions",
            ));
        }
        let expected_workspace = endpoint.directory.as_deref();
        parse_discovered_session(&response.body, expected_workspace, name)
    }

    async fn check_health(&self, port: u16) -> Result<(), SpiritError> {
        let response = self
            .request(
                "GET",
                ChatEndpoint::engine(port, &self.workspace),
                "/global/health".to_owned(),
                None,
                HEALTH_TIMEOUT,
            )
            .await?;
        if !(200..300).contains(&response.status) {
            return Err(engine_not_ready_error(format!(
                "engine health returned HTTP {}",
                response.status
            )));
        }
        let Ok(document) = serde_json::from_slice::<Value>(&response.body) else {
            return Err(engine_not_ready_error(
                "engine health returned invalid JSON",
            ));
        };
        if document.get("healthy").and_then(Value::as_bool) != Some(true) {
            return Err(engine_not_ready_error("engine health reported unhealthy"));
        }
        Ok(())
    }

    async fn request(
        &self,
        method: &'static str,
        endpoint: ChatEndpoint,
        path: String,
        body: Option<Vec<u8>>,
        read_timeout: Duration,
    ) -> Result<ChatResponse, SpiritError> {
        tokio::task::spawn_blocking(move || {
            http_request(&endpoint, method, &path, body.as_deref(), read_timeout)
        })
        .await
        .map_err(|_| {
            SpiritError::new(
                SpiritErrorCode::Internal,
                "the local chat request could not run",
            )
        })?
    }

    async fn create_native_session(&self, endpoint: &ChatEndpoint) -> Result<String, SpiritError> {
        self.create_native_session_titled(endpoint, SETUP_TITLE)
            .await
    }

    async fn create_native_session_titled(
        &self,
        endpoint: &ChatEndpoint,
        title: &str,
    ) -> Result<String, SpiritError> {
        let body = serde_json::to_vec(&json!({ "title": title })).map_err(|_| {
            SpiritError::new(
                SpiritErrorCode::Internal,
                "the setup session request could not be encoded",
            )
        })?;
        let response = self
            .request(
                "POST",
                endpoint.clone(),
                "/session".to_owned(),
                Some(body),
                SESSION_TIMEOUT,
            )
            .await?;
        ensure_success(&response)?;
        let Ok(document) = serde_json::from_slice::<Value>(&response.body) else {
            return Err(engine_request_error(
                "the local engine returned an unreadable session",
            ));
        };
        document
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| safe_native_id(id).is_ok())
            .map(str::to_owned)
            .ok_or_else(|| {
                engine_request_error("the local engine did not return a usable session id")
            })
    }

    async fn get_native_session(
        &self,
        endpoint: &ChatEndpoint,
        native: &str,
    ) -> Result<(), SpiritError> {
        let path = format!("/session/{}", safe_native_id(native)?);
        let response = self
            .request("GET", endpoint.clone(), path, None, SESSION_TIMEOUT)
            .await?;
        if response.status == 404 {
            return Err(session_lost_error());
        }
        ensure_success(&response)
    }

    async fn read_log(
        &self,
        endpoint: &ChatEndpoint,
        native: &str,
    ) -> Result<Vec<u8>, SpiritError> {
        let path = format!("/session/{}/message", safe_native_id(native)?);
        let response = self
            .request("GET", endpoint.clone(), path, None, SESSION_TIMEOUT)
            .await?;
        if response.status == 404 {
            return Err(session_lost_error());
        }
        ensure_success(&response)?;
        Ok(response.body)
    }

    async fn abort(&self, endpoint: &ChatEndpoint, native: &str) -> Result<bool, SpiritError> {
        let path = format!("/session/{}/abort", safe_native_id(native)?);
        for attempt in 0..2 {
            let result = self
                .request(
                    "POST",
                    endpoint.clone(),
                    path.clone(),
                    Some(Vec::new()),
                    ABORT_TIMEOUT,
                )
                .await;
            match result {
                Ok(response) if (200..300).contains(&response.status) => return Ok(true),
                Ok(response) if matches!(response.status, 404 | 405) => return Ok(false),
                Ok(response) => return Err(map_http_status(response.status)),
                Err(_) if attempt == 0 => continue,
                Err(error) => return Err(error),
            }
        }
        Err(engine_request_error(
            "the local engine did not answer the abort request",
        ))
    }

    async fn remove_session(&self, session: &ChatSessionId) {
        self.state.lock().await.sessions.remove(session);
    }

    fn project_and_publish_locked(
        &self,
        state: &mut ProviderState,
        event: &ChatEvent,
    ) -> Option<ChatSessionId> {
        let target = event.session().clone();
        let now = self.clock.now();
        let mut backfill = false;
        for (session, record) in &mut state.sessions {
            if *session == target {
                if record.applier.apply(event, now) == ApplyOutcome::Backfill {
                    backfill = true;
                }
            } else {
                record.applier.observe_revision(event.revision());
            }
        }
        let _ = self.bus.publish(event.clone());
        if backfill { Some(target) } else { None }
    }

    async fn emit_session(self: Arc<Self>, view: ChatSessionView) -> ChatSessionView {
        let mut state = self.state.lock().await;
        let event = self.bus.session(view.clone());
        let backfill = self.project_and_publish_locked(&mut state, &event);
        drop(state);
        if let Some(session) = backfill {
            schedule_reload(&self, session);
        }
        view
    }

    async fn emit_with(self: Arc<Self>, build: impl FnOnce(&ChatEventBus) -> ChatEvent + Send) {
        let mut state = self.state.lock().await;
        let event = build(&self.bus);
        let backfill = self.project_and_publish_locked(&mut state, &event);
        drop(state);
        if let Some(session) = backfill {
            schedule_reload(&self, session);
        }
    }

    async fn emit_message(self: Arc<Self>, session: &ChatSessionId, message: ChatMessage) {
        let session = session.clone();
        self.emit_with(move |events| events.message(session, message))
            .await;
    }

    async fn emit_delta(
        self: Arc<Self>,
        session: &ChatSessionId,
        message_id: &str,
        part_id: Option<String>,
        text: &str,
    ) {
        let session = session.clone();
        let message_id = message_id.to_owned();
        let text = text.to_owned();
        self.emit_with(move |events| events.delta(session, message_id, part_id, text))
            .await;
    }

    async fn emit_done(
        self: Arc<Self>,
        session: &ChatSessionId,
        message_id: &str,
        truncated: bool,
    ) {
        let session = session.clone();
        let message_id = message_id.to_owned();
        self.emit_with(move |events| events.done(session, message_id, truncated))
            .await;
    }

    async fn fail_session(self: Arc<Self>, session: &ChatSessionId, error: SpiritError) {
        let now = self.clock.now();
        let view = {
            let mut state = self.state.lock().await;
            let Some(record) = state.sessions.get_mut(session) else {
                return;
            };
            if record.state == ChatSessionState::Stopped {
                return;
            }
            record.state = ChatSessionState::Error;
            record.error = Some(error.clone());
            record.active_turn = None;
            record.turn_completed = true;
            record.retry = None;
            record.pending_permissions.clear();
            record.last_event_at = Some(now);
            self.view_for(session, record)
        };
        let session_for_error = session.clone();
        self.clone()
            .emit_with(move |events| events.error(session_for_error, error))
            .await;
        self.emit_session(view).await;
    }

    async fn reload_session(self: Arc<Self>, session: &ChatSessionId) {
        self.reload_log(session, true).await;
    }

    /// Reloads the native log and emits changed messages. A quiet reload (tool
    /// activity) ignores a transient read failure instead of failing the session.
    async fn reload_log(self: Arc<Self>, session: &ChatSessionId, fail_on_error: bool) {
        let (endpoint, native) = {
            let state = self.state.lock().await;
            let Some(record) = state.sessions.get(session) else {
                return;
            };
            if record.state == ChatSessionState::Stopped {
                return;
            }
            let Some(endpoint) = record.endpoint.clone() else {
                return;
            };
            (endpoint, record.native_session_id.clone())
        };
        let body = match self.read_log(&endpoint, &native).await {
            Ok(body) => body,
            Err(error) => {
                if endpoint.setup && self.engine_moved_from(endpoint.port).await {
                    // The bundled engine restarted on another port; rebind instead of
                    // failing the session on the dead one.
                    tokio::spawn(self.clone().rebind_engine_sessions());
                } else if fail_on_error {
                    self.fail_session(session, error).await;
                }
                return;
            }
        };
        let messages = parse_log(&body, self.clock.now());
        let context_limit = if matches!(session, ChatSessionId::Setup) {
            self.selected_context_limit()
        } else {
            None
        };
        let usage = parse_usage(&body, context_limit);
        let changed = {
            let mut state = self.state.lock().await;
            let Some(record) = state.sessions.get_mut(session) else {
                return;
            };
            if record.native_session_id != native {
                return;
            }
            record.usage = usage;
            let old = record.applier.messages().to_vec();
            record.applier.replace_log(messages.clone());
            record.applier.observe_revision(self.bus.revision());
            record.mapper.reset_from_messages(record.applier.messages());
            changed_messages(&old, &messages)
        };
        for message in changed {
            self.clone().emit_message(session, message).await;
        }
    }

    async fn handle_frame(self: Arc<Self>, frame: SseFrame, port: u16, setup: bool) {
        let event = parse_engine_event(&frame);
        let Some(native) = event.session_id().map(str::to_owned) else {
            return;
        };
        let now = self.clock.now();
        let (session, mapped) = {
            let mut state = self.state.lock().await;
            let Some(session) = state
                .sessions
                .iter()
                .find(|(session, record)| {
                    record.native_session_id == native
                        && if setup {
                            matches!(session, ChatSessionId::Setup) || record.engine_backed
                        } else {
                            record
                                .endpoint
                                .as_ref()
                                .is_some_and(|endpoint| endpoint.port == port)
                        }
                })
                .map(|(session, _)| session.clone())
            else {
                return;
            };
            let Some(record) = state.sessions.get_mut(&session) else {
                return;
            };
            if matches!(
                record.state,
                ChatSessionState::Stopped | ChatSessionState::Error
            ) {
                return;
            }
            // Stop already returned the ready view; a late frame from the aborted turn
            // must not re-open the retry, add text or bring back a permission card.
            if record.turn_cancelled {
                return;
            }
            record.last_event_at = Some(now);
            let mapped = record.mapper.map(&event, now);
            (session, mapped)
        };
        match mapped {
            Mapped::None => {}
            Mapped::Retry(retry) => {
                let view = {
                    let mut state = self.state.lock().await;
                    let Some(record) = state.sessions.get_mut(&session) else {
                        return;
                    };
                    record.retry = Some(retry);
                    record.state = ChatSessionState::Streaming;
                    record.error = None;
                    record.last_event_at = Some(now);
                    self.view_for(&session, record)
                };
                self.emit_session(view).await;
            }
            Mapped::RetryCleared => {
                let view = {
                    let mut state = self.state.lock().await;
                    let Some(record) = state.sessions.get_mut(&session) else {
                        return;
                    };
                    if record.retry.take().is_none() {
                        return;
                    }
                    record.error = None;
                    record.last_event_at = Some(now);
                    if record.active_turn.is_none() && record.state == ChatSessionState::Streaming {
                        record.state = ChatSessionState::Ready;
                    }
                    self.view_for(&session, record)
                };
                self.emit_session(view).await;
            }
            Mapped::Message(message) => self.emit_message(&session, message).await,
            Mapped::Delta {
                message_id,
                part_id,
                text,
            } => self.emit_delta(&session, &message_id, part_id, &text).await,
            Mapped::Done {
                message_id,
                truncated,
            } => self.handle_idle(&session, &message_id, truncated).await,
            Mapped::Error { status } => {
                self.fail_session(&session, map_provider_error(status))
                    .await;
            }
            Mapped::PermissionSync => {
                self.clone().sync_permissions(&session).await;
            }
            Mapped::ToolActivity => {
                let queue = {
                    let mut state = self.state.lock().await;
                    state.sessions.get_mut(&session).is_some_and(|record| {
                        let queue = !record.tool_refresh_queued;
                        record.tool_refresh_queued = true;
                        queue
                    })
                };
                if queue {
                    schedule_tool_refresh(&self, session);
                }
            }
            Mapped::PermissionAsked(prompt) => {
                let view = {
                    let mut state = self.state.lock().await;
                    let Some(record) = state.sessions.get_mut(&session) else {
                        return;
                    };
                    if record
                        .pending_permissions
                        .iter()
                        .any(|existing| existing.id == prompt.id)
                    {
                        return;
                    }
                    record.pending_permissions.push(prompt);
                    self.view_for(&session, record)
                };
                self.emit_session(view).await;
            }
            Mapped::PermissionReplied { id } => {
                let view = {
                    let mut state = self.state.lock().await;
                    let Some(record) = state.sessions.get_mut(&session) else {
                        return;
                    };
                    let before = record.pending_permissions.len();
                    record.pending_permissions.retain(|prompt| prompt.id != id);
                    if record.pending_permissions.len() == before {
                        return;
                    }
                    self.view_for(&session, record)
                };
                self.emit_session(view).await;
            }
        }
    }

    async fn handle_idle(
        self: Arc<Self>,
        session: &ChatSessionId,
        message_id: &str,
        truncated: bool,
    ) {
        self.clone().reload_session(session).await;
        let now = self.clock.now();
        let (view, done) = {
            let mut state = self.state.lock().await;
            let Some(record) = state.sessions.get_mut(session) else {
                return;
            };
            if matches!(
                record.state,
                ChatSessionState::Stopped | ChatSessionState::Error
            ) {
                return;
            }
            let done_id = if message_id.is_empty() {
                record
                    .applier
                    .messages()
                    .iter()
                    .rev()
                    .find(|message| message.role == ChatRole::Assistant)
                    .map(|message| message.id.clone())
                    .unwrap_or_default()
            } else {
                message_id.to_owned()
            };
            record.state = ChatSessionState::Ready;
            record.error = None;
            record.active_turn = None;
            record.turn_completed = true;
            record.turn_cancelled = false;
            record.retry = None;
            // A finished turn cannot still be waiting on a permission; clearing here
            // also removes a card whose `permission.replied` frame was missed.
            record.pending_permissions.clear();
            record.last_event_at = Some(now);
            let truncated = truncated || record.applier.is_truncated(&done_id);
            (self.view_for(session, record), (done_id, truncated))
        };
        self.clone().emit_done(session, &done.0, done.1).await;
        self.emit_session(view).await;
    }

    fn pump_covers(
        session: &ChatSessionId,
        record: &SessionRecord,
        port: u16,
        setup: bool,
    ) -> bool {
        if setup {
            matches!(session, ChatSessionId::Setup) || record.engine_backed
        } else {
            record
                .endpoint
                .as_ref()
                .is_some_and(|endpoint| endpoint.port == port)
        }
    }

    async fn mark_reconnecting(self: Arc<Self>, port: u16, setup: bool) {
        let now = self.clock.now();
        let views = {
            let mut state = self.state.lock().await;
            let mut views = Vec::new();
            for (session, record) in &mut state.sessions {
                if record.attached
                    && Self::pump_covers(session, record, port, setup)
                    && matches!(
                        record.state,
                        ChatSessionState::Ready | ChatSessionState::Streaming
                    )
                {
                    record.state = ChatSessionState::Connecting;
                    record.last_event_at = Some(now);
                    views.push(self.view_for(session, record));
                }
            }
            views
        };
        for view in views {
            self.clone().emit_session(view).await;
        }
    }

    async fn on_sse_connected(self: Arc<Self>, port: u16, setup: bool) {
        if setup {
            // The bundled engine may have restarted on a new port. Keep the setup
            // record's endpoint aligned with the live pump before matching frames.
            let mut state = self.state.lock().await;
            for (session, record) in &mut state.sessions {
                if matches!(session, ChatSessionId::Setup) || record.engine_backed {
                    record.endpoint = Some(ChatEndpoint::engine(port, &self.workspace));
                }
            }
        }
        let sessions = {
            let state = self.state.lock().await;
            state
                .sessions
                .iter()
                .filter(|(session, record)| Self::pump_covers(session, record, port, setup))
                .map(|(session, _)| session.clone())
                .collect::<Vec<_>>()
        };
        for session in &sessions {
            self.clone().reload_session(session).await;
            // A request raised while the stream was down still has to become visible.
            self.clone().sync_permissions(session).await;
        }
        let now = self.clock.now();
        let views = {
            let mut state = self.state.lock().await;
            let mut views = Vec::new();
            for (session, record) in &mut state.sessions {
                if record.attached
                    && Self::pump_covers(session, record, port, setup)
                    && !matches!(
                        record.state,
                        ChatSessionState::Idle
                            | ChatSessionState::Stopped
                            | ChatSessionState::Error
                    )
                {
                    record.state = if record.retry.is_some() || record.active_turn.is_some() {
                        ChatSessionState::Streaming
                    } else {
                        ChatSessionState::Ready
                    };
                    record.error = None;
                    record.last_event_at = Some(now);
                    views.push(self.view_for(session, record));
                }
            }
            views
        };
        for view in views {
            self.clone().emit_session(view).await;
        }
        // The status route is authoritative for a retry that began while the stream
        // was down; run it after the reconnect state reset so a retry wins.
        for session in &sessions {
            self.clone().sync_status(session).await;
        }
    }

    async fn stream_lost(self: Arc<Self>, port: u16, setup: bool) {
        let error = stream_lost_error();
        let now = self.clock.now();
        let views = {
            let mut state = self.state.lock().await;
            let mut views = Vec::new();
            for (session, record) in &mut state.sessions {
                if record.attached
                    && Self::pump_covers(session, record, port, setup)
                    && matches!(
                        record.state,
                        ChatSessionState::Connecting
                            | ChatSessionState::Streaming
                            | ChatSessionState::Ready
                    )
                {
                    record.state = ChatSessionState::Error;
                    record.error = Some(error.clone());
                    record.active_turn = None;
                    record.turn_completed = true;
                    record.retry = None;
                    record.last_event_at = Some(now);
                    views.push(self.view_for(session, record));
                }
            }
            views
        };
        for view in views {
            let session = view.session.clone();
            let error = error.clone();
            self.clone()
                .emit_with(move |events| events.error(session, error))
                .await;
            self.clone().emit_session(view).await;
        }
    }
}

type BoxedTask = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>>;

fn schedule_reload(inner: &Arc<Inner>, session: ChatSessionId) {
    let inner = Arc::clone(inner);
    let future: BoxedTask = Box::pin(async move {
        inner.reload_session(&session).await;
    });
    tokio::spawn(future);
}

fn schedule_tool_refresh(inner: &Arc<Inner>, session: ChatSessionId) {
    let inner = Arc::clone(inner);
    let future: BoxedTask = Box::pin(async move {
        {
            let mut state = inner.state.lock().await;
            if let Some(record) = state.sessions.get_mut(&session) {
                record.tool_refresh_queued = false;
            }
        }
        inner.reload_log(&session, false).await;
    });
    tokio::spawn(future);
}

fn spawn_turn(
    inner: Arc<Inner>,
    endpoint: ChatEndpoint,
    native: String,
    session: ChatSessionId,
    body: Vec<u8>,
    turn_seq: u64,
) {
    tokio::spawn(async move {
        let path = match safe_native_id(&native) {
            Ok(path) => format!("/session/{path}/message"),
            Err(error) => {
                inner.fail_session(&session, error).await;
                return;
            }
        };
        let usage_endpoint = endpoint.clone();
        let result = inner
            .request("POST", endpoint, path, Some(body), TURN_CAP)
            .await;
        inner
            .finish_turn(&session, &native, &usage_endpoint, turn_seq, result)
            .await;
    });
}

impl Inner {
    async fn finish_turn(
        self: Arc<Self>,
        session: &ChatSessionId,
        native: &str,
        endpoint: &ChatEndpoint,
        turn_seq: u64,
        result: Result<ChatResponse, SpiritError>,
    ) {
        // Stop (or a new send) invalidates this background POST before it can write a
        // late assistant message or move the session out of its ready view.
        {
            let state = self.state.lock().await;
            if state
                .sessions
                .get(session)
                .is_none_or(|record| record.turn_seq != turn_seq)
            {
                return;
            }
        }
        let now = self.clock.now();
        let succeeded = matches!(&result, Ok(response) if (200..300).contains(&response.status));
        // The log is authoritative for tokens and cost. The engine may report usage on
        // the message response, but the next log read carries the same metadata; a failed
        // read leaves the previous usage untouched instead of inventing a value.
        let usage = if succeeded {
            let context_limit = if matches!(session, ChatSessionId::Setup) {
                self.selected_context_limit()
            } else {
                None
            };
            self.read_log(endpoint, native)
                .await
                .ok()
                .and_then(|body| parse_usage(&body, context_limit))
        } else {
            None
        };
        let outcome = match result {
            Ok(response) if (200..300).contains(&response.status) => {
                parse_assistant_response(&response.body, now)
            }
            Ok(response) => AssistantOutcome::Failed(map_http_status(response.status)),
            Err(error) => AssistantOutcome::Failed(error),
        };
        let (view, emits) = {
            let mut state = self.state.lock().await;
            let Some(record) = state.sessions.get_mut(session) else {
                return;
            };
            if record.native_session_id != native
                || record.turn_seq != turn_seq
                || matches!(
                    record.state,
                    ChatSessionState::Stopped | ChatSessionState::Error
                )
            {
                return;
            }
            let active = record.active_turn.take();
            if active.is_none() && !record.turn_completed {
                return;
            }
            if let Some(usage) = usage {
                record.usage = Some(usage);
            }
            let mut emits = Vec::new();
            match outcome {
                AssistantOutcome::Failed(error) => {
                    record.state = ChatSessionState::Error;
                    record.error = Some(error.clone());
                    record.turn_completed = true;
                    record.last_event_at = Some(now);
                    emits.push(TurnEmit::Error(error));
                }
                AssistantOutcome::Empty => {
                    if active.is_some() {
                        emits.push(TurnEmit::Done {
                            message_id: String::new(),
                            truncated: false,
                        });
                    }
                    record.state = ChatSessionState::Ready;
                    record.error = None;
                    record.turn_completed = true;
                    record.last_event_at = Some(now);
                }
                AssistantOutcome::Text {
                    id,
                    text,
                    created_at,
                    truncated,
                    tools,
                } => {
                    // A mid-turn log refresh can leave partial text behind, so the final
                    // response replaces the projection whenever text or tools differ.
                    let current = record.applier.message(&id);
                    let stale = current.is_none_or(|message| {
                        message.joined_text() != normalize_text(&text)
                            || (tools.is_some() && message.tools != tools)
                    });
                    if stale {
                        let mut message = ChatMessage::model(id.clone(), &text, created_at);
                        message.streaming = false;
                        message.tools = tools;
                        emits.push(TurnEmit::Message(message));
                    }
                    if active.is_some() {
                        let done_truncated = truncated || record.applier.is_truncated(&id);
                        emits.push(TurnEmit::Done {
                            message_id: id,
                            truncated: done_truncated,
                        });
                    }
                    record.state = ChatSessionState::Ready;
                    record.error = None;
                    record.turn_completed = true;
                    record.last_event_at = Some(now);
                }
            }
            record.active_turn = None;
            record.retry = None;
            record.turn_cancelled = false;
            record.pending_permissions.clear();
            (self.view_for(session, record), emits)
        };
        for emit in emits {
            match emit {
                TurnEmit::Message(message) => self.clone().emit_message(session, message).await,
                TurnEmit::Done {
                    message_id,
                    truncated,
                } => {
                    self.clone()
                        .emit_done(session, &message_id, truncated)
                        .await;
                }
                TurnEmit::Error(error) => {
                    let session_for_error = session.clone();
                    self.clone()
                        .emit_with(move |events| events.error(session_for_error, error))
                        .await;
                }
            }
        }
        self.emit_session(view).await;
    }
}

enum TurnEmit {
    Message(ChatMessage),
    Done { message_id: String, truncated: bool },
    Error(SpiritError),
}

fn ensure_pump(inner: &Arc<Inner>, endpoint: &ChatEndpoint) {
    if !inner.stream_support.is_supported() {
        return;
    }
    let port = endpoint.port;
    let setup = endpoint.setup;
    let mut pumps = inner
        .pumps
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if pumps
        .get(&port)
        .and_then(|slot| slot.handle.as_ref())
        .is_some_and(|handle| !handle.is_finished())
    {
        return;
    }
    if setup {
        // The bundled engine has one live endpoint. A restart on a new port replaces
        // the old setup pump rather than leaving both streams alive.
        let stale: Vec<u16> = pumps
            .iter()
            .filter(|(pump_port, slot)| slot.setup && **pump_port != port)
            .map(|(pump_port, _)| *pump_port)
            .collect();
        for pump_port in stale {
            stop_pump(&mut pumps, pump_port);
        }
    } else {
        stop_pump(&mut pumps, port);
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let weak = Arc::downgrade(inner);
    let handle = tokio::spawn(run_pump(weak, endpoint.clone(), setup, Arc::clone(&cancel)));
    pumps.insert(
        port,
        PumpSlot {
            setup,
            cancel: Some(cancel),
            handle: Some(handle),
        },
    );
}

fn stop_pump(pumps: &mut HashMap<u16, PumpSlot>, port: u16) {
    if let Some(mut old) = pumps.remove(&port) {
        if let Some(cancel) = old.cancel.take() {
            cancel.store(true, Ordering::SeqCst);
        }
        if let Some(handle) = old.handle.take() {
            handle.abort();
        }
    }
}

async fn run_pump(
    inner: Weak<Inner>,
    endpoint: ChatEndpoint,
    setup: bool,
    cancel: Arc<AtomicBool>,
) {
    let port = endpoint.port;
    let mut attempts = 0_usize;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return;
        }
        let Some(provider) = inner.upgrade() else {
            return;
        };
        if attempts > 0 {
            provider.clone().mark_reconnecting(port, setup).await;
        }
        let mut unsupported = false;
        let (sender, mut receiver) = mpsc::channel(SSE_CAPACITY);
        let reader_endpoint = endpoint.clone();
        let reader_cancel = Arc::clone(&cancel);
        let reader = tokio::task::spawn_blocking(move || {
            read_sse(&reader_endpoint, sender, reader_cancel);
        });
        while let Some(signal) = receiver.recv().await {
            match signal {
                SseSignal::Connected => {
                    attempts = 0;
                    provider.clone().on_sse_connected(port, setup).await;
                }
                SseSignal::Frame(frame) => {
                    provider.clone().handle_frame(frame, port, setup).await;
                }
                SseSignal::Failed => {}
                SseSignal::Unsupported => unsupported = true,
            }
        }
        let _ = reader.await;
        if unsupported {
            // The endpoint answers 200 but is not an SSE stream (for example an SPA
            // page). Reconnecting would only flicker, so attach once and stay Ready.
            if !cancel.load(Ordering::SeqCst) {
                provider.clone().on_sse_connected(port, setup).await;
            }
            return;
        }
        if cancel.load(Ordering::SeqCst) {
            return;
        }
        attempts = attempts.saturating_add(1);
        // A dead bundled-engine port usually means the engine restarted on a new one.
        // Probe it; when the port moved, hand every bundled session to the new port from
        // a separate task (rebinding stops this pump) and never report `stream_lost`.
        if setup && provider.engine_moved_from(port).await {
            tokio::spawn(provider.clone().rebind_engine_sessions());
            return;
        }
        if attempts > MAX_SSE_ATTEMPTS {
            provider.stream_lost(port, setup).await;
            return;
        }
        let delay = SSE_BACKOFF[attempts.saturating_sub(1).min(SSE_BACKOFF.len() - 1)];
        tokio::time::sleep(delay).await;
    }
}

enum SseSignal {
    Connected,
    Frame(SseFrame),
    Failed,
    /// 2xx response whose content type is not `text/event-stream`.
    Unsupported,
}

enum SseHead {
    Stream { chunked: bool, remainder: Vec<u8> },
    Unsupported,
}

fn read_sse(endpoint: &ChatEndpoint, sender: mpsc::Sender<SseSignal>, cancel: Arc<AtomicBool>) {
    let Some(mut stream) = connect_sse(endpoint) else {
        let _ = sender.blocking_send(SseSignal::Failed);
        return;
    };
    let (chunked, remainder) = match sse_headers(&mut stream) {
        Some(SseHead::Stream { chunked, remainder }) => (chunked, remainder),
        Some(SseHead::Unsupported) => {
            let _ = sender.blocking_send(SseSignal::Unsupported);
            return;
        }
        None => {
            let _ = sender.blocking_send(SseSignal::Failed);
            return;
        }
    };
    if sender.blocking_send(SseSignal::Connected).is_err() {
        return;
    }
    if stream.set_read_timeout(Some(SSE_READ_TIMEOUT)).is_err() {
        return;
    }
    let mut decoder = chunked.then(ChunkedStreamDecoder::new);
    let mut parser = SseParser::new();
    let initial = match decoder.as_mut() {
        Some(decoder) => decoder.push(&remainder),
        None => remainder,
    };
    let mut frames = parser.push(&initial);
    for frame in frames.drain(..) {
        if sender.blocking_send(SseSignal::Frame(frame)).is_err() {
            return;
        }
    }
    let mut idle = Duration::ZERO;
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        if cancel.load(Ordering::SeqCst) {
            return;
        }
        match stream.read(&mut buffer) {
            Ok(0) => {
                if let Some(frame) = parser.flush()
                    && sender.blocking_send(SseSignal::Frame(frame)).is_err()
                {
                    return;
                }
                return;
            }
            Ok(read) => {
                idle = Duration::ZERO;
                let decoded = match decoder.as_mut() {
                    Some(decoder) => decoder.push(&buffer[..read]),
                    None => buffer[..read].to_vec(),
                };
                for frame in parser.push(&decoded) {
                    if sender.blocking_send(SseSignal::Frame(frame)).is_err() {
                        return;
                    }
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                idle = idle.saturating_add(SSE_READ_TIMEOUT);
                if idle >= SSE_IDLE_TIMEOUT {
                    return;
                }
            }
            Err(_) => return,
        }
    }
}

/// Incremental HTTP/1.1 `Transfer-Encoding: chunked` decoder for SSE streams.
///
/// OpenCode serves `/event` with chunked transfer encoding; stripping chunk framing before
/// feeding [`SseParser`] prevents `\r\n<hex>\r\n` boundaries from splitting JSON lines.
struct ChunkedStreamDecoder {
    buf: Vec<u8>,
    remaining: Option<usize>,
    expect_crlf: bool,
    done: bool,
}

impl ChunkedStreamDecoder {
    fn new() -> Self {
        Self {
            buf: Vec::new(),
            remaining: None,
            expect_crlf: false,
            done: false,
        }
    }

    fn push(&mut self, bytes: &[u8]) -> Vec<u8> {
        if self.done {
            return Vec::new();
        }
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            if self.expect_crlf {
                if self.buf.len() < 2 {
                    break;
                }
                if self.buf.starts_with(b"\r\n") {
                    self.buf.drain(..2);
                } else if self.buf.starts_with(b"\n") {
                    self.buf.drain(..1);
                }
                self.expect_crlf = false;
            }
            if let Some(rem) = self.remaining {
                let take = rem.min(self.buf.len());
                if take == 0 {
                    break;
                }
                out.extend_from_slice(&self.buf[..take]);
                self.buf.drain(..take);
                if take == rem {
                    self.remaining = None;
                    self.expect_crlf = true;
                } else {
                    self.remaining = Some(rem - take);
                }
                continue;
            }
            let Some(pos) = self.buf.windows(2).position(|w| w == b"\r\n") else {
                break;
            };
            let line = String::from_utf8_lossy(&self.buf[..pos]).into_owned();
            self.buf.drain(..pos + 2);
            let hex = line.split(';').next().unwrap_or("").trim();
            let Ok(size) = usize::from_str_radix(hex, 16) else {
                self.done = true;
                break;
            };
            if size == 0 {
                self.done = true;
                break;
            }
            self.remaining = Some(size);
        }
        out
    }
}

/// Builds one request head.
///
/// The returned string can carry the loopback credential, so it is zeroized on drop.
/// It is written straight to the socket and never logged.
fn request_head(
    endpoint: &ChatEndpoint,
    method: &str,
    path: &str,
    accept: &str,
    content_length: Option<usize>,
) -> Zeroizing<String> {
    let mut head = Zeroizing::new(String::with_capacity(256));
    head.push_str(method);
    head.push(' ');
    head.push_str(path);
    head.push_str(" HTTP/1.1\r\nHost: ");
    head.push_str(endpoint.host.host_header());
    head.push_str("\r\nAccept: ");
    head.push_str(accept);
    head.push_str("\r\n");
    if accept == "application/json" {
        head.push_str("Content-Type: application/json\r\n");
    }
    if let Some(auth) = &endpoint.auth {
        head.push_str(&auth.header);
        head.push_str(": ");
        head.push_str(auth.value.as_str());
        head.push_str("\r\n");
    }
    if let Some(directory) = &endpoint.directory {
        head.push_str("x-opencode-directory: ");
        head.push_str(&header_value(directory));
        head.push_str("\r\n");
    }
    if let Some(length) = content_length {
        head.push_str("Content-Length: ");
        head.push_str(&length.to_string());
        head.push_str("\r\n");
    }
    head.push_str("Connection: close\r\n\r\n");
    head
}

fn connect_sse(endpoint: &ChatEndpoint) -> Option<TcpStream> {
    let address = endpoint.socket_address();
    let mut stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).ok()?;
    stream.set_read_timeout(Some(SSE_HEADER_TIMEOUT)).ok()?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .ok()?;
    let request = request_head(endpoint, "GET", "/event", "text/event-stream", None);
    stream.write_all(request.as_bytes()).ok()?;
    Some(stream)
}

fn sse_headers(stream: &mut TcpStream) -> Option<SseHead> {
    let mut raw = Vec::new();
    let mut chunk = [0_u8; 4 * 1024];
    loop {
        let boundary = raw.windows(4).position(|window| window == b"\r\n\r\n");
        if let Some(boundary) = boundary {
            let head = String::from_utf8_lossy(&raw[..boundary]);
            let status = head
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .and_then(|code| code.parse::<u16>().ok());
            if !matches!(status, Some(200..=299)) {
                return None;
            }
            let chunked = head.lines().any(|line| {
                let lower = line.to_ascii_lowercase();
                lower.starts_with("transfer-encoding:") && lower.contains("chunked")
            });
            let event_stream = head.lines().any(|line| {
                let lower = line.to_ascii_lowercase();
                lower.starts_with("content-type:") && lower.contains("text/event-stream")
            });
            if !event_stream {
                return Some(SseHead::Unsupported);
            }
            return Some(SseHead::Stream {
                chunked,
                remainder: raw[boundary + 4..].to_vec(),
            });
        }
        if raw.len() > 64 * 1024 {
            return None;
        }
        match stream.read(&mut chunk) {
            Ok(0) => return None,
            Ok(read) => raw.extend_from_slice(&chunk[..read]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return None;
            }
            Err(_) => return None,
        }
    }
}

#[derive(Debug)]
struct ChatResponse {
    status: u16,
    body: Vec<u8>,
}

fn http_request(
    endpoint: &ChatEndpoint,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
    read_timeout: Duration,
) -> Result<ChatResponse, SpiritError> {
    if !path.starts_with('/') {
        return Err(SpiritError::new(
            SpiritErrorCode::Internal,
            "the local chat path is invalid",
        ));
    }
    let address = endpoint.socket_address();
    let mut stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)
        .map_err(|_| engine_unavailable_error())?;
    stream
        .set_read_timeout(Some(read_timeout))
        .map_err(|_| engine_request_error("the local engine read timeout could not be set"))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|_| engine_request_error("the local engine write timeout could not be set"))?;
    let body = body.unwrap_or_default();
    let request = request_head(endpoint, method, path, "application/json", Some(body.len()));
    stream
        .write_all(request.as_bytes())
        .map_err(|error| request_io_error(&error))?;
    if !body.is_empty() {
        stream
            .write_all(body)
            .map_err(|error| request_io_error(&error))?;
    }
    let mut raw = Vec::new();
    let mut chunk = [0_u8; 8 * 1024];
    loop {
        let read = stream
            .read(&mut chunk)
            .map_err(|error| request_io_error(&error))?;
        if read == 0 {
            break;
        }
        if raw.len() + read > MAX_HTTP_BODY {
            return Err(engine_request_error(
                "the local engine response exceeded the local size cap",
            ));
        }
        raw.extend_from_slice(&chunk[..read]);
    }
    parse_http_response(&raw)
}

fn parse_http_response(raw: &[u8]) -> Result<ChatResponse, SpiritError> {
    let boundary = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| engine_request_error("the local engine response has no HTTP headers"))?;
    let headers = String::from_utf8_lossy(&raw[..boundary]);
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| engine_request_error("the local engine response has no HTTP status"))?;
    let body = &raw[boundary + 4..];
    let chunked = headers.lines().skip(1).any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.starts_with("transfer-encoding:") && lower.contains("chunked")
    });
    let body = if chunked {
        decode_chunked(body)?
    } else {
        body.to_vec()
    };
    Ok(ChatResponse { status, body })
}

fn decode_chunked(input: &[u8]) -> Result<Vec<u8>, SpiritError> {
    let mut output = Vec::new();
    let mut cursor = 0;
    loop {
        let line_end = input[cursor..]
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or_else(|| engine_request_error("the local engine response is malformed"))?;
        let header = std::str::from_utf8(&input[cursor..cursor + line_end])
            .map_err(|_| engine_request_error("the local engine response is malformed"))?;
        let size = usize::from_str_radix(header.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| engine_request_error("the local engine response is malformed"))?;
        cursor += line_end + 2;
        if size == 0 {
            break;
        }
        if cursor + size > input.len() {
            return Err(engine_request_error(
                "the local engine response is truncated",
            ));
        }
        output.extend_from_slice(&input[cursor..cursor + size]);
        cursor += size;
        if input.get(cursor..cursor + 2) == Some(b"\r\n") {
            cursor += 2;
        }
    }
    Ok(output)
}

fn request_io_error(error: &std::io::Error) -> SpiritError {
    match error.kind() {
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => turn_timeout_error(),
        std::io::ErrorKind::ConnectionRefused
        | std::io::ErrorKind::ConnectionReset
        | std::io::ErrorKind::ConnectionAborted
        | std::io::ErrorKind::NotFound => engine_unavailable_error(),
        _ => engine_request_error("the local engine request failed"),
    }
}

fn header_value(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .filter(|character| !character.is_control())
        .collect()
}

fn safe_native_id(native: &str) -> Result<String, SpiritError> {
    if native.is_empty()
        || native.len() > 200
        || !native
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(engine_request_error(
            "the local engine returned an unusable session id",
        ));
    }
    Ok(native.to_owned())
}

fn ensure_success(response: &ChatResponse) -> Result<(), SpiritError> {
    if (200..300).contains(&response.status) {
        Ok(())
    } else {
        Err(map_http_status(response.status))
    }
}

fn preferred_setup_model(available: &[ChatModelView]) -> Option<ChatModelView> {
    available
        .iter()
        .find(|model| model.tier == ChatModelTier::Included)
        .or_else(|| {
            available
                .iter()
                .find(|model| model.tier == ChatModelTier::Byok)
        })
        .cloned()
}

fn validate_model_id(model_id: &str) -> Result<String, SpiritError> {
    let model_id = model_id.trim();
    if model_id.is_empty()
        || model_id.len() > 200
        || model_id
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(SpiritError::invalid_input(
            "model",
            "choose a model from the available list",
        ));
    }
    Ok(model_id.to_owned())
}

fn message_body(model: Option<&ChatModelView>, text: &str, message_id: &str) -> Vec<u8> {
    let mut body = json!({
        "messageID": message_id,
        "parts": [{
            "type": "text",
            "text": text,
        }],
    });
    if let Some(model) = model {
        let (provider, model_id) = model
            .id
            .split_once('/')
            .unwrap_or(("opencode", model.id.as_str()));
        body["model"] = json!({
            "providerID": provider,
            "modelID": model_id,
        });
    }
    serde_json::to_vec(&body).unwrap_or_default()
}

fn parse_log(body: &[u8], now: Timestamp) -> Vec<ChatMessage> {
    let Ok(document) = serde_json::from_slice::<Value>(body) else {
        return Vec::new();
    };
    let Value::Array(entries) = document else {
        return Vec::new();
    };
    let mut messages: Vec<ChatMessage> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let info = entry.get("info").unwrap_or(entry);
        let Some(role) = info.get("role").and_then(Value::as_str) else {
            continue;
        };
        let (role, origin) = match role {
            "user" => (ChatRole::User, ChatMessageOrigin::User),
            "assistant" => (ChatRole::Assistant, ChatMessageOrigin::Model),
            _ => continue,
        };
        let parts = entry.get("parts").and_then(Value::as_array);
        let text = parts.map(|parts| render_parts(parts)).unwrap_or_default();
        let tools = if role == ChatRole::Assistant {
            parts.and_then(|parts| parse_tool_parts(parts))
        } else {
            None
        };
        if text.is_empty() && tools.is_none() {
            continue;
        }
        let id = info
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map_or_else(|| format!("log:{index}"), str::to_owned);
        let created_at = info
            .get("time")
            .and_then(|time| time.get("created"))
            .and_then(Value::as_i64)
            .map_or(now, Timestamp::from_unix_millis);
        let mut message = ChatMessage::from_text(id.clone(), role, origin, &text, created_at);
        message.tools = tools;
        if let Some(existing) = messages.iter_mut().find(|existing| existing.id == id) {
            *existing = message;
        } else {
            messages.push(message);
        }
    }
    messages
}

#[derive(Default)]
struct UsageTotals {
    total: u64,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
    reasoning: u64,
    saw_total: bool,
    saw_input: bool,
    saw_output: bool,
    saw_cache_read: bool,
    saw_reasoning: bool,
    seen: bool,
    cost: f64,
    saw_cost: bool,
}

impl UsageTotals {
    fn add_tokens(&mut self, tokens: &Value) {
        if let Some(value) = token_count(tokens.get("total")) {
            self.total = self.total.saturating_add(value);
            self.saw_total = true;
            self.seen = true;
        }
        if let Some(value) = token_count(tokens.get("input")) {
            self.input = self.input.saturating_add(value);
            self.saw_input = true;
            self.seen = true;
        }
        if let Some(value) = token_count(tokens.get("output")) {
            self.output = self.output.saturating_add(value);
            self.saw_output = true;
            self.seen = true;
        }
        if let Some(value) = token_count(tokens.get("reasoning")) {
            self.reasoning = self.reasoning.saturating_add(value);
            self.saw_reasoning = true;
            self.seen = true;
        }
        if let Some(value) = token_count(tokens.pointer("/cache/read")) {
            self.cache_read = self.cache_read.saturating_add(value);
            self.saw_cache_read = true;
            self.seen = true;
        }
        if let Some(value) = token_count(tokens.pointer("/cache/write")) {
            self.cache_write = self.cache_write.saturating_add(value);
            self.seen = true;
        }
    }

    fn merge(&mut self, other: &Self) {
        if other.saw_total {
            self.total = self.total.saturating_add(other.total);
            self.saw_total = true;
        }
        if other.saw_input {
            self.input = self.input.saturating_add(other.input);
            self.saw_input = true;
        }
        if other.saw_output {
            self.output = self.output.saturating_add(other.output);
            self.saw_output = true;
        }
        if other.saw_reasoning {
            self.reasoning = self.reasoning.saturating_add(other.reasoning);
            self.saw_reasoning = true;
        }
        if other.saw_cache_read {
            self.cache_read = self.cache_read.saturating_add(other.cache_read);
            self.saw_cache_read = true;
        }
        self.cache_write = self.cache_write.saturating_add(other.cache_write);
        self.seen |= other.seen;
        if other.saw_cost {
            self.cost += other.cost;
            self.saw_cost = true;
        }
    }

    fn context_tokens(&self) -> u64 {
        if self.saw_total {
            // OpenCode reports the turn total directly, including reasoning tokens.
            return self.total;
        }
        self.input
            .saturating_add(self.output)
            .saturating_add(self.reasoning)
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_write)
    }
}

fn token_count(value: Option<&Value>) -> Option<u64> {
    value.and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_i64().and_then(|count| u64::try_from(count).ok()))
    })
}

/// Parses assistant token and cost metadata from an OpenCode message log.
///
/// OpenCode may report usage either on `info.tokens`/`info.cost` or on `step-finish`
/// parts. Step parts win when present so a multi-step turn is not double-counted. Each
/// field stays `None` unless at least one message reported it.
fn parse_usage(body: &[u8], context_limit: Option<u64>) -> Option<ChatUsageView> {
    let Ok(document) = serde_json::from_slice::<Value>(body) else {
        return None;
    };
    let Value::Array(entries) = document else {
        return None;
    };
    let mut totals = UsageTotals::default();
    let mut turns = 0_u64;
    let mut last_context = None;
    for entry in &entries {
        let info = entry.get("info").unwrap_or(entry);
        match info.get("role").and_then(Value::as_str) {
            Some("user") => {
                turns = turns.saturating_add(1);
                continue;
            }
            Some("assistant") => {}
            _ => continue,
        }
        let step_parts: Vec<&Value> = entry
            .get("parts")
            .and_then(Value::as_array)
            .map(|parts| {
                parts
                    .iter()
                    .filter(|part| {
                        matches!(
                            part.get("type").and_then(Value::as_str),
                            Some("step-finish" | "step_finish")
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut message = UsageTotals::default();
        if step_parts.is_empty() {
            if let Some(tokens) = info.get("tokens") {
                message.add_tokens(tokens);
            }
        } else {
            for part in &step_parts {
                if let Some(tokens) = part.get("tokens") {
                    message.add_tokens(tokens);
                }
            }
        }
        if message.seen {
            last_context = Some(message.context_tokens());
            totals.merge(&message);
        }
        let cost = if step_parts.is_empty() {
            info.get("cost").and_then(Value::as_f64)
        } else {
            let mut sum = 0.0;
            let mut seen = false;
            for part in &step_parts {
                if let Some(value) = part.get("cost").and_then(Value::as_f64) {
                    sum += value;
                    seen = true;
                }
            }
            seen.then_some(sum)
        };
        if let Some(cost) = cost {
            totals.cost += cost;
            totals.saw_cost = true;
            totals.seen = true;
        }
    }
    if !totals.seen {
        return None;
    }
    Some(ChatUsageView {
        context_tokens_used: last_context,
        context_limit,
        input_tokens: totals.saw_input.then_some(totals.input),
        output_tokens: totals.saw_output.then_some(totals.output),
        cache_read_tokens: totals.saw_cache_read.then_some(totals.cache_read),
        turns,
        cost_usd: totals.saw_cost.then_some(totals.cost),
    })
}

/// Reads OpenCode `type: "tool"` parts into compact redacted rows.
///
/// Only the tool name, its status and a short hint taken from a known input field are
/// kept. Output, metadata and error text are never read, so a tool result cannot leak.
fn parse_tool_parts(parts: &[Value]) -> Option<Vec<ChatToolActivity>> {
    let tools: Vec<ChatToolActivity> = parts
        .iter()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("tool"))
        .filter_map(|part| {
            let id = part
                .get("id")
                .or_else(|| part.get("callID"))
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())?;
            let name = part.get("tool").and_then(Value::as_str)?;
            let state = part.get("state");
            let status = match state
                .and_then(|state| state.get("status"))
                .and_then(Value::as_str)
            {
                Some("running") => ChatToolStatus::Running,
                Some("completed") => ChatToolStatus::Completed,
                Some("error") => ChatToolStatus::Error,
                _ => ChatToolStatus::Pending,
            };
            let summary = state
                .and_then(|state| state.get("input"))
                .and_then(tool_summary)
                .map(|summary| display_snippet(&summary))
                .filter(|summary| !summary.is_empty());
            Some(ChatToolActivity {
                id: id.to_owned(),
                name: display_snippet(name),
                status,
                summary,
            })
        })
        .take(MAX_ACTIVITY_ITEMS)
        .collect();
    (!tools.is_empty()).then_some(tools)
}

fn tool_summary(input: &Value) -> Option<String> {
    const KEYS: [&str; 8] = [
        "description",
        "command",
        "filePath",
        "path",
        "pattern",
        "url",
        "query",
        "prompt",
    ];
    KEYS.iter()
        .find_map(|key| input.get(key).and_then(Value::as_str))
        .map(str::to_owned)
}

/// Reads `GET /permission` and keeps the requests that belong to one native session.
fn parse_permissions(body: &[u8], native: &str, now: Timestamp) -> Vec<ChatPermissionPrompt> {
    let Ok(Value::Array(entries)) = serde_json::from_slice::<Value>(body) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter(|entry| {
            entry
                .get("sessionID")
                .or_else(|| entry.get("sessionId"))
                .and_then(Value::as_str)
                == Some(native)
        })
        .filter_map(|entry| {
            let id = entry.get("id").and_then(Value::as_str)?;
            if id.is_empty() {
                return None;
            }
            let permission = entry
                .get("permission")
                .or_else(|| entry.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("permission");
            let patterns = entry
                .get("patterns")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .take(MAX_ACTIVITY_ITEMS)
                        .map(display_snippet)
                        .collect()
                })
                .unwrap_or_default();
            Some(ChatPermissionPrompt {
                id: id.to_owned(),
                permission: display_snippet(permission),
                patterns,
                created_at: now,
            })
        })
        .collect()
}

/// Reads `GET /session/status` (a map keyed by native session id) and returns the
/// retry for one session.
///
/// `Ok(None)` means the response is readable and the session is idle or busy;
/// `Err(())` means the body was not a status map and the caller must keep its previous
/// retry state.
fn parse_session_status_map(body: &[u8], native: &str) -> Result<Option<ChatRetryStatus>, ()> {
    let Value::Object(statuses) = serde_json::from_slice::<Value>(body).map_err(|_| ())? else {
        return Err(());
    };
    let Some(status) = statuses.get(native) else {
        return Ok(None);
    };
    let Some(status) = parse_session_status(status) else {
        return Ok(None);
    };
    Ok(retry_status(&status))
}

fn render_parts(parts: &[Value]) -> String {
    let mut text = String::new();
    for part in parts {
        if part.get("type").and_then(Value::as_str) != Some("text") {
            continue;
        }
        let Some(value) = part.get("text").and_then(Value::as_str) else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(value);
    }
    let (text, _) = cap_text(&normalize_text(&text), MAX_MESSAGE_CHARS);
    text
}

enum AssistantOutcome {
    Text {
        id: String,
        text: String,
        created_at: Timestamp,
        truncated: bool,
        tools: Option<Vec<ChatToolActivity>>,
    },
    Empty,
    Failed(SpiritError),
}

fn parse_assistant_response(body: &[u8], now: Timestamp) -> AssistantOutcome {
    let Ok(document) = serde_json::from_slice::<Value>(body) else {
        return AssistantOutcome::Failed(engine_request_error(
            "the local engine returned an unreadable message",
        ));
    };
    if let Some(error) = document.get("error").filter(|error| !error.is_null()) {
        return AssistantOutcome::Failed(map_provider_error(error_status(error)));
    }
    let info = document.get("info").unwrap_or(&document);
    if let Some(error) = info.get("error").filter(|error| !error.is_null()) {
        return AssistantOutcome::Failed(map_provider_error(error_status(error)));
    }
    let parts = document.get("parts").and_then(Value::as_array);
    let text = parts.map(|parts| render_parts(parts)).unwrap_or_default();
    let tools = parts.and_then(|parts| parse_tool_parts(parts));
    if text.is_empty() && tools.is_none() {
        return AssistantOutcome::Empty;
    }
    let (text, truncated) = cap_text(&text, MAX_MESSAGE_CHARS);
    let id = info
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map_or_else(|| format!("assistant:{}", Ulid::generate()), str::to_owned);
    let created_at = info
        .get("time")
        .and_then(|time| time.get("created"))
        .and_then(Value::as_i64)
        .map_or(now, Timestamp::from_unix_millis);
    AssistantOutcome::Text {
        id,
        text,
        created_at,
        truncated,
        tools,
    }
}

/// Parses the OpenCode `/api/session` answer and chooses the newest usable session.
///
/// The accepted shape is `{"data":[{"id":...,"time":{"updated":<ms>},...}]}`. A known
/// orchestrator workspace keeps only its sessions. Without one, every usable entry must
/// report the same `location.directory`; a mixed or missing workspace is a typed
/// `chat_not_configured` rather than a guess.
fn parse_discovered_session(
    body: &[u8],
    expected_workspace: Option<&Path>,
    name: &str,
) -> Result<DiscoveredSession, SpiritError> {
    let document = serde_json::from_slice::<Value>(body).map_err(|_| {
        chat_not_configured_error(name, "did not return a readable OpenCode session list")
    })?;
    let Some(entries) = document.get("data").and_then(Value::as_array) else {
        return Err(chat_not_configured_error(
            name,
            "did not return the expected OpenCode session list",
        ));
    };
    if entries.is_empty() {
        return Err(chat_not_configured_error(
            name,
            "has no OpenCode session to continue",
        ));
    }
    let mut newest: Option<(i64, String, PathBuf)> = None;
    let mut inferred: Option<PathBuf> = None;
    for entry in entries {
        let reported = entry_workspace(entry);
        let workspace = match reported {
            Some(value) if !value.is_empty() => match parse_workspace_path(value) {
                Some(workspace) => Some(workspace),
                None => {
                    return Err(chat_not_configured_error(
                        name,
                        "reported an unreadable OpenCode workspace",
                    ));
                }
            },
            _ => None,
        };
        if let Some(expected) = expected_workspace {
            // A reported workspace outside the orchestrator's own directory is another
            // host's conversation; never rank it.
            if workspace
                .as_deref()
                .is_some_and(|workspace| workspace != expected)
            {
                continue;
            }
            let Some(updated) = entry_updated(entry) else {
                return Err(malformed_session_list(name));
            };
            let id = entry_id(entry).ok_or_else(|| malformed_session_list(name))?;
            if newest.as_ref().is_none_or(|(best, _, _)| updated > *best) {
                newest = Some((updated, id, expected.to_path_buf()));
            }
            continue;
        }
        let Some(workspace) = workspace else {
            return Err(chat_not_configured_error(
                name,
                "has no single OpenCode workspace to continue",
            ));
        };
        match &inferred {
            None => inferred = Some(workspace.clone()),
            Some(existing) if existing == &workspace => {}
            Some(_) => {
                return Err(chat_not_configured_error(
                    name,
                    "has no single OpenCode workspace to continue",
                ));
            }
        }
        let Some(updated) = entry_updated(entry) else {
            return Err(malformed_session_list(name));
        };
        let id = entry_id(entry).ok_or_else(|| malformed_session_list(name))?;
        if newest.as_ref().is_none_or(|(best, _, _)| updated > *best) {
            newest = Some((updated, id, workspace));
        }
    }
    newest
        .map(|(_, id, workspace)| DiscoveredSession { id, workspace })
        .ok_or_else(|| {
            chat_not_configured_error(name, "has no OpenCode session in its workspace to continue")
        })
}

fn entry_id(entry: &Value) -> Option<String> {
    let id = entry.get("id").and_then(Value::as_str)?;
    safe_native_id(id).ok()
}

fn entry_updated(entry: &Value) -> Option<i64> {
    entry
        .get("time")
        .and_then(|time| time.get("updated"))
        .and_then(Value::as_i64)
}

fn entry_workspace(entry: &Value) -> Option<&str> {
    entry
        .get("location")
        .and_then(|location| location.get("directory"))
        .and_then(Value::as_str)
        .or_else(|| entry.get("directory").and_then(Value::as_str))
}

fn malformed_session_list(name: &str) -> SpiritError {
    chat_not_configured_error(name, "returned an unreadable OpenCode session list")
}

fn parse_native_sessions(body: &[u8]) -> Vec<NativeSession> {
    let Ok(document) = serde_json::from_slice::<Value>(body) else {
        return Vec::new();
    };
    let Value::Array(entries) = document else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let id = entry.get("id").and_then(Value::as_str)?;
            if safe_native_id(id).is_err() {
                return None;
            }
            let title = entry
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            Some(NativeSession {
                id: id.to_owned(),
                title,
            })
        })
        .collect()
}

fn page_messages(
    messages: Vec<ChatMessage>,
    query: &ChatLogQuery,
) -> Result<ChatLogPage, SpiritError> {
    let limit = query.limit();
    let offset = match query.cursor.as_deref() {
        Some(cursor) => cursor
            .parse::<usize>()
            .map_err(|_| SpiritError::invalid_input("cursor", "invalid log cursor"))?,
        None => 0,
    };
    if offset >= messages.len() {
        return Ok(ChatLogPage {
            messages: Vec::new(),
            next: None,
            truncated: false,
        });
    }
    let end = messages.len().saturating_sub(offset);
    let start = end.saturating_sub(limit);
    let page = messages[start..end].to_vec();
    let next = if start > 0 {
        Some((offset + page.len()).to_string())
    } else {
        None
    };
    Ok(ChatLogPage {
        messages: page,
        next,
        truncated: false,
    })
}

fn changed_messages(old: &[ChatMessage], new: &[ChatMessage]) -> Vec<ChatMessage> {
    new.iter()
        .filter(|message| {
            old.iter()
                .find(|existing| existing.id == message.id)
                .is_none_or(|existing| {
                    existing.text != message.text
                        || existing.streaming != message.streaming
                        || existing.tools != message.tools
                })
        })
        .cloned()
        .collect()
}

fn orchestrator_name(session: &ChatSessionId) -> Option<&str> {
    match session {
        ChatSessionId::Global => Some("global"),
        ChatSessionId::Project(name) => Some(name),
        ChatSessionId::Setup => None,
    }
}

fn validate_start(input: &ChatStartInput) -> Result<(), SpiritError> {
    match (&input.session, input.location) {
        (ChatSessionId::Setup, ChatSessionLocation::Local) => {
            if input.project.is_some() {
                return Err(SpiritError::invalid_input(
                    "project",
                    "the setup session does not take a project",
                ));
            }
            Ok(())
        }
        (ChatSessionId::Setup, ChatSessionLocation::Server) => Err(chat_gated_error(
            "the setup assistant uses the bundled StackSpirit Engine on this Mac",
        )),
        (ChatSessionId::Global, ChatSessionLocation::Local) => {
            if input.project.is_some() {
                return Err(SpiritError::invalid_input(
                    "project",
                    "the global session does not take a project",
                ));
            }
            Ok(())
        }
        (ChatSessionId::Project(name), ChatSessionLocation::Local) => {
            validate_project_name(name)?;
            if let Some(project) = &input.project
                && project != name
            {
                return Err(SpiritError::invalid_input(
                    "project",
                    "the project name does not match the session",
                ));
            }
            Ok(())
        }
        (ChatSessionId::Global | ChatSessionId::Project(_), ChatSessionLocation::Server) => {
            Err(chat_gated_error(
                "server global/project chat needs VOWP 0.5's orchestrator endpoint; this transport does not open SSH",
            ))
        }
    }
}

fn validate_attach(session: &ChatSessionId) -> Result<(), SpiritError> {
    match session {
        ChatSessionId::Setup | ChatSessionId::Global => Ok(()),
        ChatSessionId::Project(name) => validate_project_name(name),
    }
}

/// Maps an HTTP status to a safe typed error. It never includes a URL, port or body.
#[must_use]
pub fn map_http_status(status: u16) -> SpiritError {
    match status {
        401 | 403 => SpiritError::new(
            SpiritErrorCode::Auth,
            "engine_auth: the StackSpirit Engine rejected the request credential",
        )
        .with_hint("Re-pair or re-enter the engine credential and retry."),
        404 => session_lost_error(),
        409 => chat_busy_error(),
        429 => SpiritError::new(SpiritErrorCode::Limit, "The provider rate limit is active.")
            .retryable()
            .with_hint("Wait and retry the turn."),
        500..=599 => SpiritError::new(
            SpiritErrorCode::Engine,
            "The orchestrator failed the request.",
        )
        .retryable()
        .with_hint("Restart the engine or retry the turn."),
        _ => SpiritError::new(
            SpiritErrorCode::Engine,
            format!("the local engine returned HTTP {status}"),
        )
        .retryable()
        .with_hint("Restart the engine or retry the turn."),
    }
}

/// Maps a provider status carried inside an HTTP 200 response.
#[must_use]
pub fn map_provider_error(status: Option<u16>) -> SpiritError {
    match status {
        Some(401 | 403) => SpiritError::new(
            SpiritErrorCode::Auth,
            "engine_auth: the model provider rejected the credential",
        )
        .with_hint("Check the provider key in Settings → Engine."),
        Some(429) => SpiritError::new(SpiritErrorCode::Limit, "The provider rate limit is active.")
            .retryable()
            .with_hint("Wait and retry the turn."),
        _ => SpiritError::new(
            SpiritErrorCode::Engine,
            "provider_error: the model provider failed the turn",
        )
        .retryable()
        .with_hint("Retry the turn or choose another model."),
    }
}

fn engine_unavailable_error() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Engine,
        "engine_unavailable: the StackSpirit Engine is not reachable",
    )
    .retryable()
    .with_hint("Restart StackSpirit Engine in Settings → Advanced and retry.")
}

fn engine_external_error() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Engine,
        "engine_external: the setup assistant needs the bundled StackSpirit Engine",
    )
    .with_hint("Switch to the bundled engine in Settings → Advanced.")
}

fn engine_not_ready_error(detail: impl Into<String>) -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Engine,
        format!("engine_not_ready: {}", detail.into()),
    )
    .retryable()
    .with_hint("Restart StackSpirit Engine in Settings → Advanced and retry.")
}

fn engine_request_error(detail: &str) -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Engine,
        format!("the StackSpirit Engine failed the request: {detail}"),
    )
    .retryable()
}

fn no_model_error() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Engine,
        "no_model: the bundled engine reports no free model and no provider key is configured",
    )
    .with_hint("Add a provider key or choose a model in Settings → Engine, then retry.")
}

fn model_selection_unavailable_error() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Engine,
        "model_selection_unavailable: this engine cannot switch models from chat",
    )
    .with_hint("Choose the model in Settings → Engine.")
}

fn chat_model_switch_gated_error() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Conflict,
        "chat_model_gated: an orchestrator owns its own model choice",
    )
    .with_hint("Change the model in the orchestrator's OpenCode config.")
}

fn chat_gated_error(detail: &str) -> SpiritError {
    SpiritError::new(SpiritErrorCode::Gate, format!("chat_gated: {detail}"))
        .with_hint("Server and project chat wait for VOWP 0.5's orchestrator endpoint.")
}

/// A typed Local-orchestrator discovery failure. It names the orchestrator and the
/// failure class only; no URL, port, credential, native id or workspace path appears.
fn chat_not_configured_error(name: &str, detail: &str) -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Gate,
        format!(
            "chat_not_configured: the {} orchestrator {detail}",
            safe_orchestrator_name(name)
        ),
    )
    .with_hint("Start or resume the orchestrator in VOWP, open its conversation there, then retry.")
}

/// The reported VOWP credential was rejected by the local app.
fn chat_auth_error(name: &str) -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Auth,
        format!(
            "chat_auth: the {} orchestrator rejected the endpoint credential",
            safe_orchestrator_name(name)
        ),
    )
    .with_hint("Restart the orchestrator in VOWP so it refreshes its local credential, then retry.")
}

fn safe_orchestrator_name(name: &str) -> &str {
    if !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        name
    } else {
        "requested"
    }
}

fn chat_not_started_error() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Conflict,
        "chat_not_started: start the conversation first",
    )
    .with_hint("Start or attach the conversation, then retry.")
}

fn chat_busy_error() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Conflict,
        "chat_busy: wait for the current reply or stop it",
    )
    .with_hint("Send one turn at a time.")
}

fn session_lost_error() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::NotFound,
        "session_lost: the orchestrator's app session was recreated",
    )
    .with_hint("For setup, start a new conversation. The old log stays in Sessions.")
}

fn turn_timeout_error() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::RemoteTimeout,
        "turn_timeout: the turn did not finish in time",
    )
    .retryable()
    .with_hint("Stop the turn and retry.")
}

fn is_stream_lost(error: &SpiritError) -> bool {
    error.message.starts_with("stream_lost")
}

fn stream_lost_error() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::RemoteTimeout,
        "stream_lost: live updates stopped; reconnect to continue",
    )
    .retryable()
    .with_hint("Reattach the conversation to continue.")
}

fn stop_unconfirmed_error() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Conflict,
        "stop_unconfirmed: the turn may still be running on the orchestrator",
    )
    .retryable()
    .with_hint("Check the OpenCode app; StackSpirit does not pretend the turn stopped.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_status_mapping_is_safe() {
        for status in [401, 403, 404, 409, 429, 500, 418] {
            let error = map_http_status(status);
            assert!(!error.message.contains("127.0.0.1"));
            assert!(!error.message.contains("http://"));
            assert!(!error.message.contains('/'));
        }
        assert_eq!(map_http_status(401).code, SpiritErrorCode::Auth);
        assert_eq!(map_http_status(404).code, SpiritErrorCode::NotFound);
        assert_eq!(map_http_status(409).code, SpiritErrorCode::Conflict);
        assert_eq!(map_http_status(429).code, SpiritErrorCode::Limit);
    }

    #[test]
    fn parser_reads_both_log_shapes_and_skips_tool_parts() {
        let now = Timestamp::epoch();
        let body = br#"[
          {"info":{"id":"m1","role":"user","time":{"created":1000}},"parts":[{"type":"text","text":"Hi"}]},
          {"info":{"id":"m2","role":"assistant","time":{"created":2000}},"parts":[{"type":"tool","text":"{}"},{"type":"text","text":"Hello"}]},
          {"info":{"id":"m3","role":"assistant"},"parts":[]}
        ]"#;
        let messages = parse_log(body, now);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].joined_text(), "Hi");
        assert_eq!(messages[1].joined_text(), "Hello");
        assert_eq!(messages[0].origin, ChatMessageOrigin::User);
        assert_eq!(messages[1].origin, ChatMessageOrigin::Model);
    }

    #[test]
    fn assistant_response_error_in_200_is_typed_and_has_no_body_text() {
        let outcome = parse_assistant_response(
            br#"{"info":{"error":{"statusCode":401,"message":"Invalid API key"}}}"#,
            Timestamp::epoch(),
        );
        let AssistantOutcome::Failed(error) = outcome else {
            panic!("expected a typed failure");
        };
        assert_eq!(error.code, SpiritErrorCode::Auth);
        assert!(!error.message.contains("Invalid API key"));
        assert!(!error.message.contains('/'));
    }

    #[test]
    fn log_pages_move_backward_without_duplicates() {
        let now = Timestamp::epoch();
        let messages: Vec<ChatMessage> = (0..120)
            .map(|index| ChatMessage::user(format!("m{index}"), &format!("t{index}"), now))
            .collect();
        let first = page_messages(
            messages.clone(),
            &ChatLogQuery {
                cursor: None,
                limit: Some(50),
            },
        )
        .unwrap();
        assert_eq!(first.messages.len(), 50);
        assert_eq!(first.messages[0].id, "m70");
        assert_eq!(first.next.as_deref(), Some("50"));
        let second = page_messages(
            messages.clone(),
            &ChatLogQuery {
                cursor: first.next,
                limit: Some(50),
            },
        )
        .unwrap();
        assert_eq!(second.messages[0].id, "m20");
        assert_eq!(second.next.as_deref(), Some("100"));
        let third = page_messages(
            messages,
            &ChatLogQuery {
                cursor: second.next,
                limit: Some(50),
            },
        )
        .unwrap();
        assert_eq!(third.messages.len(), 20);
        assert_eq!(third.messages[0].id, "m0");
        assert!(third.next.is_none());
    }

    #[test]
    fn header_values_cannot_inject_a_newline() {
        let value = header_value(Path::new("/tmp/space\nInjected: yes"));
        assert!(!value.contains('\n'));
        assert!(value.contains("Injected: yes"));
    }

    #[test]
    fn native_ids_reject_path_characters() {
        assert!(safe_native_id("ses_abc-123").is_ok());
        assert!(safe_native_id("../escape").is_err());
        assert!(safe_native_id("ses/a").is_err());
        assert!(safe_native_id("").is_err());
    }

    #[test]
    fn status_map_reads_a_free_limit_retry_for_one_native_session() {
        let body = br#"{
          "ses_other": {"type": "retry", "attempt": 1, "message": "other", "next": 1},
          "ses_1": {"type": "retry", "attempt": 3, "message": "AI_APICallError: Free usage limit reached", "next": 1763673600000, "action": {"reason": "free_tier_limit", "title": "Free limit reached", "message": "Subscribe to OpenCode Go", "provider": "opencode"}}
        }"#;
        let retry = parse_session_status_map(body, "ses_1")
            .expect("readable status map")
            .expect("this session retries");
        assert_eq!(retry.attempt, 3);
        assert_eq!(retry.next, Some(1_763_673_600_000));
        let action = retry.action.expect("the free-tier action");
        assert_eq!(action.reason, "free_tier_limit");
        assert_eq!(action.title, "Free limit reached");
        assert_eq!(action.provider, "opencode");
        assert!(
            parse_session_status_map(body, "ses_missing")
                .unwrap()
                .is_none()
        );
        assert!(
            parse_session_status_map(br#"{"ses_1": {"type": "idle"}}"#, "ses_1")
                .unwrap()
                .is_none()
        );
        assert!(
            parse_session_status_map(b"not-json", "ses_1").is_err(),
            "an unreadable body must not clear a live retry"
        );
    }
}
