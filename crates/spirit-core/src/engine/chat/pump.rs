//! Revisioned chat event fan-out, provider-free message projection and SSE mapping.
//!
//! The pump keeps the append-only contract from `docs/ORCHESTRATOR-CHAT.md` §4.2:
//! `revision` is monotonic, a `message` upserts by id and a `delta` only appends. A
//! revision gap asks the transport to reload the native log; the caller replaces the
//! projected messages with that log before resuming.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::broadcast;

use crate::engine::chat::model::{
    ChatEvent, ChatMessage, ChatMessageOrigin, ChatPermissionPrompt, ChatRole, ChatSessionId,
    ChatSessionView, MAX_ACTIVITY_ITEMS, MAX_MESSAGE_CHARS, cap_text, display_snippet,
    normalize_text,
};
use crate::engine::chat::sse::{EngineEvent, EnginePartUpdate};
use crate::time::Timestamp;

/// The default broadcast capacity. It only needs to absorb one renderer hiccup.
const DEFAULT_EVENT_CAPACITY: usize = 256;

/// A revisioned broadcast bus for safe chat events.
#[derive(Debug)]
pub struct ChatEventBus {
    revision: AtomicU64,
    sender: broadcast::Sender<ChatEvent>,
}

impl Default for ChatEventBus {
    fn default() -> Self {
        Self::new(DEFAULT_EVENT_CAPACITY)
    }
}

impl ChatEventBus {
    /// Creates a bus with a bounded broadcast capacity.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity.max(1));
        Self {
            revision: AtomicU64::new(0),
            sender,
        }
    }

    /// Subscribes to the event fan-out.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<ChatEvent> {
        self.sender.subscribe()
    }

    /// Returns the last assigned revision.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Relaxed)
    }

    /// Returns the number of live subscribers.
    #[must_use]
    pub fn receiver_count(&self) -> usize {
        self.sender.receiver_count()
    }

    fn next_revision(&self) -> u64 {
        self.revision
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1)
    }

    /// Builds a session event.
    #[must_use]
    pub fn session(&self, view: ChatSessionView) -> ChatEvent {
        ChatEvent::Session {
            revision: self.next_revision(),
            session: Box::new(view),
        }
    }

    /// Builds an authoritative message event.
    #[must_use]
    pub fn message(&self, session: ChatSessionId, message: ChatMessage) -> ChatEvent {
        ChatEvent::Message {
            revision: self.next_revision(),
            session,
            message,
        }
    }

    /// Builds an append-only delta event.
    #[must_use]
    pub fn delta(
        &self,
        session: ChatSessionId,
        message_id: impl Into<String>,
        part_id: Option<String>,
        text: impl Into<String>,
    ) -> ChatEvent {
        ChatEvent::Delta {
            revision: self.next_revision(),
            session,
            message_id: message_id.into(),
            part_id,
            text: text.into(),
        }
    }

    /// Builds a done event.
    #[must_use]
    pub fn done(
        &self,
        session: ChatSessionId,
        message_id: impl Into<String>,
        truncated: bool,
    ) -> ChatEvent {
        ChatEvent::Done {
            revision: self.next_revision(),
            session,
            message_id: message_id.into(),
            truncated,
        }
    }

    /// Builds an error event.
    #[must_use]
    pub fn error(&self, session: ChatSessionId, error: crate::error::SpiritError) -> ChatEvent {
        ChatEvent::Error {
            revision: self.next_revision(),
            session,
            error,
        }
    }

    /// Builds a closed event.
    #[must_use]
    pub fn closed(
        &self,
        session: ChatSessionId,
        reason: crate::engine::chat::model::ChatCloseReason,
    ) -> ChatEvent {
        ChatEvent::Closed {
            revision: self.next_revision(),
            session,
            reason,
        }
    }

    /// Publishes an event and returns it for local projection.
    #[must_use]
    pub fn publish(&self, event: ChatEvent) -> ChatEvent {
        let _ = self.sender.send(event.clone());
        event
    }
}

/// What happened when one event was applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// The event was applied.
    Applied,
    /// The event was older than or equal to the last applied revision.
    Ignored,
    /// A revision gap was detected; the caller must reload the log.
    Backfill,
}

/// The renderer-side message projection used by the core for dedup and backfill.
#[derive(Debug, Default)]
pub struct ChatEventApplier {
    messages: Vec<ChatMessage>,
    last_revision: u64,
    truncated: BTreeSet<String>,
}

impl ChatEventApplier {
    /// Creates an empty applier.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the projected messages, oldest-first.
    #[must_use]
    pub fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    /// Returns the last applied revision.
    #[must_use]
    pub const fn last_revision(&self) -> u64 {
        self.last_revision
    }

    /// Returns a message by id.
    #[must_use]
    pub fn message(&self, id: &str) -> Option<&ChatMessage> {
        self.messages.iter().find(|message| message.id == id)
    }

    /// Returns true when a message exists and has visible text.
    #[must_use]
    pub fn has_text(&self, id: &str) -> bool {
        self.message(id)
            .is_some_and(|message| !message.joined_text().is_empty())
    }

    /// Returns true when a message's text hit the core cap.
    #[must_use]
    pub fn is_truncated(&self, id: &str) -> bool {
        self.truncated.contains(id)
    }

    /// Replaces the projection with a native log page and resets truncation state.
    pub fn replace_log(&mut self, messages: Vec<ChatMessage>) {
        self.messages = messages;
        self.truncated.clear();
    }

    /// Advances the revision baseline without applying a message change.
    ///
    /// The bus revision is global, so a session must observe events for other sessions
    /// too; otherwise the next event for this session would look like a gap.
    pub fn observe_revision(&mut self, revision: u64) {
        if revision > self.last_revision {
            self.last_revision = revision;
        }
    }

    /// Applies one event, detecting a gap.
    pub fn apply(&mut self, event: &ChatEvent, now: Timestamp) -> ApplyOutcome {
        let revision = event.revision();
        if revision <= self.last_revision {
            return ApplyOutcome::Ignored;
        }
        if self.last_revision != 0 && revision > self.last_revision.saturating_add(1) {
            self.last_revision = revision;
            return ApplyOutcome::Backfill;
        }
        self.last_revision = revision;
        match event {
            ChatEvent::Message { message, .. } => {
                self.upsert(message.clone());
            }
            ChatEvent::Delta {
                message_id, text, ..
            } => {
                self.append_delta(message_id, text, now);
            }
            ChatEvent::Done {
                message_id,
                truncated,
                ..
            } => {
                if *truncated {
                    self.truncated.insert(message_id.clone());
                }
                if let Some(index) = self
                    .messages
                    .iter()
                    .position(|message| &message.id == message_id)
                {
                    self.messages[index].streaming = false;
                    if self.messages[index].joined_text().is_empty()
                        && self.messages[index]
                            .tools
                            .as_ref()
                            .is_none_or(Vec::is_empty)
                    {
                        self.messages.remove(index);
                    }
                }
            }
            ChatEvent::Session { .. } | ChatEvent::Error { .. } | ChatEvent::Closed { .. } => {}
        }
        ApplyOutcome::Applied
    }

    fn upsert(&mut self, mut message: ChatMessage) {
        if let Some(existing) = self
            .messages
            .iter_mut()
            .find(|existing| existing.id == message.id)
        {
            if message.tools.is_none() {
                message.tools = existing.tools.take();
            }
            *existing = message;
        } else {
            self.messages.push(message);
        }
    }

    fn append_delta(&mut self, message_id: &str, text: &str, now: Timestamp) {
        if let Some(message) = self
            .messages
            .iter_mut()
            .find(|message| message.id == message_id)
        {
            if message.role != ChatRole::Assistant {
                return;
            }
            message.streaming = true;
            if message.append_delta(text) {
                self.truncated.insert(message_id.to_owned());
            }
            return;
        }
        let mut message = ChatMessage {
            id: message_id.to_owned(),
            role: ChatRole::Assistant,
            text: Vec::new(),
            created_at: now,
            origin: ChatMessageOrigin::Model,
            streaming: true,
            tools: None,
        };
        if message.append_delta(text) {
            self.truncated.insert(message_id.to_owned());
        }
        self.messages.push(message);
    }
}

/// One mapped SSE action, before the transport turns it into a [`ChatEvent`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mapped {
    /// Nothing to render.
    None,
    /// An authoritative message replacement or shell.
    Message(ChatMessage),
    /// An append-only text delta.
    Delta {
        /// The assistant message id.
        message_id: String,
        /// The native text part id.
        part_id: Option<String>,
        /// The text to append.
        text: String,
    },
    /// The session became idle.
    Done {
        /// The assistant message id, or empty when no text arrived.
        message_id: String,
        /// True when the mapper capped the text.
        truncated: bool,
    },
    /// A provider error with a safe status code only.
    Error {
        /// The provider status code, when present.
        status: Option<u16>,
    },
    /// A tool part changed; the transport reloads the native log to render the rows.
    ToolActivity,
    /// A permission event carried no id, so the transport reconciles with `GET /permission`.
    PermissionSync,
    /// OpenCode is waiting on a permission. The owner answers it; nothing auto-approves.
    PermissionAsked(ChatPermissionPrompt),
    /// A permission request was answered (here or in another OpenCode client).
    PermissionReplied {
        /// The native request id.
        id: String,
    },
}

/// Derives deltas from cumulative text part updates.
#[derive(Debug, Default)]
pub struct SseEventMapper {
    last_text: HashMap<(String, Option<String>), String>,
    truncated: BTreeSet<String>,
    active_assistant: Option<String>,
}

impl SseEventMapper {
    /// Creates an empty mapper.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Rebuilds the cumulative-text baseline from a log page.
    ///
    /// This is required after a reconnect: otherwise a cumulative part update that is
    /// already in the log would be appended again.
    pub fn reset_from_messages(&mut self, messages: &[ChatMessage]) {
        self.last_text.clear();
        self.truncated.clear();
        self.active_assistant = None;
        for message in messages {
            if message.role == ChatRole::Assistant {
                let text = message.joined_text();
                self.last_text.insert((message.id.clone(), None), text);
                self.active_assistant = Some(message.id.clone());
            }
        }
    }

    /// Maps one parsed engine event.
    #[must_use]
    pub fn map(&mut self, event: &EngineEvent, now: Timestamp) -> Mapped {
        match event {
            EngineEvent::MessageUpdated { message } => {
                if message.has_error {
                    return Mapped::Error {
                        status: message.error_status,
                    };
                }
                if message.role != "assistant" {
                    return Mapped::None;
                }
                self.active_assistant = Some(message.id.clone());
                let already_has_text = self
                    .last_text
                    .get(&(message.id.clone(), None))
                    .is_some_and(|text| !text.is_empty());
                if already_has_text {
                    return Mapped::None;
                }
                Mapped::Message(ChatMessage {
                    id: message.id.clone(),
                    role: ChatRole::Assistant,
                    text: Vec::new(),
                    created_at: now,
                    origin: ChatMessageOrigin::Model,
                    streaming: true,
                    tools: None,
                })
            }
            EngineEvent::MessagePartUpdated { part } => self.map_part(part, now),
            EngineEvent::MessagePartDelta { .. } => {
                // The pinned build emits a separate additive delta event. The runtime
                // cadence is not proven, so applying it alongside cumulative
                // `message.part.updated` could double-append text. The pump ignores it
                // and relies on the cumulative text plus the idle log backfill, as the
                // ss-43 probe report permits.
                Mapped::None
            }
            EngineEvent::MessagePartRemoved {
                message_id,
                session_id: _,
            } => {
                self.last_text.retain(|(id, _), _| id != message_id);
                if self.active_assistant.as_deref() == Some(message_id) {
                    self.active_assistant = None;
                }
                Mapped::None
            }
            EngineEvent::SessionIdle { session_id: _ } => {
                let message_id = self.active_assistant.take().unwrap_or_default();
                let truncated = self.truncated.remove(&message_id);
                Mapped::Done {
                    message_id,
                    truncated,
                }
            }
            EngineEvent::SessionError { error_status, .. } => Mapped::Error {
                status: *error_status,
            },
            EngineEvent::ToolPartUpdated { .. } => Mapped::ToolActivity,
            EngineEvent::PermissionAsked {
                id,
                permission,
                patterns,
                ..
            } => match id {
                Some(id) => Mapped::PermissionAsked(ChatPermissionPrompt {
                    id: id.clone(),
                    permission: display_snippet(permission),
                    patterns: patterns
                        .iter()
                        .take(MAX_ACTIVITY_ITEMS)
                        .map(|pattern| display_snippet(pattern))
                        .collect(),
                    created_at: now,
                }),
                // The pinned event may omit the request id. The prompt is still real, so
                // reconcile with the permission list instead of ignoring it.
                None => Mapped::PermissionSync,
            },
            EngineEvent::PermissionReplied { id, .. } => match id {
                Some(id) => Mapped::PermissionReplied { id: id.clone() },
                None => Mapped::PermissionSync,
            },
            EngineEvent::ServerConnected
            | EngineEvent::ServerHeartbeat
            | EngineEvent::SessionUpdated { .. }
            | EngineEvent::Unknown => Mapped::None,
        }
    }

    fn map_part(&mut self, part: &EnginePartUpdate, now: Timestamp) -> Mapped {
        self.active_assistant = Some(part.message_id.clone());
        let key = (part.message_id.clone(), Some(part.id.clone()));
        let base = self
            .last_text
            .get(&key)
            .or_else(|| self.last_text.get(&(part.message_id.clone(), None)))
            .cloned()
            .unwrap_or_default();
        if let Some(text) = &part.text {
            let normalized = normalize_text(text);
            let (capped, truncated) = cap_text(&normalized, MAX_MESSAGE_CHARS);
            if truncated {
                self.truncated.insert(part.message_id.clone());
            }
            self.last_text.insert(key, capped.clone());
            self.last_text
                .insert((part.message_id.clone(), None), capped.clone());
            if capped == base {
                return Mapped::None;
            }
            if base.is_empty() {
                let mut message = ChatMessage::model(part.message_id.clone(), &capped, now);
                message.streaming = true;
                return Mapped::Message(message);
            }
            if let Some(delta) = capped.strip_prefix(&base) {
                if delta.is_empty() {
                    return Mapped::None;
                }
                return Mapped::Delta {
                    message_id: part.message_id.clone(),
                    part_id: Some(part.id.clone()),
                    text: delta.to_owned(),
                };
            }
            let mut message = ChatMessage::model(part.message_id.clone(), &capped, now);
            message.streaming = true;
            return Mapped::Message(message);
        }
        let Some(delta) = &part.delta else {
            return Mapped::None;
        };
        let (normalized, truncated) = cap_text(&normalize_text(delta), MAX_MESSAGE_CHARS);
        if truncated {
            self.truncated.insert(part.message_id.clone());
        }
        if normalized.is_empty() {
            return Mapped::None;
        }
        self.last_text.insert(key, format!("{base}{normalized}"));
        self.last_text.insert(
            (part.message_id.clone(), None),
            format!("{base}{normalized}"),
        );
        Mapped::Delta {
            message_id: part.message_id.clone(),
            part_id: Some(part.id.clone()),
            text: normalized,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::chat::model::ChatSessionState;
    use crate::engine::chat::sse::{EngineMessageUpdate, SseFrame, parse_engine_event};

    fn view() -> ChatSessionView {
        ChatSessionView {
            session: ChatSessionId::Setup,
            location: crate::engine::chat::model::ChatSessionLocation::Local,
            state: ChatSessionState::Streaming,
            attached: true,
            streaming_supported: true,
            model: None,
            available_models: None,
            usage: None,
            started_at: None,
            last_event_at: None,
            error: None,
            pending_permissions: None,
        }
    }

    #[test]
    fn a_revision_gap_asks_for_a_log_reload_and_does_not_duplicate() {
        let bus = ChatEventBus::new(16);
        let mut applier = ChatEventApplier::new();
        let now = Timestamp::epoch();
        assert_eq!(
            applier.apply(&bus.session(view()), now),
            ApplyOutcome::Applied
        );
        let first = ChatMessage::model("m1", "Hello", now);
        assert_eq!(
            applier.apply(&bus.message(ChatSessionId::Setup, first), now),
            ApplyOutcome::Applied
        );
        // Skip revisions 3 and 4 to simulate a dropped event.
        let _ = bus.session(view());
        let _ = bus.session(view());
        let gap = bus.delta(ChatSessionId::Setup, "m1", None, "!");
        assert_eq!(gap.revision(), 5);
        assert_eq!(applier.apply(&gap, now), ApplyOutcome::Backfill);
        assert_eq!(applier.messages().len(), 1);

        applier.replace_log(vec![ChatMessage::model("m1", "Hello!", now)]);
        let done = bus.done(ChatSessionId::Setup, "m1", false);
        assert_eq!(applier.apply(&done, now), ApplyOutcome::Applied);
        assert_eq!(applier.messages().len(), 1);
        assert_eq!(applier.messages()[0].joined_text(), "Hello!");
        assert!(!applier.messages()[0].streaming);
    }

    #[test]
    fn message_events_upsert_by_id_and_deltas_only_append() {
        let bus = ChatEventBus::new(16);
        let mut applier = ChatEventApplier::new();
        let now = Timestamp::epoch();
        let _ = applier.apply(
            &bus.message(
                ChatSessionId::Setup,
                ChatMessage::user("local:1", "hi", now),
            ),
            now,
        );
        let _ = applier.apply(
            &bus.message(ChatSessionId::Setup, ChatMessage::model("m1", "", now)),
            now,
        );
        let _ = applier.apply(&bus.delta(ChatSessionId::Setup, "m1", None, "Hello"), now);
        let _ = applier.apply(&bus.delta(ChatSessionId::Setup, "m1", None, " world"), now);
        let _ = applier.apply(
            &bus.message(
                ChatSessionId::Setup,
                ChatMessage::model("m1", "Hello world", now),
            ),
            now,
        );
        assert_eq!(applier.messages().len(), 2);
        assert_eq!(applier.messages()[1].joined_text(), "Hello world");
    }

    #[test]
    fn mapper_derives_a_delta_from_cumulative_text_and_handles_a_rewrite() {
        let mut mapper = SseEventMapper::new();
        let now = Timestamp::epoch();
        let frame = SseFrame {
            event: None,
            data: r#"{"type":"message.part.updated","properties":{"part":{"id":"p1","messageID":"m1","sessionID":"s1","type":"text","text":"Hel"}}}"#.to_owned(),
        };
        let first = mapper.map(&parse_engine_event(&frame), now);
        let Mapped::Message(message) = first else {
            panic!("the first cumulative text must be an authoritative message");
        };
        assert_eq!(message.joined_text(), "Hel");
        let frame = SseFrame {
            event: None,
            data: r#"{"type":"message.part.updated","properties":{"part":{"id":"p1","messageID":"m1","sessionID":"s1","type":"text","text":"Hello"}}}"#.to_owned(),
        };
        assert_eq!(
            mapper.map(&parse_engine_event(&frame), now),
            Mapped::Delta {
                message_id: "m1".to_owned(),
                part_id: Some("p1".to_owned()),
                text: "lo".to_owned(),
            }
        );
        let frame = SseFrame {
            event: None,
            data: r#"{"type":"message.part.updated","properties":{"part":{"id":"p1","messageID":"m1","sessionID":"s1","type":"text","text":"Rewritten"}}}"#.to_owned(),
        };
        let Mapped::Message(message) = mapper.map(&parse_engine_event(&frame), now) else {
            panic!("a non-prefix rewrite must produce a message replacement");
        };
        assert_eq!(message.joined_text(), "Rewritten");
    }

    #[test]
    fn mapper_reset_from_log_prevents_a_duplicate_append() {
        let mut mapper = SseEventMapper::new();
        mapper.reset_from_messages(&[ChatMessage::model("m1", "Hello", Timestamp::epoch())]);
        let frame = SseFrame {
            event: None,
            data: r#"{"type":"message.part.updated","properties":{"part":{"id":"p1","messageID":"m1","sessionID":"s1","type":"text","text":"Hello"}}}"#.to_owned(),
        };
        assert_eq!(
            mapper.map(&parse_engine_event(&frame), Timestamp::epoch()),
            Mapped::None
        );
    }

    #[test]
    fn mapper_maps_permission_asked_and_ignores_the_additive_delta_event() {
        let mut mapper = SseEventMapper::new();
        assert_eq!(
            mapper.map(
                &EngineEvent::PermissionAsked {
                    session_id: Some("ses_1".to_owned()),
                    id: Some("per_1".to_owned()),
                    permission: "write".to_owned(),
                    patterns: vec!["/tmp/x".to_owned()],
                },
                Timestamp::epoch()
            ),
            Mapped::PermissionAsked(ChatPermissionPrompt {
                id: "per_1".to_owned(),
                permission: "write".to_owned(),
                patterns: vec!["/tmp/x".to_owned()],
                created_at: Timestamp::epoch(),
            })
        );
        assert_eq!(
            mapper.map(
                &EngineEvent::PermissionAsked {
                    session_id: Some("ses_1".to_owned()),
                    id: None,
                    permission: "write".to_owned(),
                    patterns: Vec::new(),
                },
                Timestamp::epoch()
            ),
            Mapped::PermissionSync,
            "an id-less request must reconcile with GET /permission"
        );
        assert_eq!(
            mapper.map(
                &EngineEvent::PermissionReplied {
                    session_id: Some("ses_1".to_owned()),
                    id: None,
                },
                Timestamp::epoch()
            ),
            Mapped::PermissionSync,
            "an id-less reply must reconcile with GET /permission"
        );
        assert_eq!(
            mapper.map(
                &EngineEvent::MessagePartDelta {
                    session_id: Some("ses_1".to_owned()),
                    message_id: "msg_1".to_owned(),
                    part_id: "prt_1".to_owned(),
                    field: "text".to_owned(),
                    delta: "lo".to_owned(),
                },
                Timestamp::epoch()
            ),
            Mapped::None
        );
    }

    #[test]
    fn message_updated_with_an_error_is_a_safe_status_only() {
        let mut mapper = SseEventMapper::new();
        let event = EngineEvent::MessageUpdated {
            message: EngineMessageUpdate {
                id: "m1".to_owned(),
                role: "assistant".to_owned(),
                session_id: Some("s1".to_owned()),
                has_error: true,
                error_status: Some(429),
            },
        };
        assert_eq!(
            mapper.map(&event, Timestamp::epoch()),
            Mapped::Error { status: Some(429) }
        );
    }
}
