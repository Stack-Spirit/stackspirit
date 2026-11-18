//! Tolerant v1 SSE framing and OpenCode event parsing.
//!
//! The `ss-43` live probe pinned the pinned sidecar's v1 envelope, event names and
//! `message.part.updated`/`message.part.delta` shapes. It could not pin the runtime
//! cadence or the `session.idle` ordering without a model turn, so the production
//! transport still keeps live text behind [`StreamSupport::Unpinned`] and uses the final
//! message response. The parser and pump are exercised against the pinned shape so the
//! adapter can be enabled after the owner verification.

use serde_json::Value;

/// Why the production transport reports `streaming_supported: false`.
pub const STREAM_UNPROVEN_REASON: &str = "the pinned sidecar schema and event names are recorded, but the runtime cadence and ordering of message.part.updated, message.part.delta and session.idle remain unproven without a model turn; live text is disabled and the final message response is used";

/// Whether the endpoint's live event stream is proven and enabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamSupport {
    /// The v1 SSE frames are pinned by probe evidence.
    Pinned,
    /// The frames are not pinned. The reason is safe to show and never a URL/path.
    Unpinned {
        /// Why live text is disabled.
        reason: String,
    },
}

impl Default for StreamSupport {
    fn default() -> Self {
        Self::Unpinned {
            reason: STREAM_UNPROVEN_REASON.to_owned(),
        }
    }
}

impl StreamSupport {
    /// Returns true only for pinned evidence.
    #[must_use]
    pub const fn is_supported(&self) -> bool {
        matches!(self, Self::Pinned)
    }

    /// Returns the honest reason when streaming is disabled.
    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Pinned => None,
            Self::Unpinned { reason } => Some(reason),
        }
    }
}

/// One decoded SSE frame.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SseFrame {
    /// The optional SSE `event:` field. The JSON `type` is authoritative.
    pub event: Option<String>,
    /// The concatenated `data:` lines.
    pub data: String,
}

/// A line-oriented SSE parser.
///
/// It accepts split chunks, comments, multi-line data and unknown fields, ignores
/// unknown event types and never panics on malformed input.
#[derive(Debug, Default)]
pub struct SseParser {
    buffer: Vec<u8>,
    data_lines: Vec<String>,
    event: Option<String>,
}

impl SseParser {
    /// Creates an empty parser.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds bytes and returns every complete frame.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseFrame> {
        self.buffer.extend_from_slice(chunk);
        let mut frames = Vec::new();
        while let Some(position) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=position).collect();
            let line = &line[..line.len().saturating_sub(1)];
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            let line = String::from_utf8_lossy(line);
            if let Some(frame) = self.push_line(&line) {
                frames.push(frame);
            }
        }
        frames
    }

    /// Flushes a pending data frame at end of stream.
    pub fn flush(&mut self) -> Option<SseFrame> {
        self.dispatch()
    }

    fn push_line(&mut self, line: &str) -> Option<SseFrame> {
        if line.is_empty() {
            return self.dispatch();
        }
        if line.starts_with(':') {
            // A comment is a heartbeat or a keepalive. It still proves liveness.
            return None;
        }
        let (field, value) = line.split_once(':').map_or((line, ""), |(field, value)| {
            (field, value.strip_prefix(' ').unwrap_or(value))
        });
        match field {
            "event" => self.event = Some(value.to_owned()),
            "data" => self.data_lines.push(value.to_owned()),
            _ => {}
        }
        None
    }

    fn dispatch(&mut self) -> Option<SseFrame> {
        if self.data_lines.is_empty() {
            self.event = None;
            return None;
        }
        let data = self.data_lines.join("\n");
        self.data_lines.clear();
        Some(SseFrame {
            event: self.event.take(),
            data,
        })
    }
}

/// The proven fields of a native assistant message update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineMessageUpdate {
    /// Native message id; safe to use as a chat message id.
    pub id: String,
    /// Native role, normally `assistant`.
    pub role: String,
    /// Native session id, core-only.
    pub session_id: Option<String>,
    /// True when `info.error` is non-null.
    pub has_error: bool,
    /// Provider status code carried in `info.error`, when any.
    pub error_status: Option<u16>,
}

/// The proven fields of one native text part update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnginePartUpdate {
    /// Native part id.
    pub id: String,
    /// Native message id.
    pub message_id: String,
    /// Native session id, core-only.
    pub session_id: Option<String>,
    /// Cumulative text, when the frame carries it.
    pub text: Option<String>,
    /// A delta-only field, when the frame carries it.
    pub delta: Option<String>,
}

/// One parsed OpenCode event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineEvent {
    /// The SSE stream connected.
    ServerConnected,
    /// A roughly 10 s liveness heartbeat. It carries no chat text.
    ServerHeartbeat,
    /// A session metadata change.
    SessionUpdated {
        /// Native session id, when present.
        session_id: Option<String>,
    },
    /// An assistant or user message update.
    MessageUpdated {
        /// The proven message fields.
        message: EngineMessageUpdate,
    },
    /// One text part update with cumulative text.
    MessagePartUpdated {
        /// The proven part fields.
        part: EnginePartUpdate,
    },
    /// One additive text delta from the pinned `message.part.delta` event.
    ///
    /// The runtime cadence is not pinned; the pump deliberately ignores this event and
    /// relies on cumulative `message.part.updated` plus the idle log backfill.
    MessagePartDelta {
        /// Native session id, when present.
        session_id: Option<String>,
        /// Native message id.
        message_id: String,
        /// Native part id.
        part_id: String,
        /// The field the delta applies to; only `text` is relevant.
        field: String,
        /// The additive text.
        delta: String,
    },
    /// One part was removed.
    MessagePartRemoved {
        /// Native session id, when present.
        session_id: Option<String>,
        /// Native message id.
        message_id: String,
    },
    /// The session became idle.
    SessionIdle {
        /// Native session id, core-only.
        session_id: String,
    },
    /// The session failed.
    SessionError {
        /// Native session id, when present.
        session_id: Option<String>,
        /// Provider status code, when present.
        error_status: Option<u16>,
    },
    /// OpenCode asks for a tool permission. StackSpirit never approves it.
    PermissionAsked {
        /// Native session id, when present.
        session_id: Option<String>,
    },
    /// A permission reply, which has no UI in this contract.
    PermissionReplied,
    /// An unknown or unsupported event, ignored by the pump.
    Unknown,
}

impl EngineEvent {
    /// Returns the native session id used for routing, when the frame carries one.
    #[must_use]
    pub fn session_id(&self) -> Option<&str> {
        match self {
            Self::MessageUpdated { message } => message.session_id.as_deref(),
            Self::MessagePartUpdated { part } => part.session_id.as_deref(),
            Self::MessagePartDelta { session_id, .. }
            | Self::MessagePartRemoved { session_id, .. }
            | Self::SessionUpdated { session_id }
            | Self::SessionError { session_id, .. }
            | Self::PermissionAsked { session_id } => session_id.as_deref(),
            Self::SessionIdle { session_id } => Some(session_id),
            Self::ServerConnected
            | Self::ServerHeartbeat
            | Self::PermissionReplied
            | Self::Unknown => None,
        }
    }
}

/// Parses one SSE frame into an [`EngineEvent`].
#[must_use]
pub fn parse_engine_event(frame: &SseFrame) -> EngineEvent {
    let Ok(document) = serde_json::from_str::<Value>(frame.data.trim()) else {
        return EngineEvent::Unknown;
    };
    let Some(kind) = document.get("type").and_then(Value::as_str) else {
        return EngineEvent::Unknown;
    };
    let properties = document.get("properties").unwrap_or(&Value::Null);
    match kind {
        "server.connected" => EngineEvent::ServerConnected,
        "server.heartbeat" => EngineEvent::ServerHeartbeat,
        "session.updated" => EngineEvent::SessionUpdated {
            session_id: session_id(properties).map(str::to_owned),
        },
        "message.updated" => message_updated(properties),
        "message.part.updated" => message_part_updated(properties),
        "message.part.delta" => message_part_delta(properties),
        "message.part.removed" => {
            let part = properties.get("part").unwrap_or(properties);
            let Some(message_id) = text(part, "messageID") else {
                return EngineEvent::Unknown;
            };
            EngineEvent::MessagePartRemoved {
                session_id: session_id(part).map(str::to_owned),
                message_id: message_id.to_owned(),
            }
        }
        "session.idle" => {
            let Some(session_id) = session_id(properties) else {
                return EngineEvent::Unknown;
            };
            EngineEvent::SessionIdle {
                session_id: session_id.to_owned(),
            }
        }
        "session.error" => EngineEvent::SessionError {
            session_id: session_id(properties).map(str::to_owned),
            error_status: properties.get("error").and_then(error_status),
        },
        "permission.asked" | "permission.updated" => EngineEvent::PermissionAsked {
            session_id: session_id(properties).map(str::to_owned),
        },
        "permission.replied" => EngineEvent::PermissionReplied,
        _ => EngineEvent::Unknown,
    }
}

fn message_updated(properties: &Value) -> EngineEvent {
    let info = properties.get("info").unwrap_or(properties);
    let Some(id) = text(info, "id") else {
        return EngineEvent::Unknown;
    };
    let role = text(info, "role").unwrap_or_default();
    let has_error = info.get("error").is_some_and(|error| !error.is_null());
    let error_status = if has_error {
        info.get("error").and_then(error_status)
    } else {
        None
    };
    EngineEvent::MessageUpdated {
        message: EngineMessageUpdate {
            id: id.to_owned(),
            role: role.to_owned(),
            session_id: session_id(info).map(str::to_owned),
            has_error,
            error_status,
        },
    }
}

fn message_part_updated(properties: &Value) -> EngineEvent {
    let part = properties.get("part").unwrap_or(properties);
    if part.get("type").and_then(Value::as_str) != Some("text") {
        return EngineEvent::Unknown;
    }
    let Some(id) = text(part, "id") else {
        return EngineEvent::Unknown;
    };
    let Some(message_id) = text(part, "messageID") else {
        return EngineEvent::Unknown;
    };
    let delta = text(properties, "delta").or_else(|| text(part, "delta"));
    EngineEvent::MessagePartUpdated {
        part: EnginePartUpdate {
            id: id.to_owned(),
            message_id: message_id.to_owned(),
            session_id: session_id(part).map(str::to_owned),
            text: text(part, "text").map(str::to_owned),
            delta: delta.map(str::to_owned),
        },
    }
}

fn message_part_delta(properties: &Value) -> EngineEvent {
    let Some(message_id) = text(properties, "messageID") else {
        return EngineEvent::Unknown;
    };
    let Some(part_id) = text(properties, "partID").or_else(|| text(properties, "partId")) else {
        return EngineEvent::Unknown;
    };
    let Some(field) = text(properties, "field") else {
        return EngineEvent::Unknown;
    };
    EngineEvent::MessagePartDelta {
        session_id: session_id(properties).map(str::to_owned),
        message_id: message_id.to_owned(),
        part_id: part_id.to_owned(),
        field: field.to_owned(),
        delta: text(properties, "delta").unwrap_or_default().to_owned(),
    }
}

fn session_id(value: &Value) -> Option<&str> {
    text(value, "sessionID").or_else(|| text(value, "session_id"))
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
}

pub(crate) fn error_status(value: &Value) -> Option<u16> {
    match value {
        Value::Object(map) => {
            for key in ["statusCode", "status_code", "status", "code"] {
                if let Some(code) = map.get(key).and_then(Value::as_u64)
                    && (100..=599).contains(&code)
                {
                    return u16::try_from(code).ok();
                }
            }
            map.values().find_map(error_status)
        }
        Value::Array(items) => items.iter().find_map(error_status),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_handles_split_chunks_comments_and_multi_line_data() {
        let mut parser = SseParser::new();
        assert!(parser.push(b": keepalive\n\n").is_empty());
        let frames = parser.push(b"event: message\ndata: {\"type\":\"server.");
        assert!(frames.is_empty());
        let frames = parser.push(b"connected\",\n");
        assert!(frames.is_empty());
        let frames = parser.push(b"data: \"properties\":{}}\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].event.as_deref(), Some("message"));
        assert!(frames[0].data.contains("server.connected"));
        assert_eq!(parse_engine_event(&frames[0]), EngineEvent::ServerConnected);
    }

    #[test]
    fn parser_ignores_unknown_fields_and_events() {
        let mut parser = SseParser::new();
        let frames =
            parser.push(b"id: 42\nretry: 500\ndata: {\"type\":\"mystery\",\"properties\":{}}\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(parse_engine_event(&frames[0]), EngineEvent::Unknown);
        let frames = parser.push(b"data: not-json\n\n");
        assert_eq!(parse_engine_event(&frames[0]), EngineEvent::Unknown);
    }

    #[test]
    fn parser_reads_the_cumulative_text_part_shape() {
        let mut parser = SseParser::new();
        let frames = parser.push(
            br#"data: {"type":"message.part.updated","properties":{"part":{"id":"prt_1","messageID":"msg_1","sessionID":"ses_1","type":"text","text":"Hello"}}}

"#,
        );
        assert_eq!(frames.len(), 1);
        let event = parse_engine_event(&frames[0]);
        let EngineEvent::MessagePartUpdated { part } = event else {
            panic!("expected a part update: {event:?}");
        };
        assert_eq!(part.message_id, "msg_1");
        assert_eq!(part.session_id.as_deref(), Some("ses_1"));
        assert_eq!(part.text.as_deref(), Some("Hello"));
        assert_eq!(parse_engine_event(&frames[0]).session_id(), Some("ses_1"));
    }

    #[test]
    fn parser_maps_a_provider_error_in_message_updated() {
        let frame = SseFrame {
            event: None,
            data: r#"{"type":"message.updated","properties":{"info":{"id":"msg_1","role":"assistant","sessionID":"ses_1","error":{"statusCode":401}}}}"#.to_owned(),
        };
        let EngineEvent::MessageUpdated { message } = parse_engine_event(&frame) else {
            panic!("expected a message update");
        };
        assert!(message.has_error);
        assert_eq!(message.error_status, Some(401));
    }

    #[test]
    fn parser_reads_the_pinned_heartbeat_permission_and_delta_events() {
        let heartbeat = SseFrame {
            event: None,
            data: r#"{"id":"evt_1","type":"server.heartbeat","properties":{}}"#.to_owned(),
        };
        assert_eq!(parse_engine_event(&heartbeat), EngineEvent::ServerHeartbeat);

        let permission = SseFrame {
            event: None,
            data: r#"{"type":"permission.asked","properties":{"sessionID":"ses_1","permission":"write"}}"#.to_owned(),
        };
        assert_eq!(
            parse_engine_event(&permission),
            EngineEvent::PermissionAsked {
                session_id: Some("ses_1".to_owned())
            }
        );

        let delta = SseFrame {
            event: None,
            data: r#"{"type":"message.part.delta","properties":{"sessionID":"ses_1","messageID":"msg_1","partID":"prt_1","field":"text","delta":"lo"}}"#.to_owned(),
        };
        assert_eq!(
            parse_engine_event(&delta),
            EngineEvent::MessagePartDelta {
                session_id: Some("ses_1".to_owned()),
                message_id: "msg_1".to_owned(),
                part_id: "prt_1".to_owned(),
                field: "text".to_owned(),
                delta: "lo".to_owned(),
            }
        );
    }

    #[test]
    fn unpinned_stream_support_is_default_and_honest() {
        let support = StreamSupport::default();
        assert!(!support.is_supported());
        let reason = support.reason().unwrap();
        assert!(reason.contains("unproven"));
        assert!(!reason.contains("127.0.0.1"));
        assert!(!reason.contains('/'));
    }
}
