//! The Phase 1b transport router in front of the bundled-engine chat.
//!
//! [`HarnessChatProvider`] implements the public [`ChatTransport`] boundary from
//! `docs/HARNESS-CHAT.md` §4.1. For an app-owned `setup` session it resolves the persisted
//! harness default with [`preflight`] before touching the delegate, records the immutable
//! [`ChatHarnessCapture`] on a ready start and refuses every other selection with a typed
//! gate error whose message is exactly the stable gate token. VOWP `global`/`project`
//! sessions are OpenCode by contract and bypass the default entirely.
//!
//! The router is inert: it never spawns a child, opens a socket, calls the engine or
//! writes a file. It only reads the injected preferences source and the registry lookup
//! that [`preflight`] already performs. In Phase 1 the only ready selection is OpenCode,
//! so an existing setup session with no capture is treated as OpenCode on attach.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::engine::chat::harness::{HarnessChatGate, preflight};
use crate::engine::chat::local::ChatTransport;
use crate::engine::chat::model::{
    ChatAttachment, ChatLogPage, ChatLogQuery, ChatSendReceipt, ChatSessionId, ChatSessionView,
    ChatStartInput,
};
use crate::engine::chat::selection::{ChatHarnessCapture, ChatPreferencesSource};
use crate::error::{SpiritError, SpiritErrorCode};
use crate::harnesses::{DEFAULT_HARNESS_ID, HarnessProtocol, HarnessRegistry};

/// Routes app-owned setup chat through the harness gate before the OpenCode delegate.
///
/// `global`/`project` sessions and orchestrator provisioning bypass the gate: VOWP owns
/// those sessions as OpenCode and they are not candidates for the stored default.
pub struct HarnessChatProvider {
    delegate: Arc<dyn ChatTransport>,
    preferences: Arc<dyn ChatPreferencesSource + Send + Sync>,
    registry: Arc<HarnessRegistry>,
    captures: Mutex<HashMap<ChatSessionId, ChatHarnessCapture>>,
}

impl HarnessChatProvider {
    /// Builds the router around the existing local transport, the persisted default and
    /// the harness registry.
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
        }
    }

    /// Returns the current gate for a new app-owned setup conversation.
    ///
    /// This is the same read-only [`preflight`] the next setup `start` uses. It carries
    /// only a registry id, a protocol or a typed settings problem; never a path, endpoint,
    /// port, token or native id.
    #[must_use]
    pub fn gate(&self) -> HarnessChatGate {
        preflight(self.registry.as_ref(), self.preferences.as_ref())
    }

    /// Returns the immutable harness capture recorded for `session`, if any.
    ///
    /// Only an app-owned `setup` start (or the legacy attach described below) records a
    /// capture. The value carries only the registry id and protocol; never a native
    /// session id, endpoint, port, path, workspace or credential.
    #[must_use]
    pub fn capture(&self, session: &ChatSessionId) -> Option<ChatHarnessCapture> {
        self.captures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(session)
            .cloned()
    }

    fn store_capture(&self, session: ChatSessionId, capture: ChatHarnessCapture) {
        self.captures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(session, capture);
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
        // Attach never consults the stored default. It reuses the native session the
        // existing transport already knows; a default change must not move or gate a
        // running conversation.
        let view = self.delegate.attach(session).await?;
        if matches!(session, ChatSessionId::Setup) && self.capture(session).is_none() {
            // Phase 1 can create only an OpenCode setup session, so a setup session with
            // no capture predates this router and is the documented OpenCode default.
            // Recording that here keeps a later default change from rebinding it.
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
        self.delegate.send(session, text).await
    }

    async fn send_turn(
        &self,
        session: &ChatSessionId,
        text: &str,
        variant: Option<&str>,
        attachments: &[ChatAttachment],
    ) -> Result<ChatSendReceipt, SpiritError> {
        self.delegate
            .send_turn(session, text, variant, attachments)
            .await
    }

    async fn stop(&self, session: &ChatSessionId) -> Result<ChatSessionView, SpiritError> {
        self.delegate.stop(session).await
    }

    async fn provision_orchestrator(&self, project: &str) -> Result<ChatSessionView, SpiritError> {
        self.delegate.provision_orchestrator(project).await
    }

    async fn set_model(
        &self,
        session: &ChatSessionId,
        model_id: &str,
    ) -> Result<ChatSessionView, SpiritError> {
        self.delegate.set_model(session, model_id).await
    }

    async fn log(
        &self,
        session: &ChatSessionId,
        query: &ChatLogQuery,
    ) -> Result<ChatLogPage, SpiritError> {
        self.delegate.log(session, query).await
    }
}

/// Maps a non-ready gate to a typed error.
///
/// The message is exactly the stable token the bridge/UI maps to fixed copy. It never
/// carries the selected id, protocol, settings path, registry prose or raw parser text.
fn gate_error(gate: &HarnessChatGate) -> SpiritError {
    SpiritError::new(SpiritErrorCode::Gate, gate.reason())
}
