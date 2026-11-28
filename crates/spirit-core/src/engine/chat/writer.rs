//! The core writer/lease manager for `docs/SESSION-TAKEOVER.md` Phase 1.
//!
//! [`ChatWriter`] owns at most one `vowp orchestrator attach` child for one chat
//! session. It probes whether a takeover is honest, spawns the child with argv only,
//! reads its one-time grant, validates it and drops the token, watches the
//! authoritative endpoint for a replaced lease, and releases the child on demand or on
//! drop. Setup and engine-backed chats never use this manager.
//!
//! The lease token is parsed into a [`Zeroizing`] value, validated and dropped before
//! the grant is returned. It is never stored, logged, serialized or put in an error.
//! Surface ids, endpoint URLs, ports and native session ids stay core-only too.

use std::fmt;
use std::process::Stdio;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use serde::Deserialize;
use serde::de::IgnoredAny;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use zeroize::Zeroizing;

use crate::engine::chat::endpoint::{
    AuthDocument, EndpointAccessWire, ResolvedEndpoint, SessionOwnerWire, VowpEndpointResolver,
    VowpEnv, parse_loopback_url, valid_orchestrator_name, valid_surface_id,
};
use crate::engine::chat::model::{
    ChatOwnershipView, ChatSessionId, ChatWriterReason, ChatWriterState, ChatWriterView,
};
use crate::error::{SpiritError, SpiritErrorCode};

/// The bounded capability-probe budget from `docs/SESSION-TAKEOVER.md` §2.4.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
/// The attach budget: the 5 s takeover drain plus margin.
pub const ATTACH_TIMEOUT: Duration = Duration::from_secs(20);
/// The graceful release budget for a held child.
pub const RELEASE_TIMEOUT: Duration = Duration::from_secs(3);
/// The release budget for a child still waiting on its grant.
pub const ATTACH_DRAIN_TIMEOUT: Duration = Duration::from_secs(6);
/// The watch interval while the writer is held.
pub const WATCH_INTERVAL: Duration = Duration::from_secs(15);
/// One transient watch failure is retried after this delay.
pub const WATCH_RETRY_DELAY: Duration = Duration::from_secs(2);
/// The largest accepted attach grant document.
const MAX_GRANT_DOCUMENT: usize = 64 * 1024;
/// The longest accepted lease token.
const MAX_TOKEN_CHARS: usize = 512;
/// The most stderr kept for diagnostics. It is core-only and never rendered.
const MAX_STDERR_CHARS: usize = 1_000;
/// The most stderr lines kept for diagnostics.
const MAX_STDERR_LINES: usize = 200;
/// Extra wait after a forced kill.
const FORCE_WAIT: Duration = Duration::from_secs(1);
/// How often a drop reaper checks whether the child exited.
const REAP_SLICE: Duration = Duration::from_millis(100);

/// One chat session the writer manager may attach.
///
/// The orchestrator name and shell session key are safe metadata. The VOWP home session
/// id and native OpenCode session id stay core-only and never become a view or error.
#[derive(Clone)]
pub struct WriterTarget {
    orchestrator: String,
    session: ChatSessionId,
    expected_session_id: String,
    native_session_id: String,
    node: Option<String>,
}

impl WriterTarget {
    /// Builds a target from core-only identity values.
    #[must_use]
    pub fn new(
        orchestrator: impl Into<String>,
        session: ChatSessionId,
        expected_session_id: impl Into<String>,
        native_session_id: impl Into<String>,
    ) -> Self {
        Self {
            orchestrator: orchestrator.into(),
            session,
            expected_session_id: expected_session_id.into(),
            native_session_id: native_session_id.into(),
            node: None,
        }
    }

    /// Adds the safe node name used to build the default surface label.
    #[must_use]
    pub fn with_node(mut self, node: Option<String>) -> Self {
        self.node = node;
        self
    }

    /// True when two targets describe the same VOWP session and native log.
    fn matches(&self, other: &Self) -> bool {
        self.orchestrator == other.orchestrator
            && self.session == other.session
            && self.expected_session_id == other.expected_session_id
            && self.native_session_id == other.native_session_id
    }
}

impl fmt::Debug for WriterTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WriterTarget")
            .field("orchestrator", &self.orchestrator)
            .field("session", &self.session)
            .field("expected_session_id", &"[redacted]")
            .field("native_session_id", &"[redacted]")
            .field("node", &self.node)
            .finish()
    }
}

/// Configuration for one [`ChatWriter`] and its attach child.
///
/// Production uses [`Self::new`]. Integration tests override the child environment and
/// the timeouts so they never touch a real VOWP home, keychain, network or engine.
#[derive(Clone)]
pub struct ChatWriterConfig {
    child_environment: VowpEnv,
    holder: String,
    label: Option<String>,
    probe_timeout: Duration,
    attach_timeout: Duration,
    release_timeout: Duration,
    attach_drain_timeout: Duration,
    watch_interval: Duration,
    watch_retry_delay: Duration,
}

impl fmt::Debug for ChatWriterConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatWriterConfig")
            .field("holder", &self.holder)
            .field("label", &self.label)
            .field("probe_timeout", &self.probe_timeout)
            .field("attach_timeout", &self.attach_timeout)
            .field("release_timeout", &self.release_timeout)
            .field("attach_drain_timeout", &self.attach_drain_timeout)
            .field("watch_interval", &self.watch_interval)
            .field("watch_retry_delay", &self.watch_retry_delay)
            .finish_non_exhaustive()
    }
}

impl Default for ChatWriterConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl ChatWriterConfig {
    /// Builds the production configuration: holder `stackspirit`, inherited child environment
    /// and the bounded §2.4 timeouts.
    #[must_use]
    pub fn new() -> Self {
        Self {
            child_environment: VowpEnv::from_process(),
            holder: "stackspirit".to_owned(),
            label: None,
            probe_timeout: PROBE_TIMEOUT,
            attach_timeout: ATTACH_TIMEOUT,
            release_timeout: RELEASE_TIMEOUT,
            attach_drain_timeout: ATTACH_DRAIN_TIMEOUT,
            watch_interval: WATCH_INTERVAL,
            watch_retry_delay: WATCH_RETRY_DELAY,
        }
    }

    /// Overrides the child `PATH`. Only tests call this.
    #[must_use]
    pub fn with_path_env(mut self, path: impl Into<std::ffi::OsString>) -> Self {
        self.child_environment = self.child_environment.with_path(path);
        self
    }

    /// Overrides the child `VOWP_AGENT_HOME`. Only tests call this.
    #[must_use]
    pub fn with_agent_home(mut self, path: impl Into<std::ffi::OsString>) -> Self {
        self.child_environment = self.child_environment.with_agent_home(path);
        self
    }

    /// Overrides the child home (`STACKSPIRIT_HOME`/`VOWP_HOME`). Only tests call this.
    #[must_use]
    pub fn with_home_env(mut self, path: impl Into<std::ffi::OsString>) -> Self {
        self.child_environment = self.child_environment.with_home(path);
        self
    }

    /// Overrides the fixed holder name. Production keeps `stackspirit`.
    #[must_use]
    pub fn with_holder(mut self, holder: impl Into<String>) -> Self {
        self.holder = holder.into();
        self
    }

    /// Sets the optional friendly surface label built by core.
    #[must_use]
    pub fn with_label(mut self, label: Option<String>) -> Self {
        self.label = label;
        self
    }

    /// Overrides the capability-probe budget.
    #[must_use]
    pub fn with_probe_timeout(mut self, timeout: Duration) -> Self {
        self.probe_timeout = timeout;
        self
    }

    /// Overrides the attach budget.
    #[must_use]
    pub fn with_attach_timeout(mut self, timeout: Duration) -> Self {
        self.attach_timeout = timeout;
        self
    }

    /// Overrides the graceful release budget.
    #[must_use]
    pub fn with_release_timeout(mut self, timeout: Duration) -> Self {
        self.release_timeout = timeout;
        self
    }

    /// Overrides the release budget for a child still waiting on its grant.
    #[must_use]
    pub fn with_attach_drain_timeout(mut self, timeout: Duration) -> Self {
        self.attach_drain_timeout = timeout;
        self
    }

    /// Overrides the held-lease watch interval.
    #[must_use]
    pub fn with_watch_interval(mut self, interval: Duration) -> Self {
        self.watch_interval = interval;
        self
    }

    /// Overrides the one-transient-failure watch retry delay.
    #[must_use]
    pub fn with_watch_retry_delay(mut self, delay: Duration) -> Self {
        self.watch_retry_delay = delay;
        self
    }
}

/// A typed writer/takeover failure.
///
/// Variants carry no raw VOWP text, path, URL, port, surface id or token. They map to
/// the stable `chat_takeover_*`, `chat_writer_lost`, `chat_read_only` and
/// `harness_not_wired` prefixes from `docs/SESSION-TAKEOVER.md` §2.8.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatWriterError {
    /// The chat is app-owned (setup or engine-backed) and has no VOWP writer harness.
    HarnessNotWired,
    /// This provider does not install the writer manager.
    WriterNotConfigured,
    /// VOWP reports a node-hosted orchestrator.
    NodeHosted,
    /// There is no co-located VOWP home endpoint for this orchestrator.
    NotHome,
    /// The orchestrator has no live loopback endpoint.
    OrchestratorStopped,
    /// The installed VOWP has no `orchestrator attach` command.
    VowpTooOld,
    /// A live owner appeared before the attach and `--takeover` was not confirmed.
    AttachConflict,
    /// VOWP refused the attach (for example a node-only or stopped record).
    AttachRefused,
    /// The attach child did not answer within the attach budget.
    AttachTimeout,
    /// The attach child failed or returned an unusable document.
    AttachFailed,
    /// The grant failed validation before any write.
    GrantRejected,
    /// The grant named another VOWP session or native OpenCode session.
    SessionMismatch,
    /// The lease was replaced, disappeared or could not be verified.
    LeaseLost,
    /// A send was attempted without this app holding the writer lease.
    ReadOnly,
}

impl ChatWriterError {
    /// The safe view reason for this failure, when one exists.
    #[must_use]
    pub fn reason(self) -> Option<ChatWriterReason> {
        match self {
            Self::NodeHosted => Some(ChatWriterReason::NodeHosted),
            Self::NotHome => Some(ChatWriterReason::NotHome),
            Self::OrchestratorStopped => Some(ChatWriterReason::OrchestratorStopped),
            Self::VowpTooOld => Some(ChatWriterReason::VowpTooOld),
            Self::AttachRefused => Some(ChatWriterReason::AttachRefused),
            Self::AttachTimeout | Self::AttachFailed => Some(ChatWriterReason::AttachFailed),
            Self::SessionMismatch => Some(ChatWriterReason::SessionMismatch),
            Self::LeaseLost => Some(ChatWriterReason::LeaseLost),
            Self::HarnessNotWired
            | Self::WriterNotConfigured
            | Self::AttachConflict
            | Self::GrantRejected
            | Self::ReadOnly => None,
        }
    }

    /// Maps the failure to the existing safe error card.
    #[must_use]
    pub fn spirit(self) -> SpiritError {
        let (code, message, hint) = match self {
            Self::HarnessNotWired => (
                SpiritErrorCode::Gate,
                "harness_not_wired: this chat is app-owned and has no VOWP writer harness",
                Some("Use the bundled chat for setup and engine-backed projects."),
            ),
            Self::WriterNotConfigured => (
                SpiritErrorCode::Gate,
                "chat_takeover_unavailable: the chat writer is not enabled in this build",
                None,
            ),
            Self::NodeHosted => (
                SpiritErrorCode::Gate,
                "chat_takeover_unavailable: VOWP reports a node-hosted orchestrator; only a home-hosted orchestrator can be attached",
                Some("Wait for VOWP node-side attach or run the app on the VOWP home."),
            ),
            Self::NotHome => (
                SpiritErrorCode::Gate,
                "chat_takeover_unavailable: no co-located VOWP home endpoint was found for this orchestrator",
                Some("Run StackSpirit on the VOWP home; remote attach is not part of this design."),
            ),
            Self::OrchestratorStopped => (
                SpiritErrorCode::Gate,
                "chat_takeover_unavailable: the orchestrator has no live loopback endpoint",
                Some("Resume the orchestrator in VOWP, then try again."),
            ),
            Self::VowpTooOld => (
                SpiritErrorCode::Gate,
                "chat_takeover_unsupported: the installed VOWP has no `orchestrator attach` command",
                Some("Upgrade VOWP to 0.5.1 or newer."),
            ),
            Self::AttachConflict => (
                SpiritErrorCode::Conflict,
                "chat_takeover_conflict: another surface started writing before the attach",
                Some("Confirm a takeover, then try again."),
            ),
            Self::AttachRefused => (
                SpiritErrorCode::Gate,
                "chat_takeover_unavailable: VOWP refused the attach",
                None,
            ),
            Self::AttachTimeout => (
                SpiritErrorCode::Gate,
                "chat_takeover_failed: the attach did not answer in time",
                Some("If VOWP recorded a lease it clears within 60 seconds."),
            ),
            Self::AttachFailed | Self::GrantRejected => (
                SpiritErrorCode::Gate,
                "chat_takeover_failed: the attach child failed or returned an unusable grant",
                Some("Retry once; if it keeps failing, update VOWP."),
            ),
            Self::SessionMismatch => (
                SpiritErrorCode::Gate,
                "chat_not_mirrored: the attach grant named another session",
                Some("Re-open the orchestrator and use the mirrored chat session."),
            ),
            Self::LeaseLost => (
                SpiritErrorCode::Conflict,
                "chat_writer_lost: the session lease was replaced or disappeared",
                Some("Stay read-only, or take the session over again."),
            ),
            Self::ReadOnly => (
                SpiritErrorCode::Refused,
                "chat_read_only: this app does not hold the session writer lease",
                Some("Take over the session or use the surface that holds it."),
            ),
        };
        let error = SpiritError::new(code, message);
        match hint {
            Some(hint) => error.with_hint(hint),
            None => error,
        }
    }
}

impl fmt::Display for ChatWriterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.spirit().message)
    }
}

impl std::error::Error for ChatWriterError {}

impl From<ChatWriterError> for SpiritError {
    fn from(value: ChatWriterError) -> Self {
        value.spirit()
    }
}

impl ChatWriterReason {
    fn error(self) -> ChatWriterError {
        match self {
            Self::NodeHosted => ChatWriterError::NodeHosted,
            Self::VowpTooOld => ChatWriterError::VowpTooOld,
            Self::NotHome => ChatWriterError::NotHome,
            Self::OrchestratorStopped => ChatWriterError::OrchestratorStopped,
            Self::AttachRefused => ChatWriterError::AttachRefused,
            Self::AttachFailed => ChatWriterError::AttachFailed,
            Self::LeaseLost => ChatWriterError::LeaseLost,
            Self::SessionMismatch => ChatWriterError::SessionMismatch,
        }
    }
}

/// The state machine for one chat session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Unavailable(ChatWriterReason),
    Available,
    Attaching,
    Held,
    Releasing,
    Lost(ChatWriterReason),
}

impl Phase {
    fn view(self) -> ChatWriterView {
        match self {
            Self::Unavailable(reason) => ChatWriterView {
                state: ChatWriterState::Unavailable,
                can_take_over: false,
                can_release: false,
                reason: Some(reason),
            },
            Self::Available => ChatWriterView {
                state: ChatWriterState::Available,
                can_take_over: true,
                can_release: false,
                reason: None,
            },
            Self::Attaching => ChatWriterView {
                state: ChatWriterState::Attaching,
                can_take_over: false,
                can_release: false,
                reason: None,
            },
            Self::Held => ChatWriterView {
                state: ChatWriterState::Held,
                can_take_over: false,
                can_release: true,
                reason: None,
            },
            Self::Releasing => ChatWriterView {
                state: ChatWriterState::Releasing,
                can_take_over: false,
                can_release: false,
                reason: None,
            },
            Self::Lost(reason) => ChatWriterView {
                state: ChatWriterState::Lost,
                can_take_over: false,
                can_release: false,
                reason: Some(reason),
            },
        }
    }
}

struct WriterState {
    phase: Phase,
    /// The validated, available endpoint; never a lease.
    available: Option<ResolvedEndpoint>,
    /// The active grant endpoint while held, refreshed by the watch.
    active: Option<ResolvedEndpoint>,
    /// The last display-only owner seen by a probe or watch, so a lost lease can name
    /// the surface that replaced it without a surface id.
    last_owner: Option<ChatOwnershipView>,
    lease: Option<WriterLease>,
}

impl WriterState {
    fn new() -> Self {
        Self {
            phase: Phase::Unavailable(ChatWriterReason::NotHome),
            available: None,
            active: None,
            last_owner: None,
            lease: None,
        }
    }

    fn view(&self) -> ChatWriterView {
        self.phase.view()
    }

    /// Detects an attach child that exited without a release. The manager does not retry.
    fn refresh_exited(&mut self) {
        if self.phase != Phase::Held {
            return;
        }
        let exited = self.lease.as_mut().is_some_and(WriterLease::has_exited);
        if exited {
            self.phase = Phase::Lost(ChatWriterReason::LeaseLost);
            self.active = None;
            if let Some(mut lease) = self.lease.take() {
                lease.reap_in_background();
            }
        }
    }
}

/// A core-side observer for writer state transitions.
///
/// The chat provider installs one so a watcher loss can flip the owning session view to
/// `lost`, fence a running turn and backfill the log without a renderer round trip. The
/// callback sees only the safe [`ChatWriterView`]; it never receives a lease identity.
pub type WriterViewListener = Arc<dyn Fn(ChatWriterView) + Send + Sync>;

/// The core writer/lease manager for one chat session.
///
/// One instance owns at most one attach child. All transitions are serialised behind an
/// async mutex; a duplicate `take_over` returns the first operation's held state instead
/// of spawning a second child. The [`Drop`] implementation is a non-blocking safety net.
pub struct ChatWriter {
    target: WriterTarget,
    config: ChatWriterConfig,
    resolver: VowpEndpointResolver,
    state: Mutex<WriterState>,
    op: tokio::sync::Mutex<()>,
    view_listener: Mutex<Option<WriterViewListener>>,
}

impl fmt::Debug for ChatWriter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.lock_state();
        f.debug_struct("ChatWriter")
            .field("orchestrator", &self.target.orchestrator)
            .field("session", &self.target.session)
            .field("state", &state.phase)
            .finish_non_exhaustive()
    }
}

impl Drop for ChatWriter {
    fn drop(&mut self) {
        // The lease owns the child; its Drop reaps without blocking this thread.
        let lease = match self.state.lock() {
            Ok(mut state) => state.lease.take(),
            Err(poisoned) => poisoned.into_inner().lease.take(),
        };
        drop(lease);
    }
}

impl ChatWriter {
    /// Builds a manager for one chat session. No child is spawned until `take_over`.
    #[must_use]
    pub fn new(target: WriterTarget, config: ChatWriterConfig) -> Self {
        let resolver = VowpEndpointResolver::with_env(config.child_environment.clone());
        Self {
            target,
            config,
            resolver,
            state: Mutex::new(WriterState::new()),
            op: tokio::sync::Mutex::new(()),
            view_listener: Mutex::new(None),
        }
    }

    /// Installs the core-side state observer. Later calls replace it.
    ///
    /// The listener runs outside the state lock and must not block; the chat provider
    /// only spawns the async view/fence work from it.
    pub fn set_view_listener(&self, listener: WriterViewListener) {
        *self
            .view_listener
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(listener);
    }

    /// Publishes one safe transition to the installed observer, when any.
    fn publish_view(&self, view: ChatWriterView) {
        let listener = self
            .view_listener
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(listener) = listener {
            listener(view);
        }
    }

    /// True when the manager was built for the same session and native log.
    #[must_use]
    pub fn matches_target(&self, target: &WriterTarget) -> bool {
        self.target.matches(target)
    }

    /// The stable safe view. This also notices an attach child that exited unexpectedly.
    #[must_use]
    pub fn view(&self) -> ChatWriterView {
        let (view, changed) = {
            let mut state = self.lock_state();
            let before = state.phase;
            state.refresh_exited();
            (state.view(), state.phase != before)
        };
        if changed {
            self.publish_view(view);
        }
        view
    }

    /// Probes takeover eligibility without spawning an attach.
    ///
    /// The checks are the §3 contract: a usable `orchestrator attach` command, a
    /// co-located home-hosted endpoint, a live loopback URL/auth, the expected VOWP home
    /// session and the chat's native session. A held writer is left held.
    pub async fn probe(&self) -> ChatWriterView {
        let _op = self.op.lock().await;
        self.probe_locked().await
    }

    /// Attaches as the writer and returns the held view.
    ///
    /// `takeover` must be true only after the owner confirmed replacing a live holder;
    /// it is the only case that passes `--takeover`. A duplicate call never spawns a
    /// second child.
    pub async fn take_over(&self, takeover: bool) -> Result<ChatWriterView, ChatWriterError> {
        let _op = self.op.lock().await;
        {
            let mut state = self.lock_state();
            state.refresh_exited();
            match state.phase {
                Phase::Held => return Ok(state.view()),
                Phase::Attaching | Phase::Releasing => return Ok(state.view()),
                _ => {}
            }
        }
        let endpoint = match self.ensure_available().await {
            Ok(endpoint) => endpoint,
            Err(error) => {
                let reason = error.reason().unwrap_or(ChatWriterReason::NotHome);
                self.set_unavailable(reason);
                return Err(error);
            }
        };
        {
            let mut state = self.lock_state();
            state.phase = Phase::Attaching;
            state.available = Some(endpoint);
            state.active = None;
        }
        match self.spawn_attach(takeover).await {
            Ok((validated, lease)) => {
                let mut state = self.lock_state();
                state.phase = Phase::Held;
                state.available = Some(validated.endpoint.clone());
                state.active = Some(validated.endpoint);
                state.last_owner = None;
                state.lease = Some(lease);
                Ok(state.view())
            }
            Err(error) => {
                let phase = match error {
                    ChatWriterError::AttachConflict => Phase::Available,
                    ChatWriterError::AttachTimeout => Phase::Available,
                    ChatWriterError::NodeHosted => Phase::Unavailable(ChatWriterReason::NodeHosted),
                    ChatWriterError::NotHome => Phase::Unavailable(ChatWriterReason::NotHome),
                    ChatWriterError::OrchestratorStopped => {
                        Phase::Unavailable(ChatWriterReason::OrchestratorStopped)
                    }
                    ChatWriterError::VowpTooOld => Phase::Unavailable(ChatWriterReason::VowpTooOld),
                    ChatWriterError::AttachRefused => {
                        Phase::Unavailable(ChatWriterReason::AttachRefused)
                    }
                    ChatWriterError::SessionMismatch => {
                        Phase::Unavailable(ChatWriterReason::SessionMismatch)
                    }
                    ChatWriterError::LeaseLost => Phase::Lost(ChatWriterReason::LeaseLost),
                    ChatWriterError::HarnessNotWired
                    | ChatWriterError::WriterNotConfigured
                    | ChatWriterError::AttachFailed
                    | ChatWriterError::GrantRejected
                    | ChatWriterError::ReadOnly => {
                        Phase::Unavailable(ChatWriterReason::AttachFailed)
                    }
                };
                let mut state = self.lock_state();
                state.phase = phase;
                if phase == Phase::Available {
                    state.active = None;
                }
                Err(error)
            }
        }
    }

    /// Releases the lease and returns the safe view.
    ///
    /// Release is not Stop: a running turn may continue on the engine. A released
    /// writer returns to `available` so the owner can take over again.
    pub async fn release(&self) -> ChatWriterView {
        let _op = self.op.lock().await;
        let (lease, lost) = {
            let mut state = self.lock_state();
            state.refresh_exited();
            let lost = matches!(state.phase, Phase::Lost(_));
            if !matches!(state.phase, Phase::Held | Phase::Lost(_)) {
                return state.view();
            }
            let lease = state.lease.take();
            if lease.is_some() {
                state.phase = Phase::Releasing;
            }
            (lease, lost)
        };
        if let Some(lease) = lease {
            lease.release(self.config.release_timeout).await;
        }
        let mut state = self.lock_state();
        state.active = None;
        state.last_owner = None;
        state.phase = if lost {
            Phase::Lost(ChatWriterReason::LeaseLost)
        } else {
            Phase::Available
        };
        state.view()
    }

    /// Checks the authoritative endpoint once (the pre-send/focus check).
    ///
    /// A transient command failure is retried once after 2 s. Two failures, a different
    /// owner, a stopped endpoint or a vanished record is terminal for this grant: the
    /// child is released and the state becomes `lost`.
    pub async fn check_lease(&self) -> ChatWriterView {
        let _op = self.op.lock().await;
        self.check_lease_locked().await
    }

    /// The unlocked lease check shared by [`Self::check_lease`], the watcher and a held
    /// [`Self::probe`] (the focus/resume path). The caller holds the operation mutex.
    async fn check_lease_locked(&self) -> ChatWriterView {
        let (surface_id, expected_session_id) = {
            let mut state = self.lock_state();
            state.refresh_exited();
            if state.phase != Phase::Held {
                return state.view();
            }
            let Some(lease) = state.lease.as_ref() else {
                return state.view();
            };
            (
                lease.surface_id.clone(),
                self.target.expected_session_id.clone(),
            )
        };
        let mut attempts = 0_u8;
        loop {
            attempts = attempts.saturating_add(1);
            match self
                .resolver
                .watch_endpoint(&self.target.orchestrator, &expected_session_id, &surface_id)
                .await
            {
                Ok(endpoint) => {
                    let observed = endpoint.owner.as_ref().map(SessionOwnerWire::display_view);
                    // A lease that is still ours but stale (for example after a long
                    // sleep) must be released, not extended: a reissue is a new attach.
                    let stale = endpoint
                        .owner
                        .as_ref()
                        .and_then(|owner| owner.state.as_deref())
                        .is_some_and(|state| matches!(state, "stale" | "expired"));
                    let still_owner = endpoint.access == EndpointAccessWire::Owner
                        && !stale
                        && endpoint.session_id == self.target.expected_session_id
                        && endpoint.native_session_id.as_deref()
                            == Some(self.target.native_session_id.as_str());
                    if still_owner {
                        let mut state = self.lock_state();
                        if state.phase == Phase::Held {
                            state.active = Some(endpoint);
                            return state.view();
                        }
                        return state.view();
                    }
                    {
                        let mut state = self.lock_state();
                        state.last_owner = observed;
                    }
                    return self.lose(ChatWriterReason::LeaseLost).await;
                }
                Err(reason) => {
                    let terminal = match reason {
                        ChatWriterReason::NodeHosted => Some(ChatWriterReason::NodeHosted),
                        ChatWriterReason::NotHome => Some(ChatWriterReason::NotHome),
                        ChatWriterReason::OrchestratorStopped => {
                            Some(ChatWriterReason::OrchestratorStopped)
                        }
                        ChatWriterReason::SessionMismatch => {
                            Some(ChatWriterReason::SessionMismatch)
                        }
                        _ => None,
                    };
                    if let Some(reason) = terminal {
                        return self.lose(reason).await;
                    }
                    if attempts >= 2 {
                        return self.lose(ChatWriterReason::LeaseLost).await;
                    }
                    tokio::time::sleep(self.config.watch_retry_delay).await;
                }
            }
        }
    }

    /// Starts the held-lease watcher. It holds only a weak reference, so dropping the
    /// manager ends the watch instead of keeping it alive.
    pub fn start_watcher(self: &Arc<Self>) {
        let weak: Weak<Self> = Arc::downgrade(self);
        let interval = self.config.watch_interval;
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                let Some(writer) = weak.upgrade() else {
                    return;
                };
                if writer.view().state == ChatWriterState::Held {
                    // A lost/replaced lease is terminal for this grant; a later explicit
                    // takeover is a new grant and the next tick watches it.
                    let _ = writer.check_lease().await;
                }
            }
        });
    }

    /// The active grant endpoint while held; core-only.
    #[must_use]
    pub(crate) fn endpoint_snapshot(&self) -> Option<ResolvedEndpoint> {
        let mut state = self.lock_state();
        state.refresh_exited();
        if state.phase == Phase::Held {
            state.active.clone()
        } else {
            None
        }
    }

    /// The display-only owner produced by the held grant; core-only accessor.
    #[must_use]
    pub(crate) fn ownership(&self) -> Option<ChatOwnershipView> {
        let mut state = self.lock_state();
        state.refresh_exited();
        if state.phase != Phase::Held {
            return None;
        }
        state.lease.as_ref().map(|lease| lease.ownership.clone())
    }

    /// The last display-only owner seen by a probe or watch.
    ///
    /// After another surface replaces the lease, this names the new owner without a
    /// surface id. It is `None` when no owner was reported.
    #[must_use]
    pub(crate) fn observed_ownership(&self) -> Option<ChatOwnershipView> {
        let mut state = self.lock_state();
        state.refresh_exited();
        state.last_owner.clone()
    }

    async fn probe_locked(&self) -> ChatWriterView {
        let held = {
            let mut state = self.lock_state();
            state.refresh_exited();
            state.phase == Phase::Held
        };
        if held {
            // Focus/resume/reconnect: re-resolve the home endpoint with this surface id.
            // A still-owned lease keeps the grant and picks up a restarted engine's new
            // loopback URL/auth; anything else is terminal for this grant.
            return self.check_lease_locked().await;
        }
        match self.probe_inner().await {
            Ok(endpoint) => {
                let mut state = self.lock_state();
                state.available = Some(endpoint.clone());
                state.active = None;
                state.last_owner = endpoint.owner.as_ref().map(SessionOwnerWire::display_view);
                state.phase = Phase::Available;
                state.view()
            }
            Err(reason) => {
                self.set_unavailable(reason);
                self.view()
            }
        }
    }

    async fn probe_inner(&self) -> Result<ResolvedEndpoint, ChatWriterReason> {
        if !valid_orchestrator_name(&self.target.orchestrator) {
            return Err(ChatWriterReason::NotHome);
        }
        let endpoint = self
            .resolver
            .probe_endpoint(
                &self.target.orchestrator,
                Some(&self.target.expected_session_id),
            )
            .await?;
        self.validate_probe(&endpoint)?;
        self.probe_attach_command().await?;
        Ok(endpoint)
    }

    fn validate_probe(&self, endpoint: &ResolvedEndpoint) -> Result<(), ChatWriterReason> {
        if endpoint.location != "home" {
            return Err(ChatWriterReason::NodeHosted);
        }
        if endpoint.session_id != self.target.expected_session_id {
            return Err(ChatWriterReason::SessionMismatch);
        }
        if endpoint.native_session_id.as_deref() != Some(self.target.native_session_id.as_str()) {
            return Err(ChatWriterReason::SessionMismatch);
        }
        if endpoint.auth.is_none() {
            return Err(ChatWriterReason::OrchestratorStopped);
        }
        Ok(())
    }

    async fn probe_attach_command(&self) -> Result<(), ChatWriterReason> {
        let search_path = self.config.child_environment.search_path();
        let program = crate::ssh::command::resolve_local_engine_binary(search_path.as_deref());
        let mut command = Command::new(program);
        command
            .args(["orchestrator", "attach", "--help"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        self.config.child_environment.apply(&mut command);
        let mut child = command
            .spawn()
            .map_err(|_| ChatWriterReason::AttachFailed)?;
        match tokio::time::timeout(self.config.probe_timeout, child.wait()).await {
            Ok(Ok(status)) if status.success() => Ok(()),
            Ok(Ok(status)) if status.code() == Some(2) => Err(ChatWriterReason::VowpTooOld),
            _ => Err(ChatWriterReason::AttachFailed),
        }
    }

    async fn ensure_available(&self) -> Result<ResolvedEndpoint, ChatWriterError> {
        let available = {
            let mut state = self.lock_state();
            state.refresh_exited();
            match state.phase {
                Phase::Available => state.available.clone(),
                _ => None,
            }
        };
        if let Some(endpoint) = available {
            return Ok(endpoint);
        }
        match self.probe_inner().await {
            Ok(endpoint) => {
                let mut state = self.lock_state();
                state.available = Some(endpoint.clone());
                Ok(endpoint)
            }
            Err(reason) => Err(reason.error()),
        }
    }

    async fn spawn_attach(
        &self,
        takeover: bool,
    ) -> Result<(ValidatedGrant, WriterLease), ChatWriterError> {
        let search_path = self.config.child_environment.search_path();
        let program = crate::ssh::command::resolve_local_engine_binary(search_path.as_deref());
        let holder = self.config.holder.trim();
        if holder.is_empty() || holder.len() > 64 || holder.chars().any(char::is_control) {
            return Err(ChatWriterError::AttachFailed);
        }
        let mut command = Command::new(program);
        command
            .args([
                "--json",
                "orchestrator",
                "attach",
                self.target.orchestrator.as_str(),
                "--holder",
                holder,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(label) = self.surface_label() {
            command.args(["--label", label.as_str()]);
        }
        if takeover {
            command.arg("--takeover");
        }
        self.config.child_environment.apply(&mut command);
        let mut child = command.spawn().map_err(|_| ChatWriterError::AttachFailed)?;
        let stdin = child.stdin.take().ok_or(ChatWriterError::AttachFailed)?;
        let stdout = child.stdout.take().ok_or(ChatWriterError::AttachFailed)?;
        let stderr = child.stderr.take().ok_or(ChatWriterError::AttachFailed)?;
        let (stderr_buffer, stderr_task) = spawn_stderr_drain(stderr);
        let outcome = tokio::time::timeout(self.config.attach_timeout, read_attach(stdout)).await;
        let read = match outcome {
            Ok(Ok(read)) => read,
            Ok(Err(error)) => {
                let error = classify_attach_failure(&mut child, error).await;
                reap_attaching(
                    &mut child,
                    stdin,
                    stderr_task,
                    self.config.attach_drain_timeout,
                )
                .await;
                return Err(error);
            }
            Err(_) => {
                reap_attaching(
                    &mut child,
                    stdin,
                    stderr_task,
                    self.config.attach_drain_timeout,
                )
                .await;
                return Err(ChatWriterError::AttachTimeout);
            }
        };
        let grant = match read {
            AttachRead::Grant(grant) => *grant,
            AttachRead::Error(error) => {
                reap_attaching(
                    &mut child,
                    stdin,
                    stderr_task,
                    self.config.attach_drain_timeout,
                )
                .await;
                return Err(error);
            }
        };
        let validated = match validate_grant(grant, &self.target) {
            Ok(validated) => validated,
            Err(error) => {
                reap_attaching(
                    &mut child,
                    stdin,
                    stderr_task,
                    self.config.attach_drain_timeout,
                )
                .await;
                return Err(error);
            }
        };
        let lease = WriterLease {
            child: Some(child),
            stdin: Some(stdin),
            stderr: stderr_buffer,
            stderr_task: Some(stderr_task),
            surface_id: validated.surface_id.clone(),
            ownership: validated.ownership.clone(),
        };
        Ok((validated, lease))
    }

    fn surface_label(&self) -> Option<String> {
        if let Some(label) = &self.config.label {
            return safe_label(label);
        }
        self.target
            .node
            .as_deref()
            .and_then(safe_detail)
            .map(|node| safe_label(&format!("StackSpirit · {node}")).unwrap_or_default())
            .filter(|label| !label.is_empty())
    }

    async fn lose(&self, reason: ChatWriterReason) -> ChatWriterView {
        let (view, lease) = {
            let mut state = self.lock_state();
            state.phase = Phase::Lost(reason);
            state.active = None;
            (state.view(), state.lease.take())
        };
        if let Some(lease) = lease {
            lease.release(self.config.release_timeout).await;
        }
        // The chat provider uses this to flip the session view read-only and fence a
        // running turn; a raw view consumer never sees a lease identity.
        self.publish_view(view);
        view
    }

    fn set_unavailable(&self, reason: ChatWriterReason) {
        let mut state = self.lock_state();
        state.phase = Phase::Unavailable(reason);
        state.available = None;
        state.active = None;
    }

    fn lock_state(&self) -> MutexGuard<'_, WriterState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A validated grant, with the token already dropped.
struct ValidatedGrant {
    surface_id: String,
    endpoint: ResolvedEndpoint,
    ownership: ChatOwnershipView,
}

/// The one-time document `vowp orchestrator attach --json` prints.
#[derive(Deserialize)]
struct WriterGrant {
    surface_id: String,
    token: String,
    owner: GrantOwner,
    endpoint: GrantEndpoint,
}

impl fmt::Debug for WriterGrant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WriterGrant")
            .field("surface_id", &"[redacted]")
            .field("token", &"[redacted]")
            .field("owner", &"[redacted]")
            .field("endpoint", &"[redacted]")
            .finish()
    }
}

#[derive(Deserialize)]
struct GrantOwner {
    session_id: String,
    #[serde(default)]
    surface_id: Option<String>,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    holder: Option<String>,
    #[serde(default)]
    node: Option<String>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    activity: Option<String>,
}

#[derive(Deserialize)]
struct GrantEndpoint {
    name: String,
    app: String,
    session_id: String,
    location: String,
    #[serde(default)]
    node: Option<String>,
    #[serde(default)]
    state: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    native_session_id: Option<String>,
    #[serde(default)]
    auth: Option<AuthDocument>,
    #[serde(default)]
    access: Option<String>,
}

#[derive(Deserialize)]
struct ErrorDocument {
    error: ErrorBody,
}

#[derive(Deserialize)]
struct ErrorBody {
    #[serde(default)]
    kind: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum AttachDocument {
    Error(ErrorDocument),
    Grant(Box<WriterGrant>),
}

enum AttachRead {
    Grant(Box<WriterGrant>),
    Error(ChatWriterError),
}

/// Reads one complete JSON document without waiting for EOF.
///
/// VOWP pretty-prints the grant and keeps stdout open while it waits on stdin, so the
/// reader stops as soon as one complete value parses. The raw buffer is zeroized.
async fn read_attach(mut stdout: ChildStdout) -> Result<AttachRead, ChatWriterError> {
    let mut buffer = Zeroizing::new(Vec::with_capacity(1024));
    let mut chunk = [0_u8; 4096];
    loop {
        let read = stdout
            .read(&mut chunk)
            .await
            .map_err(|_| ChatWriterError::AttachFailed)?;
        if read == 0 {
            if buffer.is_empty() {
                return Err(ChatWriterError::AttachFailed);
            }
            return parse_attach(&buffer);
        }
        if buffer.len() + read > MAX_GRANT_DOCUMENT {
            return Err(ChatWriterError::AttachFailed);
        }
        buffer.extend_from_slice(&chunk[..read]);
        if serde_json::from_slice::<IgnoredAny>(&buffer).is_ok() {
            return parse_attach(&buffer);
        }
    }
}

fn parse_attach(bytes: &[u8]) -> Result<AttachRead, ChatWriterError> {
    match serde_json::from_slice::<AttachDocument>(bytes) {
        Ok(AttachDocument::Grant(grant)) => Ok(AttachRead::Grant(grant)),
        Ok(AttachDocument::Error(error)) => {
            let classified = match error.error.kind.as_deref() {
                Some("conflict") => ChatWriterError::AttachConflict,
                Some("refused" | "guard") => ChatWriterError::AttachRefused,
                Some("not_found") => ChatWriterError::NotHome,
                Some("invalid_input") => ChatWriterError::VowpTooOld,
                _ => ChatWriterError::AttachFailed,
            };
            Ok(AttachRead::Error(classified))
        }
        Err(_) => Err(ChatWriterError::AttachFailed),
    }
}

fn validate_grant(
    grant: WriterGrant,
    target: &WriterTarget,
) -> Result<ValidatedGrant, ChatWriterError> {
    let WriterGrant {
        surface_id,
        token,
        owner,
        endpoint,
    } = grant;
    let token = Zeroizing::new(token);
    if !valid_surface_id(&surface_id)
        || token.is_empty()
        || token.chars().count() > MAX_TOKEN_CHARS
        || token.chars().any(char::is_control)
    {
        return Err(ChatWriterError::GrantRejected);
    }
    drop(token);
    if owner.session_id != target.expected_session_id {
        return Err(ChatWriterError::SessionMismatch);
    }
    if let Some(owner_surface) = owner.surface_id.as_deref()
        && owner_surface != surface_id
    {
        return Err(ChatWriterError::GrantRejected);
    }
    if endpoint.name != target.orchestrator {
        return Err(ChatWriterError::GrantRejected);
    }
    if !endpoint.app.eq_ignore_ascii_case("opencode") {
        return Err(ChatWriterError::HarnessNotWired);
    }
    if endpoint.location != "home" {
        return Err(if endpoint.location == "node" {
            ChatWriterError::NodeHosted
        } else {
            ChatWriterError::GrantRejected
        });
    }
    if endpoint.access.as_deref() != Some("owner") {
        return Err(ChatWriterError::GrantRejected);
    }
    if endpoint.session_id != target.expected_session_id {
        return Err(ChatWriterError::SessionMismatch);
    }
    if endpoint.native_session_id.as_deref() != Some(target.native_session_id.as_str()) {
        return Err(ChatWriterError::SessionMismatch);
    }
    let Some(url) = endpoint.url.as_deref() else {
        return Err(ChatWriterError::GrantRejected);
    };
    let Some((host, port)) = parse_loopback_url(url) else {
        return Err(ChatWriterError::GrantRejected);
    };
    let Some(auth) = endpoint.auth else {
        return Err(ChatWriterError::GrantRejected);
    };
    let auth = auth
        .into_auth(&target.orchestrator)
        .map_err(|_| ChatWriterError::GrantRejected)?;
    let owner_wire = SessionOwnerWire {
        kind: owner.kind,
        label: owner.label,
        holder: owner.holder,
        node: owner.node,
        mode: owner.mode,
        state: owner.state,
        activity: owner.activity,
    };
    let ownership = owner_wire.display_view();
    let resolved = ResolvedEndpoint {
        name: endpoint.name,
        app: endpoint.app,
        session_id: endpoint.session_id,
        location: endpoint.location,
        node: endpoint.node,
        state: endpoint.state,
        host,
        port,
        native_session_id: endpoint.native_session_id,
        owner: Some(owner_wire),
        access: EndpointAccessWire::Owner,
        workspace: None,
        auth: Some(auth),
    };
    Ok(ValidatedGrant {
        surface_id,
        endpoint: resolved,
        ownership,
    })
}

fn safe_label(label: &str) -> Option<String> {
    let label = crate::error::redact(label);
    let label: String = label
        .chars()
        .filter(|character| !character.is_control())
        .take(128)
        .collect();
    let label = label.trim();
    (!label.is_empty()).then(|| label.to_owned())
}

fn safe_detail(value: &str) -> Option<String> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return None;
    }
    Some(value.to_owned())
}

/// The live attach child and its one-time grant.
///
/// The child holds the only live lease token. This struct stores the surface id and the
/// grant endpoint for the watch and read path, but never the token.
struct WriterLease {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stderr: Arc<Mutex<BoundedText>>,
    stderr_task: Option<tokio::task::JoinHandle<()>>,
    surface_id: String,
    ownership: ChatOwnershipView,
}

impl fmt::Debug for WriterLease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WriterLease")
            .field("surface_id", &"[redacted]")
            .field("endpoint", &"[redacted]")
            .field(
                "stderr_chars",
                &self.stderr.lock().map(|text| text.len()).unwrap_or(0),
            )
            .finish_non_exhaustive()
    }
}

impl Drop for WriterLease {
    fn drop(&mut self) {
        self.reap_in_background();
    }
}

impl WriterLease {
    fn has_exited(&mut self) -> bool {
        self.child
            .as_mut()
            .is_some_and(|child| matches!(child.try_wait(), Ok(Some(_))))
    }

    /// Closes stdin, waits for a graceful exit, then forces the child. The lease is
    /// consumed so a second release is impossible.
    async fn release(mut self, timeout: Duration) {
        if let Some(stdin) = self.stdin.take() {
            drop(stdin);
        }
        if let Some(mut child) = self.child.take() {
            match tokio::time::timeout(timeout, child.wait()).await {
                Ok(_) => {}
                Err(_) => {
                    let _ = child.start_kill();
                    let _ = tokio::time::timeout(FORCE_WAIT, child.wait()).await;
                }
            }
        }
        if let Some(task) = self.stderr_task.take() {
            task.abort();
        }
    }

    /// The non-blocking safety net used by `Drop` and a lazy lost-lease check.
    fn reap_in_background(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        drop(self.stdin.take());
        // Keep the stderr drain alive until the child exits. Closing the read end first
        // can give the child SIGPIPE before it reaches its own release path.
        drop(self.stderr_task.take());
        let _ = std::thread::spawn(move || {
            // The Drop path is a safety net, not the primary release. Give a healthy
            // child a little longer than the held-release budget, then force it.
            for _ in 0..50 {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    return;
                }
                std::thread::sleep(REAP_SLICE);
            }
            let _ = child.start_kill();
            for _ in 0..10 {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    return;
                }
                std::thread::sleep(REAP_SLICE);
            }
        });
    }
}

/// Uses the exit class when a failed attach printed no error document.
async fn classify_attach_failure(child: &mut Child, error: ChatWriterError) -> ChatWriterError {
    if error != ChatWriterError::AttachFailed {
        return error;
    }
    for _ in 0..5 {
        if let Ok(Some(status)) = child.try_wait() {
            return match status.code() {
                Some(2) => ChatWriterError::VowpTooOld,
                Some(4) => ChatWriterError::AttachConflict,
                Some(5) => ChatWriterError::AttachRefused,
                _ => ChatWriterError::AttachFailed,
            };
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    ChatWriterError::AttachFailed
}

/// Closes stdin and reaps a child that never produced a usable grant.
async fn reap_attaching(
    child: &mut Child,
    stdin: ChildStdin,
    stderr_task: tokio::task::JoinHandle<()>,
    timeout: Duration,
) {
    drop(stdin);
    match tokio::time::timeout(timeout, child.wait()).await {
        Ok(_) => {}
        Err(_) => {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(FORCE_WAIT, child.wait()).await;
        }
    }
    stderr_task.abort();
}

fn spawn_stderr_drain(
    mut stderr: ChildStderr,
) -> (Arc<Mutex<BoundedText>>, tokio::task::JoinHandle<()>) {
    let buffer = Arc::new(Mutex::new(BoundedText::default()));
    let task_buffer = Arc::clone(&buffer);
    let task = tokio::spawn(async move {
        let mut chunk = [0_u8; 1024];
        loop {
            match stderr.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    let mut text = task_buffer.lock().unwrap_or_else(PoisonError::into_inner);
                    text.push(&chunk[..read]);
                }
            }
        }
    });
    (buffer, task)
}

/// A bounded, core-only stderr buffer. It is never rendered or serialized.
#[derive(Default)]
struct BoundedText {
    text: String,
}

impl BoundedText {
    fn push(&mut self, bytes: &[u8]) {
        let chunk = String::from_utf8_lossy(bytes);
        let chunk = crate::error::redact(&chunk);
        for character in chunk.chars() {
            if self.text.chars().count() >= MAX_STDERR_CHARS {
                break;
            }
            self.text.push(character);
        }
        while self.text.lines().count() > MAX_STDERR_LINES {
            let Some(newline) = self.text.find('\n') else {
                break;
            };
            self.text.drain(..=newline);
        }
    }

    fn len(&self) -> usize {
        self.text.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> WriterTarget {
        WriterTarget::new(
            "global",
            ChatSessionId::Global,
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "ses_fake_native",
        )
    }

    #[test]
    fn writer_view_never_carries_a_token_or_transport_value() {
        let writer = ChatWriter::new(target(), ChatWriterConfig::new());
        let debug = format!("{writer:?}");
        for forbidden in [
            "token",
            "surface",
            "ses_fake_native",
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        ] {
            assert!(
                !debug.contains(forbidden),
                "writer Debug leaked {forbidden}"
            );
        }
        let view = writer.view();
        assert_eq!(view.state, ChatWriterState::Unavailable);
        assert!(!view.can_take_over);
        assert!(!view.can_release);
    }

    #[test]
    fn grant_debug_and_validation_drop_the_canary_token() {
        let canary = "fake-lease-canary-debug";
        let grant = WriterGrant {
            surface_id: "surf_fake_current".to_owned(),
            token: canary.to_owned(),
            owner: GrantOwner {
                session_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned(),
                surface_id: Some("surf_fake_current".to_owned()),
                kind: "desktop_ui".to_owned(),
                label: None,
                holder: Some("stackspirit".to_owned()),
                node: None,
                mode: None,
                state: None,
                activity: None,
            },
            endpoint: GrantEndpoint {
                name: "global".to_owned(),
                app: "opencode".to_owned(),
                session_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned(),
                location: "home".to_owned(),
                node: None,
                state: "running".to_owned(),
                url: Some("http://127.0.0.1:1".to_owned()),
                native_session_id: Some("ses_fake_native".to_owned()),
                auth: Some(AuthDocument {
                    header: "Authorization".to_owned(),
                    scheme: "Basic".to_owned(),
                    username: "opencode".to_owned(),
                    password: "fake-engine-credential".to_owned(),
                }),
                access: Some("owner".to_owned()),
            },
        };
        let debug = format!("{grant:?}");
        assert!(!debug.contains(canary));
        assert!(!debug.contains("surf_fake_current"));
        let validated = validate_grant(grant, &target()).expect("the grant validates");
        assert_eq!(validated.surface_id, "surf_fake_current");
        assert_eq!(
            validated.endpoint.native_session_id.as_deref(),
            Some("ses_fake_native")
        );
    }

    #[test]
    fn error_messages_have_no_raw_vowp_text() {
        for error in [
            ChatWriterError::NodeHosted,
            ChatWriterError::NotHome,
            ChatWriterError::OrchestratorStopped,
            ChatWriterError::VowpTooOld,
            ChatWriterError::AttachConflict,
            ChatWriterError::AttachRefused,
            ChatWriterError::AttachTimeout,
            ChatWriterError::AttachFailed,
            ChatWriterError::GrantRejected,
            ChatWriterError::SessionMismatch,
            ChatWriterError::LeaseLost,
            ChatWriterError::ReadOnly,
            ChatWriterError::HarnessNotWired,
            ChatWriterError::WriterNotConfigured,
        ] {
            let rendered = format!("{} {:?}", error.spirit().message, error.spirit().hint);
            for forbidden in ["http://", "127.0.0.1", "token", "Authorization", "surface_"] {
                assert!(
                    !rendered.contains(forbidden),
                    "{error:?} leaked {forbidden}: {rendered}"
                );
            }
        }
    }
}
